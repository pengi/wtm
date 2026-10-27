#!/bin/sh
# Check that an installed wtm works: files in place, and the basic flow
# against a local upstream.
#
# Usage: smoke-test.sh ZSH_COMPLETION_FILE
set -eux

wtm --version
command -v curl
test -f /usr/share/bash-completion/completions/wtm
test -f "$1"
test -f /usr/share/man/man1/wtm.1.gz
test -f /usr/share/man/man1/wtm-new.1.gz

HOME=$(mktemp -d)
export HOME
git config --global user.name Smoke
git config --global user.email smoke@example.invalid
git config --global init.defaultBranch main

upstream=$HOME/upstream
git init -q "$upstream"
echo hello > "$upstream/hello.txt"
git -C "$upstream" add hello.txt
git -C "$upstream" commit -q -m hello

wtm register "$upstream" demo
path=$(wtm new demo)
test "$path" = "$HOME/wt/demo"
test -f "$path/hello.txt"
wtm new demo side
wtm list
test "$(wtm list --names | tr '\n' ' ')" = "demo side "
test "$(wtm locate side)" = "$HOME/wt/side"
wtm drop side
wtm drop demo
wtm unregister demo
wtm shell-init bash > /dev/null
wtm auth list

set +x
echo "smoke test passed"
