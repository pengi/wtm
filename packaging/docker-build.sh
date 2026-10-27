#!/bin/sh
# Build the packages in a container, so the only local requirement is
# docker. Packages end up in target/packages.
set -eu
cd "$(dirname "$0")/.."

docker build -q -t wtm-build packaging/build
docker run --rm \
    -u "$(id -u):$(id -g)" \
    -e CARGO_HOME=/src/target/docker-cargo-home \
    -v "$PWD:/src" -w /src \
    wtm-build packaging/build-packages.sh "$@"
