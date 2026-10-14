#!/bin/sh
# Writes the release identity into the built Info.plist.
#
# CFBundleShortVersionString becomes `git describe --tags --always --dirty`
# with one leading `v` removed. MewoGitCommit is `git rev-parse --short=7 HEAD`.
# MEWO_VERSION and MEWO_COMMIT win when set, matching the server and web builds.
# If git is unavailable, the short version stays on MARKETING_VERSION and the
# commit key is left out.
set -eu

plist="${TARGET_BUILD_DIR}/${INFOPLIST_PATH}"
if [ ! -f "$plist" ]; then
  echo "warning: built Info.plist not found at ${plist}"
  exit 0
fi

root=$(CDPATH= cd "${SRCROOT}/.." && pwd)

trim() {
  printf '%s' "$1" | tr -d '\r' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//'
}

# Empty is allowed. Anything outside the git-describe alphabet is refused
# so a value cannot be read as a PlistBuddy command.
safe() {
  case "$1" in
    '') return 0 ;;
    *[!A-Za-z0-9._+-]*) return 1 ;;
    *) return 0 ;;
  esac
}

version=$(trim "${MEWO_VERSION:-}")
commit=$(trim "${MEWO_COMMIT:-}")

if ! safe "$version"; then
  echo "warning: ignoring MEWO_VERSION"
  version=""
fi
if ! safe "$commit"; then
  echo "warning: ignoring MEWO_COMMIT"
  commit=""
fi

if [ -z "$version" ] && git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  described=$(git -C "$root" describe --tags --always --dirty 2>/dev/null || true)
  described=$(trim "$described")
  case "$described" in
    v*) described=${described#v} ;;
  esac
  if safe "$described"; then
    version=$described
  fi
fi

if [ -z "$commit" ] && git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  sha=$(git -C "$root" rev-parse --short=7 HEAD 2>/dev/null || true)
  sha=$(trim "$sha")
  if safe "$sha"; then
    commit=$sha
  fi
fi

if [ -n "$version" ]; then
  /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString ${version}" "$plist"
fi

if [ -n "$commit" ]; then
  /usr/libexec/PlistBuddy -c "Delete :MewoGitCommit" "$plist" >/dev/null 2>&1 || true
  /usr/libexec/PlistBuddy -c "Add :MewoGitCommit string ${commit}" "$plist"
fi

mkdir -p "${DERIVED_FILE_DIR}"
printf '%s %s\n' "$version" "$commit" > "${DERIVED_FILE_DIR}/mewo-git-identity-stamp"

# The script edits Info.plist after it may already have been signed.
if [ "${CODE_SIGNING_ALLOWED:-YES}" != "NO" ] && [ -n "${CODESIGNING_FOLDER_PATH:-}" ] \
  && codesign --verify "${CODESIGNING_FOLDER_PATH}" >/dev/null 2>&1; then
  identity=${EXPANDED_CODE_SIGN_IDENTITY:--}
  if [ -z "$identity" ]; then
    identity="-"
  fi
  codesign --force --sign "$identity" --preserve-metadata=identifier,entitlements \
    "${CODESIGNING_FOLDER_PATH}"
fi

echo "embed-build-identity: version=${version:-unchanged} commit=${commit:-none}"
