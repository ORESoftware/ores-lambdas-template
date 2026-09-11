#!/bin/sh
set -eu

repo=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp" "$tmp-compat" "$tmp-conflict" "$tmp-blocked" "$tmp-invalid" "$tmp-duplicate" "$tmp-empty"' EXIT INT TERM

# Parse the catalog, root TOMLs, and reviewed templates with Rust so the same
# production-language toolchain that builds Lambda code owns semantic validation.
cargo test --quiet --manifest-path "$repo/Cargo.toml" --test concern_catalog_contract

cp -R "$repo/config" "$tmp/config"
cp "$repo/.ores-otel.toml" "$tmp/.ores-otel.toml"

sh "$repo/scripts/enable-concern.sh" "$tmp" "otel,middleware,rate-limit,chat,shared-auth"
for file in .ores-otel.toml .ores-mw.toml .ores-rl.toml .ores-chat.toml .shared-auth.toml; do
  test -s "$tmp/$file"
done
test ! -e "$tmp/.auth-shared.toml"
cmp -s "$repo/config/concerns/templates/ores-mw.toml" "$tmp/.ores-mw.toml"
cmp -s "$repo/config/concerns/templates/ores-rl.toml" "$tmp/.ores-rl.toml"
cmp -s "$repo/config/concerns/templates/ores-chat.toml" "$tmp/.ores-chat.toml"
cmp -s "$repo/config/concerns/templates/shared-auth.toml" "$tmp/.shared-auth.toml"

compat="$tmp-compat"
mkdir -p "$compat/config/concerns/templates"
cp "$repo/config/concerns/templates/shared-auth.toml" "$compat/config/concerns/templates/shared-auth.toml"
cp "$repo/.ores-otel.toml" "$compat/.ores-otel.toml"
sh "$repo/scripts/enable-concern.sh" "$compat" "shared-auth-compat"
test -s "$compat/.auth-shared.toml"
test ! -e "$compat/.shared-auth.toml"
cmp -s "$repo/config/concerns/templates/shared-auth.toml" "$compat/.auth-shared.toml"
if sh "$repo/scripts/enable-concern.sh" "$compat" "shared-auth"; then
  echo "canonical and compatibility Shared Auth files must not coexist" >&2
  exit 1
fi

# A conflicting request must fail before either alias is materialized.
conflict="$tmp-conflict"
mkdir -p "$conflict/config/concerns/templates"
cp "$repo/config/concerns/templates/shared-auth.toml" "$conflict/config/concerns/templates/shared-auth.toml"
cp "$repo/.ores-otel.toml" "$conflict/.ores-otel.toml"
if sh "$repo/scripts/enable-concern.sh" "$conflict" "shared-auth,shared-auth-compat"; then
  echo "conflicting Shared Auth request unexpectedly succeeded" >&2
  exit 1
fi
test ! -e "$conflict/.shared-auth.toml"
test ! -e "$conflict/.auth-shared.toml"

# Every catalogued-but-unadmitted concern remains fail-closed.
blocked="$tmp-blocked"
cp -R "$repo/config" "$blocked/config"
cp "$repo/.ores-otel.toml" "$blocked/.ores-otel.toml"
for concern in redis-lru forms opto-sync legal wasm rpc fanwaave; do
  if sh "$repo/scripts/enable-concern.sh" "$blocked" "$concern"; then
    echo "unadmitted concern unexpectedly materialized: $concern" >&2
    exit 1
  fi
done
find "$blocked" -maxdepth 1 -type f ! -name '.ores-otel.toml' -print | grep -q . && {
  echo "blocked concern request mutated destination root" >&2
  exit 1
}

# Preflight the entire comma-list before writing anything. A valid first item
# must not be left behind when a later item is blocked or unknown.
invalid="$tmp-invalid"
cp -R "$repo/config" "$invalid/config"
cp "$repo/.ores-otel.toml" "$invalid/.ores-otel.toml"
if sh "$repo/scripts/enable-concern.sh" "$invalid" "middleware,forms"; then
  echo "mixed admitted/blocked request unexpectedly succeeded" >&2
  exit 1
fi
test ! -e "$invalid/.ores-mw.toml"
if sh "$repo/scripts/enable-concern.sh" "$invalid" "chat,unknown-concern"; then
  echo "mixed admitted/unknown request unexpectedly succeeded" >&2
  exit 1
fi
test ! -e "$invalid/.ores-chat.toml"

# Duplicate and empty requests are configuration mistakes, not no-ops.
duplicate="$tmp-duplicate"
cp -R "$repo/config" "$duplicate/config"
cp "$repo/.ores-otel.toml" "$duplicate/.ores-otel.toml"
if sh "$repo/scripts/enable-concern.sh" "$duplicate" "chat,chat"; then
  echo "duplicate concern request unexpectedly succeeded" >&2
  exit 1
fi
test ! -e "$duplicate/.ores-chat.toml"

empty="$tmp-empty"
cp -R "$repo/config" "$empty/config"
cp "$repo/.ores-otel.toml" "$empty/.ores-otel.toml"
if sh "$repo/scripts/enable-concern.sh" "$empty" ", ,"; then
  echo "empty concern request unexpectedly succeeded" >&2
  exit 1
fi
