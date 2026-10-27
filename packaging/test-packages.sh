#!/bin/sh
# Install the packages from target/packages in clean containers of each
# supported distribution, and run the smoke test there.
#
# Usage: packaging/test-packages.sh [IMAGE...]
set -eu
cd "$(dirname "$0")/.."

images=${*:-debian:12 ubuntu:22.04 ubuntu:24.04 almalinux:9}
for image in $images; do
    echo "=== $image"
    docker run --rm \
        -v "$PWD/target/packages:/pkgs:ro" \
        -v "$PWD/packaging:/packaging:ro" \
        "$image" /packaging/install-and-test.sh
done
