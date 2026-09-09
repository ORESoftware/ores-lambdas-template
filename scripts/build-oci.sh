#!/usr/bin/env sh
set -eu

if [ "$#" -ne 0 ]; then
  printf '%s\n' 'usage: configure OCI_OUTPUT/OCI_PLATFORMS/BINARY/CARGO_FEATURES/VCS_REF via environment; positional arguments are not accepted' >&2
  exit 64
fi

output="${OCI_OUTPUT:-dist/lambdas.oci}"
platforms="${OCI_PLATFORMS:-linux/amd64,linux/arm64}"
binary="${BINARY:-worker-http}"
features="${CARGO_FEATURES:-http}"
vcs_ref="${VCS_REF:-unknown}"

mkdir -p "$(dirname -- "$output")"
docker buildx build \
  --platform "$platforms" \
  --build-arg "BINARY=$binary" \
  --build-arg "CARGO_FEATURES=$features" \
  --build-arg "VCS_REF=$vcs_ref" \
  --output "type=oci,dest=$output" \
  .
