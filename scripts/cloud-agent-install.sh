#!/usr/bin/env bash
# Idempotent, non-interactive Cloud Agent install for mewo-reader.
# iOS builds stay on a Mac. Do not install Swift, xtool, or usbmuxd.
# `mise install` with no arguments would also install those from mise.toml.
#
# .cursor/environment.json replaces the dashboard install, so this file keeps
# the toolchain bootstrap and then adds the emulator packages. It does not boot
# the emulator. Processes do not survive the environment snapshot.
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
export MISE_YES=1

if [ -f mise.toml ]; then
  ROOT="$(pwd)"
elif [ -f /workspace/mise.toml ]; then
  ROOT=/workspace
  cd "$ROOT"
else
  echo "mewo install: run from the repository root (mise.toml not found)" >&2
  exit 1
fi

echo "==> apt packages"
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential \
  ca-certificates \
  curl \
  git \
  pkg-config \
  unzip \
  wget \
  xz-utils

echo "==> mise"
if [ ! -x "${HOME}/.local/bin/mise" ]; then
  curl -fsSL https://mise.run | sh
fi
sudo ln -sf "${HOME}/.local/bin/mise" /usr/local/bin/mise
export PATH="${HOME}/.local/share/mise/shims:${HOME}/.local/bin:${PATH}"

cd "$ROOT"
mise trust "$ROOT/mise.toml"

echo "==> pinned toolchains (java 17, node 22, rust 1.88)"
mise install java@temurin-17 node@22 rust@1.88.0
mise reshim

sudo tee /etc/profile.d/mewo-dev.sh >/dev/null <<'EOF'
# Cloud Agent login shells (bash -l) skip ~/.bashrc when it is non-interactive.
# Keep mise shims, JAVA_HOME, and the Android SDK on the default PATH.
export PATH="${HOME}/.local/share/mise/shims:${HOME}/.local/bin:${PATH}"
if [ -x /usr/local/bin/mise ]; then
  _mewo_java="$(/usr/local/bin/mise where java 2>/dev/null || true)"
  if [ -n "${_mewo_java}" ]; then
    export JAVA_HOME="${_mewo_java}"
    export PATH="${JAVA_HOME}/bin:${PATH}"
  fi
fi
export ANDROID_HOME="${HOME}/Android/Sdk"
export ANDROID_SDK_ROOT="${ANDROID_HOME}"
export PATH="${ANDROID_HOME}/emulator:${ANDROID_HOME}/cmdline-tools/latest/bin:${ANDROID_HOME}/platform-tools:${PATH}"
unset _mewo_java
EOF
sudo chmod 644 /etc/profile.d/mewo-dev.sh

# shellcheck disable=SC1091
source /etc/profile.d/mewo-dev.sh
export JAVA_HOME="$(mise where java)"
export PATH="${JAVA_HOME}/bin:${PATH}"

echo "==> toolchain versions"
# Login shells source ~/.bashrc, which prepends /usr/local/cargo/bin ahead of
# mise shims. Point that rustup proxy at 1.88 so `cargo` is not the image's 1.83.
if [ -x /usr/local/cargo/bin/rustup ]; then
  /usr/local/cargo/bin/rustup toolchain install 1.88.0 --profile minimal --no-self-update
  /usr/local/cargo/bin/rustup default 1.88.0
fi
echo "java: $(bash -lc 'command -v java')"
bash -lc 'java -version'
echo "node: $(bash -lc 'command -v node') $(bash -lc 'node -v')"
echo "rustc: $(bash -lc 'command -v rustc') $(bash -lc 'rustc --version')"
bash -lc 'java -version' 2>&1 | grep -q 'version "17'
bash -lc 'node -v' | grep -q '^v22\.'
bash -lc 'rustc --version' | grep -q ' 1\.88\.0 '

echo "==> Android SDK 36"
CMD_ZIP="commandlinetools-linux-15859902_latest.zip"
if [ ! -x "${ANDROID_HOME}/cmdline-tools/latest/bin/sdkmanager" ]; then
  tmp="$(mktemp -d)"
  curl -fsSL -o "${tmp}/${CMD_ZIP}" "https://dl.google.com/android/repository/${CMD_ZIP}"
  mkdir -p "${ANDROID_HOME}/cmdline-tools"
  unzip -q "${tmp}/${CMD_ZIP}" -d "$tmp"
  rm -rf "${ANDROID_HOME}/cmdline-tools/latest"
  mv "${tmp}/cmdline-tools" "${ANDROID_HOME}/cmdline-tools/latest"
  rm -rf "$tmp"
fi
export PATH="${ANDROID_HOME}/emulator:${ANDROID_HOME}/cmdline-tools/latest/bin:${ANDROID_HOME}/platform-tools:${PATH}"

mkdir -p "${ANDROID_HOME}/licenses"
printf '%s\n' "24333f8a63b6825ea9c5514f83c2829b004d1fee" > "${ANDROID_HOME}/licenses/android-sdk-license"

sdkmanager --sdk_root="${ANDROID_HOME}" --install \
  "platforms;android-36" \
  "build-tools;36.0.0" \
  "platform-tools" </dev/null

test -d "${ANDROID_HOME}/platforms/android-36"
echo "sdk.dir=${ANDROID_HOME}" > "${ROOT}/android/local.properties"

echo "==> web dependencies"
cd "${ROOT}/web"
npm ci

echo "==> server binary"
cd "${ROOT}/server"
export CARGO_BUILD_JOBS=2
cargo fetch --locked
cargo build --locked --bin mewo-server --jobs 2

echo "==> android debug APK"
cd "${ROOT}/android"
./gradlew :app:assembleDebug --max-workers=2 --no-daemon
test -f "${ROOT}/android/app/build/outputs/apk/debug/app-debug.apk"

echo "==> Android emulator image and AVD"
bash "${ROOT}/scripts/android-emulator.sh" install

echo "INSTALL_OK"
