#!/usr/bin/env python3
"""Fail-closed bootstrap mirror of ores-lambda repository admission.

Canonical authority: ORESoftware/ores-lambda@cc7e5da3b42a75f8b172d85bf13a0c2844c2d431.
This mirror exists only because consumer CI cannot read the private sibling repository and the
central workflow is dispatch-only. Remove it once oresoftware/ores-lambda is consumable through
its reviewed Zed package or an automatically triggered central-authority receipt.

The mirror intentionally includes the repo-owned Cargo-entrypoint rule being hardened in
ORESoftware/ores-lambda#9; that rule is stricter than the pinned main authority, never weaker.
"""

from __future__ import annotations

import argparse
import ipaddress
import re
import stat
import sys
import tomllib
from pathlib import Path

AUTHORITY_SHA = "cc7e5da3b42a75f8b172d85bf13a0c2844c2d431"
MANIFEST_SCHEMA = "ores.lambda.manifest/v1"
MIDDLEWARE_ZPKG_PACKAGE = "oresoftware/ores-middleware"
MIDDLEWARE_CARGO_PACKAGE = "ores-middleware"
TRIGGERS = {"http", "queue", "schedule", "direct"}
SOURCE_KINDS = {"api_server", "web_server", "custom"}
SAFE_ID = re.compile(r"^[a-z0-9_-]+$")
METHOD = re.compile(r"^[A-Z]+$")
MAX_ROOT_TOML_BYTES = 2 * 1024 * 1024


class AdmissionError(RuntimeError):
    pass


def fail(message: str) -> None:
    raise AdmissionError(message)


def regular_file(path: Path, label: str) -> Path:
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        fail(f"required {label} is missing: {path}")
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
        fail(f"{label} must be a regular non-symlink file: {path}")
    return path


def real_dir(path: Path, label: str) -> Path:
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        fail(f"required {label} is missing: {path}")
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
        fail(f"{label} must be a real non-symlink directory: {path}")
    return path


def optional_regular_file(path: Path, label: str) -> bool:
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return False
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
        fail(f"{label} must be a regular non-symlink file: {path}")
    return True


def safe_relative(root: Path, value: str, label: str) -> Path:
    if not value or value.startswith("/"):
        fail(f"{label} must be a non-empty relative path: {value!r}")
    parts = Path(value).parts
    if any(part in {"..", ""} for part in parts):
        fail(f"{label} must not traverse outside the repository: {value!r}")
    return root / value


def load_toml(path: Path, label: str) -> dict:
    regular_file(path, label)
    if path.stat().st_size > MAX_ROOT_TOML_BYTES:
        fail(f"{label} exceeds {MAX_ROOT_TOML_BYTES} bytes: {path}")
    try:
        with path.open("rb") as handle:
            document = tomllib.load(handle)
    except (tomllib.TOMLDecodeError, OSError) as error:
        fail(f"cannot parse {label}: {error}")
    if not isinstance(document, dict):
        fail(f"{label} must decode to a TOML table")
    return document


def cargo_declares_package(cargo: dict, package: str) -> bool:
    dependencies = cargo.get("dependencies")
    if not isinstance(dependencies, dict):
        return False
    for key, value in dependencies.items():
        if key == package:
            return True
        if isinstance(value, dict) and value.get("package") == package:
            return True
    return False


def cargo_binary_names(root: Path, cargo: dict) -> set[str]:
    names: set[str] = set()
    binaries = cargo.get("bin", [])
    if binaries is None:
        binaries = []
    if not isinstance(binaries, list):
        fail("Cargo.toml [[bin]] must be an array of tables")
    for index, binary in enumerate(binaries):
        if not isinstance(binary, dict):
            fail(f"Cargo [[bin]] entry {index} must be a table")
        name = binary.get("name")
        if not isinstance(name, str) or not name.strip():
            fail(f"Cargo [[bin]] entry {index} must declare a non-empty name")
        path = binary.get("path")
        if path is not None:
            if not isinstance(path, str):
                fail(f"Cargo [[bin]] {name!r} path must be a string")
            regular_file(safe_relative(root, path, f"Cargo [[bin]] {name!r} path"), f"Cargo [[bin]] source for {name}")
        names.add(name)

    package = cargo.get("package")
    if isinstance(package, dict) and optional_regular_file(root / "src/main.rs", "Cargo src/main.rs"):
        name = package.get("name")
        if isinstance(name, str) and name.strip():
            names.add(name)

    bin_dir = root / "src/bin"
    if bin_dir.exists() or bin_dir.is_symlink():
        real_dir(bin_dir, "src/bin")
        for candidate in sorted(bin_dir.iterdir(), key=lambda path: path.name):
            metadata = candidate.lstat()
            if stat.S_ISLNK(metadata.st_mode):
                fail(f"Cargo binary candidate must not be a symlink: {candidate}")
            if stat.S_ISREG(metadata.st_mode) and candidate.suffix == ".rs":
                names.add(candidate.stem)
            elif stat.S_ISDIR(metadata.st_mode) and optional_regular_file(candidate / "main.rs", f"Cargo binary {candidate.name} main.rs"):
                names.add(candidate.name)
    return names


