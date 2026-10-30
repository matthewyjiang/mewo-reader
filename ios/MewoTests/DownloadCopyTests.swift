import XCTest
@testable import Mewo

final class DownloadCopyTests: XCTestCase {
    func testSettingsRowNamesTheThreeStates() {
        XCTAssertEqual(downloadAllLabel(OfflineSummary(downloaded: 0, total: 12, running: false)), "Download all books")
        XCTAssertTrue(downloadAllEnabled(OfflineSummary(downloaded: 0, total: 12, running: false)))
        XCTAssertEqual(downloadAllLabel(OfflineSummary(downloaded: 3, total: 12, running: true)), "Downloading 3 of 12")
        XCTAssertTrue(downloadAllEnabled(OfflineSummary(downloaded: 3, total: 12, running: true)))
        XCTAssertEqual(downloadAllLabel(OfflineSummary(downloaded: 12, total: 12, running: false)), "All books downloaded")
        XCTAssertFalse(downloadAllEnabled(OfflineSummary(downloaded: 12, total: 12, running: false)))
        XCTAssertEqual(downloadAllLabel(OfflineSummary(downloaded: 0, total: 0, running: false)), "All books downloaded")
        XCTAssertFalse(downloadAllEnabled(OfflineSummary(downloaded: 0, total: 0, running: false)))
    }

    func testTheRingNamesAPercentOnlyWhenTheLengthIsKnown() {
        XCTAssertEqual(downloadingLabel(nil), "Downloading")
        XCTAssertEqual(downloadingLabel(0.4), "Downloading, 40%")
        XCTAssertEqual(downloadingLabel(0), "Downloading, 0%")
        XCTAssertEqual(downloadingLabel(1), "Downloading, 100%")
    }
}
