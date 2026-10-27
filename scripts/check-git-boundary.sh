#!/bin/sh
# All git access must go through wtm-core/src/git/. Fail if git2 is used, or
# the git binary is spawned, anywhere else.
set -eu
cd "$(dirname "$0")/.."

hits=$(grep -rnE 'git2::|extern crate git2|Command::new\("git"\)' \
    --include='*.rs' wtm-core/src wtm/src 2>/dev/null \
    | grep -v '^wtm-core/src/git/' || true)

if [ -n "$hits" ]; then
    echo "git access outside wtm-core/src/git/:" >&2
    echo "$hits" >&2
    exit 1
fi
