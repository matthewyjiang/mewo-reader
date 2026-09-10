#!/usr/bin/env bash
# Idempotent Cloud Agent bootstrap for the Mewo repo.
# - Installs the Android SDK (cmdline-tools, platform-tools, platform 36, build-tools 36).
# - Points Gradle at the SDK and exposes it to interactive shells.
# - Warms the Rust and Gradle caches so the first real build is fast.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID_HOME="${ANDROID_HOME:-$HOME/android-sdk}"
CMDLINE_TOOLS_ZIP="commandlinetools-linux-15859902_latest.zip"

echo "==> Android SDK -> $ANDROID_HOME"
if [ ! -x "$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" ]; then
  mkdir -p "$ANDROID_HOME/cmdline-tools"
  tmp="$(mktemp -d)"
  curl -fsSL -o "$tmp/cmdtools.zip" "https://dl.google.com/android/repository/${CMDLINE_TOOLS_ZIP}"
  unzip -q "$tmp/cmdtools.zip" -d "$tmp"
  rm -rf "$ANDROID_HOME/cmdline-tools/latest"
  mv "$tmp/cmdline-tools" "$ANDROID_HOME/cmdline-tools/latest"
  rm -rf "$tmp"
fi

export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"
yes | sdkmanager --licenses >/dev/null 2>&1 || true
sdkmanager "platform-tools" "platforms;android-36" "build-tools;36.0.0"

echo "==> Gradle sdk.dir"
printf 'sdk.dir=%s\n' "$ANDROID_HOME" > "$REPO_ROOT/android/local.properties"

echo "==> Persist ANDROID_HOME for interactive shells"
if ! grep -q 'ANDROID_HOME=' "$HOME/.bashrc" 2>/dev/null; then
  {
    echo "export ANDROID_HOME=$ANDROID_HOME"
    echo 'export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"'
  } >> "$HOME/.bashrc"
fi

echo "==> Warm Rust build (server)"
( cd "$REPO_ROOT/server" && cargo build )

echo "==> Warm Gradle build (android debug APK)"
# Best effort: Maven Central occasionally rate-limits (HTTP 429) fresh pods.
# The SDK and local.properties above are what the agent strictly needs; a
# failed warm-up here only means the first real build downloads dependencies.
if ! ( cd "$REPO_ROOT/android" && JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-21-openjdk-amd64}" ./gradlew :app:assembleDebug ); then
  echo "WARNING: Gradle warm-up did not finish (likely a transient Maven 429). Skipping."
fi

echo "==> install.sh done"
