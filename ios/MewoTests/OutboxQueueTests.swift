import XCTest
@testable import Mewo

final class OutboxQueueTests: XCTestCase {
    private let book = "11111111-1111-1111-1111-111111111111"
    private let other = "22222222-2222-2222-2222-222222222222"

    func testCoalesceKeepsRepliesAndFoldsLikesAndProgressInPlace() {
        let likeOn = OutboxOp.like(.init(bookId: book, postId: "p", liked: true))
        let likeOff = OutboxOp.like(.init(bookId: book, postId: "p", liked: false))
        let otherLike = OutboxOp.like(.init(bookId: book, postId: "q", liked: true))
        let progress = OutboxOp.progress(.init(bookId: book, index: 1))
        let moved = OutboxOp.progress(.init(bookId: book, index: 4))
        let first = reply("a", "one")
        let second = reply("b", "two")
        var queue = OutboxQueue()
        queue.enqueue(likeOn)
        queue.enqueue(progress)
        queue.enqueue(first)
        queue.enqueue(otherLike)
        queue.enqueue(likeOff)
        queue.enqueue(moved)
        queue.enqueue(second)
        XCTAssertEqual(queue.ops, [likeOff, moved, first, otherLike, second])
    }

    func testFlipLikeUsesTheQueuedIntentOverTheBase() {
        var queue = OutboxQueue()
        queue.flipLike(bookId: book, postId: "p", base: [])
        XCTAssertEqual(queue.likes(over: [], bookId: book), ["p"])
        queue.flipLike(bookId: book, postId: "p", base: ["p"])
        XCTAssertEqual(queue.likes(over: ["p"], bookId: book), [])

        var fromBase = OutboxQueue()
        fromBase.flipLike(bookId: book, postId: "p", base: ["p"])
        XCTAssertEqual(fromBase.likes(over: ["p"], bookId: book), [])
    }

    func testSentLeavesANewerLikeAndDropsAnUnchangedOne() {
        let older = OutboxOp.like(.init(bookId: book, postId: "p", liked: true))
        let newer = OutboxOp.like(.init(bookId: book, postId: "p", liked: false))
        var queue = OutboxQueue()
        queue.enqueue(older)
        queue.enqueue(newer)
        queue.sent(older)
        XCTAssertEqual(queue.ops, [newer])
        queue.sent(newer)
        XCTAssertTrue(queue.ops.isEmpty)
    }

    func testRefuseKeepsAReplyAndDropsLikeAndProgress() {
        var queue = OutboxQueue()
        let pending = reply("a", "one")
        let like = OutboxOp.like(.init(bookId: book, postId: "p", liked: true))
        let progress = OutboxOp.progress(.init(bookId: book, index: 3))
        queue.enqueue(pending)
        queue.enqueue(like)
        queue.enqueue(progress)
        queue.refuse(pending, message: "That post is gone.")
        queue.refuse(like, message: "missing")
        queue.refuse(progress, message: "bad index")
        guard case .reply(let left) = queue.ops.first else {
            return XCTFail("reply was dropped")
        }
        XCTAssertEqual(queue.ops.count, 1)
        XCTAssertEqual(left.id, "a")
        XCTAssertEqual(left.failed, "That post is gone.")
        queue.retry(replyId: "a")
        guard case .reply(let retried) = queue.ops.first else { return }
        XCTAssertNil(retried.failed)
    }

    func testClassifySeparatesRetryHoldRefusalGoneAndAccepted() {
        XCTAssertEqual(classifyOutbox(URLError(.notConnectedToInternet)), .retry)
        XCTAssertEqual(classifyOutbox(HostedException(message: "slow", code: 408)), .retry)
        XCTAssertEqual(classifyOutbox(HostedException(message: "slow", code: 429)), .retry)
        XCTAssertEqual(classifyOutbox(HostedException(message: "down", code: 503)), .retry)
        XCTAssertEqual(classifyOutbox(HostedException(message: "expired", code: 401)), .hold401)
        XCTAssertEqual(classifyOutbox(HostedException(message: "Too long.", code: 400)), .refused("Too long."))
        XCTAssertEqual(classifyOutbox(HostedException(message: "Already there.", code: 409)), .refused("Already there."))
        XCTAssertEqual(classifyOutbox(HostedException(message: "forbidden", code: 403)), .gone)
        XCTAssertEqual(classifyOutbox(HostedException(message: "missing", code: 404)), .gone)
        XCTAssertEqual(
            classifyOutbox(HostedException(message: "Could not read the server response.", code: 200)),
            .accepted
        )
        XCTAssertTrue(isOutboxCancellation(CancellationError()))
        XCTAssertTrue(isOutboxCancellation(URLError(.cancelled)))
        XCTAssertFalse(isOutboxCancellation(URLError(.notConnectedToInternet)))
    }

