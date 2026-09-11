#!/usr/bin/env python3
"""Execute the repository .ores-otel.toml as a cold-start config contract.

This test goes beyond TOML parsing: it resolves declared environment bindings,
coerces typed defaults/environment values, follows the server binding names,
and proves exporter values are supplied at runtime without exposing secrets.
"""

from __future__ import annotations

import json
import math
import os
from pathlib import Path
import tomllib
from typing import Any, Mapping
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parents[1]
CONFIG_PATH = ROOT / ".ores-otel.toml"


def _parse_bool(value: str) -> bool:
    lowered = value.strip().lower()
    if lowered in {"1", "true", "yes", "on"}:
        return True
    if lowered in {"0", "false", "no", "off"}:
        return False
    raise ValueError(f"invalid boolean: {value!r}")


def _coerce(kind: str, raw: str) -> Any:
    if kind == "string":
        return raw
    if kind == "bool":
        return _parse_bool(raw)
    if kind == "integer":
        return int(raw, 10)
    if kind == "double":
        value = float(raw)
        if not math.isfinite(value):
            raise ValueError("double must be finite")
        return value
    if kind == "json":
        return json.loads(raw)
    if kind == "url":
        parsed = urlparse(raw)
        if parsed.scheme not in {"http", "https"} or not parsed.netloc:
            raise ValueError(f"invalid HTTP(S) URL: {raw!r}")
        return raw
    raise ValueError(f"unsupported .ores-otel.toml kind: {kind!r}")


def resolve_runtime_config(
    path: Path = CONFIG_PATH,
    environ: Mapping[str, str] = os.environ,
) -> dict[str, Any]:
    with path.open("rb") as handle:
        document = tomllib.load(handle)

    if document.get("version") != 1:
        raise AssertionError(".ores-otel.toml must declare version = 1")
    if document.get("mode") != "server" or document.get("strict") is not True:
        raise AssertionError(".ores-otel.toml must be strict server mode")

    flags = document.get("flags2env")
    expected_flags = {
        "contract": ".cli-flags.toml",
        "require_audit": True,
        "precedence": "argv-over-env",
    }
    if flags != expected_flags:
        raise AssertionError("flags2env envelope is not canonical for the lambda template")

    bindings: dict[str, dict[str, Any]] = {}
    values: dict[str, Any] = {}
    sources: dict[str, str] = {}

    for item in document.get("env", []):
        if not isinstance(item, dict):
            raise AssertionError("env entries must be TOML tables")
        name = item.get("name")
        key = item.get("key")
        kind = item.get("kind")
        if not all(isinstance(value, str) and value for value in (name, key, kind)):
            raise AssertionError("env entries require non-empty name/key/kind strings")
        if name in bindings:
            raise AssertionError(f"duplicate binding name: {name}")
        if any(existing.get("key") == key for existing in bindings.values()):
            raise AssertionError(f"duplicate environment key: {key}")

        secret = item.get("secret") is True
        if secret and ("default" in item or "default_value" in item):
            raise AssertionError(f"secret binding {name} may not have a plaintext default")

        bindings[name] = item
        raw = environ.get(key)
        if raw is not None:
            values[name] = _coerce(kind, raw)
            sources[name] = "environment"
        elif "default" in item:
            default = item["default"]
            if not isinstance(default, str):
                raise AssertionError(f"binding {name} default must be encoded as a string")
            values[name] = _coerce(kind, default)
            sources[name] = "default"
        elif item.get("required") is True:
            raise AssertionError(f"required environment key is missing: {key}")
        else:
            values[name] = None
            sources[name] = "unset"

    server = document.get("server")
    if not isinstance(server, dict) or server.get("enabled") is not True:
        raise AssertionError("server telemetry must be enabled")

    field_bindings = {
        "service_name": "service_name_binding",
        "log_filter": "log_filter_binding",
        "sample_ratio": "sample_ratio_binding",
        "otlp_endpoint": "otlp_endpoint_binding",
        "otlp_headers": "otlp_headers_binding",
        "batch_max_queue": "batch_max_queue_binding",
        "redact_fields": "redact_fields_binding",
    }

    resolved: dict[str, Any] = {"enabled": True}
    resolved_sources: dict[str, str] = {}
    for field, binding_field in field_bindings.items():
        binding_name = server.get(binding_field)
        if not isinstance(binding_name, str) or binding_name not in bindings:
            raise AssertionError(f"server.{binding_field} must reference a declared env binding")
        resolved[field] = values[binding_name]
        resolved_sources[field] = sources[binding_name]

    ratio = resolved["sample_ratio"]
    if not isinstance(ratio, float) or not 0.0 <= ratio <= 1.0:
        raise AssertionError("resolved sample ratio must be within 0..=1")
    queue = resolved["batch_max_queue"]
    if not isinstance(queue, int) or isinstance(queue, bool) or queue <= 0:
        raise AssertionError("resolved batch_max_queue must be a positive integer")
    redact = resolved["redact_fields"]
    if not isinstance(redact, list) or not all(isinstance(item, str) for item in redact):
        raise AssertionError("resolved redact_fields must be a JSON string array")

    return {
        "resolved": resolved,
        "sources": resolved_sources,
        "secret_fields": [
            field
            for field, binding_field in field_bindings.items()
            if bindings[server[binding_field]].get("secret") is True
        ],
    }


def main() -> None:
    endpoint = "https://collector.invalid/lambda-runtime-config-test"
    headers = "authorization=Bearer runtime-only-test-secret"
    environ = dict(os.environ)
    environ["OTEL_EXPORTER_OTLP_ENDPOINT"] = endpoint
    environ["OTEL_EXPORTER_OTLP_HEADERS"] = headers

    receipt = resolve_runtime_config(environ=environ)
    resolved = receipt["resolved"]
    sources = receipt["sources"]

    if resolved["service_name"] != "__PREFIX__-lambdas":
        raise AssertionError("template service name default was not consumed")
    if resolved["log_filter"] != "info":
        raise AssertionError("template log filter default was not consumed")
    if resolved["sample_ratio"] != 1.0:
        raise AssertionError("template sample ratio default was not consumed")
    if resolved["otlp_endpoint"] != endpoint or sources["otlp_endpoint"] != "environment":
        raise AssertionError("OTLP endpoint environment binding was not consumed")
    if resolved["otlp_headers"] != headers or sources["otlp_headers"] != "environment":
        raise AssertionError("OTLP header environment binding was not consumed")
    if "otlp_headers" not in receipt["secret_fields"]:
        raise AssertionError("OTLP headers must remain classified as secret")

    raw_config = CONFIG_PATH.read_text(encoding="utf-8")
    if endpoint in raw_config or headers in raw_config:
        raise AssertionError("runtime exporter values must not be embedded in .ores-otel.toml")

    safe_resolved = dict(resolved)
    safe_resolved["otlp_endpoint"] = "<resolved-from-environment>"
    safe_resolved["otlp_headers"] = "<redacted>"
    print(json.dumps({"resolved": safe_resolved, "sources": sources}, sort_keys=True))


if __name__ == "__main__":
    main()
