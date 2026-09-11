#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

calls="$tmp/calls.txt"
fake_oresc="$tmp/oresc"
cat >"$fake_oresc" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"${ORESC_TEST_CALLS:?}"
report=''
while (($#)); do
  if [[ "$1" == '--report' ]]; then
    shift
    report="${1:?missing report path}"
    break
  fi
  shift
done
if [[ -n "$report" ]]; then
  mkdir -p "$(dirname "$report")"
  printf '{"status":"passed","producer":"fake-oresc"}\n' >"$report"
fi
SH
chmod +x "$fake_oresc"

report_dir="$tmp/reports"
(
  cd "$repo_root"
  ORESC_BIN="$fake_oresc" \
  ORESC_REPORT_DIR="$report_dir" \
  ORESC_TEST_CALLS="$calls" \
    bash scripts/oresc-audit.sh
)

mapfile -t actual <"$calls"
[[ ${#actual[@]} -eq 2 ]] || {
  printf 'expected 2 ores-cli invocations, got %s\n' "${#actual[@]}" >&2
  exit 1
}

[[ "${actual[0]}" == '--no-json audit repo --path . --profile standards' ]] || {
  printf 'unexpected repository audit invocation: %s\n' "${actual[0]}" >&2
  exit 1
}
expected_contract="--no-json audit contract --typespec schema-authority/main.tsp --schema schema-authority/authored.schema.json --report $report_dir/schema-authority.json"
[[ "${actual[1]}" == "$expected_contract" ]] || {
  printf 'unexpected contract audit invocation: %s\n' "${actual[1]}" >&2
  exit 1
}
[[ -s "$report_dir/schema-authority.json" ]] || {
  echo 'expected contract audit receipt was not created' >&2
  exit 1
}

set +e
missing_output="$(
  cd "$repo_root"
  ORESC_BIN="$tmp/does-not-exist" bash scripts/oresc-audit.sh 2>&1
)"
missing_status=$?
set -e
[[ $missing_status -eq 70 ]] || {
  printf 'missing ores-cli must exit 70, got %s\n' "$missing_status" >&2
  exit 1
}
[[ "$missing_output" == *'oresc is required'* ]] || {
  echo 'missing ores-cli error did not explain the dependency' >&2
  exit 1
}

printf 'oresc audit wrapper contract: ok\n'
