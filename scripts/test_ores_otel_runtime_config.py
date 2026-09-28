#!/usr/bin/env python3
"""Exercise the owner-authoritative .ores-otel.toml consumer boundary."""

from __future__ import annotations

import os
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[1]
CONFIG_PATH = ROOT / ".ores-otel.toml"
CLI_PATH = ROOT / ".cli-flags.toml"


def resolve_runtime_config(path: Path = CONFIG_PATH) -> dict[str, object]:
    with path.open("rb") as handle:
        document = tomllib.load(handle)

    if set(document) - {"version", "common", "client", "server"}:
        raise AssertionError(".ores-otel.toml contains keys outside OresOtelConfigV1")
    if document.get("version") != 1:
        raise AssertionError(".ores-otel.toml must declare version = 1")

    server = document.get("server")
    if not isinstance(server, dict) or server.get("enabled") is not True:
        raise AssertionError("server telemetry must be enabled")
    if server.get("service_name") != "__PREFIX__-lambdas":
        raise AssertionError("template service name must remain deterministic")

    logging = server.get("logging")
    if not isinstance(logging, dict) or logging.get("enabled") is not True:
        raise AssertionError("server logging must be enabled")
    if logging.get("level") != "info":
        raise AssertionError("default log level must remain info")

    tracing = server.get("tracing")
    if not isinstance(tracing, dict) or tracing.get("enabled") is not True:
        raise AssertionError("server tracing must be enabled")
    ratio = tracing.get("sample_ratio")
    if not isinstance(ratio, float) or not 0.0 <= ratio <= 1.0:
        raise AssertionError("sample_ratio must be within 0..=1")

    exporter = server.get("exporter")
    if not isinstance(exporter, dict):
        raise AssertionError("server exporter policy is required")
    if exporter.get("protocol") != "otlp_http":
        raise AssertionError("lambda template uses the portable OTLP HTTP profile")
    if exporter.get("endpoint_env") != "OTEL_EXPORTER_OTLP_ENDPOINT":
        raise AssertionError("exporter endpoint must stay environment-bound")

    return document


def main() -> None:
    document = resolve_runtime_config()
    raw = CONFIG_PATH.read_text(encoding="utf-8")
    cli = CLI_PATH.read_text(encoding="utf-8")

    secret = "authorization=Bearer runtime-only-test-secret"
    environ = dict(os.environ)
    environ["OTEL_EXPORTER_OTLP_HEADERS"] = secret
    if environ["OTEL_EXPORTER_OTLP_HEADERS"] != secret:
        raise AssertionError("runtime secret fixture was not installed")

    if secret in raw:
        raise AssertionError("runtime exporter secret must never be embedded in .ores-otel.toml")
    if "OTEL_EXPORTER_OTLP_HEADERS" in cli:
        raise AssertionError("credential-bearing OTLP headers must not be exposed as a CLI flag")
    if "OTEL_EXPORTER_OTLP_HEADERS" not in raw:
        raise AssertionError("root policy must document the environment-only OTLP header boundary")

    print(
        {
            "version": document["version"],
            "server": True,
            "endpoint_env": document["server"]["exporter"]["endpoint_env"],
            "otlp_headers": "<environment-only:redacted>",
        }
    )


if __name__ == "__main__":
    main()
