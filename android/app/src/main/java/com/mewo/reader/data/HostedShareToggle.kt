package com.mewo.reader.data

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * Owner share switch. The shelf flips immediately. Toggles for one book run
 * in order. A failed PATCH puts the switch back to the last value the server
 * confirmed, unless a newer toggle already moved it.
 */
internal class HostedShareToggle(
    private val books: () -> List<BookRecord>,
    private val replace: suspend (BookRecord) -> Unit,
) {
    private val gate = Mutex()
    private val tails = mutableMapOf<String, CompletableDeferred<Unit>>()
    private val confirmed = mutableMapOf<String, Boolean>()

    suspend fun setShared(id: String, shared: Boolean, patch: suspend () -> BookRecord) {
        val result = CompletableDeferred<Unit>()
        val previous = gate.withLock {
            val current = books().find { it.id == id } ?: error("That book is gone.")
            if (!current.mine) {
                error("Only the person who added this book can change sharing.")
            }
            if (current.shared == shared && !tails.containsKey(id)) {
                return
            }
            if (!confirmed.containsKey(id)) confirmed[id] = current.shared
            val queued = tails[id]
            tails[id] = result
            queued
        }

        val snapshot = books().find { it.id == id }
        if (snapshot != null && snapshot.shared != shared) {
            replace(snapshot.copy(shared = shared))
        }

        try {
            awaitPrevious(previous)
            val current = books().find { it.id == id }
            if (current == null || current.shared != shared) return
            val updated = try {
                patch()
            } catch (err: Exception) {
                rollback(id, shared)
                throw err
            }
            val latest = books().find { it.id == id }
            if (latest != null && latest.shared == shared) {
                gate.withLock { confirmed[id] = updated.shared }
                replace(
                    latest.copy(
                        shared = updated.shared,
                        mine = updated.mine,
                        ownerUsername = updated.ownerUsername,
                    ),
                )
            }
        } finally {
            if (!result.isCompleted) result.complete(Unit)
            gate.withLock {
                if (tails[id] === result) {
                    tails.remove(id)
                    confirmed.remove(id)
                }
            }
        }
    }

    private suspend fun awaitPrevious(previous: Deferred<Unit>?) {
        if (previous == null) return
        try {
            previous.await()
        } catch (err: CancellationException) {
            throw err
        } catch (_: Exception) {
            // The earlier toggle already surfaced its own error.
        }
    }

    private suspend fun rollback(id: String, shared: Boolean) {
        val latest = books().find { it.id == id } ?: return
        if (latest.shared != shared) return
        val prior = gate.withLock { confirmed[id] } ?: return
        replace(latest.copy(shared = prior))
    }
}