    func testOverlaysApplyPendingWrites() {
        var queue = OutboxQueue()
        queue.enqueue(.like(.init(bookId: book, postId: "p", liked: false)))
        queue.enqueue(.like(.init(bookId: book, postId: "q", liked: true)))
        XCTAssertEqual(queue.likes(over: ["p"], bookId: book), ["q"])
        XCTAssertEqual(queue.likes(over: ["p"], bookId: other), ["p"])

        let pending = reply("new", "queued")
        var failed = reply("bad", "no")
        if case .reply(var body) = failed {
            body.failed = "That post is gone."
            failed = .reply(body)
        }
        queue.enqueue(pending)
        queue.enqueue(failed)
        let thread = queue.thread(
            over: [
                PostComment(id: "old", username: "ada", text: "hi", createdAt: 1, mine: true),
                PostComment(id: "new", username: "ada", text: "server copy", createdAt: 2, mine: true),
            ],
            bookId: book,
            postId: "p",
            me: "ada"
        )
        XCTAssertEqual(thread.map(\.id), ["old", "new", "bad"])
        XCTAssertEqual(thread[1].text, "queued")
        XCTAssertEqual(thread[1].status, .sending)
        XCTAssertEqual(thread[2].status, .failed("That post is gone."))
        XCTAssertEqual(thread[1].username, "ada")
        XCTAssertTrue(thread[1].mine)

        let counted = queue.index(
            over: CommentIndex(counts: ["p": 1], reposts: ["p": 2]),
            bookId: book,
            threads: ["p": [PostComment(id: "old", username: "ada", text: "hi", createdAt: 1)]]
        )
        XCTAssertEqual(counted.count("p"), 3)
        XCTAssertTrue(counted.commented("p"))
        XCTAssertEqual(counted.repostCount("p"), 2)

        let already = queue.index(
            over: CommentIndex(counts: ["p": 1], mine: ["p"]),
            bookId: book,
            threads: [
                "p": [
                    PostComment(id: "new", username: "ada", text: "queued", createdAt: 1, mine: true),
                    PostComment(id: "bad", username: "ada", text: "no", createdAt: 2, mine: true),
                ],
            ]
        )
        XCTAssertEqual(already.count("p"), 1)
        XCTAssertTrue(already.commented("p"))

        queue.enqueue(.progress(.init(bookId: book, index: 9)))
        let rows = [record(book, progress: 1), record(other, progress: 2)]
        let shown = queue.books(over: rows, landed: [book: 4])
        XCTAssertEqual(shown[0].progressIndex, 9)
        XCTAssertEqual(shown[1], rows[1])

        var landedOnly = OutboxQueue()
        let kept = landedOnly.books(over: rows, landed: [book: 7])
        XCTAssertEqual(kept[0].progressIndex, 7)
        XCTAssertEqual(kept[1].progressIndex, 2)
    }

    func testCodableRoundTripAndReplyIdCase() throws {
        var failed = reply("a", "note")
        if case .reply(var body) = failed {
            body.failed = "nope"
            body.isPrivate = true
            failed = .reply(body)
        }
        let ops = [
            failed,
            OutboxOp.like(.init(bookId: book, postId: "p", liked: false)),
            OutboxOp.progress(.init(bookId: book, index: 3)),
        ]
        let file = OutboxFile(
            version: outboxFileVersion,
            account: OutboxAccount(url: "https://mewo.example", username: "ada"),
            ops: ops
        )
        let data = try JSONFiles.encoder.encode(file)
        let decoded = try JSONFiles.decoder.decode(OutboxFile.self, from: data)
        XCTAssertEqual(decoded, file)
        let text = String(decoding: data, as: UTF8.self)
        XCTAssertTrue(text.contains("\"private\" : true"))
        XCTAssertFalse(text.contains("isPrivate"))

        let id = outboxReplyId()
        XCTAssertEqual(id, id.lowercased())
        XCTAssertNotNil(UUID(uuidString: id))
        XCTAssertFalse(id.contains { $0.isCased && $0.isUppercase })
    }

    func testStatusStaysOutOfCommentEqualityAndJson() throws {
        let sent = PostComment(id: "c", username: "ada", text: "hi", createdAt: 1, mine: true)
        let sending = PostComment(id: "c", username: "ada", text: "hi", createdAt: 1, mine: true, status: .sending)
        let failed = PostComment(id: "c", username: "ada", text: "hi", createdAt: 1, mine: true, status: .failed("nope"))
        XCTAssertEqual(sent, sending)
        XCTAssertEqual(sent, failed)
        XCTAssertEqual(sent.hashValue, sending.hashValue)
        XCTAssertEqual(Set([sent]), Set([sent, sending, failed]))
        let json = String(decoding: try JSONFiles.encoder.encode(failed), as: UTF8.self)
        XCTAssertFalse(json.contains("status"))
        XCTAssertFalse(json.contains("Failed"))
        XCTAssertFalse(json.contains("nope"))
        let decoded = try JSONFiles.decoder.decode(PostComment.self, from: Data(json.utf8))
        XCTAssertEqual(decoded.status, .sent)
    }

    func testReplyPrivacyMatchesTheServerTrigger() {
        let rows: [(Bool, Bool, Bool, Bool)] = [
            (false, false, false, true),
            (false, false, true, false),
            (false, true, false, false),
            (true, false, false, false),
            (true, true, true, false),
        ]
        for (mine, shared, sample, isPrivate) in rows {
            var book = record(book, progress: 0)
            book.mine = mine
            book.shared = shared
            book.isSample = sample
            XCTAssertEqual(replyIsPrivate(book), isPrivate, "mine=\(mine) shared=\(shared) sample=\(sample)")
        }
    }

    func testDiscardTitleUsesTheCount() {
        XCTAssertEqual(discardUnsentTitle(1), "Discard 1 unsent reply?")
        XCTAssertEqual(discardUnsentTitle(2), "Discard 2 unsent replies?")
    }

    private func reply(_ id: String, _ text: String) -> OutboxOp {
        .reply(.init(
            bookId: book,
            postId: "p",
            id: id,
            text: text,
            isPrivate: false,
            createdAt: 5,
            failed: nil
        ))
    }

    private func record(_ id: String, progress: Int) -> BookRecord {
        BookRecord(
            id: id,
            title: "Emma",
            author: "Austen",
            handle: "austen",
            importedAt: 1,
            progressIndex: progress,
            mine: true
        )
    }
}
