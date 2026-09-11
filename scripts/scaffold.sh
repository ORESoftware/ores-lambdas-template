#!/bin/sh
# Render this template into ~/codes/<org>/<prefix>-lambdas.
# Usage: ORES_LAMBDA_CONCERNS=middleware,rate-limit,chat,shared-auth scripts/scaffold.sh <org> <prefix> [gcp-project]
set -eu

[ "$#" -ge 2 ] && [ "$#" -le 3 ] || {
  echo "usage: scripts/scaffold.sh <org> <prefix> [gcp-project]" >&2
  echo "optional: ORES_LAMBDA_CONCERNS=middleware,rate-limit,chat,shared-auth" >&2
  exit 2
}

org=$1
prefix=$2
gcp=${3:-}
here=$(cd "$(dirname "$0")/.." && pwd)
dest="$HOME/codes/$org/$prefix-lambdas"

[ -e "$dest" ] && { echo "exists: $dest" >&2; exit 1; }
mkdir -p "$dest"
(cd "$here" && tar cf - --exclude=.git --exclude=target --exclude=scripts . ) | (cd "$dest" && tar xf -)

crate=$(echo "$prefix" | tr '-' '_')_lambdas
find "$dest" -type f \( -name '*.rs' -o -name '*.toml' -o -name '*.md' -o -name '*.json' -o -name '*.yaml' -o -name '*.yml' -o -name 'Dockerfile' \) -print0 |
  xargs -0 sed -i.bak \
    -e "s/__ORG__/$org/g" \
    -e "s/__PREFIX__/$prefix/g" \
    -e "s/__CRATE__/$crate/g" \
    -e "s/__GCP_PROJECT__/${gcp:-$org}/g"
find "$dest" -name '*.bak' -delete

if [ -n "${ORES_LAMBDA_CONCERNS:-}" ]; then
  sh "$here/scripts/enable-concern.sh" "$dest" "$ORES_LAMBDA_CONCERNS"
fi

echo "scaffolded $dest (crate $crate; concerns ${ORES_LAMBDA_CONCERNS:-otel})"
