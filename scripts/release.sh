#!/usr/bin/env bash
# Usage: scripts/release.sh 0.1.4
#
# Bumps Cargo.toml + Cargo.lock, runs the checks, commits, tags and pushes.
# The tag push triggers .github/workflows/release.yml, which builds the
# binaries and afterwards commits release.json to main (so `git pull` later).
set -euo pipefail

version="${1:?usage: scripts/release.sh <version>   e.g. 0.1.4}"
version="${version#v}"
tag="v${version}"

die() { echo "error: $*" >&2; exit 1; }

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "version must look like 1.2.3"
[[ -z "$(git status --porcelain)" ]] || die "working tree is not clean"
[[ "$(git branch --show-current)" == "main" ]] || die "not on main"

git fetch --quiet origin main --tags
[[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] \
  || die "main is not in sync with origin/main (pull or push first)"
git rev-parse -q --verify "refs/tags/${tag}" >/dev/null && die "tag ${tag} already exists"

# Replace only the first `version = "..."` line, which is the [package] one.
V="$version" perl -0pi -e 's/^version = "[^"]*"/version = "$ENV{V}"/m' Cargo.toml
cargo check --quiet # refreshes the nixos-status entry in Cargo.lock

cargo fmt --check
cargo clippy --locked --quiet -- -D warnings
cargo test --locked --quiet

git add Cargo.toml Cargo.lock
git commit --quiet -m "release: ${tag}"
git tag "${tag}"

read -r -p "Push main and ${tag} to origin? [y/N] " answer
if [[ "$answer" == [yY] ]]; then
  git push --atomic origin main "${tag}"
  echo "Pushed. Watch the Release workflow; afterwards run: git pull"
else
  echo "Not pushed. Undo with: git tag -d ${tag} && git reset --hard HEAD~1"
fi
