# Mewo

An Android EPUB reader that turns a book into an X-style timeline. One paragraph is one post. You scroll. You can like a line, quote it, or share it.

Import a file from the picker, or tap **Open a sample** if you just want to see the feed.

Kotlin, Jetpack Compose, and [Readium](https://github.com/readium/kotlin-toolkit) for parsing. The reader itself is Compose, not Readium's page navigator.

## Requirements

- Android 8 (API 26) or newer
- JDK 17+ to build
- Android SDK with `platforms;android-36`

## Build

```bash
export JAVA_HOME=/path/to/jdk-21
export ANDROID_HOME=/path/to/Android/Sdk
./gradlew :app:assembleDebug --max-workers=12
```

The APK lands at `app/build/outputs/apk/debug/app-debug.apk`.

## Sideload

Mewo is not on the Play Store. You install the APK yourself.

### On a phone

1. Copy `app-debug.apk` onto the phone (USB, Drive, AirDrop-to-adjacent-Android, whatever).
2. Open the file. Android will ask you to allow installs from that app (Files, Drive, Messages, etc.). Allow it.
3. Tap Install.

If the phone refuses the package, check Settings > Apps > Special app access > Install unknown apps, and turn it on for the app you used to open the APK.

### With adb

Plug the phone in, turn on USB debugging (Settings > Developer options), then:

```bash
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n com.mewo.reader/.MainActivity
```

Same commands work on an emulator.

### Handing it to a friend

Send them the APK. Tell them to open the file on the phone and allow the one-time "install unknown apps" prompt. They do not need Android Studio, adb, or a Google account for Mewo.

Debug builds expire in the sense that you can overwrite them with `adb install -r`. They are not signed for Play. Don't put this APK on a public link if you care about people running unsigned debug code.

## Emulator

```bash
export ANDROID_HOME=/path/to/Android/Sdk
export QT_QPA_PLATFORM=xcb   # if you're on Wayland
$ANDROID_HOME/emulator/emulator -avd mewo -gpu host
```

The `mewo` AVD in this repo's notes is a medium phone image, not a Pixel with a camera hole. The hole covers the system status bar and makes the top of the app look broken.
