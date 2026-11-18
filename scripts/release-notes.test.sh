#!/usr/bin/env bash
# Builds a throwaway repo with tags and checks the exact notes for each.
#
#   scripts/release-notes.test.sh
set -euo pipefail

script="$(cd "$(dirname "$0")" && pwd)/release-notes.sh"
repo="$(mktemp -d)"
trap 'rm -rf "$repo"' EXIT
cd "$repo"

export GITHUB_REPOSITORY=acme/app
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.com
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.com
git init -q -b main
git config tag.gpgSign false
git config commit.gpgSign false

commit() {
  git commit -q --allow-empty "$@"
}

failures=0
expect() {
  local tag="$1" expected="$2" actual
  actual="$("$script" "$tag")"
  if [ "$actual" = "$expected" ]; then
    echo "ok $tag"
  else
    echo "FAIL $tag"
    diff <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") || true
    failures=$((failures + 1))
  fi
}

commit -m "Initial commit"
commit -m "chore: add a license (#1)"
git tag -a v0.1.0 -m "Mewo 0.1.0"

commit -m "feat: genre filter for catalog search (#2)"
commit -m "fix(ios): keep downloads when a session expires (#3)"
commit -m "feat(server)!: drop the v1 sync API (#4)"
commit -m "refactor(web): move auth into a hook (#5)" -m "BREAKING CHANGE: the login route moved to /auth."
commit -m "perf(android): cache cover thumbnails (#6)"
commit -m "docs: explain hosted handles"
commit -m "Let the reader pick a font (#7)"
git tag -a v0.2.0 -F - <<'EOF'
Mewo 0.2.0

Catalog search gets genre filters.
Sync now needs the v2 server.
EOF

commit -m "fix: trim handle whitespace (#8)" -m "Body that must not become a summary."
git tag v0.2.1

expect v0.1.0 "## Other

- add a license ([#1](https://github.com/acme/app/pull/1))
- Initial commit

**Full Changelog**: https://github.com/acme/app/commits/v0.1.0"

expect v0.2.0 "Catalog search gets genre filters.
Sync now needs the v2 server.

## Breaking changes

- **web:** move auth into a hook ([#5](https://github.com/acme/app/pull/5))
- **server:** drop the v1 sync API ([#4](https://github.com/acme/app/pull/4))

## Features

- genre filter for catalog search ([#2](https://github.com/acme/app/pull/2))

## Fixes

- **ios:** keep downloads when a session expires ([#3](https://github.com/acme/app/pull/3))

## Performance

- **android:** cache cover thumbnails ([#6](https://github.com/acme/app/pull/6))

## Other

- Let the reader pick a font (#7)
- explain hosted handles

**Full Changelog**: https://github.com/acme/app/compare/v0.1.0...v0.2.0"

expect v0.2.1 "## Fixes

- trim handle whitespace ([#8](https://github.com/acme/app/pull/8))

**Full Changelog**: https://github.com/acme/app/compare/v0.2.0...v0.2.1"

if [ "$failures" -ne 0 ]; then
  echo "$failures failed"
  exit 1
fi
