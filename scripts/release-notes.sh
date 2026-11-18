#!/usr/bin/env bash
# Release notes for a v* tag, grouped by Conventional Commit type.
#
#   scripts/release-notes.sh v0.1.24 > notes.md
#
# Reads the first-parent (squash) subjects since the previous v* tag. A body
# in the annotated tag, after its "Mewo X.Y.Z" subject, is printed first as a
# summary. GITHUB_REPOSITORY sets the repo for links.
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "usage: scripts/release-notes.sh <tag>" >&2
  exit 2
fi

tag="$1"
repo="${GITHUB_REPOSITORY:-matthewyjiang/mewo-reader}"
prev="$(git describe --tags --abbrev=0 --match 'v*' "$tag^" 2>/dev/null || true)"
range="${prev:+$prev..}$tag"

breaking=()
features=()
fixes=()
performance=()
other=()

conventional='^([a-z]+)(\(([^)]+)\))?(!)?: (.+)$'
pull_number='^(.*) \(#([0-9]+)\)$'

while read -r sha; do
  subject="$(git log -1 --format=%s "$sha")"
  if ! [[ "$subject" =~ $conventional ]]; then
    other+=("- $subject")
    continue
  fi
  type="${BASH_REMATCH[1]}"
  scope="${BASH_REMATCH[3]}"
  bang="${BASH_REMATCH[4]}"
  description="${BASH_REMATCH[5]}"

  line="- ${scope:+**$scope:** }"
  if [[ "$description" =~ $pull_number ]]; then
    line+="${BASH_REMATCH[1]} ([#${BASH_REMATCH[2]}](https://github.com/$repo/pull/${BASH_REMATCH[2]}))"
  else
    line+="$description"
  fi

  if [ -n "$bang" ] || git log -1 --format=%b "$sha" | grep -qE '^BREAKING[ -]CHANGE: '; then
    breaking+=("$line")
    continue
  fi
  case "$type" in
    feat) features+=("$line") ;;
    fix) fixes+=("$line") ;;
    perf) performance+=("$line") ;;
    refactor | docs | test | build | ci | chore | style | revert) other+=("$line") ;;
    *) other+=("- $subject") ;;
  esac
done < <(git log --first-parent --format=%H "$range")

if [ "$(git cat-file -t "$tag")" = tag ]; then
  summary="$(git for-each-ref --format='%(contents:body)' "refs/tags/$tag")"
  if [ -n "$summary" ]; then
    printf '%s\n\n' "$summary"
  fi
fi

section() {
  local title="$1"
  shift
  if [ $# -gt 0 ]; then
    printf '## %s\n\n' "$title"
    printf '%s\n' "$@"
    printf '\n'
  fi
}

section "Breaking changes" "${breaking[@]}"
section "Features" "${features[@]}"
section "Fixes" "${fixes[@]}"
section "Performance" "${performance[@]}"
section "Other" "${other[@]}"

if [ -n "$prev" ]; then
  echo "**Full Changelog**: https://github.com/$repo/compare/$prev...$tag"
else
  echo "**Full Changelog**: https://github.com/$repo/commits/$tag"
fi
