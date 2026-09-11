#!/bin/sh
set -eu

usage() {
  echo "usage: scripts/test-aws-rie.sh <aws-lambda-rie> <worker-aws>" >&2
  exit 2
}

[ "$#" -eq 2 ] || usage
rie=$1
worker=$2
[ -x "$rie" ] || { echo "RIE is not executable: $rie" >&2; exit 2; }
[ -x "$worker" ] || { echo "AWS worker is not executable: $worker" >&2; exit 2; }

work=$(mktemp -d)
pid=''
cleanup() {
  if [ -n "$pid" ]; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT INT TERM

"$rie" "$worker" >"$work/rie.log" 2>&1 &
pid=$!

invoke() {
  payload=$1
  output=$2
  attempts=0
  while [ "$attempts" -lt 50 ]; do
    attempts=$((attempts + 1))
    status=$(
      curl --silent --show-error \
        --connect-timeout 1 \
        --max-time 3 \
        --output "$output" \
        --write-out '%{http_code}' \
        --header 'Content-Type: application/json' \
        --request POST \
        --data "$payload" \
        'http://127.0.0.1:8080/2015-03-31/functions/function/invocations' \
        2>"$work/curl.err" || true
    )
    if [ "$status" = "200" ]; then
      return 0
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "RIE/worker exited before becoming ready" >&2
      cat "$work/rie.log" >&2
      return 1
    fi
    sleep 0.2
  done
  echo "RIE invocation did not return HTTP 200" >&2
  cat "$work/curl.err" >&2 || true
  cat "$work/rie.log" >&2 || true
  return 1
}

success_payload='{"command":{"schemaVersion":"__PREFIX__.worker-command.v1","operation":"echo","payload":{"sentinel":"rie-parity-ok-57b9"}}}'
invoke "$success_payload" "$work/success.json"
grep -F '"provider":"aws-lambda"' "$work/success.json" >/dev/null
grep -F '"ok":true' "$work/success.json" >/dev/null
grep -F '"operation":"echo"' "$work/success.json" >/dev/null
grep -F '"sentinel":"rie-parity-ok-57b9"' "$work/success.json" >/dev/null

secret='rie-secret-must-never-reflect-c301'
invalid_payload='{"command":{"schemaVersion":"wrong-schema","operation":"echo","payload":{"secret":"rie-secret-must-never-reflect-c301"}}}'
invoke "$invalid_payload" "$work/rejected.json"
grep -F '"provider":"aws-lambda"' "$work/rejected.json" >/dev/null
grep -F '"ok":false' "$work/rejected.json" >/dev/null
grep -F '"code":"unsupported_schema_version"' "$work/rejected.json" >/dev/null
if grep -F "$secret" "$work/rejected.json" >/dev/null; then
  echo "rejected invocation reflected submitted secret payload" >&2
  exit 1
fi