def require_mapping(value: object, label: str) -> dict:
    if not isinstance(value, dict):
        fail(f"{label} must be a table")
    return value


def require_positive_int(value: object, label: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        fail(f"{label} must be a positive integer")
    return value


def admit_middleware(root: Path, manifest: dict) -> tuple[Path, Path]:
    policy = require_mapping(manifest.get("middleware"), "middleware")
    expected = {
        "required": True,
        "package": MIDDLEWARE_ZPKG_PACKAGE,
        "config_path": ".ores-mw.toml",
        "execution": "invocation",
        "fail_closed": True,
    }
    for key, value in expected.items():
        if policy.get(key) != value:
            fail(f"middleware.{key} must be {value!r}, got {policy.get(key)!r}")

    middleware_path = regular_file(root / ".ores-mw.toml", "middleware config")
    middleware = load_toml(middleware_path, "middleware config")
    default_target = middleware.get("default_target")
    targets = middleware.get("targets")
    if not isinstance(default_target, str) or not default_target:
        fail(".ores-mw.toml must declare non-empty default_target")
    if not isinstance(targets, list):
        fail(".ores-mw.toml must declare [[targets]]")
    matches = [target for target in targets if isinstance(target, dict) and target.get("name") == default_target]
    if len(matches) != 1:
        fail(f".ores-mw.toml default_target {default_target!r} must resolve exactly once")
    stack_value = matches[0].get("stack_config")
    if not isinstance(stack_value, str):
        fail("default middleware target must declare stack_config")
    stack_path = regular_file(safe_relative(root, stack_value, "middleware stack path"), "middleware stack config")
    return middleware_path, stack_path


def admit_local_ingress(manifest: dict) -> None:
    ingress = require_mapping(manifest.get("local_ingress"), "local_ingress")
    if ingress.get("enabled") is not True:
        fail("local_ingress.enabled must be true for the generated template")
    bind = ingress.get("bind")
    if not isinstance(bind, str) or ":" not in bind:
        fail("local_ingress.bind must be an IP socket address")
    host, _, port_text = bind.rpartition(":")
    try:
        ipaddress.ip_address(host)
        port = int(port_text)
    except ValueError as error:
        fail(f"local_ingress.bind must be an IP socket address: {error}")
    if not 1 <= port <= 65535:
        fail("local_ingress.bind port must be non-zero and <= 65535")
    for field in ("request_timeout_ms", "max_request_body_bytes", "max_response_body_bytes", "max_attempts"):
        require_positive_int(ingress.get(field), f"local_ingress.{field}")


def admit_function(root: Path, repository_name: str, cargo_bins: set[str], function: object, index: int, ids: set[str]) -> None:
    function = require_mapping(function, f"functions[{index}]")
    function_id = function.get("id")
    if not isinstance(function_id, str) or SAFE_ID.fullmatch(function_id) is None:
        fail(f"functions[{index}].id must use lowercase ASCII letters, digits, '-' or '_'")
    if function_id in ids:
        fail(f"duplicate function id: {function_id!r}")
    ids.add(function_id)

    entrypoint = function.get("entrypoint")
    if not isinstance(entrypoint, str) or not entrypoint.strip():
        fail(f"function {function_id!r} entrypoint must be non-empty")
    trigger = function.get("trigger")
    if trigger not in TRIGGERS:
        fail(f"function {function_id!r} has unsupported trigger {trigger!r}")
    if function.get("middleware_required") is not True:
        fail(f"function {function_id!r} must require invocation middleware")

    source = require_mapping(function.get("source"), f"function {function_id!r} source")
    kind = source.get("kind")
    source_repository = source.get("repository")
    source_path = source.get("path")
    if kind not in SOURCE_KINDS:
        fail(f"function {function_id!r} has unsupported source kind {kind!r}")
    if not isinstance(source_repository, str) or not source_repository:
        fail(f"function {function_id!r} source.repository must be non-empty")
    if not isinstance(source_path, str):
        fail(f"function {function_id!r} source.path must be a string")
    safe_relative(root, source_path, f"function {function_id!r} source.path")
    if kind == "api_server" and not source_repository.endswith("-api-server.rs"):
        fail(f"api_server function {function_id!r} source repository must end with -api-server.rs")
    if kind == "web_server" and not source_repository.endswith("-web-server.rs"):
        fail(f"web_server function {function_id!r} source repository must end with -web-server.rs")
    if kind == "custom":
        if source_repository != repository_name:
            fail(f"custom function {function_id!r} must be owned by {repository_name!r}")
        regular_file(root / source_path, f"custom Lambda source for {function_id}")
        if entrypoint not in cargo_bins:
            fail(f"custom Lambda {function_id!r} entrypoint {entrypoint!r} is not a Cargo binary")

    local = function.get("local")
    http = function.get("http")
    if trigger == "http":
        local = require_mapping(local, f"HTTP function {function_id!r} local")
        http = require_mapping(http, f"HTTP function {function_id!r} http")
        command = local.get("command")
        if not isinstance(command, list) or not command or not all(isinstance(arg, str) and arg for arg in command):
            fail(f"HTTP function {function_id!r} local.command must contain non-empty argv")
        if "--bin" in command:
            position = command.index("--bin")
            if position + 1 >= len(command) or command[position + 1] != entrypoint:
                fail(f"HTTP function {function_id!r} local command must run entrypoint {entrypoint!r}")
        host = local.get("host")
        try:
            ipaddress.ip_address(host)
        except (ValueError, TypeError) as error:
            fail(f"HTTP function {function_id!r} local.host must be an IP address: {error}")
        require_positive_int(local.get("start_port"), f"HTTP function {function_id!r} local.start_port")
        replicas = require_positive_int(local.get("replicas"), f"HTTP function {function_id!r} local.replicas")
        if replicas > 32:
            fail(f"HTTP function {function_id!r} local.replicas exceeds ores-compose limit 32")
        if not isinstance(local.get("bind_env"), str) or not local["bind_env"].strip():
            fail(f"HTTP function {function_id!r} local.bind_env must be non-empty")
        prefix = http.get("ingress_path_prefix")
        if not isinstance(prefix, str) or prefix == "/" or not prefix.startswith("/") or prefix.endswith("/"):
            fail(f"HTTP function {function_id!r} ingress path must be a non-root absolute prefix without trailing slash")
        methods = http.get("methods")
        if not isinstance(methods, list) or not methods or any(not isinstance(method, str) or METHOD.fullmatch(method) is None for method in methods):
            fail(f"HTTP function {function_id!r} methods must be non-empty uppercase tokens")
    elif http is not None:
        fail(f"non-HTTP function {function_id!r} must not declare an http section")


def admit_repository(root: Path, authority_sha: str) -> None:
    if authority_sha != AUTHORITY_SHA:
        fail(f"bootstrap authority drift: expected {AUTHORITY_SHA}, got {authority_sha}")
    real_dir(root, "repository root")
    for boundary in ("contracts", "conformance"):
        real_dir(root / boundary, boundary)

    manifest = load_toml(root / ".ores-lambda.toml", ".ores-lambda.toml")
    if manifest.get("schema_version") != MANIFEST_SCHEMA:
        fail(f"schema_version must be {MANIFEST_SCHEMA!r}")
    repository = require_mapping(manifest.get("repository"), "repository")
    organization = repository.get("organization")
    repository_name = repository.get("repository")
    if not isinstance(organization, str) or not organization.strip():
        fail("repository.organization must be non-empty")
    if not isinstance(repository_name, str) or not repository_name.endswith(("-lambdas", "-lambda")):
        fail("repository.repository must end with -lambdas or -lambda")

    admit_local_ingress(manifest)
    middleware_path, stack_path = admit_middleware(root, manifest)

    cargo = load_toml(root / "Cargo.toml", "Cargo.toml")
    if not cargo_declares_package(cargo, MIDDLEWARE_CARGO_PACKAGE):
        fail(f"Cargo.toml must declare canonical {MIDDLEWARE_CARGO_PACKAGE!r} dependency")
    regular_file(root / "Cargo.lock", "Cargo.lock")
    cargo_bins = cargo_binary_names(root, cargo)
    if not cargo_bins:
        fail("Cargo.toml/src layout exposes no runnable binary entrypoints")

    zpkg = load_toml(root / ".zpkg.toml", ".zpkg.toml")
    dependencies = zpkg.get("dependencies")
    if not isinstance(dependencies, dict) or MIDDLEWARE_ZPKG_PACKAGE not in dependencies:
        fail(f".zpkg.toml must declare {MIDDLEWARE_ZPKG_PACKAGE!r}")
    targets = zpkg.get("targets")
    if not isinstance(targets, dict):
        fail(".zpkg.toml must declare [targets]")
    for target in ("contracts", "conformance"):
        if target not in targets:
            fail(f".zpkg.toml must publish [targets.{target}]")

    functions = manifest.get("functions")
    if not isinstance(functions, list) or not functions:
        fail("manifest must declare at least one [[functions]] entry")
    ids: set[str] = set()
    for index, function in enumerate(functions):
        admit_function(root, repository_name, cargo_bins, function, index, ids)

    print(
        "fleet repository admitted: "
        f"authority={AUTHORITY_SHA} repository={organization}/{repository_name} "
        f"functions={len(functions)} cargo_bins={len(cargo_bins)} "
        f"middleware={middleware_path.relative_to(root)} stack={stack_path.relative_to(root)}"
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--authority-sha", default=AUTHORITY_SHA)
    args = parser.parse_args()
    root = Path(args.root).resolve()
    try:
        admit_repository(root, args.authority_sha)
    except AdmissionError as error:
        print(f"fleet repository admission failed: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
