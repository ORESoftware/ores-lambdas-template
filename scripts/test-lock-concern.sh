#!/bin/sh
set -eu

repo=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

cp -R "$repo/config" "$tmp/config"
cp "$repo/.ores-otel.toml" "$tmp/.ores-otel.toml"

sh "$repo/scripts/enable-concern.sh" "$tmp" locks
test -s "$tmp/.ores-lock.toml"

python3 - "$repo/config/concerns/catalog.toml" "$tmp/.ores-lock.toml" <<'PY'
import pathlib
import re
import sys
import tomllib

catalog_path = pathlib.Path(sys.argv[1])
config_path = pathlib.Path(sys.argv[2])
catalog = tomllib.loads(catalog_path.read_text(encoding="utf-8"))
config = tomllib.loads(config_path.read_text(encoding="utf-8"))

locks = next(item for item in catalog["concerns"] if item["id"] == "locks")
assert locks["canonical_file"] == ".ores-lock.toml"
assert locks["mode"] == "optional"
assert locks["template"] == "config/concerns/templates/ores-lock.toml"
assert locks["tjsv_required"] is True
assert re.fullmatch(r"[0-9a-f]{40}", locks["source_revision"])
assert locks["typespec"] == "contracts/lock-config/typespec/main.tsp"
assert locks["json_schema"] == "contracts/lock-config/json-schema/contract.schema.json"

assert config["schema_version"] == "ores.lock.config.v1"
assert config["default_profile"] == "service-composed"
assert "selected_profile_env" not in config, "single-profile Lambda template needs no selector"
assert len(config["profiles"]) == 1
profile = config["profiles"][0]
assert profile["profile_id"] == "service-composed"
assert profile["providers"] == {
    "local_file": False,
    "fiducia": True,
    "pg_advisory": True,
}
assert "local_file" not in profile
assert profile["postgres"]["scope"] == "transaction"
assert 0 < profile["renew_interval_ms"] <= profile["ttl_ms"] // 2

bindings = {entry["key"]: entry for entry in config["env"]}
assert set(bindings) == {"FIDUCIA_BASE_URL", "FIDUCIA_AUTH_TOKEN", "DATABASE_URL"}
assert bindings["FIDUCIA_BASE_URL"]["secret"] is False
assert bindings["FIDUCIA_AUTH_TOKEN"]["secret"] is True
assert bindings["DATABASE_URL"]["secret"] is True
assert profile["fiducia"]["endpoint_env"] in bindings
assert bindings[profile["fiducia"]["auth_token_env"]]["secret"] is True
assert bindings[profile["postgres"]["database_url_env"]]["secret"] is True

raw = config_path.read_text(encoding="utf-8")
for forbidden in ("postgres://", "postgresql://", "Bearer ", "redis://", "rediss://"):
    assert forbidden not in raw, f"lock template embeds forbidden secret/endpoint material: {forbidden}"
PY

# Materialization is additive and must not overwrite an existing config.
if sh "$repo/scripts/enable-concern.sh" "$tmp" locks; then
  echo "lock concern unexpectedly overwrote existing .ores-lock.toml" >&2
  exit 1
fi
