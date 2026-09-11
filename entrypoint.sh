#!/usr/bin/env sh
# Provider-neutral Docker/OCI process wrapper.
# Sidecar variables name executables only; this script never evals command strings.
set -u

if [ "$#" -eq 0 ]; then
  printf '%s\n' 'entrypoint: missing workload command' >&2
  exit 64
fi

# Do not print the full argv: function arguments can contain credentials or payloads.
printf "command is '%s'\n" "$1" >&2

mode="${LAMBDA_SIDECAR_MODE:-combined}"
# Fail closed unless a deployment explicitly opts into fail-open behavior. Sidecars
# can carry telemetry/security policy, so absence or failure must not be silently
# treated as successful admission by the reusable template.
fail_mode="${LAMBDA_SIDECAR_FAIL_MODE:-closed}"
combined_proc="${LAMBDA_SIDECAR_PROC:-any_such_sidecar_proc}"
stdout_proc="${LAMBDA_STDOUT_SIDECAR_PROC:-$combined_proc}"
stderr_proc="${LAMBDA_STDERR_SIDECAR_PROC:-$combined_proc}"

case "$mode" in
  off|combined|split) ;;
  *) printf "entrypoint: invalid LAMBDA_SIDECAR_MODE '%s'\n" "$mode" >&2; exit 64 ;;
esac
case "$fail_mode" in
  open|closed) ;;
  *) printf "entrypoint: invalid LAMBDA_SIDECAR_FAIL_MODE '%s'\n" "$fail_mode" >&2; exit 64 ;;
esac

app_pid=''
runtime_dir=''
sidecar_pids=''

forward_term() {
  if [ -n "$app_pid" ]; then
    kill -TERM "$app_pid" 2>/dev/null || true
  fi
}
forward_int() {
  if [ -n "$app_pid" ]; then
    kill -INT "$app_pid" 2>/dev/null || true
  fi
}
forward_hup() {
  if [ -n "$app_pid" ]; then
    kill -HUP "$app_pid" 2>/dev/null || true
  fi
}
cleanup() {
  for pid in $sidecar_pids; do
    kill -TERM "$pid" 2>/dev/null || true
  done
  if [ -n "$runtime_dir" ] && [ -d "$runtime_dir" ]; then
    rm -f "$runtime_dir"/stdout "$runtime_dir"/stderr "$runtime_dir"/combined
    rmdir "$runtime_dir" 2>/dev/null || true
  fi
}
trap forward_term TERM
trap forward_int INT
trap forward_hup HUP
trap cleanup EXIT

sidecar_available() {
  command -v "$1" >/dev/null 2>&1
}

finish() {
  app_status="$1"
  sidecar_status="$2"
  if [ "$app_status" -ne 0 ]; then
    exit "$app_status"
  fi
  if [ "$sidecar_status" -ne 0 ]; then
    if [ "$fail_mode" = closed ]; then
      exit "$sidecar_status"
    fi
    printf "entrypoint: sidecar exited %s; fail-open policy preserved workload success\n" "$sidecar_status" >&2
  fi
  exit 0
}

if [ "$mode" = off ]; then
  exec "$@"
fi

runtime_dir="$(mktemp -d "${TMPDIR:-/tmp}/lambda-sidecar.XXXXXX")" || exit 70

if [ "$mode" = combined ]; then
  if ! sidecar_available "$combined_proc"; then
    printf "entrypoint: sidecar '%s' unavailable; using configured failure policy\n" "$combined_proc" >&2
    if [ "$fail_mode" = closed ]; then
      exit 69
    fi
    exec "$@"
  fi

  mkfifo "$runtime_dir/combined" || exit 70
  "$combined_proc" <"$runtime_dir/combined" &
  sidecar_pid=$!
  sidecar_pids="$sidecar_pid"

  "$@" >"$runtime_dir/combined" 2>&1 &
  app_pid=$!
  wait "$app_pid"
  app_status=$?
  app_pid=''

  wait "$sidecar_pid"
  sidecar_status=$?
  sidecar_pids=''
  finish "$app_status" "$sidecar_status"
fi

if ! sidecar_available "$stdout_proc" || ! sidecar_available "$stderr_proc"; then
  printf '%s\n' 'entrypoint: split sidecar executable unavailable; using configured failure policy' >&2
  if [ "$fail_mode" = closed ]; then
    exit 69
  fi
  exec "$@"
fi

mkfifo "$runtime_dir/stdout" "$runtime_dir/stderr" || exit 70
"$stdout_proc" <"$runtime_dir/stdout" &
stdout_pid=$!
"$stderr_proc" <"$runtime_dir/stderr" &
stderr_pid=$!
sidecar_pids="$stdout_pid $stderr_pid"

"$@" >"$runtime_dir/stdout" 2>"$runtime_dir/stderr" &
app_pid=$!
wait "$app_pid"
app_status=$?
app_pid=''

wait "$stdout_pid"
stdout_status=$?
wait "$stderr_pid"
stderr_status=$?
sidecar_pids=''

sidecar_status=0
if [ "$stdout_status" -ne 0 ]; then
  sidecar_status="$stdout_status"
elif [ "$stderr_status" -ne 0 ]; then
  sidecar_status="$stderr_status"
fi
finish "$app_status" "$sidecar_status"
