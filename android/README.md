# Android

Phone app. Kotlin, Jetpack Compose, and Readium 3.3.0 for local EPUBs. Hosted books come from the server API. See the [root README](../README.md) for what Mewo is and how versions are chosen.

## Requirements

- Android 8 (API 26) or newer to install
- JDK 21 to run Gradle. Paparazzi is applied to `:app` and configures only on JDK 21. The app still targets JVM 17
- Android SDK with `platforms;android-36` and `build-tools;36.0.0`

`mise.toml` pins Temurin 17, which matches the bytecode target. Point `JAVA_HOME` at a JDK 21 before `./gradlew`.

## Build

```bash
export JAVA_HOME=/path/to/jdk-21
export ANDROID_HOME=/path/to/Android/Sdk
cd android
./gradlew :app:assembleDebug --max-workers=12
```

The APK is `android/app/build/outputs/apk/debug/app-debug.apk`.

### Tests

```bash
./gradlew :app:testDebugUnitTest --max-workers=12
./gradlew :app:verifyPaparazziDebug
```

`verifyPaparazziDebug` checks `android/app/src/test/snapshots`. Record new goldens with `:app:recordPaparazziDebug`. CI on a pull request that touches `android/` runs both, then `:app:assembleRelease` without a keystore.

## Release

A `vMAJOR.MINOR.PATCH` tag runs `.github/workflows/android-apk.yml`. It sets `MEWO_VERSION_NAME` from the tag and `MEWO_VERSION_CODE` to `major * 1000000 + minor * 1000 + patch`, signs the APK, and attaches `mewo-<version>.apk` to the GitHub Release. `workflow_dispatch` can build a signed APK without a tag. Version rules: [root README](../README.md#version-and-release).

A debug APK and a release APK use different keys. Uninstall the debug build before installing a release APK. Later release APKs then upgrade in place.

### Signing

CI reads these Actions secrets. The workflow fails the tag if any one is missing.

- `MEWO_STORE_BASE64` — base64 of the release keystore (`base64 -w0 mewo-release.jks`; on macOS, `base64 < mewo-release.jks | tr -d '\n'`)
- `MEWO_STORE_PASSWORD`
- `MEWO_KEY_ALIAS`
- `MEWO_KEY_PASSWORD`

Create the keystore outside this repo and keep a backup. `keytool -genkeypair` with `-alias mewo`, RSA 2048, store type JKS.

A local release build uses the same four names. `MEWO_STORE_FILE` is the keystore path. Environment variables or `~/.gradle/gradle.properties` both work.

```bash
cd android
./gradlew :app:assembleRelease --max-workers=12
```

The APK is `android/app/build/outputs/apk/release/app-release.apk`.

## Install

Mewo is not on the Play Store. Send a GitHub Release APK. The recipient opens the file and allows install from that app once (Settings > Apps > Special app access > Install unknown apps).

```bash
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n com.mewo.reader/.MainActivity
```

## Headless emulator

`scripts/android-emulator.sh` installs the `mewo` AVD when it is missing, boots it, and waits until `sys.boot_completed` is 1. The image is a medium phone (1080x2400, API 36, Google APIs, x86_64) with no camera cutout. A cutout sits on the status bar and covers the top of the app.

```bash
bash scripts/android-emulator.sh
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n com.mewo.reader/.MainActivity
adb exec-out screencap -p > /tmp/mewo.png
```

`scripts/android-emulator.sh install` installs the system image and creates the AVD, then exits. Layout screenshots belong in Paparazzi. See [AGENTS.md](../AGENTS.md). Use this emulator when the shot has to come from a system image.
