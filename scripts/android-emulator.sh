#!/usr/bin/env bash
# Headless Android emulator for cloud agents.
#
#   scripts/android-emulator.sh install   # sdk packages + AVD, then exit
#   scripts/android-emulator.sh           # install if needed, boot, wait
#
# Nested KVM on the cloud kernel (6.12.94) is not usable. /dev/kvm opens and
# `emulator -accel-check` reports KVM, then kvm_arch_vcpu_create BUGs
# (kvm_spurious_fault) and the guest never runs. Default is -accel off.
# MEWO_EMULATOR_ACCEL=on opts into KVM on a host where vCPU creation works.
set -euo pipefail

AVD_NAME="${MEWO_AVD:-mewo}"
API_IMAGE="system-images;android-36;google_apis;x86_64"
BOOT_TIMEOUT="${MEWO_EMULATOR_BOOT_TIMEOUT:-1200}"
SERIAL="${MEWO_EMULATOR_SERIAL:-emulator-5554}"
ACCEL="${MEWO_EMULATOR_ACCEL:-off}"

ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-${HOME}/Android/Sdk}}"
export ANDROID_HOME ANDROID_SDK_ROOT="${ANDROID_HOME}"
export PATH="${ANDROID_HOME}/emulator:${ANDROID_HOME}/cmdline-tools/latest/bin:${ANDROID_HOME}/platform-tools:${PATH}"

AVD_DIR="${HOME}/.android/avd/${AVD_NAME}.avd"
LOG="${HOME}/.android/${AVD_NAME}-emulator.log"
PIDFILE="${HOME}/.android/${AVD_NAME}-emulator.pid"

usage() {
  echo "usage: scripts/android-emulator.sh [install|boot]" >&2
  exit 2
}

require_sdkmanager() {
  if [[ ! -x "${ANDROID_HOME}/cmdline-tools/latest/bin/sdkmanager" ]]; then
    echo "android emulator: sdkmanager is not at ${ANDROID_HOME}/cmdline-tools/latest/bin" >&2
    echo "android emulator: the cloud install has not set up the Android SDK" >&2
    exit 1
  fi
}

