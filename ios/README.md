# iPhone

Phone app. SwiftUI and the [Readium Swift toolkit](https://github.com/readium/swift-toolkit) 3.3.0 for local EPUBs. Hosted books come from the server API. See the [root README](../README.md) for what Mewo is and how versions are chosen.

## Requirements

- A Mac with full Xcode 26 or newer, and the iOS 26 SDK. Command Line Tools are not enough
- [XcodeGen](https://github.com/yonaskolb/XcodeGen) (`brew install xcodegen`) regenerates `Mewo.xcodeproj` from `project.yml`

CI runs on `macos-26`, generates the project, and tests `MewoTests` on the first available iOS 26 iPhone simulator. UI tests are not in that job.

## UI screenshot tests

`project.yml` is what XcodeGen reads. CI runs `xcodegen generate` first, and the `MewoUITests` target already globs `MewoUITests/`, which is how `CatalogScreenshotTests` gets into the scheme. The committed `Mewo.xcodeproj` is left as it is.

On a Mac, with the social fixture listening, shots land in `/tmp/mewo-catalog-shots`:

```bash
cd ios && xcodegen generate && (python3 MewoUITests/Fixtures/social-fixture.py &) && xcodebuild test -project Mewo.xcodeproj -scheme Mewo -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:MewoUITests/CatalogScreenshotTests
```

## Build on a Mac

```bash
cd ios
xcodegen generate
xcodebuild -project Mewo.xcodeproj -scheme Mewo -destination 'platform=iOS Simulator,name=iPhone 17' -jobs 12 build
```

`project.yml` is the Xcode source of truth. Install from Xcode onto a phone or Simulator. Distribution is TestFlight or an Xcode install.

`ios/scripts/embed-build-identity.sh` runs as an Xcode post-build script. It sets `CFBundleShortVersionString` from `MEWO_VERSION` or `git describe`, and `MewoGitCommit` from `MEWO_COMMIT` or the short sha. See [Version and release](../README.md#version-and-release).

## Build on Linux

[xtool](https://github.com/xtool-org/xtool) builds `ios/Package.swift` into an `.app` and installs it on a USB-connected iPhone. There is no Simulator on Linux. `mise install` provides Swift 6.4 and xtool. Also install:

- `usbmuxd` (`sudo apt-get install usbmuxd`)
- an `Xcode.xip` from <https://developer.apple.com/download/all/?q=Xcode> (Apple ID, browser only)

`xtool setup` logs in to Apple and builds the iOS SDK from the xip. Then:

```bash
cd ios
xtool dev build
xtool dev
```

`Package.swift`, `xtool.yml`, and `xtool-Info.plist` exist for xtool. Xcode still uses `project.yml`. A new Swift package, bundled resource, or `Info.plist` key has to land on both sides.

`ios/mise.toml` points `PKG_CONFIG_PATH` at the stub `zlib.pc` in `ios/xtool-pkgconfig/`. Without it, Readium's ZIPFoundation links the Linux host zlib and fails with `libz.so: unhandled file type` ([xtool#140](https://github.com/xtool-org/xtool/issues/140)). Run xtool from `ios/` with mise active. If a failed build is cached, `rm .build/manifest.pif` first.

xtool does not compile `Assets.xcassets` and does not run `embed-build-identity.sh`. Linux builds get `AppIcon.png`, the `xtool-Info.plist` version fallback, and no commit. They omit the Teddy icon, `MewoTeddy` header image, accent color, and launch screen image. Check those on a Mac build.
