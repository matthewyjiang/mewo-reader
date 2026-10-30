import XCTest
@testable import Mewo

@MainActor
final class HostedOfflineTests: XCTestCase {
    private var root: URL!
    private var suite: String!
    private var defaults: UserDefaults!
    private var sessions: HostedSessionStore!
    private var client: OfflineClient!
    private var store: HostedLibraryStore?

    override func setUp() async throws {
        HostedDisk.beforeClaim = nil
        HostedDisk.beforeScan = nil
        HostedDisk.recordPublishes = false
        HostedDisk.resetPublishes()
        HostedDisk.note = { HostedDiskLog.shared.note() }
        HostedDisk.noteDecode = { HostedDiskLog.shared.noteDecode() }
        HostedDiskLog.shared.reset()
        root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        suite = "mewo-offline-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suite)
        sessions = HostedSessionStore(defaults: defaults, persistToken: false)
        sessions.setBaseUrl(server)
        client = OfflineClient()
    }

    override func tearDown() async throws {
        defer {
            store = nil
            client = nil
            if let suite { defaults?.removePersistentDomain(forName: suite) }
            if let root { try? FileManager.default.removeItem(at: root) }
        }
        HostedDisk.beforeClaim = nil
        HostedDisk.beforeScan = nil
        HostedDisk.note = nil
        HostedDisk.noteDecode = nil
        HostedDisk.recordPublishes = false
        client?.releaseAll()
        sessions?.signOut()
        if let store {
            try await waitUntil("sign-out did not settle") {
                if case .loaded(_, false) = store.loadState { return store.books.isEmpty }
                return false
            }
        }
    }

    func testReaderOpensCachedPostsWhenSocialReadsThrow() async throws {
        client.postsError = offlineError()
        client.likesError = offlineError()
        client.indexError = offlineError()
        writePosts(idA, "cached line")
        let store = try await open([book(idA, importedAt: 3)])
        let vm = ReaderViewModel(library: store, bookId: idA)

        await vm.load()

        XCTAssertNil(vm.error)
        XCTAssertEqual(vm.posts.map(\.text), ["cached line"])
        XCTAssertTrue(vm.likes.isEmpty)
        XCTAssertEqual(vm.comments.count("p"), 0)
        XCTAssertTrue(client.postsCalls.isEmpty)
    }

    func testReaderUsesCachedSocialWhenTheNetworkThrows() async throws {
        writePosts(idA, "cached line")
        let store = try await open([book(idA, importedAt: 3)])
        let cachedLikes = try await store.likes(id: idA)
        let cachedIndex = try await store.commentIndex(id: idA)
        XCTAssertEqual(cachedLikes, ["p"])
        XCTAssertEqual(cachedIndex.count("p"), 1)
        client.likesError = offlineError()
        client.indexError = offlineError()
        let vm = ReaderViewModel(library: store, bookId: idA)

        await vm.load()

        XCTAssertNil(vm.error)
        XCTAssertEqual(vm.likes, ["p"])
        XCTAssertEqual(vm.comments.count("p"), 1)
    }

