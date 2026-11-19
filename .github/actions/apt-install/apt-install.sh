#!/usr/bin/env bash
# Backend of the apt-install composite action. Modes:
#   check   — exit 1 listing packages that are not installed (self-hosted path)
#   key     — print the cache key line `key=...` for $GITHUB_OUTPUT
#   install — install from the cached .deb closure, else from the network
# Input: APT_PACKAGES (space-separated), APT_NO_RECOMMENDS ("true"/"false").
#
# The cache holds exactly the .debs apt downloaded on this runner image, so it
# is keyed on ImageOS/ImageVersion: a new image gets a fresh closure instead of
# a stale one that dpkg would reject.
set -euo pipefail

read -ra PACKAGES <<< "${APT_PACKAGES:?APT_PACKAGES is required}"
CACHE_DIR="$HOME/.cache/apt-debs"
# Per-attempt ceiling for each network phase (index update, download). Two
# attempts stay inside the caller's 15-minute step timeout instead of letting
# one stalled mirror burn the whole job budget.
ATTEMPT_SECONDS=150

missing_packages() {
    local p
    for p in "${PACKAGES[@]}"; do
        dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q 'install ok installed' \
            || printf '%s\n' "$p"
    done
}

apt_network_install() {
    # --fix-broken: a partial dpkg -i from a stale cache must not wedge apt.
    local opts=(-y -q --fix-broken -o Acquire::Retries=3 -o Acquire::http::Timeout=30
                -o Acquire::https::Timeout=30 -o Dpkg::Use-Pty=0)
    [[ "${APT_NO_RECOMMENDS:-false}" == true ]] && opts+=(--no-install-recommends)
    sudo timeout "$ATTEMPT_SECONDS" apt-get update -q -o Acquire::Retries=3 \
        -o Acquire::http::Timeout=30 \
        && sudo timeout "$ATTEMPT_SECONDS" apt-get install --download-only "${opts[@]}" "${PACKAGES[@]}" \
        && sudo apt-get install "${opts[@]}" "${PACKAGES[@]}"
}

use_fallback_mirror() {
    local f
    for f in /etc/apt/sources.list.d/ubuntu.sources /etc/apt/sources.list; do
        [[ -f "$f" ]] && sudo sed -i -E \
            's#https?://[a-z0-9.-]*archive\.ubuntu\.com/ubuntu#http://archive.ubuntu.com/ubuntu#g' "$f"
    done
    echo "::warning::apt: default mirror failed; retrying via archive.ubuntu.com"
}

case "${1:-}" in
check)
    mapfile -t missing < <(missing_packages)
    if ((${#missing[@]})); then
        echo "::error::self-hosted runner lacks packages: ${missing[*]}" \
             "(install once: sudo apt-get install -y ${missing[*]})"
        exit 1
    fi
    echo "apt-install: all ${#PACKAGES[@]} packages present"
    ;;
key)
    digest=$(printf '%s\n' "${PACKAGES[@]}" "${APT_NO_RECOMMENDS:-false}" | sort -u \
        | sha256sum | cut -c1-16)
    echo "key=apt-debs-${ImageOS:-unknown}-${ImageVersion:-unknown}-$digest"
    ;;
install)
    if [[ -z "$(missing_packages)" ]]; then
        echo "apt-install: all packages already on the image"
        exit 0
    fi
    if compgen -G "$CACHE_DIR/*.deb" > /dev/null \
        && sudo dpkg -i "$CACHE_DIR"/*.deb > /dev/null \
        && [[ -z "$(missing_packages)" ]]; then
        echo "apt-install: installed from the .deb cache"
        exit 0
    fi
    apt_network_install || { use_fallback_mirror; apt_network_install; }
    mkdir -p "$CACHE_DIR"
    cp /var/cache/apt/archives/*.deb "$CACHE_DIR"/ 2>/dev/null || true
    mapfile -t missing < <(missing_packages)
    if ((${#missing[@]})); then
        echo "::error::apt-install: still missing after install: ${missing[*]}"
        exit 1
    fi
    ;;
*)
    echo "usage: $0 check|key|install" >&2
    exit 2
    ;;
esac
