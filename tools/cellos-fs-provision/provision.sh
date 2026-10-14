#!/usr/bin/env bash
# Called only by offline disposable/staged image producers. Never a guest fallback.
set -euo pipefail
[[ $# -eq 2 ]] || { echo "Usage: bash tools/cellos-fs-provision/provision.sh IMAGE fresh-blank|verify-existing" >&2; exit 1; }
IMAGE=$1
INTENT=$2
[[ "$INTENT" == fresh-blank || "$INTENT" == verify-existing ]] || { echo "Explicit provisioning intent required" >&2; exit 1; }
TOOL_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
HOST_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
[[ -n "$HOST_TARGET" ]] || { echo "Cannot determine Rust host target" >&2; exit 1; }
# Receipts never overwrite historical records; image SHA-256 binds staging/published bytes.
RECEIPTS=$(mktemp -d "${IMAGE}.cellosfs-receipts.XXXXXX")
for BASE in 800000 931072; do
    env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS cargo run --quiet --locked \
        --manifest-path "$TOOL_DIR/Cargo.toml" --target "$HOST_TARGET" -- \
        --image "$IMAGE" --base "$BASE" --sectors 131072 --intent "$INTENT" \
        --receipt "$RECEIPTS/$BASE.json"
done
# Bind both native partitions to the final image identity, after all formatting.
if [[ "$INTENT" == fresh-blank ]]; then
    for BASE in 800000 931072; do
        env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS cargo run --quiet --locked \
            --manifest-path "$TOOL_DIR/Cargo.toml" --target "$HOST_TARGET" -- \
            --image "$IMAGE" --base "$BASE" --sectors 131072 --intent verify-existing \
            --receipt "$RECEIPTS/$BASE-final.json"
    done
fi
echo "[cellos-fs-provision] Receipts: $RECEIPTS"
