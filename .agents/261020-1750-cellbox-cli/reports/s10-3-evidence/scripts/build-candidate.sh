#!/usr/bin/env bash
# S10-3 candidate build (C1..C6). Everything under W, outside the repository. Dev key through the
# S10-2a routes only; no seed, no fixture or override variable. Kernel = the CI kernel command
# (default features; per-target rustflags from the repository cargo config), plus EMBEDDED_OVERRIDE
# and a private CARGO_TARGET_DIR.
set -euo pipefail
W=/tmp/cellbox-s10-3-7yWa
cd /home/dmin/cellos
unset CELLBOX_PRIVATE_FIXTURE_SIGNING CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI RUSTFLAGS CARGO_ENCODED_RUSTFLAGS \
      CELLOS_PHASE02_CELL_PUBLIC_KEY CELLOS_CONFIG_DIR CELLOS_INIT_FEATURES
export CC_x86_64_unknown_none=cc
export CFLAGS_x86_64_unknown_none="-ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$PWD/third_party/freestanding-include"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="--target=x86_64-linux-gnu"
L="$W/logs"
for p in headless desktop; do
    echo "==> C1/C2 assemble $p"
    CARGO_TARGET_DIR="$W/target" python3 scripts/assemble-x86-image.py --profile "$p" --out-dir "$W/cand/$p" \
        > "$L/assemble-$p.log" 2>&1
done
build_kernel() {  # profile features out
    local p="$1" features="$2" out="$3"
    EMBEDDED_OVERRIDE="$W/cand/$p" CARGO_TARGET_DIR="$W/ktarget-$out" \
        cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc \
        ${features:+--features "$features"} > "$L/kernel-$out.log" 2>&1
    cp "$W/ktarget-$out/x86_64-unknown-none/release/cellos-kernel" "$W/cand/$out.kernel.elf"
    rm -rf "$W/iso-root-$out"
    X86_KERNEL="$W/cand/$out.kernel.elf" X86_ISO_ROOT="$W/iso-root-$out" \
        bash scripts/x86/make-iso-ci.sh "$W/cand/$out.iso" > "$L/iso-$out.log" 2>&1
}
echo "==> C3/C4 kernel + ISO headless";  build_kernel headless "" headless
echo "==> C3/C4 kernel + ISO desktop";   build_kernel desktop "" desktop
echo "==> C5 PC-profile kernel + ISO";   build_kernel headless board-x86-pc pc
echo "==> C6 candidate.json"
python3 "$W/candidate_json.py" "$W" > "$L/candidate-json.log" 2>&1
cat "$L/candidate-json.log"
