#!/usr/bin/env bash
# Ubuntu 24.04 CI dependencies with independent, signed mirror fallbacks.
set -euo pipefail
check_only=false
case "${1:-}" in
    '') ;;
    --check-only) check_only=true ;;
    *) echo 'Usage: sudo bash scripts/install-ci-dependencies.sh [--check-only]' >&2; exit 2 ;;
esac
if (( EUID != 0 )); then
    echo 'Run this script with sudo on an Ubuntu 24.04 build runner.' >&2
    exit 1
fi
# shellcheck disable=SC1091
source /etc/os-release
if [[ "$ID" != ubuntu || "$VERSION_ID" != 24.04 ]]; then
    echo 'This dependency installer requires Ubuntu 24.04.' >&2
    exit 1
fi
for command in curl apt-get dpkg timeout; do
    command -v "$command" >/dev/null || { echo "Missing bootstrap tool: $command" >&2; exit 1; }
done
keyring=/usr/share/keyrings/ubuntu-archive-keyring.gpg
test -r "$keyring"
arch="$(dpkg --print-architecture)"
if [[ "$arch" != amd64 ]]; then
    echo 'This CI installer supports the x86_64 GitHub runner only.' >&2
    exit 1
fi
work="$(mktemp -d /tmp/screennotes-apt.XXXXXX)"
trap 'rm -rf -- "$work"' EXIT
chmod 755 "$work"
export DEBIAN_FRONTEND=noninteractive
packages=(
    build-essential ca-certificates curl pkg-config libgtk-4-dev
    librsvg2-dev libgirepository1.0-dev libgdk-pixbuf2.0-bin
    adwaita-icon-theme dpkg-dev desktop-file-utils python3 file
    patchelf squashfs-tools zsync
)
# These mirrors carry Ubuntu-signed metadata for all four Noble suites.
mirrors=(
    https://mirrors.edge.kernel.org/ubuntu
    https://mirror.math.princeton.edu/pub/ubuntu
    https://archive.ubuntu.com/ubuntu
)
for index in "${!mirrors[@]}"; do
    mirror="${mirrors[$index]}"
    echo "Checking Ubuntu mirror: $mirror"
    if ! curl --fail --silent --show-error --location --connect-timeout 5 --max-time 15 \
        "$mirror/dists/noble/InRelease" --output /dev/null; then
        echo "Mirror unavailable; trying the next mirror." >&2
        continue
    fi
    attempt="$work/$index"
    mkdir -p "$attempt/lists/partial"
    for suite in noble noble-updates noble-backports noble-security; do
        printf 'deb [arch=%s signed-by=%s] %s %s main restricted universe multiverse\n' \
            "$arch" "$keyring" "$mirror" "$suite"
    done > "$attempt/ubuntu.list"
    # Use only these sources for this invocation. Do not modify the runner's
    # mirror+file sources or mix stale indices from unsuccessful attempts.
    options=(
        -o "Dir::Etc::sourcelist=$attempt/ubuntu.list"
        -o Dir::Etc::sourceparts=-
        -o "Dir::State::lists=$attempt/lists"
        -o Acquire::Retries=1
        -o Acquire::http::Timeout=20
        -o Acquire::https::Timeout=20
        -o APT::Update::Error-Mode=any
        -o Acquire::Languages=none
    )
    if ! timeout 180s apt-get "${options[@]}" update; then
        echo "Could not fetch and verify all suites; trying the next mirror." >&2
        continue
    fi
    if [[ "$check_only" == true ]]; then
        echo "Verified all Ubuntu suites from $mirror"
        exit 0
    fi
    if timeout 600s apt-get "${options[@]}" install -y --no-install-recommends "${packages[@]}"; then
        echo "Build dependencies installed from $mirror"
        exit 0
    fi
    echo "Dependency installation failed; trying the next mirror." >&2
done
echo 'All Ubuntu mirrors failed. Check this runner’s DNS and outbound HTTPS access; no unsigned repositories were accepted.' >&2
exit 1
