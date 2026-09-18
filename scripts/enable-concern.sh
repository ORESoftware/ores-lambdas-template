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
[ -d "$templates" ] && [ ! -L "$templates" ] || {
  echo "missing or unsafe concern templates directory: $templates" >&2
  exit 2
}

copy_concern() {
  source=$1
  target=$2
  [ -f "$source" ] && [ ! -L "$source" ] || {
    echo "missing or unsafe reviewed concern template: $source" >&2
    exit 3
  }
  # -e follows symlinks, so a dangling destination symlink would otherwise
  # bypass the overwrite guard and let cp write outside the generated repo.
  [ ! -e "$target" ] && [ ! -L "$target" ] || {
    echo "refusing to overwrite existing or symlinked concern config: $target" >&2
    exit 4
  }
  cp "$source" "$target"
}

old_ifs=$IFS
IFS=','
for concern in $requested; do
  IFS=$old_ifs
  concern=$(printf '%s' "$concern" | tr -d '[:space:]')
  [ -n "$concern" ] || continue
  case "$concern" in
    otel)
      [ -f "$root/.ores-otel.toml" ] && [ ! -L "$root/.ores-otel.toml" ] || {
        echo "required baseline missing or unsafe: .ores-otel.toml" >&2
        exit 5
      }
      ;;
    middleware)
      # .ores-mw.toml references a consumer-owned stack JSON; never emit a dangling
      # reference, and never admit a test-shaped stack into a deployable function.
      stack="$root/config/ores-middleware.stack.json"
      [ -f "$stack" ] && [ ! -L "$stack" ] || {
        echo "middleware requires a reviewed, regular config/ores-middleware.stack.json (owner MiddlewareStackConfig)" >&2
        exit 8
      }
      if grep -Eq '"environment"[[:space:]]*:[[:space:]]*"test"|test-auth-bypass|fault-injection' "$stack"; then
        echo "refusing test-only middleware stack (environment=test, test-auth-bypass or fault-injection)" >&2
        exit 8
      fi
      copy_concern "$templates/ores-mw.toml" "$root/.ores-mw.toml"
      ;;
    redis-lru)
      copy_concern "$templates/ores-lru.toml" "$root/.ores-lru.toml"
      ;;
    rate-limit)
      copy_concern "$templates/ores-rl.toml" "$root/.ores-rl.toml"
      ;;
    locks)
      copy_concern "$templates/ores-lock.toml" "$root/.ores-lock.toml"
      ;;
    chat)
      copy_concern "$templates/ores-chat.toml" "$root/.ores-chat.toml"
      ;;
    shared-auth)
      [ ! -e "$root/.auth-shared.toml" ] && [ ! -L "$root/.auth-shared.toml" ] || {
        echo "legacy .auth-shared.toml conflicts with canonical .shared-auth.toml" >&2
        exit 6
      }
      copy_concern "$templates/shared-auth.toml" "$root/.shared-auth.toml"
      ;;
    shared-auth-compat)
      [ ! -e "$root/.shared-auth.toml" ] && [ ! -L "$root/.shared-auth.toml" ] || {
        echo "canonical .shared-auth.toml conflicts with compatibility .auth-shared.toml" >&2
        exit 6
      }
      copy_concern "$templates/shared-auth.toml" "$root/.auth-shared.toml"
      ;;
    forms|opto-sync|legal|wasm|rpc|fanwaave)
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
