import XCTest
@testable import Mewo

final class SharingTests: XCTestCase {
    func testDecodesSharingFields() throws {
        let book = try decode("""
        {"id":"1","title":"T","author":"A","handle":"a","importedAt":5,"postCount":3,"progressIndex":1,"isSample":false,"mine":false,"shared":true,"ownerUsername":"ada"}
        """)
        XCTAssertFalse(book.mine)
        XCTAssertTrue(book.shared)
        XCTAssertEqual(book.ownerUsername, "ada")
        XCTAssertEqual(book.postCount, 3)
        XCTAssertEqual(book.progressIndex, 1)
    }

    func testSharingFieldsDefaultWhenMissingOrNull() throws {
        let missing = try decode("""
        {"id":"1","title":"T","author":"A","handle":"a","importedAt":5}
        """)
        XCTAssertFalse(missing.shared)
        XCTAssertNil(missing.ownerUsername)
        XCTAssertTrue(missing.mine)
        XCTAssertEqual(missing.postCount, 0)

        let nulls = try decode("""
        {"id":"1","title":"T","author":"A","handle":"a","importedAt":5,"shared":null,"ownerUsername":null,"mine":null}
        """)
        XCTAssertFalse(nulls.shared)
        XCTAssertNil(nulls.ownerUsername)
        XCTAssertTrue(nulls.mine)
    }

    func testRoundTripOmitsNilOwner() throws {
        let book = makeBook(id: "1", shared: false, mine: true, owner: nil)
        let data = try JSONFiles.encoder.encode(book)
        let object = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        XCTAssertEqual(object?["shared"] as? Bool, false)
        XCTAssertNil(object?["ownerUsername"])
        let decoded = try JSONFiles.decoder.decode(BookRecord.self, from: data)
        XCTAssertEqual(decoded, book)
    }

    func testPatchEncodesOnlyTheFieldBeingSent() throws {
        let share = try jsonObject(PatchBook(shared: true))
        XCTAssertEqual(share["shared"] as? Bool, true)
        XCTAssertNil(share["progressIndex"])

        let progress = try jsonObject(PatchBook(progressIndex: 4))
        XCTAssertEqual(progress["progressIndex"] as? Int, 4)
        XCTAssertNil(progress["shared"])

        let unshare = try jsonObject(PatchBook(shared: false))
        XCTAssertEqual(unshare["shared"] as? Bool, false)
    }

    func testHostedShelfGroupsAndOwnerCredit() {
        let mine = makeBook(id: "mine", shared: false, mine: true, owner: "bea")
        let shared = makeBook(id: "shared", shared: true, mine: false, owner: "ada")
        let sample = makeBook(id: "sample", shared: false, mine: false, owner: "ada", sample: true)
        let done = makeBook(id: "done", shared: true, mine: false, owner: "ada", posts: 4, index: 3)
        let groups = hostedShelfGroups([done, shared, mine, sample])
        XCTAssertEqual(groups.mine.map(\.id), ["mine"])
        XCTAssertEqual(groups.sharedWithMe.map(\.id), ["shared", "sample"])
        XCTAssertEqual(groups.finished.map(\.id), ["done"])
        XCTAssertEqual(ownerCredit(shared), "Shared by @ada")
        XCTAssertEqual(ownerCredit(sample), "Added by @ada")
        XCTAssertNil(ownerCredit(mine))
        XCTAssertNil(ownerCredit(makeBook(id: "x", shared: true, mine: false, owner: "  ")))
    }

