#!/usr/bin/env sh
set -eu
root="$(git rev-parse --show-toplevel)"; cd "$root"
for hook in .githooks/pre-commit .githooks/pre-push; do test -f "$hook"; chmod +x "$hook"; done
git config core.hooksPath .githooks
printf '%s\n' 'installed tracked ORES contract hooks from .githooks'
