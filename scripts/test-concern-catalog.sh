#!/bin/sh
set -eu

repo=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp" "$tmp-compat" "$tmp-blocked" "$tmp-dst-link" "$tmp-src-link" "$tmp-dir-link" "$tmp-otel-link" "$tmp-outside"' EXIT INT TERM

cp -R "$repo/config" "$tmp/config"
cp "$repo/.ores-otel.toml" "$tmp/.ores-otel.toml"

python3 - "$repo/config/concerns/catalog.toml" <<'PY'
import pathlib
import re
import sys
import tomllib

path = pathlib.Path(sys.argv[1])
doc = tomllib.loads(path.read_text(encoding="utf-8"))
assert doc.get("schema_version") == 1
concerns = doc.get("concerns")
assert isinstance(concerns, list) and concerns
ids = set()
files = set()
aliases = set()
for concern in concerns:
    cid = concern.get("id")
    canonical = concern.get("canonical_file")
    mode = concern.get("mode")
    owner = concern.get("owner")
    template = concern.get("template")
    assert isinstance(cid, str) and cid and cid not in ids
    assert isinstance(canonical, str) and canonical.startswith(".") and canonical.endswith(".toml")
    assert canonical not in files and canonical not in aliases
    assert isinstance(owner, str) and owner.startswith("https://github.com/")
    assert mode in {"required", "optional", "owner-schema-required"}
    assert isinstance(template, str)

    revision = concern.get("source_revision")
    if revision is not None:
        assert isinstance(revision, str) and re.fullmatch(r"[0-9a-f]{40}", revision), (
            f"{cid}: source_revision must be an immutable lowercase 40-hex commit"
        )

    if template:
        template_path = pathlib.PurePosixPath(template)
        assert not template_path.is_absolute(), f"{cid}: template must be repository-relative"
        assert ".." not in template_path.parts, f"{cid}: template may not traverse parents"

    if mode == "optional":
        assert template, f"optional concern {cid} must have a reviewed template"
        assert (path.parent.parent.parent / template).is_file(), template
    if mode == "owner-schema-required":
        assert not template, f"unadmitted concern {cid} must not fabricate a template"

    for alias in concern.get("aliases", []):
        assert isinstance(alias, str) and alias.startswith(".") and alias.endswith(".toml")
        assert alias != canonical
        assert alias not in files and alias not in aliases
        aliases.add(alias)

    if concern.get("tjsv_required") is True:
        assert revision, f"{cid}: TJSV-gated concern requires an exact owner revision"
        typespec = concern.get("typespec")
        schema = concern.get("json_schema")
        assert isinstance(typespec, str) and typespec.startswith("contracts/") and typespec.endswith(".tsp"), (
            f"{cid}: missing owner TypeSpec authority path"
        )
        assert isinstance(schema, str) and schema.startswith("contracts/") and schema.endswith(".json"), (
            f"{cid}: missing owner authored JSON Schema authority path"
        )

    ids.add(cid)
    files.add(canonical)

shared = next(item for item in concerns if item["id"] == "shared-auth")
assert shared["canonical_file"] == ".shared-auth.toml"
assert shared.get("aliases") == [".auth-shared.toml"]

for cid in ("rate-limit", "chat"):
    concern = next(item for item in concerns if item["id"] == cid)
    assert concern.get("tjsv_required") is True

blocked = {
    item["id"] for item in concerns if item["mode"] == "owner-schema-required"
}
assert blocked == {"redis-lru", "forms", "opto-sync", "legal", "wasm", "rpc", "fanwaave"}
PY

sh "$repo/scripts/enable-concern.sh" "$tmp" "otel,middleware,rate-limit,chat,shared-auth"
for file in .ores-otel.toml .ores-mw.toml .ores-rl.toml .ores-chat.toml .shared-auth.toml; do
  test -s "$tmp/$file"
done
test ! -e "$tmp/.auth-shared.toml"

python3 - "$tmp" <<'PY'
import pathlib
import sys
import tomllib

