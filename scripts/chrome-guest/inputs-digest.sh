#!/usr/bin/env bash
# Print one digest over the Chrome guest root's build inputs: the builder and the
# guest init it installs. scripts/build-chrome-guest-image.sh records it next to the
# image; scripts/make-chrome-guest-disk.sh rebuilds an image whose digest differs.
set -euo pipefail
REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cat "$REPO_ROOT/scripts/build-chrome-guest-image.sh" "$REPO_ROOT/scripts/chrome-guest/init.sh" \
    | sha256sum | cut -d' ' -f1
