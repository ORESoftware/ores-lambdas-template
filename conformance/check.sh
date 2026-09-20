#!/bin/sh
set -eu

root=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
cd "$root"

fail() {
  echo "[lambda-template-conformance] $*" >&2
  exit 1
}

for boundary in contracts conformance; do
  [ ! -L "$boundary" ] || fail "$boundary must be a real directory, not a symbolic link"
  [ -d "$boundary" ] || fail "missing required top-level $boundary/ boundary"
done

escaped=$(find contracts conformance -type l -print -quit 2>/dev/null || true)
[ -z "$escaped" ] || fail "symbolic links are not allowed inside contract/conformance boundaries: $escaped"

[ -f Cargo.lock ] || fail "Cargo.lock is required for reproducible template verification"
cmp -s Dockerfile Containerfile || fail "Dockerfile and Containerfile must remain byte-identical"

sh scripts/test-entrypoint.sh
bash scripts/test-oresc-audit.sh
sh scripts/test-concern-catalog.sh
sh scripts/test-lock-concern.sh

command -v cargo >/dev/null 2>&1 || fail "cargo is required for Rust conformance"
cargo test --locked --features http,portable,aws --all-targets

echo "[lambda-template-conformance] ok"
