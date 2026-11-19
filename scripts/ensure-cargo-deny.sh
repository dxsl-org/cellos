#!/usr/bin/env bash
# Put the pinned cargo-deny on PATH for CI. Used instead of
# EmbarkStudios/cargo-deny-action: that is a container action, and the runner
# builds container actions while setting up the job — even for steps whose
# `if:` skips them — so it failed every Security Scan on the self-hosted
# runners. Self-hosted runners already carry the binary (setup-runner.sh);
# hosted runners fetch the release tarball and verify its pinned SHA-256.
# 0.21.0 parses the CVSS 4.0 entries now in the RustSec advisory DB.
set -euo pipefail

VERSION=0.21.0
SHA256=a2170b854ff697e79e39c4c538981c9c1bdd803addbae6719a4cb66b4585a903
NAME="cargo-deny-$VERSION-x86_64-unknown-linux-musl"

if command -v cargo-deny > /dev/null \
    && [[ "$(cargo-deny --version)" == "cargo-deny $VERSION" ]]; then
    echo "cargo-deny $VERSION already installed"
    exit 0
fi

dest="$HOME/.local/bin"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
curl -fsSL --retry 3 -o "$work/$NAME.tar.gz" \
    "https://github.com/EmbarkStudios/cargo-deny/releases/download/$VERSION/$NAME.tar.gz"
echo "$SHA256  $work/$NAME.tar.gz" | sha256sum -c --quiet
tar -xzf "$work/$NAME.tar.gz" -C "$work"
mkdir -p "$dest"
install -m 0755 "$work/$NAME/cargo-deny" "$dest/cargo-deny"
[[ -n "${GITHUB_PATH:-}" ]] && echo "$dest" >> "$GITHUB_PATH"
"$dest/cargo-deny" --version
