#!/usr/bin/env sh
set -eu

repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
entrypoint="${ENTRYPOINT_UNDER_TEST:-$repo_root/entrypoint.sh}"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/entrypoint-test.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

cat >"$tmp/pass" <<'SH'
#!/usr/bin/env sh
cat
SH
cat >"$tmp/fail" <<'SH'
#!/usr/bin/env sh
cat >/dev/null
exit 17
SH
cat >"$tmp/out" <<'SH'
#!/usr/bin/env sh
sed 's/^/OUT:/'
SH
cat >"$tmp/err" <<'SH'
#!/usr/bin/env sh
sed 's/^/ERR:/'
SH
chmod 0755 "$tmp/pass" "$tmp/fail" "$tmp/out" "$tmp/err"

set +e
combined_out="$(LAMBDA_SIDECAR_PROC="$tmp/pass" "$entrypoint" sh -c 'printf "stdout-line\n"; printf "stderr-line\n" >&2; exit 23' secret-value 2>"$tmp/combined.err")"
combined_status=$?
set -e
[ "$combined_status" -eq 23 ]
printf '%s\n' "$combined_out" | grep -q 'stdout-line'
printf '%s\n' "$combined_out" | grep -q 'stderr-line'
grep -q "command is 'sh'" "$tmp/combined.err"
if grep -q 'secret-value' "$tmp/combined.err"; then
  printf '%s\n' 'entrypoint leaked a workload argument' >&2
  exit 1
fi

# Explicit fail-open remains supported, but it must be an opt-in. The reusable
# template defaults to fail-closed when a sidecar exits unsuccessfully.
set +e
LAMBDA_SIDECAR_PROC="$tmp/fail" LAMBDA_SIDECAR_FAIL_MODE=open "$entrypoint" sh -c 'printf ok; exit 0' >/dev/null 2>"$tmp/fail-open.err"
open_status=$?
LAMBDA_SIDECAR_PROC="$tmp/fail" LAMBDA_SIDECAR_FAIL_MODE=closed "$entrypoint" sh -c 'printf ok; exit 0' >/dev/null 2>"$tmp/fail-closed.err"
closed_status=$?
LAMBDA_SIDECAR_PROC="$tmp/fail" "$entrypoint" sh -c 'printf ok; exit 0' >/dev/null 2>"$tmp/fail-default.err"
default_failed_sidecar_status=$?
set -e
[ "$open_status" -eq 0 ]
[ "$closed_status" -eq 17 ]
[ "$default_failed_sidecar_status" -eq 17 ]

split_out="$(LAMBDA_SIDECAR_MODE=split LAMBDA_STDOUT_SIDECAR_PROC="$tmp/out" LAMBDA_STDERR_SIDECAR_PROC="$tmp/err" "$entrypoint" sh -c 'printf "one\n"; printf "two\n" >&2' 2>"$tmp/split.err")"
printf '%s\n' "$split_out" | grep -q 'OUT:one'
printf '%s\n' "$split_out" | grep -q 'ERR:two'

set +e
LAMBDA_SIDECAR_PROC="$tmp/does-not-exist" LAMBDA_SIDECAR_FAIL_MODE=closed "$entrypoint" true >/dev/null 2>"$tmp/missing.err"
missing_status=$?
LAMBDA_SIDECAR_PROC="$tmp/does-not-exist" "$entrypoint" true >/dev/null 2>"$tmp/missing-default.err"
default_missing_status=$?
set -e
[ "$missing_status" -eq 69 ]
[ "$default_missing_status" -eq 69 ]
grep -F 'using configured failure policy' "$tmp/missing-default.err" >/dev/null

# A caller may still choose fail-open deliberately, including for an unavailable
# sidecar, and that decision is explicit in the environment rather than implicit.
LAMBDA_SIDECAR_PROC="$tmp/does-not-exist" LAMBDA_SIDECAR_FAIL_MODE=open "$entrypoint" true >/dev/null 2>"$tmp/missing-open.err"

printf '%s\n' 'entrypoint contract tests passed'
