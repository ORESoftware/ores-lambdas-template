#!/usr/bin/env bash
set -euo pipefail

ORESC_BIN="${ORESC_BIN:-oresc}"
REPORT_DIR="${ORESC_REPORT_DIR:-target/oresc-audit}"

if ! command -v "$ORESC_BIN" >/dev/null 2>&1; then
  echo "oresc is required; install the canonical ORESoftware/ores-cli package before running this audit" >&2
  exit 70
fi

mkdir -p "$REPORT_DIR"

echo "[oresc] repository standards"
"$ORESC_BIN" --no-json audit repo --path . --profile standards

echo "[oresc] TypeSpec / JSON Schema peer-authority admission"
"$ORESC_BIN" --no-json audit contract \
  --typespec schema-authority/main.tsp \
  --schema schema-authority/authored.schema.json \
  --report "$REPORT_DIR/schema-authority.json"

# This repository is a template: .zpkg.toml intentionally contains __ORG__ and
# __PREFIX__ placeholders until scaffolding. Running `audit package` against the
# unmaterialized template would conflate template placeholders with package drift.
echo "[oresc] package audit intentionally deferred until the template is materialized"
