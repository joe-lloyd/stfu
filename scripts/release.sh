#!/usr/bin/env bash
# Cut a release. main only takes changes through pull requests, so this bumps
# the version on a release/vX.Y.Z branch, opens and merges a "release: vX.Y.Z"
# PR, then tags the merged commit on main and pushes the tag. The pushed tag
# starts .github/workflows/release.yml. Needs an authenticated `gh`.
#
# Usage: scripts/release.sh 0.10.0

set -euo pipefail

cd "$(dirname "$0")/.."

version="${1:-}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: $0 <X.Y.Z>" >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "error: working tree not clean — commit or stash first." >&2
  exit 1
fi
if [[ "$(git rev-parse --abbrev-ref HEAD)" != "main" ]]; then
  echo "error: releases are cut from main." >&2
  exit 1
fi
git fetch --quiet origin main
if [[ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]]; then
  echo "error: local main is not origin/main — pull first." >&2
  exit 1
fi

# The version lives in four places; each edit touches only the app's own line.
perl -0pi -e 's/("version": ")[^"]+"/${1}'"$version"'"/' package.json src-tauri/tauri.conf.json
perl -0pi -e 's/(\[package\]\nname = "stfu"\nversion = ")[^"]+"/${1}'"$version"'"/' src-tauri/Cargo.toml
perl -0pi -e 's/(name = "stfu"\nversion = ")[^"]+"/${1}'"$version"'"/' src-tauri/Cargo.lock
if [[ "$(git diff --numstat | wc -l | tr -d ' ')" != "4" ]]; then
  echo "error: expected the version bump to change 4 files:" >&2
  git diff --stat >&2
  git checkout -- . && exit 1
fi

branch="release/v$version"
git switch --quiet -c "$branch"
git commit --quiet -am "release: v$version"
git push --quiet -u origin "$branch"
gh pr create --base main --head "$branch" \
  --title "release: v$version" --body "Bumps the app version to $version. Merging tags v$version."
gh pr merge "$branch" --squash --delete-branch --subject "release: v$version" --body ""

git switch --quiet main
git pull --quiet --ff-only origin main
if ! grep -q "\"version\": \"$version\"" src-tauri/tauri.conf.json; then
  echo "error: main does not carry $version after the merge — not tagging." >&2
  exit 1
fi
git tag "v$version"
git push origin "v$version"

echo "released v$version — CI is building installers (Actions → release)."