    func testSocialCacheIsWrittenAndUsedOnlyForTransportFailure() async throws {
        let store = try await open([book(idA, importedAt: 3)])
        let firstLikes = try await store.likes(id: idA)
        let firstThread = try await store.comments(id: idA, postId: "p")
        XCTAssertEqual(firstLikes, ["p"])
        XCTAssertEqual(firstThread.map(\.text), ["hi"])
        let cache = try readSocial(idA)
        XCTAssertEqual(cache.likes, ["p"])
        XCTAssertEqual(cache.threads["p"]?.map(\.text), ["hi"])
        XCTAssertEqual(cache.index.count("p"), 1)

        client.likes = ["p", "q"]
        _ = try await store.likes(id: idA)
        XCTAssertEqual(try readSocial(idA).likes, ["p", "q"])
        XCTAssertEqual(try readSocial(idA).threads["p"]?.map(\.text), ["hi"])

        client.likesError = offlineError()
        let offlineLikes = try await store.likes(id: idA)
        XCTAssertEqual(offlineLikes, ["p", "q"])
        client.commentsError = offlineError()
        let offlineThread = try await store.comments(id: idA, postId: "p")
        XCTAssertEqual(offlineThread.map(\.text), ["hi"])
        do {
            _ = try await store.comments(id: idA, postId: "missing")
            XCTFail("missing thread should stay offline")
        } catch {
            XCTAssertEqual(error.localizedDescription, "You're offline.")
        }

        client.likesError = HostedException(message: "nope", code: 500)
        do {
            _ = try await store.likes(id: idA)
            XCTFail("HTTP errors must not fall back")
        } catch let err as HostedException {
            XCTAssertEqual(err.code, 500)
        } catch {
            XCTFail("expected HTTP 500, got \(error)")
        }
        XCTAssertEqual(try readSocial(idA).likes, ["p", "q"])

        try writeSocialRaw(idA, #"{"version":2,"likes":["z"]}"#)
        client.likesError = offlineError()
        do {
            _ = try await store.likes(id: idA)
            XCTFail("a different cache version is a miss")
        } catch {
            XCTAssertEqual(error.localizedDescription, "offline")
        }
    }

    func testDownloadedIsTheCurrentPostsFileOnTheShelf() async throws {
        writePosts(idA, "secret")
        writePosts(idB, "other", version: 2)
        let store = try await open([book(idA, importedAt: 3), book(idB, importedAt: 2)])
        XCTAssertEqual(try XCTUnwrap(store.offline[idA]), .downloaded)
        XCTAssertEqual(try XCTUnwrap(store.offline[idB]), .notDownloaded)
        let cached = try await store.feed(id: idA)
        XCTAssertEqual(cached.map(\.text), ["secret"])
        XCTAssertTrue(client.postsCalls.isEmpty)

        let refreshed = try await store.feed(id: idB)
        XCTAssertEqual(refreshed.map(\.text), ["secret"])
        XCTAssertEqual(client.postsCalls, [idB])
        XCTAssertEqual(try XCTUnwrap(store.offline[idB]), .downloaded)
        XCTAssertEqual(try readPostsVersion(idB), 1)
    }

    func testCachedTextIsRefusedOnceTheBookLeavesTheShelf() async throws {
        client.armLibraryGate()
        writePosts(idB, "secret")
        let store = makeStore()
        self.store = store
        sessions.setSignedIn(username: "ada", token: "token-a")
        try await waitUntil("library did not start") { client.libraryArrived }
        XCTAssertTrue(store.loadState == .loading || store.loadState.isRefreshing)
        let cached = try await store.feed(id: idB)
        XCTAssertEqual(cached.map(\.text), ["secret"])
        XCTAssertTrue(client.postsCalls.isEmpty)

        client.completeLibrary([book(idA, importedAt: 3)])
        try await waitUntil("refresh did not finish") {
            if case .loaded(let rows, false) = store.loadState { return rows.map(\.id) == [idA] }
            return false
        }
        do {
            _ = try await store.feed(id: idB)
            XCTFail("served text for a book that is not on the shelf")
        } catch {
            XCTAssertEqual(error.localizedDescription, "That book is not on your shelf.")
        }
        XCTAssertTrue(postsExist(idB))
        XCTAssertTrue(client.postsCalls.isEmpty)
        XCTAssertEqual(store.books.map(\.id), [idA])
    }

    func testSetDownloadedWritesPostsAndSocialAndRemoveKeepsTheCover() async throws {
        let store = try await open([book(idA, importedAt: 3)])
        let cover = bookDir(idA).appendingPathComponent("cover.jpg")
        try FileManager.default.createDirectory(at: cover.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data([1, 2, 3]).write(to: cover)

        await store.setDownloaded(id: idA, on: true)
        XCTAssertEqual(try XCTUnwrap(store.offline[idA]), .downloaded)
        XCTAssertTrue(postsExist(idA))
        XCTAssertEqual(try readSocial(idA).likes, ["p"])
        XCTAssertEqual(client.postsCalls.count, 1)

        await store.setDownloaded(id: idA, on: true)
        XCTAssertEqual(client.postsCalls.count, 1)

        await store.setDownloaded(id: idA, on: false)
        XCTAssertEqual(try XCTUnwrap(store.offline[idA]), .notDownloaded)
        XCTAssertFalse(postsExist(idA))
        XCTAssertFalse(socialExists(idA))
        XCTAssertEqual(try Data(contentsOf: cover).count, 3)
    }

    func testAFailedDownloadLeavesNoPostsFile() async throws {
        client.likesError = offlineError()
        let store = try await open([book(idA, importedAt: 3)])
        await store.setDownloaded(id: idA, on: true)
        guard case .failed(let message) = try XCTUnwrap(store.offline[idA]) else {
            return XCTFail("state \(String(describing: store.offline[idA]))")
        }
        XCTAssertEqual(message, "offline")
        XCTAssertFalse(postsExist(idA))
    }

    func testDownloadProgressIsVisibleAndATapCancels() async throws {
        client.blockPosts = true
        client.progress = 0.4
        let store = try await open([book(idA, importedAt: 3)])
        let running = Task { await store.setDownloaded(id: idA, on: true) }
        try await waitUntil("download did not show progress") {
            if case .downloading(let fraction) = store.offline[idA] { return fraction != nil }
            return false
        }
        guard case .downloading(let fraction) = try XCTUnwrap(store.offline[idA]) else {
            return XCTFail("state \(String(describing: store.offline[idA]))")
        }
        XCTAssertEqual(fraction ?? -1, 0.4, accuracy: 0.001)

        await store.setDownloaded(id: idA, on: false)
        await running.value
        XCTAssertEqual(try XCTUnwrap(store.offline[idA]), .notDownloaded)
        XCTAssertFalse(postsExist(idA))
    }

    func testDownloadAllRunsTwoAtATimeInShelfOrder() async throws {
        client.blockPosts = true
        let books = [
            book(idA, importedAt: 40),
            book(idB, importedAt: 30),
            book(idC, importedAt: 20),
            book(idD, importedAt: 10),
        ]
        let store = try await open(books)
        let order = store.books.map(\.id)
        XCTAssertEqual(order, [idA, idB, idC, idD])
        let running = Task { await store.downloadAll() }
        try await waitUntil("first pair did not start") { client.postsCalls.count >= 2 }
        try await Task.sleep(nanoseconds: 80_000_000)
        XCTAssertEqual(Set(client.postsCalls), Set(order.prefix(2)))
        XCTAssertLessThanOrEqual(client.maxInFlight, 2)
        XCTAssertTrue(store.offlineSummary.running)
        XCTAssertEqual(store.offlineSummary.downloaded, 0)
        XCTAssertEqual(store.offlineSummary.total, 4)

        client.hold.release()
        await running.value
        XCTAssertEqual(Set(client.postsCalls), Set(order))
        XCTAssertEqual(client.postsCalls.count, 4)
        XCTAssertLessThanOrEqual(client.maxInFlight, 2)
        XCTAssertEqual(store.offlineSummary, OfflineSummary(downloaded: 4, total: 4, running: false))
        XCTAssertTrue(order.allSatisfy { postsExist($0) })
    }

    func testDownloadAllCancelsWhenTappedAgain() async throws {
        client.blockPosts = true
        let books = [
            book(idA, importedAt: 40),
            book(idB, importedAt: 30),
            book(idC, importedAt: 20),
            book(idD, importedAt: 10),
        ]
        let store = try await open(books)
        var firstDone = false
        var secondDone = false
        let first = Task { @MainActor in
            await store.downloadAll()
            firstDone = true
        }
        try await waitUntil("batch did not start") { client.entered >= 2 }
        XCTAssertTrue(store.offlineSummary.running)
        let second = Task { @MainActor in
            await store.downloadAll()
            secondDone = true
        }
        try await waitUntil("cancel did not finish") { firstDone && secondDone }
        _ = first
        _ = second
        XCTAssertEqual(client.postsCalls.count, 2)
        XCTAssertFalse(store.offlineSummary.running)
        XCTAssertEqual(store.offlineSummary.downloaded, 0)
        for book in books {
            XCTAssertFalse(postsExist(book.id))
        }
    }

    func testSwitchingAccountsDropsSocialCacheAndKeepsPosts() async throws {
        writePosts(idA, "secret")
        client.shelf = [book(idA, importedAt: 3)]
        let store = makeStore()
        self.store = store
        sessions.setSignedIn(username: "ada", token: "token-a")
        try await waitUntil("Ada's shelf did not load") {
            settled(store, ids: [idA])
        }
        _ = try await store.likes(id: idA)
        XCTAssertTrue(socialExists(idA))

        client.shelf = [book(idB, importedAt: 1)]
        sessions.setSignedIn(username: "bea", token: "token-b")
        try await waitUntil("Bea's shelf did not load") { settled(store, ids: [idB]) }
        XCTAssertFalse(socialExists(idA))
        XCTAssertTrue(postsExist(idA))
        do {
            _ = try await store.feed(id: idA)
            XCTFail("Ada's download must not open on Bea's shelf")
        } catch {
            XCTAssertEqual(error.localizedDescription, "That book is not on your shelf.")
        }
    }

    func testACachedShelfCountsDownloadsWhenRefreshFails() async throws {
        client.libraryError = offlineError()
        writePosts(idA, "secret")
        try shelfCache().write(serverUrl: server, username: "ada", books: [book(idA, importedAt: 3)])
        let store = makeStore()
        self.store = store
        sessions.setSignedIn(username: "ada", token: "token-a")
        try await waitUntil("refresh did not fail") {
            if case .failed = store.loadState { return true }
            return false
        }
        XCTAssertEqual(try XCTUnwrap(store.offline[idA]), .downloaded)
        XCTAssertEqual(store.offlineSummary, OfflineSummary(downloaded: 1, total: 1, running: false))
    }

    func testSavingProgressDoesNotRereadPosts() async throws {
        writePosts(idA, "secret")
        let store = try await open([book(idA, importedAt: 3)])
        XCTAssertEqual(HostedDiskLog.shared.decodeCount, 0)

        for index in 0..<6 {
            try await store.saveProgress(id: idA, index: index)
        }
        XCTAssertEqual(HostedDiskLog.shared.decodeCount, 0)
    }

    func testLeavingTheCallerDoesNotCancelTheDownload() async throws {
        client.blockPosts = true
        let store = try await open([book(idA, importedAt: 3)])
        var returned = false
        let caller = Task { @MainActor in
            await store.setDownloaded(id: idA, on: true)
            returned = true
        }
        try await waitUntil("download did not start") { client.entered >= 1 }
        caller.cancel()
        try await waitUntil("leaving the caller cancelled the wait") { returned }
        client.hold.release()
        try await waitUntil("download did not finish") { isDownloaded(store.offline[idA]) }
        XCTAssertTrue(postsExist(idA))
        caller.cancel()
    }

    func testAFailedDownloadClearsOnceThePostsExist() async throws {
        client.likesError = offlineError()
        let store = try await open([book(idA, importedAt: 3)])
        await store.setDownloaded(id: idA, on: true)
        XCTAssertTrue(isFailed(store.offline[idA]))

        client.likesError = nil
        let opened = try await store.feed(id: idA)
        XCTAssertEqual(opened.map(\.text), ["secret"])
        XCTAssertEqual(try XCTUnwrap(store.offline[idA]), .downloaded)

        client.likesError = offlineError()
        await store.setDownloaded(id: idA, on: false)
        await store.setDownloaded(id: idA, on: true)
        XCTAssertTrue(isFailed(store.offline[idA]))
        writePosts(idA, "secret")
        client.likesError = nil
        await store.setDownloaded(id: idA, on: true)
        XCTAssertEqual(try XCTUnwrap(store.offline[idA]), .downloaded)
        XCTAssertEqual(client.postsCalls.count, 3)
    }

    func testABookThatLeavesTheShelfDropsSocialCache() async throws {
        writePosts(idB, "secret")
        try JSONFiles.write(SocialCache(likes: ["p"]), to: bookDir(idB).appendingPathComponent("social.json"))
        let store = try await open([book(idA, importedAt: 3), book(idB, importedAt: 2)])
        client.shelf = [book(idA, importedAt: 3)]
        try await store.load()
        try await waitUntil("the book stayed on the shelf") {
            !store.books.contains { $0.id == idB } && !store.loadState.isRefreshing
        }
        XCTAssertFalse(postsExist(idB))
        XCTAssertFalse(socialExists(idB))
    }

    func testSignOutDuringADownloadLeavesTheCacheEmpty() async throws {
        client.blockPosts = true
        client.blockIndex = true
        client.shelf = [book(idA, importedAt: 3)]
        let store = makeStore()
        self.store = store
        sessions.setSignedIn(username: "ada", token: "token-a")
        try await waitUntil("shelf did not load") { settled(store, ids: [idA]) }
        let marker = hostedRoot().appendingPathComponent("marker")
        try Data("x".utf8).write(to: marker)
        var downloadDone = false
        let downloading = Task { @MainActor in
            await store.setDownloaded(id: idA, on: true)
            downloadDone = true
        }
        try await waitUntil("download did not start") { client.entered >= 1 }
        var indexDone = false
        let reading = Task { @MainActor in
            _ = try? await store.commentIndex(id: idA)
            indexDone = true
        }
        try await waitUntil("index did not start") { client.indexEntered >= 1 }

        sessions.signOut()
        try await waitUntil("wipe did not finish") { !FileManager.default.fileExists(atPath: marker.path) }
        client.indexHold.release()
        try await waitUntil("index did not finish") { indexDone }
        try await waitUntil("download did not finish") { downloadDone }
        _ = downloading
        _ = reading
        XCTAssertEqual(hostedFileNames(), [])
    }

    func testASecondDownloadAllLeavesTheRowDownloadRunning() async throws {
        client.blockPosts = true
        let store = try await open([book(idA, importedAt: 3), book(idB, importedAt: 2)])
        let row = Task { await store.setDownloaded(id: idA, on: true) }
        try await waitUntil("row download did not start") { client.entered >= 1 }
        let batch = Task { await store.downloadAll() }
        try await waitUntil("batch did not start the other book") { client.entered >= 2 }

        var cancelDone = false
        let cancel = Task { @MainActor in
            await store.downloadAll()
            cancelDone = true
        }
        try await waitUntil("cancel did not finish") { cancelDone }
        client.hold.release()
        await row.value
        await batch.value
        _ = cancel
        XCTAssertEqual(try XCTUnwrap(store.offline[idA]), .downloaded)
        XCTAssertTrue(postsExist(idA))
        XCTAssertFalse(postsExist(idB))
    }

    func testCancellingDownloadAllJoinsTheBookJobsItStarted() async throws {
        client.blockPosts = true
        client.lingerAfterInterrupt = true
        let store = try await open([book(idA, importedAt: 3)])
        let batch = Task { await store.downloadAll() }
        try await waitUntil("batch did not start") { client.entered >= 1 }
        var cancelDone = false
        let cancel = Task { @MainActor in
            await store.downloadAll()
            cancelDone = true
        }
        try await Task.sleep(nanoseconds: 400_000_000)
        XCTAssertFalse(cancelDone, "cancel returned before the book job it started had finished")
        client.lingerHold.release()
        try await waitUntil("cancel did not join the book job") { cancelDone }
        await batch.value
        _ = cancel
    }

    func testACancelledDownloadAllDoesNotLeaveItsDownloadRunning() async throws {
        client.blockPosts = true
        let store = try await open([book(idA, importedAt: 3)])
        let release = QuietPark()
        var enteredClaim = false
        HostedDisk.beforeClaim = {
            enteredClaim = true
            await release.wait()
        }
        defer {
            HostedDisk.beforeClaim = nil
            release.release()
        }
        let batch = Task { await store.downloadAll() }
        try await waitUntil("worker did not reach the claim gap") { enteredClaim }
        var cancelDone = false
        let cancel = Task { @MainActor in
            await store.downloadAll()
            cancelDone = true
        }
        try await Task.sleep(nanoseconds: 100_000_000)
        release.release()
        try await waitUntil("cancel did not finish") { cancelDone }
        await batch.value
        _ = cancel
        XCTAssertFalse(store.offline.values.contains { if case .downloading = $0 { return true }; return false })
        XCTAssertFalse(store.offlineSummary.running)
        client.hold.release()
        try await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertFalse(postsExist(idA))
        XCTAssertFalse(isDownloaded(store.offline[idA]))
    }

    func testCancellingDownloadAllLeavesARowDownloadOfAFinishedBook() async throws {
        client.blockIds = [idB]
        let store = try await open([book(idA, importedAt: 3), book(idB, importedAt: 2)])
        let batch = Task { await store.downloadAll() }
        try await waitUntil("first book did not finish") { isDownloaded(store.offline[idA]) }
        try await waitUntil("second book did not start") { client.entered >= 1 }

        await store.setDownloaded(id: idA, on: false)
        client.blockIds = [idA, idB]
        let row = Task { await store.setDownloaded(id: idA, on: true) }
        try await waitUntil("row download did not start") { client.postsCalls.filter { $0 == idA }.count >= 2 }
        var cancelDone = false
        let cancel = Task { @MainActor in
            await store.downloadAll()
            cancelDone = true
        }
        try await waitUntil("cancel did not finish") { cancelDone }
        client.hold.release()
        await row.value
        await batch.value
        _ = cancel
        XCTAssertEqual(try XCTUnwrap(store.offline[idA]), .downloaded)
        XCTAssertTrue(postsExist(idA))
        XCTAssertFalse(postsExist(idB))
    }

    func testARetryDuringCancelStartsAFreshDownload() async throws {
        client.blockPosts = true
        client.lingerAfterInterrupt = true
        let store = try await open([book(idA, importedAt: 3)])
        let first = Task { await store.setDownloaded(id: idA, on: true) }
        try await waitUntil("download did not start") { client.entered >= 1 }
        var stopped = false
        let stopping = Task { @MainActor in
            await store.setDownloaded(id: idA, on: false)
            stopped = true
        }
        try await waitUntil("cancel did not reach the book job") { client.lingerEntered >= 1 }
        let retry = Task { await store.setDownloaded(id: idA, on: true) }
        client.lingerHold.release()
        try await waitUntil("remove did not finish") { stopped }
        try await waitUntil("retry did not start") { client.postsCalls.count >= 2 }
        client.hold.release()
        await retry.value
        await first.value
        _ = stopping
        XCTAssertEqual(try XCTUnwrap(store.offline[idA]), .downloaded)
        XCTAssertTrue(postsExist(idA))
    }

    func testTwoTapsShareOneDownload() async throws {
        client.blockPosts = true
        let store = try await open([book(idA, importedAt: 3)])
        for _ in 0..<20 {
            client.hold.reset()
            client.resetCalls()
            let taps = (0..<6).map { _ in Task { await store.setDownloaded(id: idA, on: true) } }
            try await waitUntil("download did not start") { client.entered >= 1 }
            try await Task.sleep(nanoseconds: 30_000_000)
            XCTAssertEqual(client.postsCalls.count, 1)
            client.hold.release()
            for tap in taps { await tap.value }
            await store.setDownloaded(id: idA, on: false)
        }
    }

    func testShelfFileWorkStaysOffTheMainThread() async throws {
        writePosts(idA, "secret")
        writePosts(idB, "other")
        client.shelf = [book(idA, importedAt: 3), book(idB, importedAt: 2)]
        let store = makeStore()
        self.store = store
        sessions.setSignedIn(username: "ada", token: "token-a")
        try await waitUntil("shelf did not load") { settled(store, ids: [idA, idB]) }

        HostedDiskLog.shared.reset()
        try await store.saveProgress(id: idA, index: 4)
        assertDiskOffMain()

        client.removeFromShelf(idB)
        HostedDiskLog.shared.reset()
        try await store.load()
        try await waitUntil("dropped book stayed") {
            !store.books.contains { $0.id == idB } && !store.loadState.isRefreshing
        }
        assertDiskOffMain()

        try JSONFiles.write(SocialCache(likes: ["p"]), to: bookDir(idA).appendingPathComponent("social.json"))
        HostedDiskLog.shared.reset()
        sessions.setSignedIn(username: "bea", token: "token-b")
        try await waitUntil("account switch did not finish") {
            !socialExists(idA) && settled(store, ids: [idA])
        }
        assertDiskOffMain()

        HostedDiskLog.shared.reset()
        sessions.signOut()
        try await waitUntil("sign-out left hosted files") {
            if case .loaded(_, false) = store.loadState { return hostedFileNames().isEmpty }
            return false
        }
        assertDiskOffMain()
    }

    func testSignOutDuringFeedLeavesTheCacheEmpty() async throws {
        client.blockPosts = true
        client.shelf = [book(idA, importedAt: 3)]
        let store = makeStore()
        self.store = store
        sessions.setSignedIn(username: "ada", token: "token-a")
        try await waitUntil("shelf did not load") { settled(store, ids: [idA]) }
        var readDone = false
        let reading = Task { @MainActor in
            _ = try? await store.feed(id: idA)
            readDone = true
        }
        try await waitUntil("feed did not start") { client.entered >= 1 }
        let marker = hostedRoot().appendingPathComponent("marker")
        try Data("x".utf8).write(to: marker)

        sessions.signOut()
        try await waitUntil("wipe did not finish") { !FileManager.default.fileExists(atPath: marker.path) }
        client.hold.release()
        try await waitUntil("feed did not finish") { readDone }
        _ = reading
        XCTAssertEqual(hostedFileNames(), [])
    }

    func testAnOldPostsCacheIsParsedOnce() async throws {
        writePosts(idA, "old", version: 2)
        let store = try await open([book(idA, importedAt: 3)])
        XCTAssertEqual(HostedDiskLog.shared.decodeCount, 0)
        XCTAssertFalse(isDownloaded(store.offline[idA]))

        for index in 0..<6 {
            try await store.saveProgress(id: idA, index: index)
        }
        XCTAssertEqual(HostedDiskLog.shared.decodeCount, 0)

        try FileManager.default.removeItem(at: bookDir(idA).appendingPathComponent("posts.json"))
        try await store.saveProgress(id: idA, index: 8)
        XCTAssertEqual(HostedDiskLog.shared.decodeCount, 0)
        writePosts(idA, "new", version: 1)
        try await store.saveProgress(id: idA, index: 9)
        XCTAssertEqual(HostedDiskLog.shared.decodeCount, 0)
        XCTAssertEqual(try XCTUnwrap(store.offline[idA]), .downloaded)
        let posts = try await store.feed(id: idA)
        XCTAssertEqual(posts.map(\.text), ["new"])
        XCTAssertEqual(HostedDiskLog.shared.decodeCount, 1)
    }

    func testTwoDownloadsFinishDownloaded() async throws {
        let store = try await open([book(idA, importedAt: 3), book(idB, importedAt: 2)])
        for _ in 0..<20 {
            client.blockPosts = true
            client.hold.reset()
            client.resetCalls()
            let first = Task { await store.setDownloaded(id: idA, on: true) }
            let second = Task { await store.setDownloaded(id: idB, on: true) }
            try await waitUntil("both downloads did not start") { client.entered >= 2 }
            client.hold.release()
            await first.value
            await second.value
            XCTAssertEqual(try XCTUnwrap(store.offline[idA]), .downloaded)
            XCTAssertEqual(try XCTUnwrap(store.offline[idB]), .downloaded)
            XCTAssertEqual(store.offlineSummary, OfflineSummary(downloaded: 2, total: 2, running: false))
            await store.setDownloaded(id: idA, on: false)
            await store.setDownloaded(id: idB, on: false)
        }
    }

    func testToggleLikeIsStillThereOffline() async throws {
        try JSONFiles.write(SocialCache(likes: []), to: bookDir(idA).appendingPathComponent("social.json"))
        let store = try await open([book(idA, importedAt: 3)])
        client.likes = ["p"]
        let toggled = try await store.toggleLike(id: idA, postId: "p")
        XCTAssertEqual(toggled, ["p"])
        client.likesError = offlineError()
        let offline = try await store.likes(id: idA)
        XCTAssertEqual(offline, ["p"])
    }

    func testAReplyIsStillThereOffline() async throws {
        try JSONFiles.write(SocialCache(), to: bookDir(idA).appendingPathComponent("social.json"))
        let store = try await open([book(idA, importedAt: 3)])
        let thread = try await store.addComment(id: idA, postId: "p", text: "hello")
        XCTAssertEqual(thread.map(\.text), ["hello"])
        client.commentsError = offlineError()
        let offline = try await store.comments(id: idA, postId: "p")
        XCTAssertEqual(offline.map(\.text), ["hello"])
        XCTAssertEqual(try readSocial(idA).index.count("p"), 1)
    }

    func testADeletedReplyIsStillThereOffline() async throws {
        let kept = PostComment(id: "c", username: "ada", text: "kept", createdAt: 1, mine: true)
        try JSONFiles.write(
            SocialCache(threads: ["p": [kept, PostComment(id: "d", username: "ada", text: "gone", createdAt: 2, mine: true)]]),
            to: bookDir(idA).appendingPathComponent("social.json")
        )
        let store = try await open([book(idA, importedAt: 3)])
        client.deletedThread = [kept]
        let thread = try await store.deleteComment(id: idA, postId: "p", commentId: "d")
        XCTAssertEqual(thread.map(\.id), ["c"])
        client.commentsError = offlineError()
        let offline = try await store.comments(id: idA, postId: "p")
        XCTAssertEqual(offline.map(\.id), ["c"])
    }

    func testAFeedThatFinishesAfterTheBookLeavesDoesNotWritePosts() async throws {
        client.blockPosts = true
        let store = try await open([book(idA, importedAt: 3), book(idB, importedAt: 2)])
        var readDone = false
        let reading = Task { @MainActor in
            _ = try? await store.feed(id: idA)
            readDone = true
        }
        try await waitUntil("feed did not start") { client.entered >= 1 }
        client.shelf = [book(idB, importedAt: 2)]
        try await store.load()
        try await waitUntil("the book stayed on the shelf") {
            !store.books.contains { $0.id == idA } && !store.loadState.isRefreshing
        }
        client.hold.release()
        try await waitUntil("feed did not finish") { readDone }
        _ = reading
        XCTAssertFalse(postsExist(idA))
    }

    func testLikesThatFinishAfterTheBookLeavesDoNotWriteSocial() async throws {
        client.blockLikes = true
        let store = try await open([book(idA, importedAt: 3), book(idB, importedAt: 2)])
        var readDone = false
        let reading = Task { @MainActor in
            _ = try? await store.likes(id: idA)
            readDone = true
        }
        try await waitUntil("likes did not start") { client.likesEntered >= 1 }
        client.shelf = [book(idB, importedAt: 2)]
        try await store.load()
        try await waitUntil("the book stayed on the shelf") {
            !store.books.contains { $0.id == idA } && !store.loadState.isRefreshing
        }
        client.likesHold.release()
        try await waitUntil("likes did not finish") { readDone }
        _ = reading
        XCTAssertFalse(socialExists(idA))
    }

    func testRepeatedProgressPublishesOnce() async throws {
        let store = try await open([book(idA, importedAt: 3)])
        let generation = store.beginLive(idA)
        HostedDisk.recordPublishes = true
        HostedDisk.resetPublishes()
        defer { HostedDisk.recordPublishes = false }
        for _ in 0..<1000 {
            store.reportProgress(idA, fraction: nil, generation: generation)
        }
        try await waitUntil("progress did not publish") { HostedDisk.publishCount >= 1 }
        try await Task.sleep(nanoseconds: 200_000_000)
        XCTAssertEqual(HostedDisk.publishCount, 1)
    }

    func testAShelfScanDoesNotBlockOpeningAnotherBook() async throws {
        writePosts(idA, String(repeating: "a", count: 200_000))
        writePosts(idB, "ready")
        let gate = DispatchSemaphore(value: 0)
        let scanning = Flag()
        HostedDisk.beforeScan = {
            HostedDisk.beforeScan = nil
            scanning.set()
            gate.wait()
        }
        defer {
            HostedDisk.beforeScan = nil
            gate.signal()
        }
        client.shelf = [book(idA, importedAt: 3), book(idB, importedAt: 2)]
        let store = makeStore()
        self.store = store
        sessions.setSignedIn(username: "ada", token: "token-a")
        try await waitUntil("scan did not start") { scanning.isSet }
        let posts = try await store.feed(id: idB)
        XCTAssertEqual(posts.map(\.text), ["ready"])
        gate.signal()
        try await waitUntil("shelf did not settle") { settled(store, ids: [idA, idB]) }
    }

    func testSignInDuringSignOutKeepsTheNewDownload() async throws {
        client.blockPosts = true
        client.lingerAfterInterrupt = true
        client.shelf = [book(idA, importedAt: 3)]
        let store = makeStore()
        self.store = store
        sessions.setSignedIn(username: "ada", token: "token-a")
        try await waitUntil("shelf did not load") { settled(store, ids: [idA]) }
        let downloading = Task { await store.setDownloaded(id: idA, on: true) }
        try await waitUntil("download did not start") { client.entered >= 1 }
        sessions.signOut()
        try await waitUntil("cancel did not reach the book job") { client.lingerEntered >= 1 }
        sessions.setSignedIn(username: "ada", token: "token-b")
        let cover = bookDir(idA).appendingPathComponent("cover.jpg")
        try FileManager.default.createDirectory(at: cover.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data("cover".utf8).write(to: cover)
        client.lingerHold.release()
        client.hold.release()
        await downloading.value
        try await waitUntil("sign-in did not settle") { settled(store, ids: [idA]) }
        XCTAssertTrue(FileManager.default.fileExists(atPath: cover.path))
    }

    func testACancelledLikeDoesNotReadTheCache() async throws {
        try JSONFiles.write(SocialCache(likes: ["stale"]), to: bookDir(idA).appendingPathComponent("social.json"))
        let store = try await open([book(idA, importedAt: 3)])
        client.likesError = URLError(.cancelled)
        do {
            _ = try await store.likes(id: idA)
            XCTFail("cancelled like returned the cache")
        } catch let error as URLError {
            XCTAssertEqual(error.code, .cancelled)
        }
        client.likesError = nil
        client.commentsError = URLError(.cancelled)
        do {
            _ = try await store.comments(id: idA, postId: "p")
            XCTFail("cancelled comments returned the cache")
        } catch let error as URLError {
            XCTAssertEqual(error.code, .cancelled)
        }
    }

    private func open(_ books: [BookRecord]) async throws -> HostedLibraryStore {
        client.shelf = books
        let store = makeStore()
        self.store = store
        sessions.setSignedIn(username: "ada", token: "token-a")
        let want = Set(books.map(\.id))
        try await waitUntil("shelf did not settle") { settled(store, ids: want) }
        return store
    }

    private func makeStore() -> HostedLibraryStore {
        HostedLibraryStore(opener: EpubOpener(), sessions: sessions, client: client.client, files: root)
    }

    private func settled(_ store: HostedLibraryStore, ids: Set<String>) -> Bool {
        guard case .loaded(let rows, false) = store.loadState else { return false }
        return Set(rows.map(\.id)) == ids
    }

    private func assertDiskOffMain() {
        let log = HostedDiskLog.shared
        XCTAssertGreaterThan(log.touches, 0)
        XCTAssertFalse(log.touchedMain)
    }

    private func writePosts(_ id: String, _ text: String, version: Int = 1) {
        let dir = bookDir(id)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try? JSONFiles.write(
            FeedCache(version: version, posts: [FeedPost(id: "p", kind: .body, text: text, chapter: "1")]),
            to: dir.appendingPathComponent("posts.json")
        )
    }

    private func writeSocialRaw(_ id: String, _ text: String) throws {
        let dir = bookDir(id)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try Data(text.utf8).write(to: dir.appendingPathComponent("social.json"))
    }

    private func readSocial(_ id: String) throws -> SocialCache {
        let url = bookDir(id).appendingPathComponent("social.json")
        return try XCTUnwrap(JSONFiles.read(SocialCache.self, from: url))
    }

    private func readPostsVersion(_ id: String) throws -> Int {
        let url = bookDir(id).appendingPathComponent("posts.json")
        return try XCTUnwrap(JSONFiles.read(FeedCache.self, from: url)).version
    }

    private func bookDir(_ id: String) -> URL {
        hostedRoot().appendingPathComponent(id, isDirectory: true)
    }

    private func hostedRoot() -> URL {
        root.appendingPathComponent("hosted", isDirectory: true)
    }

    private func postsExist(_ id: String) -> Bool {
        FileManager.default.fileExists(atPath: bookDir(id).appendingPathComponent("posts.json").path)
    }

    private func socialExists(_ id: String) -> Bool {
        FileManager.default.fileExists(atPath: bookDir(id).appendingPathComponent("social.json").path)
    }

    private func hostedFileNames() -> [String] {
        guard let enumerator = FileManager.default.enumerator(at: hostedRoot(), includingPropertiesForKeys: [.isRegularFileKey]) else {
            return []
        }
        var names: [String] = []
        for case let url as URL in enumerator {
            if (try? url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true {
                names.append(url.lastPathComponent)
            }
        }
        return names.sorted()
    }

    private func shelfCache() -> ShelfCache {
        ShelfCache(directory: root.appendingPathComponent("hosted-shelves", isDirectory: true))
    }

    private func book(_ id: String, importedAt: Int64) -> BookRecord {
        BookRecord(
            id: id,
            title: id,
            author: "Austen",
            handle: "austen",
            importedAt: importedAt,
            postCount: 1,
            mine: true,
            shared: false,
            ownerUsername: "ada"
        )
    }

    private func isFailed(_ state: OfflineState?) -> Bool {
        if case .failed = state { return true }
        return false
    }

    private func isDownloaded(_ state: OfflineState?) -> Bool {
        if case .downloaded = state { return true }
        return false
    }
}

private final class OfflineClient: @unchecked Sendable {
    let client = HostedClient()
    private let lock = NSLock()
    private var books: [BookRecord] = []
    var libraryError: Error?
    var postsError: Error?
    var likesError: Error?
    var indexError: Error?
    var commentsError: Error?
    var likes: Set<String> = ["p"]
    var index = CommentIndex(counts: ["p": 1])
    var thread = [PostComment(id: "c", username: "ada", text: "hi", createdAt: 1, mine: true)]
    var blockPosts = false
    var blockIds: Set<String> = []
    var progress: Double? = 0.4
    var lingerAfterInterrupt = false
    var blockIndex = false
    var blockLikes = false
    var deletedThread: [PostComment] = []
    let hold = TestPark()
    let lingerHold = QuietPark()
    let indexHold = TestPark()
    let likesHold = TestPark()
    private let libraryHold = BooksGate()
    private var libraryGated = false
    private(set) var libraryArrived = false
    private var calls: [String] = []
    private var enteredCount = 0
    private var peak = 0
    private var lingerCount = 0
    private var indexCount = 0
    private var likesCount = 0

    init() {
        client.hooks.library = { [weak self] _ in
            guard let self else { throw CancellationError() }
            if let libraryError = self.libraryError { throw libraryError }
            if self.libraryGated {
                self.libraryArrived = true
                let books = await self.libraryHold.wait()
                return LibrarySnapshot(books: books)
            }
            return LibrarySnapshot(books: self.shelf)
        }
        client.hooks.postsProgress = { [weak self] _, id, onProgress in
            guard let self else { throw CancellationError() }
            self.noteEnter(id)
            onProgress(self.progress)
            if self.shouldBlock(id) {
                do {
                    try await self.hold.wait()
                } catch {
                    if self.lingerAfterInterrupt {
                        self.noteLinger()
                        await self.lingerHold.wait()
                    }
                    throw error
                }
            }
            self.noteLeave()
            if let postsError = self.postsError { throw postsError }
            return [FeedPost(id: "p", kind: .body, text: "secret", chapter: "1")]
        }
        client.hooks.downloadCover = { _, _, _ in false }
        client.hooks.likes = { [weak self] _, _ in
            guard let self else { throw CancellationError() }
            if self.blockLikes {
                self.noteLikes()
                try await self.likesHold.wait()
            }
            if let likesError = self.likesError { throw likesError }
            return self.likes
        }
        client.hooks.toggleLike = { [weak self] _, _, _ in
            guard let self else { throw CancellationError() }
            return self.likes
        }
        client.hooks.addComment = { [weak self] _, _, _, text in
            guard let self else { throw CancellationError() }
            let thread = [PostComment(id: "n", username: "ada", text: text, createdAt: 2, mine: true)]
            self.thread = thread
            return thread
        }
        client.hooks.deleteComment = { [weak self] _, _, _, _ in
            guard let self else { throw CancellationError() }
            return self.deletedThread
        }
        client.hooks.commentIndex = { [weak self] _, _ in
            guard let self else { throw CancellationError() }
            if self.blockIndex {
                self.noteIndex()
                try await self.indexHold.wait()
            }
            if let indexError = self.indexError { throw indexError }
            return self.index
        }
        client.hooks.comments = { [weak self] _, _, _ in
            guard let self else { throw CancellationError() }
            if let commentsError = self.commentsError { throw commentsError }
            return self.thread
        }
        client.hooks.patch = { [weak self] _, id, body in
            guard let self else { throw CancellationError() }
            guard var book = self.book(id) else { throw HostedException(message: "missing") }
            if let index = body.progressIndex { book.progressIndex = index }
            if let shared = body.shared { book.shared = shared }
            self.replace(book)
            return book
        }
    }

    var shelf: [BookRecord] {
        get { lock.lock(); defer { lock.unlock() }; return books }
        set { lock.lock(); books = newValue; lock.unlock() }
    }

    func removeFromShelf(_ id: String) {
        lock.lock()
        books.removeAll { $0.id == id }
        lock.unlock()
    }

    var postsCalls: [String] {
        lock.lock(); defer { lock.unlock() }; return calls
    }

    var entered: Int {
        lock.lock(); defer { lock.unlock() }; return enteredCount
    }

    var maxInFlight: Int {
        lock.lock(); defer { lock.unlock() }; return peak
    }

    var lingerEntered: Int {
        lock.lock(); defer { lock.unlock() }; return lingerCount
    }

    var indexEntered: Int {
        lock.lock(); defer { lock.unlock() }; return indexCount
    }

    var likesEntered: Int {
        lock.lock(); defer { lock.unlock() }; return likesCount
    }

    func resetCalls() {
        lock.lock()
        calls.removeAll()
        enteredCount = 0
        peak = 0
        lingerCount = 0
        lock.unlock()
    }

    func armLibraryGate() {
        libraryGated = true
    }

    func completeLibrary(_ books: [BookRecord]) {
        libraryHold.complete(books)
    }

    func releaseAll() {
        hold.release()
        lingerHold.release()
        indexHold.release()
        likesHold.release()
        libraryHold.complete([])
    }

    private func book(_ id: String) -> BookRecord? {
        lock.lock(); defer { lock.unlock() }
        return books.first { $0.id == id }
    }

    private func replace(_ book: BookRecord) {
        lock.lock()
        if let index = books.firstIndex(where: { $0.id == book.id }) {
            books[index] = book
        }
        lock.unlock()
    }

    private func shouldBlock(_ id: String) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return blockPosts || blockIds.contains(id)
    }

    private func noteEnter(_ id: String) {
        lock.lock()
        calls.append(id)
        enteredCount += 1
        peak = max(peak, enteredCount)
        lock.unlock()
    }

    private func noteLeave() {
        lock.lock()
        enteredCount = max(0, enteredCount - 1)
        lock.unlock()
    }

    private func noteLinger() {
        lock.lock()
        lingerCount += 1
        lock.unlock()
    }

    private func noteIndex() {
        lock.lock()
        indexCount += 1
        lock.unlock()
    }

    private func noteLikes() {
        lock.lock()
        likesCount += 1
        lock.unlock()
    }
}

private final class BooksGate: @unchecked Sendable {
    private let lock = NSLock()
    private var waiters: [CheckedContinuation<[BookRecord], Never>] = []
    private var books: [BookRecord]?

    func wait() async -> [BookRecord] {
        await withCheckedContinuation { cont in
            lock.lock()
            if let books {
                lock.unlock()
                cont.resume(returning: books)
                return
            }
            waiters.append(cont)
            lock.unlock()
        }
    }

    func complete(_ books: [BookRecord]) {
        lock.lock()
        self.books = books
        let pending = waiters
        waiters.removeAll()
        lock.unlock()
        pending.forEach { $0.resume(returning: books) }
    }
}

/// A gate a download can wait on. Cancelling the waiting task resumes that waiter with an error.
final class TestPark: @unchecked Sendable {
    private let lock = NSLock()
    private var slots: [ParkSlot] = []
    private var open = false

    func wait() async throws {
        let slot = ParkSlot()
        lock.lock()
        if open {
            lock.unlock()
            return
        }
        slots.append(slot)
        lock.unlock()
        try await withTaskCancellationHandler {
            try Task.checkCancellation()
            try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
                slot.arm(cont)
            }
        } onCancel: {
            slot.cancel()
        }
    }

    func release() {
        lock.lock()
        open = true
        let pending = slots
        slots.removeAll()
        lock.unlock()
        pending.forEach { $0.succeed() }
    }

    func reset() {
        lock.lock()
        open = false
        slots.removeAll()
        lock.unlock()
    }
}

private final class ParkSlot: @unchecked Sendable {
    private let lock = NSLock()
    private var cont: CheckedContinuation<Void, Error>?
    private var outcome: Result<Void, Error>?

    func arm(_ cont: CheckedContinuation<Void, Error>) {
        lock.lock()
        if let outcome {
            lock.unlock()
            cont.resume(with: outcome)
            return
        }
        self.cont = cont
        lock.unlock()
    }

    func succeed() { finish(.success(())) }
    func cancel() { finish(.failure(CancellationError())) }

    private func finish(_ result: Result<Void, Error>) {
        lock.lock()
        if outcome != nil {
            lock.unlock()
            return
        }
        outcome = result
        let cont = self.cont
        self.cont = nil
        lock.unlock()
        cont?.resume(with: result)
    }
}

/// Waits out a cancelled download. A second cancel must not skip the linger.
final class QuietPark: @unchecked Sendable {
    private let lock = NSLock()
    private var waiters: [CheckedContinuation<Void, Never>] = []
    private var open = false

    func wait() async {
        await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
            lock.lock()
            if open {
                lock.unlock()
                cont.resume()
                return
            }
            waiters.append(cont)
            lock.unlock()
        }
    }

    func release() {
        lock.lock()
        open = true
        let pending = waiters
        waiters.removeAll()
        lock.unlock()
        pending.forEach { $0.resume() }
    }
}

/// Records which threads touched hosted files. Installed on `HostedDisk` by the tests.
final class HostedDiskLog: @unchecked Sendable {
    static let shared = HostedDiskLog()
    private let lock = NSLock()
    private var flags: [Bool] = []
    private var decodes = 0

