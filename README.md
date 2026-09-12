# Mewo

An EPUB reader that turns a book into an X-style timeline. One paragraph is one post. You scroll. You can comment on a line, like it, quote it, or share it.

This repo holds both sides:

```
android/   Android phone app
ios/       iPhone app
server/    hosted library API
```

Import a file from the picker, or tap **Open a sample** if you just want to see the feed.

Android is Kotlin, Jetpack Compose, and [Readium](https://github.com/readium/kotlin-toolkit) for parsing. iPhone is SwiftUI and the [Readium Swift toolkit](https://github.com/readium/swift-toolkit). The reader itself is the app's timeline, not Readium's page navigator. Open `android/` in Android Studio, or `ios/Mewo.xcodeproj` in Xcode.

## Android

### Requirements

- Android 8 (API 26) or newer
- JDK 17+ to build
- Android SDK with `platforms;android-36`

### Build

```bash
export JAVA_HOME=/path/to/jdk-21
export ANDROID_HOME=/path/to/Android/Sdk
cd android
./gradlew :app:assembleDebug --max-workers=12
```

The APK lands at `android/app/build/outputs/apk/debug/app-debug.apk`.

### Release

A tag `vMAJOR.MINOR.PATCH` (example: `v0.1.0`) runs `:app:assembleRelease` in CI. The workflow sets `versionName` from the tag and `versionCode` to `major * 1000000 + minor * 1000 + patch`, then attaches `mewo-<version>.apk` to the GitHub Release.

PRs that touch `android/` run the same Gradle task without signing, so a broken release compile fails the PR. `workflow_dispatch` builds a signed APK when you want one without tagging.

Friends who already installed a debug APK have to uninstall it before a release APK will install. The signatures differ. Later `v*` releases then upgrade in place.

#### Signing secrets

CI signs with a release keystore. Create one and keep a backup outside this repo:

```bash
keytool -genkeypair -v \
  -keystore mewo-release.jks \
  -keyalg RSA -keysize 2048 -validity 10000 \
  -alias mewo \
  -storetype JKS
```

Set these repo Actions secrets (`gh secret set NAME`):

- `MEWO_STORE_BASE64` - `base64 -w0 mewo-release.jks` (on macOS: `base64 < mewo-release.jks | tr -d '\n'`)
- `MEWO_STORE_PASSWORD`
- `MEWO_KEY_ALIAS` - `mewo` if you used the command above
- `MEWO_KEY_PASSWORD`

A tagged release fails out loud if any of those are missing.

#### Local release build

```bash
export MEWO_STORE_FILE=/path/to/mewo-release.jks
export MEWO_STORE_PASSWORD=...
export MEWO_KEY_ALIAS=mewo
export MEWO_KEY_PASSWORD=...
cd android
./gradlew :app:assembleRelease --max-workers=12
```

The APK lands at `android/app/build/outputs/apk/release/app-release.apk`. You can put the same four names in `~/.gradle/gradle.properties` instead of exporting them.

### Sideload

Mewo is not on the Play Store. You install the APK yourself.

#### On a phone

1. Copy `app-debug.apk` onto the phone (USB, Drive, AirDrop-to-adjacent-Android, whatever).
2. Open the file. Android will ask you to allow installs from that app (Files, Drive, Messages, etc.). Allow it.
3. Tap Install.

If the phone refuses the package, check Settings > Apps > Special app access > Install unknown apps, and turn it on for the app you used to open the APK.

#### With adb

Plug the phone in, turn on USB debugging (Settings > Developer options), then:

```bash
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n com.mewo.reader/.MainActivity
```

Same commands work on an emulator.

#### Handing it to a friend

Send them the APK. Tell them to open the file on the phone and allow the one-time "install unknown apps" prompt. First open asks for a handle. They do not need Android Studio, adb, or a Google account for Mewo.

Debug builds expire in the sense that you can overwrite them with `adb install -r`. They use the local debug keystore, not the release key. Send friends a GitHub Release APK, not `app-debug.apk`.

### Emulator

```bash
export ANDROID_HOME=/path/to/Android/Sdk
export QT_QPA_PLATFORM=xcb   # if you're on Wayland
$ANDROID_HOME/emulator/emulator -avd mewo -gpu host
```

The `mewo` AVD in this repo's notes is a medium phone image, not a Pixel with a camera hole. The hole covers the system status bar and makes the top of the app look broken.

## iPhone

### Requirements

- A Mac with full Xcode 26 or newer (the Command Line Tools package is not enough)
- The iOS 26 SDK
- [XcodeGen](https://github.com/yonaskolb/XcodeGen) if you need to regenerate `Mewo.xcodeproj` (`brew install xcodegen`)

### Build

```bash
cd ios
xcodegen generate
xcodebuild -scheme Mewo -destination 'platform=iOS Simulator,name=iPhone 17' -jobs 12 build
```

Install from Xcode onto a phone or Simulator. This is not an App Store listing. Friends get a TestFlight build or an Xcode install, same idea as the Android APK.

## Server

Rust API for the hosted shelf. See [server/README.md](server/README.md).

```bash
cd server
cargo run
```

Or pull the image CI publishes to GHCR:

```bash
docker run --rm -p 8787:8787 -v mewo-data:/data ghcr.io/matthewyjiang/mewo-reader/server:latest
```

That listens on port 8787. In the app, open the account drawer, pick **Hosted server**, and sign in with that URL. Local books stay on the phone. Hosted books live on the server. They do not merge. See [server/README.md](server/README.md) for env vars and the Dockerfile.
