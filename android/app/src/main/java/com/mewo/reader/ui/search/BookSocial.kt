package com.mewo.reader.ui.search

import com.mewo.reader.data.CommentIndex
import com.mewo.reader.data.LibraryStore
import com.mewo.reader.data.PostComment
import com.mewo.reader.data.commitRepost
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * Likes and comment counts per book for the search screen. Queries come and go,
 * but this state belongs to the book: a load still running is reused by the next
 * query, and a successful load is kept until [markStale]. A failed field keeps
 * whatever it showed before and loads again the next time its book is shown.
 *
 * Loads run one request at a time. The hosted client blocks a thread per call
 * and cancel cannot stop it, so a fan-out could fill the IO pool. A load still
 * waiting its turn is dropped when its book leaves the results.
 *
 * Like toggles, reposts, and comment edits come from the user, so they win over
 * any load that started before them. Reposts live in the comment index, and this
 * class is the only writer of that map.
 *
 * Call everything from [scope]'s thread (the main thread in the app); the
 * per-book fields have no locks.
 */
internal class BookSocial(
    private val store: LibraryStore,
    private val scope: CoroutineScope,
    private val onChange: (likes: Map<String, Set<String>>, comments: Map<String, CommentIndex>) -> Unit,
) {
    private class Book {
        var likesFresh = false
        var commentsFresh = false

        // Bumped by markStale. A load that spans a bump still shows its data but
        // is not fresh, so the book loads again.
        var epoch = 0L

        // The one load for this book, and whether it got past the queue.
        var load: Job? = null
        var loadStarted = false

        // Toggles started, the newest toggle shown, and toggles still running.
        var toggles = 0L
        var shownToggle = 0L
        var togglesRunning = 0

        // Same ticket for reposts, which share the comment index with loads.
        var reposts = 0L
        var shownRepost = 0L
        var repostsRunning = 0

        // Threads edited since the current comment load began.
        val edits = mutableMapOf<String, List<PostComment>>()
    }

    private val books = mutableMapOf<String, Book>()
    private val queue = Mutex()
    private var shown = emptySet<String>()
    private var likes = emptyMap<String, Set<String>>()
    private var comments = emptyMap<String, CommentIndex>()

    /** Loads what [ids] lack, in order, and drops queued loads for other books. */
    fun show(ids: List<String>) {
        shown = ids.toSet()
        books.forEach { (id, book) ->
            if (id !in shown && !book.loadStarted) {
                book.load?.cancel()
                book.load = null
            }
        }
        ids.forEach(::ensure)
    }

    /** Marks every book out of date and reloads the shown ones, keeping their data meanwhile. */
    fun markStale() {
        books.values.forEach {
            it.epoch++
            it.likesFresh = false
            it.commentsFresh = false
        }
        shown.forEach(::ensure)
    }

    fun toggleLike(id: String, postId: String) {
        val book = book(id)
        val ticket = ++book.toggles
        book.togglesRunning++
        scope.launch {
            try {
                val next = attempt { store.toggleLike(id, postId) } ?: return@launch
                // Responses can arrive out of order. Only a newer toggle may replace the shown one.
                if (ticket > book.shownToggle) {
                    book.shownToggle = ticket
                    book.likesFresh = true
                    likes = likes + (id to next)
                    onChange(likes, comments)
                }
            } finally {
                book.togglesRunning--
            }
        }
    }

    fun toggleRepost(id: String, postId: String) {
        val book = book(id)
        val ticket = ++book.reposts
        book.repostsRunning++
        val before = comments[id] ?: CommentIndex()
        scope.launch {
            try {
                commitRepost(
                    before,
                    postId,
                    { on -> store.setRepost(id, postId, on) },
                    { comments[id] ?: CommentIndex() },
                    { next ->
                        if (ticket >= book.shownRepost) {
                            book.shownRepost = ticket
                            comments = comments + (id to next)
                            onChange(likes, comments)
                        }
                    },
                )
            } catch (error: CancellationException) {
                throw error
            } catch (error: Exception) {
                // commitRepost already put the post back when this ticket still owned it.
            } finally {
                book.repostsRunning--
                // The index answer is one post. A load skipped for this toggle still
                // has to fetch the other posts' counts once the toggle has landed.
                if (book.repostsRunning == 0 && !book.commentsFresh && id in shown) ensure(id)
            }
        }
    }

    /** Shows the edited thread of [postId] now, and keeps it over a comment load already running. */
    fun onThreadChanged(id: String, postId: String, thread: List<PostComment>) {
        book(id).edits[postId] = thread
        val current = comments[id] ?: CommentIndex()
        comments = comments + (id to current.afterThread(postId, thread))
        onChange(likes, comments)
    }

    private fun book(id: String) = books.getOrPut(id) { Book() }

    private fun ensure(id: String) {
        val book = book(id)
        if (book.load != null || (book.likesFresh && book.commentsFresh)) return
        lateinit var job: Job
        job = scope.launch(start = CoroutineStart.LAZY) {
            var again = false
            try {
                queue.withLock {
                    book.loadStarted = true
                    again = fetch(id, book)
                }
            } finally {
                if (book.load === job) {
                    book.load = null
                    book.loadStarted = false
                }
            }
            if (again && id in shown && book.repostsRunning == 0) ensure(id)
        }
        book.load = job
        job.start()
    }

    /**
     * Loads the fields of [id] that are not fresh. Returns true when the book
     * should load again: [markStale] ran, or a repost overlapped the index.
     */
    private suspend fun fetch(id: String, book: Book): Boolean {
        val epoch = book.epoch
        var overlapped = false
        if (!book.likesFresh) {
            // A toggle that overlaps this load may land on the server before or
            // after it, so its answer is the one to trust.
            val ticket = book.toggles
            val calm = book.togglesRunning == 0
            val loaded = attempt { store.likes(id) }
            if (loaded != null && calm && ticket == book.toggles) {
                book.likesFresh = epoch == book.epoch
                likes = likes + (id to loaded)
                onChange(likes, comments)
            }
        }
        if (!book.commentsFresh) {
            book.edits.clear()
            // A repost that overlaps this load may land before or after it.
            val ticket = book.reposts
            val calm = book.repostsRunning == 0
            val loaded = attempt { store.commentIndex(id) }
            if (loaded != null && calm && ticket == book.reposts) {
                book.commentsFresh = epoch == book.epoch
                val merged = book.edits.entries.fold(loaded) { index, (postId, thread) ->
                    index.afterThread(postId, thread)
                }
                comments = comments + (id to merged)
                onChange(likes, comments)
            } else if (loaded != null) {
                overlapped = true
            }
        }
        return epoch != book.epoch || overlapped
    }
}

/** Runs [block] and returns null on failure, but lets cancellation through. */
internal suspend fun <T> attempt(block: suspend () -> T): T? = try {
    block()
} catch (e: CancellationException) {
    throw e
} catch (e: Exception) {
    null
}