install_packages() {
  require_sdkmanager
  local missing=()
  [[ -x "${ANDROID_HOME}/emulator/emulator" ]] || missing+=("emulator")
  [[ -x "${ANDROID_HOME}/platform-tools/adb" ]] || missing+=("platform-tools")
  [[ -d "${ANDROID_HOME}/system-images/android-36/google_apis/x86_64" ]] || missing+=("${API_IMAGE}")
  if ((${#missing[@]} == 0)); then
    echo "android emulator: packages already installed"
    return 0
  fi
  mkdir -p "${ANDROID_HOME}/licenses"
  printf '%s\n' "24333f8a63b6825ea9c5514f83c2829b004d1fee" > "${ANDROID_HOME}/licenses/android-sdk-license"
  echo "android emulator: installing ${missing[*]}"
  sdkmanager --sdk_root="${ANDROID_HOME}" --install "${missing[@]}" </dev/null
}

set_cfg() {
  local key="$1" value="$2" file="$3"
  if grep -q "^${key}=" "$file"; then
    sed -i "s|^${key}=.*|${key}=${value}|" "$file"
  else
    printf '%s=%s\n' "$key" "$value" >> "$file"
  fi
}

create_avd() {
  require_sdkmanager
  if [[ ! -f "${AVD_DIR}/config.ini" ]]; then
    echo "android emulator: creating AVD ${AVD_NAME}"
    # This system image ships no devices.xml. avdmanager still writes the
    # medium_phone profile from cmdline-tools and may print that error.
    local status=0
    printf 'no\n' | avdmanager create avd \
      -n "${AVD_NAME}" \
      -k "${API_IMAGE}" \
      -d medium_phone \
      --force || status=$?
    if [[ ! -f "${AVD_DIR}/config.ini" ]]; then
      echo "android emulator: avdmanager failed (${status}) and wrote no config.ini" >&2
      exit 1
    fi
  else
    echo "android emulator: AVD ${AVD_NAME} already exists"
  fi
  # Cold boot only. A downloadable Google snapshot hides a broken accelerator.
  set_cfg avd.id "${AVD_NAME}" "${AVD_DIR}/config.ini"
  set_cfg avd.name "${AVD_NAME}" "${AVD_DIR}/config.ini"
  set_cfg fastboot.forceColdBoot yes "${AVD_DIR}/config.ini"
  set_cfg fastboot.forceFastBoot no "${AVD_DIR}/config.ini"
  set_cfg firstboot.bootFromDownloadableSnapshot no "${AVD_DIR}/config.ini"
  set_cfg firstboot.bootFromLocalSnapshot no "${AVD_DIR}/config.ini"
  set_cfg firstboot.saveToLocalSnapshot no "${AVD_DIR}/config.ini"
  if ! grep -q '^hw.device.name=medium_phone$' "${AVD_DIR}/config.ini"; then
    echo "android emulator: ${AVD_NAME} is not the medium_phone profile" >&2
    exit 1
  fi
}

boot_completed() {
  local value
  value="$(adb -s "${SERIAL}" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r' || true)"
  [[ "${value}" == "1" ]]
}

qemu_pid() {
  local dir cmd
  for dir in /proc/[0-9]*; do
    [[ -r "${dir}/cmdline" ]] || continue
    cmd="$(tr '\0' ' ' < "${dir}/cmdline" 2>/dev/null || true)"
    if [[ "${cmd}" == *"qemu-system"* && "${cmd}" == *"-avd ${AVD_NAME}"* ]]; then
      basename "${dir}"
      return 0
    fi
  done
  return 1
}

emulator_pid() {
  local pid
  if [[ -f "${PIDFILE}" ]]; then
    pid="$(cat "${PIDFILE}")"
    if [[ -n "${pid}" ]] && kill -0 "${pid}" 2>/dev/null; then
      echo "${pid}"
      return 0
    fi
  fi
  # The launcher execs into qemu and keeps this pid. A previous boot may not
  # have written the pidfile, so adopt that process instead of starting another.
  if pid="$(qemu_pid)"; then
    echo "${pid}" > "${PIDFILE}"
    echo "${pid}"
    return 0
  fi
  return 1
}

start_emulator() {
  if emulator_pid >/dev/null; then
    echo "android emulator: already running (pid $(emulator_pid))"
    return 0
  fi
  local -a accel_args
  if [[ "${ACCEL}" == "on" ]]; then
    echo "android emulator: MEWO_EMULATOR_ACCEL=on. This cloud kernel BUGs in kvm_arch_vcpu_create." >&2
    accel_args=(-accel on)
  else
    accel_args=(-accel off)
  fi
  mkdir -p "${HOME}/.android"
  echo "android emulator: booting ${AVD_NAME} (${accel_args[*]} -gpu swiftshader_indirect)"
  # -no-boot-anim skips a software-rendered animation on the TCG path.
  emulator \
    -avd "${AVD_NAME}" \
    -no-window \
    -no-audio \
    -no-snapshot \
    -gpu swiftshader_indirect \
    -no-boot-anim \
    "${accel_args[@]}" \
    -port 5554 \
    >"${LOG}" 2>&1 &
  echo $! > "${PIDFILE}"
  echo "android emulator: pid $(cat "${PIDFILE}"), log ${LOG}"
}

wait_for_boot() {
  local start now elapsed
  start="$(date +%s)"
  echo "android emulator: waiting up to ${BOOT_TIMEOUT}s for sys.boot_completed"
  while true; do
    if boot_completed; then
      now="$(date +%s)"
      elapsed=$((now - start))
      echo "android emulator: sys.boot_completed in ${elapsed}s"
      return 0
    fi
    now="$(date +%s)"
    elapsed=$((now - start))
    if ((elapsed >= BOOT_TIMEOUT)); then
      echo "android emulator: timed out after ${elapsed}s" >&2
      if [[ -f "${LOG}" ]]; then
        tail -n 40 "${LOG}" >&2 || true
      fi
      return 1
    fi
    if ! emulator_pid >/dev/null; then
      echo "android emulator: process exited before boot completed" >&2
      if [[ -f "${LOG}" ]]; then
        tail -n 40 "${LOG}" >&2 || true
      fi
      return 1
    fi
    sleep 5
  done
}

cmd="${1:-boot}"
case "${cmd}" in
  install)
    install_packages
    create_avd
    ;;
  boot)
    install_packages
    create_avd
    if boot_completed; then
      echo "android emulator: ${AVD_NAME} is already up (sys.boot_completed=1)"
      exit 0
    fi
    start_emulator
    wait_for_boot
    ;;
  *)
    usage
    ;;
esac