    func testFieldRulesStayHiddenUntilTheValueIsInvalid() {
        XCTAssertNil(usernameFieldError(""))
        XCTAssertNil(usernameFieldError("ada"))
        XCTAssertEqual(usernameFieldError("ab"), usernameRule)
        XCTAssertEqual(usernameFieldError("bad name"), usernameRule)
        XCTAssertNil(passwordFieldError(""))
        XCTAssertNil(passwordFieldError("12345678"))
        XCTAssertEqual(passwordFieldError("short"), passwordRule)
        XCTAssertNil(commentBudgetError("ok"))
        XCTAssertEqual(
            commentBudgetError(String(repeating: "a", count: 2001)),
            "Comment budget: asked 2001 characters, limit is 2000."
        )
        XCTAssertFalse(LocalIdentity.showsHandle(name: "Matt", handle: "matt"))
        XCTAssertTrue(LocalIdentity.showsHandle(name: "Matt", handle: "matthew"))
        XCTAssertFalse(LocalIdentity.showsHandle(name: "", handle: ""))
        XCTAssertFalse(hostedAuthReady(username: "ab", password: "12345678", baseUrl: "http://127.0.0.1:8787"))
        XCTAssertTrue(hostedAuthReady(username: "ada", password: "12345678", baseUrl: "127.0.0.1:8787"))
    }

    func testPrivateUploadNotice() {
        let book = makeBook(id: "1", title: "Notes", shared: false, mine: true, owner: "bea")
        XCTAssertEqual(
            privateUploadNotice(for: book),
            "Notes is private."
        )
        XCTAssertNil(privateUploadNotice(for: makeBook(id: "2", shared: true, mine: true, owner: "bea")))
        XCTAssertNil(privateUploadNotice(for: makeBook(id: "3", shared: false, mine: true, owner: "bea", sample: true)))
    }

    func testShareToggleOptimisticRollbackAndStaleResponse() {
        let book = makeBook(id: "1", shared: false, mine: true, owner: "bea")
        guard case .update(let on) = startShareToggle(book: book, requested: true, pending: nil) else {
            return XCTFail("expected an optimistic update")
        }
        XCTAssertFalse(on.confirmed)
        XCTAssertTrue(on.displayed)
        XCTAssertEqual(startShareToggle(book: book, requested: false, pending: nil), .ignore)

        var shown = book
        shown.shared = true
        let server = makeBook(id: "1", shared: true, mine: true, owner: "bea")
        let committed = applyShareSuccess(book: shown, requested: true, server: server)
        XCTAssertEqual(committed?.shared, true)
        XCTAssertEqual(committed?.ownerUsername, "bea")

        let rolled = applyShareFailure(book: shown, requested: true, confirmed: false)
        XCTAssertEqual(rolled?.shared, false)

        var newer = shown
        newer.shared = false
        XCTAssertNil(applyShareSuccess(book: newer, requested: true, server: server))
        XCTAssertNil(applyShareFailure(book: newer, requested: true, confirmed: false))
        XCTAssertEqual(applyShareFailure(book: newer, requested: false, confirmed: false)?.shared, false)

        let pending = ShareToggle(confirmed: false, displayed: true)
        guard case .update(let again) = startShareToggle(book: shown, requested: false, pending: pending) else {
            return XCTFail("a newer toggle in the same chain should still send")
        }
        XCTAssertFalse(again.confirmed)
        XCTAssertFalse(again.displayed)
    }

    func testShareToggleRejectsNonOwner() {
        let book = makeBook(id: "1", shared: true, mine: false, owner: "ada")
        XCTAssertEqual(startShareToggle(book: book, requested: false, pending: nil), .forbidden)
    }

    private func decode(_ json: String) throws -> BookRecord {
        try JSONFiles.decoder.decode(BookRecord.self, from: Data(json.utf8))
    }

    private func jsonObject<T: Encodable>(_ value: T) throws -> [String: Any] {
        let data = try JSONFiles.encoder.encode(value)
        return try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    private func makeBook(
        id: String,
        title: String = "T",
        shared: Bool,
        mine: Bool,
        owner: String?,
        posts: Int = 2,
        index: Int = 0,
        sample: Bool = false
    ) -> BookRecord {
        BookRecord(
            id: id, title: title, author: "A", handle: "a", importedAt: 1,
            postCount: posts, progressIndex: index, isSample: sample, mine: mine,
            shared: shared, ownerUsername: owner
        )
    }
}
