#!/bin/sh
# Render this template into ~/codes/<org>/<prefix>-lambdas. Usage: scripts/scaffold.sh <org> <prefix> [gcp-project]
set -eu
org=$1; prefix=$2; gcp=${3:-}
here=$(cd "$(dirname "$0")/.." && pwd); dest="$HOME/codes/$org/$prefix-lambdas"
[ -e "$dest" ] && { echo "exists: $dest"; exit 1; }
mkdir -p "$dest"; (cd "$here" && tar cf - --exclude=.git --exclude=target --exclude=scripts . ) | (cd "$dest" && tar xf -)
crate=$(echo "$prefix" | tr '-' '_')_lambdas
find "$dest" -type f \( -name '*.rs' -o -name '*.toml' -o -name '*.md' -o -name '*.json' -o -name '*.yaml' -o -name '*.yml' -o -name 'Dockerfile' \) -print0 | xargs -0 sed -i.bak -e "s/__ORG__/$org/g" -e "s/__PREFIX__/$prefix/g" -e "s/__CRATE__/$crate/g" -e "s/__GCP_PROJECT__/${gcp:-$org}/g"
find "$dest" -name '*.bak' -delete
echo "scaffolded $dest (crate $crate)"