root = pathlib.Path(sys.argv[1])
for path in root.glob(".*.toml"):
    tomllib.loads(path.read_text(encoding="utf-8"))

shared = tomllib.loads((root / ".shared-auth.toml").read_text(encoding="utf-8"))
assert shared["compatibility"]["repository"] == "https://github.com/shared-auth/shared-auth-interfaces"
assert shared["factors"]["two_factor"]["required"] is True

chat = tomllib.loads((root / ".ores-chat.toml").read_text(encoding="utf-8"))
assert chat["flags2env"]["contract"] == ".cli-flags.toml"
assert chat["strict"] is True

rate = tomllib.loads((root / ".ores-rl.toml").read_text(encoding="utf-8"))
for policy in rate["policies"]:
    assert policy["backendFailureMode"] == "fail-closed"
PY

compat="$tmp-compat"
mkdir -p "$compat/config/concerns/templates"
cp "$repo/config/concerns/templates/shared-auth.toml" "$compat/config/concerns/templates/shared-auth.toml"
cp "$repo/.ores-otel.toml" "$compat/.ores-otel.toml"
sh "$repo/scripts/enable-concern.sh" "$compat" "shared-auth-compat"
test -s "$compat/.auth-shared.toml"
test ! -e "$compat/.shared-auth.toml"
if sh "$repo/scripts/enable-concern.sh" "$compat" "shared-auth"; then
  echo "canonical and compatibility Shared Auth files must not coexist" >&2
  exit 1
fi

blocked="$tmp-blocked"
mkdir -p "$blocked/config/concerns/templates"
cp "$repo/.ores-otel.toml" "$blocked/.ores-otel.toml"
for concern in redis-lru forms opto-sync legal wasm rpc fanwaave; do
  if sh "$repo/scripts/enable-concern.sh" "$blocked" "$concern"; then
    echo "unadmitted concern unexpectedly materialized: $concern" >&2
    exit 1
  fi
done

# A dangling destination symlink must not bypass the ordinary -e overwrite guard.
dst_link="$tmp-dst-link"
outside="$tmp-outside"
mkdir -p "$dst_link/config/concerns/templates"
cp "$repo/config/concerns/templates/ores-mw.toml" "$dst_link/config/concerns/templates/ores-mw.toml"
cp "$repo/.ores-otel.toml" "$dst_link/.ores-otel.toml"
ln -s "$outside" "$dst_link/.ores-mw.toml"
if sh "$repo/scripts/enable-concern.sh" "$dst_link" "middleware"; then
  echo "dangling destination symlink unexpectedly accepted" >&2
  exit 1
fi
test ! -e "$outside"

# Reviewed templates themselves must be regular files, never symlinks.
src_link="$tmp-src-link"
mkdir -p "$src_link/config/concerns/templates"
cp "$repo/.ores-otel.toml" "$src_link/.ores-otel.toml"
ln -s "$repo/config/concerns/templates/ores-mw.toml" "$src_link/config/concerns/templates/ores-mw.toml"
if sh "$repo/scripts/enable-concern.sh" "$src_link" "middleware"; then
  echo "symlinked concern template unexpectedly accepted" >&2
  exit 1
fi

# The template directory and required OTEL baseline are trust boundaries too.
dir_link="$tmp-dir-link"
mkdir -p "$dir_link/config/concerns"
cp "$repo/.ores-otel.toml" "$dir_link/.ores-otel.toml"
ln -s "$repo/config/concerns/templates" "$dir_link/config/concerns/templates"
if sh "$repo/scripts/enable-concern.sh" "$dir_link" "middleware"; then
  echo "symlinked template directory unexpectedly accepted" >&2
  exit 1
fi

otel_link="$tmp-otel-link"
mkdir -p "$otel_link/config/concerns/templates"
ln -s "$repo/.ores-otel.toml" "$otel_link/.ores-otel.toml"
if sh "$repo/scripts/enable-concern.sh" "$otel_link" "otel"; then
  echo "symlinked required baseline unexpectedly accepted" >&2
  exit 1
fi
