#!/bin/sh
# Runs inside a clean container: install the package with the system's
# package manager (which also pulls in git), then run the smoke test.
set -eu

if command -v apt-get >/dev/null; then
    export DEBIAN_FRONTEND=noninteractive
    # Minimal Ubuntu images skip man pages; a real system installs them.
    rm -f /etc/dpkg/dpkg.cfg.d/excludes
    apt-get update -qq
    apt-get install -y -qq /pkgs/*.deb >/dev/null
    zsh_completion=/usr/share/zsh/vendor-completions/_wtm
elif command -v dnf >/dev/null; then
    dnf install -y -q /pkgs/*.rpm >/dev/null
    zsh_completion=/usr/share/zsh/site-functions/_wtm
else
    echo "no supported package manager" >&2
    exit 1
fi

exec /packaging/smoke-test.sh "$zsh_completion"
