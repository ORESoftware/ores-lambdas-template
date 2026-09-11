#!/bin/sh
set -eu

usage() {
  echo "usage: scripts/enable-concern.sh <repo-root> <comma-separated-concerns>" >&2
  exit 2
}

[ "$#" -eq 2 ] || usage
root=$1
requested=$2
[ -d "$root" ] || { echo "missing repository root: $root" >&2; exit 2; }

templates="$root/config/concerns/templates"
[ -d "$templates" ] || { echo "missing concern templates: $templates" >&2; exit 2; }

trim_concern() {
  printf '%s' "$1" | tr -d '[:space:]'
}

preflight_target() {
  source=$1
  target=$2
  [ -f "$source" ] || { echo "missing reviewed concern template: $source" >&2; exit 3; }
  [ ! -L "$source" ] || { echo "reviewed concern template must not be a symlink: $source" >&2; exit 3; }
  [ ! -e "$target" ] || { echo "refusing to overwrite existing concern config: $target" >&2; exit 4; }
}

copy_concern() {
  source=$1
  target=$2
  preflight_target "$source" "$target"
  cp "$source" "$target"
}

# Validate the complete request before mutating the destination. This prevents a
# request such as "middleware,forms" from partially materializing middleware
# before the blocked forms concern is discovered.
seen='|'
count=0
want_shared_canonical=0
want_shared_compat=0
old_ifs=$IFS
IFS=','
for raw_concern in $requested; do
  IFS=$old_ifs
  concern=$(trim_concern "$raw_concern")
  [ -n "$concern" ] || { IFS=','; continue; }
  case "$seen" in
    *"|$concern|"*)
      echo "duplicate Lambda concern requested: $concern" >&2
      exit 2
      ;;
  esac
  seen="${seen}${concern}|"
  count=$((count + 1))

  case "$concern" in
    otel)
      [ -f "$root/.ores-otel.toml" ] && [ ! -L "$root/.ores-otel.toml" ] || {
        echo "required baseline missing or unsafe: .ores-otel.toml" >&2
        exit 5
      }
      ;;
    middleware)
      preflight_target "$templates/ores-mw.toml" "$root/.ores-mw.toml"
      ;;
    rate-limit)
      preflight_target "$templates/ores-rl.toml" "$root/.ores-rl.toml"
      ;;
    chat)
      preflight_target "$templates/ores-chat.toml" "$root/.ores-chat.toml"
      ;;
    shared-auth)
      want_shared_canonical=1
      [ ! -e "$root/.auth-shared.toml" ] || {
        echo "legacy .auth-shared.toml conflicts with canonical .shared-auth.toml" >&2
        exit 6
      }
      preflight_target "$templates/shared-auth.toml" "$root/.shared-auth.toml"
      ;;
    shared-auth-compat)
      want_shared_compat=1
      [ ! -e "$root/.shared-auth.toml" ] || {
        echo "canonical .shared-auth.toml conflicts with compatibility .auth-shared.toml" >&2
        exit 6
      }
      preflight_target "$templates/shared-auth.toml" "$root/.auth-shared.toml"
      ;;
    redis-lru|forms|opto-sync|legal|wasm|rpc|fanwaave|indiebuild|sidecar)
      echo "concern '$concern' is catalogued but blocked until its owner schema/example is admitted" >&2
      exit 7
      ;;
    *)
      echo "unknown Lambda concern: $concern" >&2
      exit 2
      ;;
  esac
  IFS=','
done
IFS=$old_ifs

[ "$count" -gt 0 ] || { echo "at least one Lambda concern must be requested" >&2; exit 2; }
if [ "$want_shared_canonical" -eq 1 ] && [ "$want_shared_compat" -eq 1 ]; then
  echo "canonical and compatibility Shared Auth concerns may not be requested together" >&2
  exit 6
fi

# The complete request has passed preflight; materialize only reviewed templates.
IFS=','
for raw_concern in $requested; do
  IFS=$old_ifs
  concern=$(trim_concern "$raw_concern")
  [ -n "$concern" ] || { IFS=','; continue; }
  case "$concern" in
    otel)
      ;;
    middleware)
      copy_concern "$templates/ores-mw.toml" "$root/.ores-mw.toml"
      ;;
    rate-limit)
      copy_concern "$templates/ores-rl.toml" "$root/.ores-rl.toml"
      ;;
    chat)
      copy_concern "$templates/ores-chat.toml" "$root/.ores-chat.toml"
      ;;
    shared-auth)
      copy_concern "$templates/shared-auth.toml" "$root/.shared-auth.toml"
      ;;
    shared-auth-compat)
      copy_concern "$templates/shared-auth.toml" "$root/.auth-shared.toml"
      ;;
    *)
      echo "internal concern preflight invariant failed: $concern" >&2
      exit 70
      ;;
  esac
  IFS=','
done
IFS=$old_ifs