    func note() {
        lock.lock()
        flags.append(Thread.isMainThread)
        lock.unlock()
    }

    func noteDecode() {
        lock.lock()
        decodes += 1
        lock.unlock()
    }

    func reset() {
        lock.lock()
        flags.removeAll()
        decodes = 0
        lock.unlock()
    }

    var decodeCount: Int {
        lock.lock()
        defer { lock.unlock() }
        return decodes
    }

    var touchedMain: Bool {
        lock.lock()
        defer { lock.unlock() }
        return flags.contains(true)
    }

    var touches: Int {
        lock.lock()
        defer { lock.unlock() }
        return flags.count
    }
}

private final class Flag: @unchecked Sendable {
    private let lock = NSLock()
    private var value = false
    func set() { lock.lock(); value = true; lock.unlock() }
    var isSet: Bool { lock.lock(); defer { lock.unlock() }; return value }
}

func offlineError(_ message: String = "offline") -> Error {
    NSError(
        domain: NSURLErrorDomain,
        code: URLError.Code.networkConnectionLost.rawValue,
        userInfo: [NSLocalizedDescriptionKey: message]
    )
}

@MainActor
func waitUntil(_ message: String, _ condition: () -> Bool) async throws {
    let deadline = Date().addingTimeInterval(5)
    while !condition() {
        if Date() > deadline {
            XCTFail(message)
            throw CancellationError()
        }
        try await Task.sleep(nanoseconds: 20_000_000)
    }
}

private let server = "https://mewo.example"
private let idA = "11111111-1111-1111-1111-111111111111"
private let idB = "22222222-2222-2222-2222-222222222222"
private let idC = "33333333-3333-3333-3333-333333333333"
private let idD = "44444444-4444-4444-4444-444444444444"
