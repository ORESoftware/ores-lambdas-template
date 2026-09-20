#!/bin/sh
set -eu
mode=${1:-full}
case "$mode" in
  full|--full) mode=full ;;
  structural|--structural-only) mode=structural ;;
  *) echo "usage: conformance/check.sh [--full|--structural-only]" >&2; exit 2 ;;
esac

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

# Lifecycle wiring is itself part of conformance: these boundaries may not be decorative.
grep -q 'conformance/check.sh' .zpkg.toml || fail ".zpkg.toml must invoke conformance/check.sh"
for phase in post-install pre-build pre-pack pre-publish; do
  hook=".zpkg/hooks/$phase.bash"
  [ -f "$hook" ] || fail "missing Zed lifecycle hook: $hook"
  grep -q 'conformance/check.sh' "$hook" || fail "$hook must invoke conformance/check.sh"
done
[ -f .githooks/pre-push ] || fail "missing tracked Git pre-push hook"
grep -q 'conformance/check.sh' .githooks/pre-push || fail ".githooks/pre-push must invoke conformance/check.sh"

[ -f Cargo.lock ] || fail "Cargo.lock is required for reproducible template verification"
cmp -s Dockerfile Containerfile || fail "Dockerfile and Containerfile must remain byte-identical"

echo "[lambda-template-conformance] structural and lifecycle wiring checks passed"
[ "$mode" = full ] || exit 0

sh scripts/test-entrypoint.sh
bash scripts/test-oresc-audit.sh
sh scripts/test-concern-catalog.sh
sh scripts/test-lock-concern.sh

command -v cargo >/dev/null 2>&1 || fail "cargo is required for Rust conformance"
cargo test --locked --features http,portable,aws --all-targets

echo "[lambda-template-conformance] ok"
