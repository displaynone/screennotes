#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
if [[ "${1:-}" == --help || $# == 0 ]]; then
    echo 'Usage: bash scripts/package-container.sh deb|appimage|all [package.py options]'
    echo 'Builds for Ubuntu 24.04 using Podman (preferred) or Docker.'
    exit 0
fi
if [[ -n "${CONTAINER_ENGINE:-}" ]]; then
    engine="$CONTAINER_ENGINE"
elif command -v podman >/dev/null; then
    engine=podman
else
    engine=docker
fi
command -v "$engine" >/dev/null || { echo 'Install Podman or Docker first.' >&2; exit 1; }
mkdir -p dist
"$engine" build -t screennotes-builder:24.04 -f packaging/Containerfile packaging
# Read-only source and a dedicated writable output mount. Compilation and downloaded
# packaging tools stay in the disposable container. Never change host ownership.
user_flags=(--user "$(id -u):$(id -g)")
if [[ "$(basename "$engine")" == podman ]]; then user_flags+=(--userns=keep-id); fi
exec "$engine" run --rm "${user_flags[@]}" \
    -v "$PWD:/source:ro" -v "$PWD/dist:/output:rw" \
    screennotes-builder:24.04 "$@" --output-dir /output --work-dir /tmp/screennotes-packaging
