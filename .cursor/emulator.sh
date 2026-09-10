#!/usr/bin/env bash
# Optional: provision + boot a headless Android emulator for GUI testing.
#
# Not run by install.sh (the system image is ~1.5 GB and booting needs KVM),
# so run it on demand:
#     bash .cursor/emulator.sh
#
# Then install and launch the app:
#     adb install -r android/app/build/outputs/apk/debug/app-debug.apk
#     adb shell monkey -p com.mewo.reader -c android.intent.category.LAUNCHER 1
#
# The app can reach a server running on this host at http://10.0.2.2:8787
# (the emulator's alias for the host loopback).
set -euo pipefail

ANDROID_HOME="${ANDROID_HOME:-$HOME/android-sdk}"
AVD_NAME="${AVD_NAME:-mewo}"
SYSTEM_IMAGE="system-images;android-36;google_apis;x86_64"
export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$PATH"

echo "==> Emulator + system image"
yes | sdkmanager --licenses >/dev/null 2>&1 || true
sdkmanager "emulator" "$SYSTEM_IMAGE"

echo "==> KVM access"
if [ -e /dev/kvm ] && [ ! -w /dev/kvm ]; then
  sudo chmod 666 /dev/kvm || echo "WARNING: could not chmod /dev/kvm; emulator needs hardware acceleration."
fi

echo "==> AVD '$AVD_NAME'"
if ! emulator -list-avds 2>/dev/null | grep -qx "$AVD_NAME"; then
  echo "no" | avdmanager create avd -n "$AVD_NAME" -k "$SYSTEM_IMAGE" -d "medium_phone" --force
fi

echo "==> Booting emulator (headless)"
nohup emulator -avd "$AVD_NAME" -no-window -no-audio -no-boot-anim \
  -gpu swiftshader_indirect -no-snapshot -accel on >/tmp/emulator.log 2>&1 &

adb start-server >/dev/null 2>&1 || true
adb wait-for-device
echo "==> Waiting for boot to complete"
until [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; do sleep 3; done
echo "==> Emulator ready ($(adb devices | awk 'NR==2{print $1}'))"
