import XCTest
@testable import Mewo

final class SessionChangeTests: XCTestCase {
    private let url = "https://mewo.example"
    private let other = "https://other.example"

    private var ada: OutboxAccount { OutboxAccount(url: url, username: "ada") }

    func testA401OnTheCurrentTokenExpires() {
        let signed = HostedSession(baseUrl: url, username: "ada", token: "token-a")
        let expired = HostedSession(baseUrl: url, expired: ada)
        XCTAssertEqual(sessionChange(from: signed, to: expired), .expired)
    }

    func testExpiredAccountSignsBackInAndKeepsFiles() {
        let expired = HostedSession(baseUrl: url, expired: ada)
        let again = HostedSession(baseUrl: url, username: "ada", token: "token-b")
        XCTAssertEqual(sessionChange(from: expired, to: again), .keep)
    }

    func testExpiredAccountToAnotherUserOnTheSameServer() {
        let expired = HostedSession(baseUrl: url, expired: ada)
        let switched = HostedSession(baseUrl: url, username: "bea", token: "token-b")
        XCTAssertEqual(sessionChange(from: expired, to: switched), .otherUser(ada))
    }

    func testExpiredAccountToAnotherServerWipes() {
        let expired = HostedSession(baseUrl: url, expired: ada)
        let moved = HostedSession(baseUrl: other, username: "ada", token: "token-c")
        XCTAssertEqual(sessionChange(from: expired, to: moved), .wipe(ada))
    }

    func testExplicitSignOutWipes() {
        let signed = HostedSession(baseUrl: url, username: "ada", token: "token-a")
        XCTAssertEqual(sessionChange(from: signed, to: HostedSession(baseUrl: url)), .wipe(ada))
    }

    func testEditingTheUrlWhileExpiredKeepsFiles() {
        let expired = HostedSession(baseUrl: url, expired: ada)
        let typed = HostedSession(baseUrl: other, expired: ada)
        XCTAssertEqual(sessionChange(from: expired, to: typed), .keep)
    }

    func testSignOutWithNoUsernameWipesHostedFiles() {
        let bare = HostedSession(baseUrl: url, token: "token")
        XCTAssertEqual(sessionChange(from: bare, to: HostedSession(baseUrl: url)), .wipeHosted)
    }

    func testDiskOwnerStillDeletesWhenTheSessionWouldKeep() {
        let bea = HostedSession(baseUrl: url, username: "bea", token: "token-b")
        let owed = sessionChange(from: sessionForFiles(onDisk: ada), to: bea)
        XCTAssertEqual(pendingFileChange(.keep, owed), .otherUser(ada))
        XCTAssertEqual(pendingFileChange(.expired, owed), .otherUser(ada))
    }

    func testTokenRotationKeepsFiles() {
        let first = HostedSession(baseUrl: url, username: "ada", token: "token-a")
        let rotated = HostedSession(baseUrl: url, username: "ada", token: "token-b")
        XCTAssertEqual(sessionChange(from: first, to: rotated), .keep)
    }
}

final class HostedSessionStoreTests: XCTestCase {
    func testExpirePersistsAcrossInstancesAndSignInAndSignOutClearIt() {
        let name = "mewo-session-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defer { defaults.removePersistentDomain(forName: name) }

        let store = HostedSessionStore(defaults: defaults, persistToken: false)
        store.setBaseUrl("https://mewo.example/")
        store.setSignedIn(username: "ada", token: "token-a")
        store.expire()
        XCTAssertNil(store.session.token)
        XCTAssertNil(store.session.username)
        XCTAssertFalse(store.session.signedIn)
        XCTAssertEqual(
            store.session.expired,
            OutboxAccount(url: "https://mewo.example", username: "ada")
        )
        XCTAssertEqual(defaults.string(forKey: "expired_url"), "https://mewo.example")
        XCTAssertEqual(defaults.string(forKey: "expired_user"), "ada")
        XCTAssertNil(defaults.string(forKey: "username"))
        XCTAssertNil(defaults.string(forKey: "token"))

        let restored = HostedSessionStore(defaults: defaults, persistToken: false)
        XCTAssertEqual(restored.session.expired, store.session.expired)
        XCTAssertNil(restored.session.token)
        XCTAssertNil(restored.session.username)

        restored.setSignedIn(username: "ada", token: "token-b")
        XCTAssertNil(restored.session.expired)
        XCTAssertEqual(restored.session.token, "token-b")
        let signed = HostedSessionStore(defaults: defaults, persistToken: false)
        XCTAssertNil(signed.session.expired)
        XCTAssertEqual(signed.session.username, "ada")
        XCTAssertNil(defaults.string(forKey: "expired_url"))
        XCTAssertNil(defaults.string(forKey: "expired_user"))

        signed.signOut()
        XCTAssertNil(signed.session.expired)
        XCTAssertNil(signed.session.username)
        XCTAssertNil(signed.session.token)
        let cleared = HostedSessionStore(defaults: defaults, persistToken: false)
        XCTAssertNil(cleared.session.expired)
        XCTAssertNil(cleared.session.username)
    }

    func testExpireWithNoUsernameClearsTheSavedAccount() {
        let name = "mewo-session-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defer { defaults.removePersistentDomain(forName: name) }
        let store = HostedSessionStore(defaults: defaults, persistToken: false)
        store.setBaseUrl("https://mewo.example")
        store.setSignedIn(username: "ada", token: "token-a")
        store.expire()
        XCTAssertEqual(store.session.expired?.username, "ada")
        store.expire()
        XCTAssertNil(store.session.expired)
        XCTAssertNil(defaults.string(forKey: "expired_url"))
        XCTAssertNil(defaults.string(forKey: "expired_user"))
    }

    func testFilesOwnerSurvivesSignInExpireAndSignOut() {
        let name = "mewo-session-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defer { defaults.removePersistentDomain(forName: name) }
        let store = HostedSessionStore(defaults: defaults, persistToken: false)
        store.setBaseUrl("https://mewo.example")
        let ada = OutboxAccount(url: "https://mewo.example", username: "ada")
        store.noteFilesOnDisk(ada)
        store.setSignedIn(username: "bea", token: "token-b")
        store.expire()
        store.signOut()
        XCTAssertEqual(store.filesOnDisk, ada)
        XCTAssertEqual(defaults.string(forKey: "files_url"), ada.url)
        XCTAssertEqual(defaults.string(forKey: "files_user"), "ada")
        let restored = HostedSessionStore(defaults: defaults, persistToken: false)
        XCTAssertEqual(restored.filesOnDisk, ada)
    }
}
