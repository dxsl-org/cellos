#!/usr/bin/env bash
# S10-3 canonical snapshot (read-only): canonical.sh before|after
set -euo pipefail
cd /home/dmin/cellos
OUT=/tmp/cellbox-s10-3-7yWa/canonical
tag="$1"
sha256sum kernel/src/embedded-x86_64/kernel_fs.img kernel/src/embedded-x86_64/init \
    target/x86_64-unknown-none/release/cellos-kernel target/x86_64-unknown-none/release/cellbox \
    target/x86_64-unknown-none/release/terminal build/vicell-x86.iso build/x86-iso-root/boot/kernel.elf \
    build/cellos-x86_64.iso build/cellos-x86_64-desktop-fixture.iso build/cellos-x86_64-desktop-wip.qcow2 \
    build/x86-pc-lane/vicell-x86-pc.iso > "$OUT/$tag.sha256"
find kernel/src/embedded-x86_64 -type f -exec sha256sum {} \; | sort -k2 > "$OUT/$tag-embedded.sha256"
ls -la --time-style=full-iso kernel/src/embedded-x86_64/ target/x86_64-unknown-none/release/ build/ build/x86-pc-lane/ \
    > "$OUT/$tag-listings.ls" 2>&1
if [[ "$tag" == before ]]; then touch "$OUT/start-marker"; fi
if [[ "$tag" == after ]]; then
    diff "$OUT/before.sha256" "$OUT/after.sha256" && echo "UNCHANGED: canonical artifacts"
    diff "$OUT/before-embedded.sha256" "$OUT/after-embedded.sha256" && echo "UNCHANGED: embedded dir"
    diff "$OUT/before-listings.ls" "$OUT/after-listings.ls" && echo "UNCHANGED: listings with mtimes"
    n=$(find target/x86_64-unknown-none build kernel/src -newer "$OUT/start-marker" 2>/dev/null | wc -l)
    echo "files newer than the start marker under target/x86_64-unknown-none, build/, kernel/src/: $n"
fi
