# Mewo

An EPUB reader that turns a book into a timeline. One paragraph is one post. Like a line, comment on it, quote it, or share it.

## Install

<p align="center">
  <a href="https://apps.obtainium.imranr.dev/redirect?r=obtainium://app/%7B%22id%22%3A%22com.mewo.reader%22%2C%22url%22%3A%22https%3A%2F%2Fgithub.com%2Fmatthewyjiang%2Fmewo-reader%22%2C%22author%22%3A%22matthewyjiang%22%2C%22name%22%3A%22Mewo%22%2C%22additionalSettings%22%3A%22%7B%5C%22apkFilterRegEx%5C%22%3A%5C%22mewo-.*%5C%5C%5C%5C.apk%5C%22%7D%22%7D"><img src="docs/assets/badge_obtainium.png" width="161" height="48" alt="Get it on Obtainium"></a>
  <br>
  <a href="https://github.com/matthewyjiang/mewo-reader/releases/latest">Download the APK</a>
  <br>
  Obtainium keeps the app updated automatically.
</p>

## Layout

```
android/   phone app (Kotlin, Jetpack Compose)
ios/       phone app (SwiftUI)
server/    hosted library API; also serves the web app
web/       hosted shelf in the browser (Svelte)
```

Phones use [Readium](https://github.com/readium/kotlin-toolkit) to parse local books. The server parses hosted books, so Android, iPhone, and the web app read the same posts. Local and hosted shelves stay separate.

## Toolchains

`mise install` at the repo root installs the pins in `mise.toml`: Node 22, Rust 1.88, Swift 6.4, and [xtool](https://github.com/xtool-org/xtool) on Linux. Android Gradle needs JDK 21 because Paparazzi is applied to `:app`. The app still targets JVM 17. The Android SDK, and `usbmuxd` for xtool, come from outside mise.

## Quick start

**Android.** JDK 21 and Android SDK 36. [android/README.md](android/README.md)

```bash
cd android
./gradlew :app:assembleDebug --max-workers=12
```

**iPhone.** [ios/README.md](ios/README.md)

```bash
cd ios
xcodegen generate
xcodebuild -project Mewo.xcodeproj -scheme Mewo -destination 'platform=iOS Simulator,name=iPhone 17' -jobs 12 build
```

**Server.** Rust 1.88. Listens on `0.0.0.0:8787`. The server speaks plain HTTP, so put a reverse proxy in front of it before a password leaves your own machine. [server/README.md](server/README.md)

```bash
cd server
cargo run
```

**Web.** Node 22. The dev server proxies `/v1` to port 8787. [web/README.md](web/README.md)

```bash
cd web
npm ci
npm run dev
```

## Version and release

This is the version source of truth. Component READMEs only add how each build passes it through.

`git describe --tags --always --dirty`, with one leading `v` removed, is the version for a server binary, a web bundle, and an iOS app built in a checkout. Describe only sees tags that clone has fetched. `MEWO_VERSION` overrides it. `MEWO_COMMIT` overrides `git rev-parse --short=7 HEAD`. With no git and no override, the server and the web bundle use the version in `Cargo.toml` or `web/package.json` and omit the commit. iOS then keeps `MARKETING_VERSION` from `ios/project.yml`.

Android `versionName` is `MEWO_VERSION_NAME`. When that is unset, Gradle uses the `versionName` default in `android/app/build.gradle.kts`. `versionCode` is `MEWO_VERSION_CODE`, or `1`. The short commit is `MEWO_COMMIT` or `git rev-parse --short=7 HEAD`, stored in `BuildConfig.COMMIT`. Android does not use `git describe` for `versionName`.

Pushing a `vMAJOR.MINOR.PATCH` tag (the `v0.1.*` releases) builds the server image and a signed Android APK, and attaches `mewo-<version>.apk` to the GitHub Release. CI sets `MEWO_VERSION` / `MEWO_VERSION_NAME` from the tag with the leading `v` removed, and `versionCode` to `major * 1000000 + minor * 1000 + patch`. Deploying the image is outside this repo.
