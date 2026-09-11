#!/bin/sh
set -eu

usage() {
  echo "usage: scripts/test-container-runtime.sh <image>" >&2
  exit 2
}

[ "$#" -eq 1 ] || usage
image=$1
name="lambda-runtime-smoke-$$"
work=$(mktemp -d)
container=''
cleanup() {
  if [ -n "$container" ]; then
    docker rm -f "$container" >/dev/null 2>&1 || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT INT TERM

user=$(docker image inspect --format '{{.Config.User}}' "$image")
[ "$user" = "lambda" ] || {
  echo "runtime image must declare non-root user 'lambda', got '$user'" >&2
  exit 1
}

container=$(docker run -d \
  --name "$name" \
  --read-only \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=16m \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --pids-limit 128 \
  -e LAMBDA_SIDECAR_MODE=off \
  -e PORT=8080 \
  -p 127.0.0.1::8080 \
  "$image")

mapped=$(docker port "$container" 8080/tcp | head -n 1)
port=${mapped##*:}
case "$port" in
  ''|*[!0-9]*) echo "unable to determine mapped runtime port from '$mapped'" >&2; exit 1 ;;
esac
base="http://127.0.0.1:$port"

ready=0
attempts=0
while [ "$attempts" -lt 50 ]; do
  attempts=$((attempts + 1))
  status=$(curl --silent --show-error --max-time 2 --output "$work/ready" --write-out '%{http_code}' "$base/readyz" 2>"$work/curl.err" || true)
  if [ "$status" = "200" ] && grep -Fx 'ok' "$work/ready" >/dev/null; then
    ready=1
    break
  fi
  if ! docker inspect --format '{{.State.Running}}' "$container" 2>/dev/null | grep -Fx true >/dev/null; then
    echo "runtime container exited before readiness" >&2
    docker logs "$container" >&2 || true
    exit 1
  fi
  sleep 0.2
done
[ "$ready" -eq 1 ] || {
  echo "runtime container never became ready" >&2
  cat "$work/curl.err" >&2 || true
  docker logs "$container" >&2 || true
  exit 1
}

valid='{"provider":"local","requestId":"container-1","command":{"schemaVersion":"__PREFIX__.worker-command.v1","operation":"echo","payload":{"sentinel":"container-smoke-ok-82c4"}}}'
status=$(curl --silent --show-error --max-time 3 --output "$work/valid.json" --write-out '%{http_code}' \
  --header 'Content-Type: application/json' --request POST --data "$valid" "$base/invoke")
[ "$status" = "200" ] || { echo "valid invocation returned HTTP $status" >&2; cat "$work/valid.json" >&2; exit 1; }
grep -F '"provider":"local"' "$work/valid.json" >/dev/null
grep -F '"requestId":"container-1"' "$work/valid.json" >/dev/null
grep -F '"ok":true' "$work/valid.json" >/dev/null
grep -F '"sentinel":"container-smoke-ok-82c4"' "$work/valid.json" >/dev/null

secret='container-secret-must-never-reflect-9dd0'
invalid='{"provider":"local","requestId":"container-2","command":{"schemaVersion":"wrong-schema","operation":"echo","payload":{"secret":"container-secret-must-never-reflect-9dd0"}}}'
status=$(curl --silent --show-error --max-time 3 --output "$work/invalid.json" --write-out '%{http_code}' \
  --header 'Content-Type: application/json' --request POST --data "$invalid" "$base/invoke")
[ "$status" = "400" ] || { echo "invalid invocation returned HTTP $status" >&2; cat "$work/invalid.json" >&2; exit 1; }
grep -F '"ok":false' "$work/invalid.json" >/dev/null
grep -F '"code":"unsupported_schema_version"' "$work/invalid.json" >/dev/null
if grep -F "$secret" "$work/invalid.json" >/dev/null; then
  echo "rejected HTTP invocation reflected submitted secret payload" >&2
  exit 1
fi

# Axum's body limiter sits one byte above the core's 256 KiB limit so the core
# can return its structured 413 receipt at MAX+1 rather than a framework body error.
{
  printf '{"provider":"local","requestId":"container-3","command":{"schemaVersion":"__PREFIX__.worker-command.v1","operation":"echo","payload":{"padding":"'
  head -c 262145 /dev/zero | tr '\000' 'a'
  printf '"}}}'
} >"$work/oversized.json"
status=$(curl --silent --show-error --max-time 5 --output "$work/oversized-response.json" --write-out '%{http_code}' \
  --header 'Content-Type: application/json' --request POST --data-binary @"$work/oversized.json" "$base/invoke")
[ "$status" = "413" ] || { echo "oversized invocation returned HTTP $status" >&2; cat "$work/oversized-response.json" >&2; exit 1; }
# Depending on whether the framework or core rejects first, never allow input reflection.
if grep -F 'aaaaaa' "$work/oversized-response.json" >/dev/null; then
  echo "oversized invocation reflected submitted payload" >&2
  exit 1
fi
