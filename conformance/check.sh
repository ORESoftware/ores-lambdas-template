#!/usr/bin/env sh
set -eu
mode=${1:---full}
case "$mode" in --structural-only|--full) ;; *) echo 'usage: conformance/check.sh [--structural-only|--full]' >&2; exit 2;; esac
root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"; cd "$root"
fail(){ echo "[lambda-conformance] $*" >&2; exit 1; }
for dir in contracts conformance; do [ -d "$dir" ] && [ ! -L "$dir" ] || fail "$dir must be a real non-symlink directory"; done
[ -z "$(find contracts conformance -type l -print -quit 2>/dev/null)" ] || fail 'symlinks are forbidden inside contracts/ and conformance/'
for path in schema-authority/main.tsp schema-authority/authored.schema.json .zpkg.toml zed-env.toml .zed/pre-install .zed/post-install .githooks/pre-commit .githooks/pre-push scripts/install-git-hooks.sh; do [ -f "$path" ] && [ ! -L "$path" ] || fail "missing or symlinked lifecycle authority: $path"; done
[ "$mode" = --full ] || exit 0
command -v zed >/dev/null 2>&1 || fail 'zed is required for full Lambda contract admission'
out="${TMPDIR:-/tmp}/ores-lambda-template-parity-$$"; trap 'rm -rf "$out"' EXIT INT TERM
mkdir -p "$out"
zed run tjsv check --typespec="$root/schema-authority/main.tsp" --schema="$root/schema-authority/authored.schema.json" --report="$out/report.json" --output-dir="$out/generated" --quiet
bash scripts/test-concern-catalog.sh
bash scripts/test-lock-concern.sh
cargo test --features http,portable,aws --all-targets
