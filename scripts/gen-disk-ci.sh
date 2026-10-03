#!/usr/bin/env bash
# gen-disk-ci.sh — assemble the RV64 QEMU images on Linux, the way gen_disk.ps1
# does on a Windows dev box.
#
# Produces (all gitignored build artifacts):
#   kernel/src/embedded/kernel_fs.img   VIFS1 — the bootstrap ramdisk embedded in
#                                       the kernel via include_bytes! (FAT16)
#   target/$TARGET/release/cellos-kernel  production kernel with the fresh VIFS1
#   disk_v3.img                         MBR disk: P1 FAT32, P2 cell table,
#                                       P6 FAT cell-store (VirtIO-BLK)
#
# WHY THIS EXISTS
# ---------------
# The RV64 integration lanes in tests/integration boot these two images:
#
#   tests/integration/tests/launch-profile.rs     (`vfs-test`, snapshot authority)
#   tests/integration/tests/tier2_fault_isolation.rs (`tier2-*`, `posix-shim-test`)
#
# They fail on Linux for image reasons, not code reasons, when the disk is built
# by hand:
#
#   * `service-vfs` and `app-vfs-test` have a `test-hooks` flavour (1.1 KiB quota
#     plus in-service selftests). It exists for scripts/build-test-hooks-ci.sh and
#     its `vfs-quota` lane. A test-hooks `/bin/vfs-test` on the *production* disk
#     fails its quota scenarios against the production `/bin/vfs` (32 MB quota) and
#     prints `[vfs-test] FAILURES DETECTED` — which is exactly what
#     launch-profile.rs then reports, because it waits for `ALL TESTS PASSED`.
#     This script builds both cells with their DEFAULT (production) features and
#     asserts the test-hooks marker is absent before assembling the image.
#   * `/bin/tier2-smoke` must be rebuilt from the current tree: at HEAD it takes
#     the phase-03 branch (the private-root grant lifecycle is open on RV64 =>
#     `[tier2-smoke] Grant unregistered and the id refused`). A binary from the
#     phase-01 era instead prints the fail-closed sentinel
#     (`... Grant registration denied fail-closed (phase-01 gate)`) and the lane
#     times out on a marker that no current build emits.
#
# FLAVOUR RULE: only build-test-hooks-ci.sh may build service-vfs / app-vfs-test
# with `--features test-hooks`, and only build-shell-test-ci.sh may build
# app-shell with `--features shell_test`; every image assembled here uses the
# production flavour. The assertions below turn a violation into a build failure
# instead of a mysterious lane failure.
#
# OPTIONAL CELLS (omit, never ship a stale binary)
# -----------------------------------------------
# gen_disk.ps1 attempts doom / tetris / tetris-c / tetris-lua and ships whatever
# built. On Linux the picolibc/newlib-linked cells (lua, tetris-lua) and the
# doomgeneric C objects are not available — the same reason CI passes
# `--exclude doom,lua,tetris-lua` (see .github/workflows/ci.yml). Those builds are
# attempted, and a failure omits the cell exactly like gen_disk.ps1 does.
#
# Usage:
#   bash scripts/gen-disk-ci.sh [--disk <path>] [--no-cells] [--help]
#
# Requirements (all present on this workstation): python3, cargo + nightly
# toolchain, riscv64-unknown-elf-gcc/ar/objcopy, libclang (littlefs bindgen),
# python `cryptography` (policy signing).

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

DISK="disk_v3.img"
BUILD_CELLS=1

usage() {
    cat <<'EOF'
Assemble the RV64 QEMU images on Linux, the way gen_disk.ps1 does on Windows:
kernel_fs.img (VIFS1), target/riscv64gc-unknown-none-elf/release/cellos-kernel
and disk_v3.img (P1 FAT32 + P2 cell table + P6 FAT cell-store).

Usage: bash scripts/gen-disk-ci.sh [--disk <path>] [--no-cells] [--help]

  --disk <path>   disk image to (re)write (default: disk_v3.img)
  --no-cells      skip the cargo cell builds; assemble from the current target/
  --help          this text

service-vfs and app-vfs-test are always built with their DEFAULT (production)
features; the `test-hooks` flavour belongs to scripts/build-test-hooks-ci.sh and
is asserted absent before the image is assembled. The picolibc/newlib-linked
cells (lua, tetris-lua) and doomgeneric (doom) do not build on Linux and are
omitted exactly as gen_disk.ps1 omits a failed optional cell.
EOF
    exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --disk)     DISK="${2:?--disk needs a path}"; shift 2 ;;
        --no-cells) BUILD_CELLS=0; shift ;;
        -h|--help)  usage 0 ;;
        *) echo "unknown argument: $1" >&2; usage 1 ;;
    esac
done

TARGET=riscv64gc-unknown-none-elf
REL="target/$TARGET/release"

# ── Toolchain probe ───────────────────────────────────────────────────────────
# `python3` is not universal and on Windows the bare name is a Store alias stub;
# the shared libs consume $PYTHON_BIN.
if command -v python3 >/dev/null 2>&1 && python3 -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN=python3
elif command -v python >/dev/null 2>&1 && python -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN=python
else
    echo "FAIL: a working Python 3 interpreter is required" >&2
    exit 1
fi

# The littlefs C core inside service-vfs compiles through cc-rs, which probes
# <target>-gcc. Ubuntu ships `riscv64-unknown-elf-*`, the xpack distribution
# `riscv-none-elf-*`; probe instead of hard-coding either (gen_disk.ps1 does the
# same). CFLAGS carries the vendored freestanding headers the bare-elf gcc lacks.
probe_tool() {
    local what=$1; shift
    local c
    for c in "$@"; do
        if command -v "$c" >/dev/null 2>&1; then echo "$c"; return 0; fi
    done
    echo "FAIL: no $what found (tried: $*)" >&2
    return 1
}

export CC_riscv64gc_unknown_none_elf="${CC_riscv64gc_unknown_none_elf:-$(probe_tool 'riscv cross gcc' riscv64-unknown-elf-gcc riscv-none-elf-gcc)}"
export AR_riscv64gc_unknown_none_elf="${AR_riscv64gc_unknown_none_elf:-$(probe_tool 'riscv cross ar' riscv64-unknown-elf-ar riscv-none-elf-ar)}"
export CFLAGS_riscv64gc_unknown_none_elf="${CFLAGS_riscv64gc_unknown_none_elf:--march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include}"
export OBJCOPY="${OBJCOPY:-$(probe_tool 'riscv cross objcopy' riscv64-unknown-elf-objcopy riscv-none-elf-objcopy)}"

echo "==> toolchain: python=$PYTHON_BIN cc=$CC_riscv64gc_unknown_none_elf objcopy=$OBJCOPY"

# ── 1. Build the cells ────────────────────────────────────────────────────────
# Package groups mirror gen_disk.ps1's Build-Cargo calls one-for-one (same
# packages, same order, same fail-fast semantics): a core failure aborts before a
# stale target/ artifact can be signed and shipped.
build_cells() {
    local what=$1; shift
    echo "==> building $what"
    cargo build --release --target "$TARGET" "$@"
}

FAILED_OPTIONAL=()
build_optional() {
    local what=$1; shift
    echo "==> building $what (optional)"
    if ! cargo build --release --target "$TARGET" "$@"; then
        FAILED_OPTIONAL+=("$what")
        echo "  WARN: optional '$what' failed — cell omitted, no stale artifact shipped" >&2
    fi
    return 0
}

if [[ $BUILD_CELLS -eq 1 ]]; then
    build_cells "core services + drivers" -p app-init -p app-shell -p service-platform \
        -p service-vfs -p service-config -p service-input -p service-net \
        -p service-compositor -p service-kms -p service-net-broker -p supervisor \
        -p driver-nvme -p driver-e1000 -p driver-virtio-net -p driver-virtio-blk \
        -p driver-virtio-gpu
    build_cells "ai inference service + oracle" -p service-ai -p ai-test
    build_cells "httpd" -p service-httpd
    build_cells "app-bench" -p app-bench
    build_cells "app-net-tools" -p app-net-tools
    build_cells "app-sys-tools" -p app-sys-tools
    build_cells "robot-demo + robot-dashboard" -p robot-demo -p robot-dashboard
    build_cells "fb-console + desktop + ocel + ocel-js" -p fb-console -p desktop -p ocel -p ocel-js
    build_cells "hypha cells" -p hypha-llm-gateway -p hypha-core -p hypha-tool-fs -p hypha-tool-sys -p hypha-tool-spawn
    build_cells "input-test" -p input-test
    build_cells "window-policy-probe" -p window-policy-probe
    build_cells "viui-demo" -p viui-demo
    build_cells "audio-demo" -p audio-demo
    build_cells "app-https-demo" -p app-https-demo
    build_cells "app-http-smoke" -p app-http-smoke
    build_cells "cfi-test" -p cfi-test
    build_cells "wx-test" -p wx-test
    build_cells "vfs-test" -p app-vfs-test
    build_cells "hotswap demos" -p hotswap-demo-v1 -p hotswap-demo-v2
    build_cells "posix-shim-test" -p app-posix-shim-test
    build_cells "tier2 test cells" -p tier2-smoke -p tier2-exploit
    build_cells "backend supervisor witness" -p app-backend

    # Optional demo cells. Drop the previous artifact first: a failed build must
    # omit the cell rather than let the presence-guarded image steps below ship the
    # binary from an earlier run (gen_disk.ps1's Remove-Item, same reason).
    rm -f "$REL/tetris" "$REL/tetris-c" "$REL/tetris-lua" "$REL/doom"
    build_optional "tetris" -p tetris
    if [[ -f cells/demos/tetris-c/src/c/tetris-os/src/tetris.c ]]; then
        build_optional "tetris-c" -p tetris-c
    fi
    build_optional "tetris-lua" -p tetris-lua
    if [[ -d cells/demos/doom/src/c/doomgeneric/doomgeneric ]]; then
        build_optional "doom" -p doom -Z build-std=core,alloc
    fi
fi

# ── 2. Flavour assertions (see the header) ───────────────────────────────────
# `strings` markers, not sizes: the test-hooks flavour adds `access/selftest.rs`
# to service-vfs and the `rdir-quota` / `quota:` scenarios to vfs-test, and both
# are absent from the production binaries.
#
# NB: never pipe `strings` into `grep -q` here. Under `set -o pipefail`, `grep -q`
# exits at its first match, `strings` then dies on SIGPIPE (status 141), and the
# pipeline's non-zero status makes the `if` false — the guard silently passes with
# the marker present. Capture the dump first. (Measured 2026-10-02 while adding
# the app-shell guard: it reported a clean shell on a `--no-cells` run that had
# embedded the shell_test harness, and the two test-hooks guards above it were
# disabled by the same flaw.)
assert_absent_marker() {
    local bin=$1 marker=$2 guidance=$3
    if [[ ! -f $bin ]]; then
        echo "FAIL: required cell missing: $bin" >&2
        exit 1
    fi
    local dump
    dump=$(strings -n 8 "$bin" || true)
    if grep -qF -- "$marker" <<<"$dump"; then
        echo "FAIL: $bin carries the marker '$marker'." >&2
        echo "      $guidance" >&2
        exit 1
    fi
}

assert_absent_marker "$REL/service-vfs" "access/selftest.rs" \
    "service-vfs must be built WITHOUT --features test-hooks for a production disk (scripts/build-test-hooks-ci.sh owns that flavour)."
assert_absent_marker "$REL/vfs-test" "rdir-quota" \
    "app-vfs-test must be built WITHOUT --features test-hooks for a production disk (scripts/build-test-hooks-ci.sh owns that flavour)."

# app-shell's `shell_test` flavour replaces the REPL with a deterministic scenario
# harness. scripts/build-shell-test-ci.sh builds it at $REL/app-shell and copies
# only the KERNEL to cellos-kernel-shell-test, so the harness binary outlives that
# lane: a later `--no-cells` run embeds it as /bin/shell, the guest never prints
# `Cellos >`, and every boot lane fails on a phantom cause.
assert_absent_marker "$REL/app-shell" "[shell-test] COMPLETE" \
    "app-shell must be built WITHOUT --features shell_test for a production disk (scripts/build-shell-test-ci.sh owns that flavour); re-run a full 'bash scripts/gen-disk-ci.sh' without --no-cells."

# tier2-smoke must be a current build: the phase-01 sentinel only exists in the
# closed-lifecycle branch, which RV64 no longer compiles in (dead-code eliminated).
assert_absent_marker "$REL/tier2-smoke" "Grant registration denied fail-closed (phase-01 gate)" \
    "tier2-smoke is a phase-01-era binary (closed-lifecycle branch present)."

# Required artifacts: gen_disk.ps1's Add-RequiredCellToSign list.
for required in app-init app-shell service-vfs service-config service-kms supervisor \
                bench viui-demo backend-supervisor backend-worker hotswap-demo-v1 \
                hotswap-demo-v2 free hotswap; do
    if [[ ! -f "$REL/$required" ]]; then
        echo "FAIL: required artifact missing: $REL/$required — refusing to sign a partial image." >&2
        exit 1
    fi
done

# ── 3. Sign every cell (Ed25519 dev key) ─────────────────────────────────────
# One cellos-sign invocation for the whole set: the signature attests that F1/F5
# held for the tree, which is a per-tree claim, not a per-binary one. Under the
# `signing-required` feature (a kernel default) an unsigned cell is DENIED at
# spawn and the guest never reaches a shell.
echo "==> signing cells"
# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh

SIGNABLE=()
add_signable() { [[ -f $1 ]] && SIGNABLE+=("$1"); return 0; }

for cell in app-init app-shell platform service-vfs service-config service-net service-kms \
            service-net-broker service-compositor supervisor driver-nvme driver-e1000 \
            driver-virtio-net driver-virtio-blk driver-virtio-gpu service-ai service-httpd \
            ai-test service-input bench bench-probe capacity-probe heavy-probe app-net-tools app-sys-tools \
            robot-demo robot-dashboard fb-console desktop ocel ocel-js hypha-llm-gateway \
            hypha-core hypha-tool-fs hypha-tool-sys hypha-tool-spawn input-test \
            window-policy-probe viui-demo audio-demo app-https-demo http-smoke cfi-test wx-test \
            vfs-test backend-supervisor backend-worker hotswap-demo-v1 hotswap-demo-v2 ls cat \
            echo ps kill free hotswap posix-shim-test tier2-smoke tier2-exploit tetris \
            tetris-c tetris-lua doom lua micropython; do
    add_signable "$REL/$cell"
done

sign_cells "${SIGNABLE[@]}"

# ── 4. VIFS1 (kernel_fs.img) ─────────────────────────────────────────────────
# VIFS1 carries ONLY what must resolve before/without VFS: the bootstrap chain
# (loader::early::BOOTSTRAP_CELLS + init), kernel-FD data, and the cells whose
# launch edge needs the kernel loader (capability-bearing ELFs and VIFS1-only path
# readers). Everything else lives in the P2 table + P6 FAT cell-store below.
# Keep this list byte-for-byte in sync with gen_disk.ps1's $kfs_args.
echo "==> assembling kernel_fs.img (VIFS1)"
KFS_TMP="target/ViCell_kfs"
EMBED_DIR="target/ViCell_embedded"
rm -rf "$KFS_TMP" "$EMBED_DIR"
mkdir -p "$KFS_TMP" "$EMBED_DIR"

printf 'ViCell' > "$KFS_TMP/hostname"
printf 'Welcome to ViCell!' > "$KFS_TMP/readme"

# shellcheck source=scripts/lib-bake-policy.sh
source scripts/lib-bake-policy.sh
bake_policy "$KFS_TMP/POLICY.BIN"

KFS="$EMBED_DIR/kernel_fs.img"
kfs_args=(
    "$KFS"
    "$REL/app-init"       "/bin/init"
    "$REL/app-shell"      "/bin/shell"
    "$REL/service-vfs"    "/bin/vfs"
    "$REL/service-config" "/bin/config"
    "$KFS_TMP/hostname"   "/etc/hostname"
    "$KFS_TMP/readme"     "/readme.txt"
    # Root-level 8.3-uppercase: kernel/src/policy.rs reads exactly /POLICY.BIN.
    "$KFS_TMP/POLICY.BIN" "/POLICY.BIN"
)
add_kfs() { [[ -f $1 ]] && kfs_args+=("$1" "$2"); return 0; }

add_kfs "$REL/platform"             "/bin/platform"
add_kfs "$REL/driver-virtio-blk"    "/bin/block"
add_kfs "$(pwd)/doom1.wad"          "/doom1.wad"
add_kfs "$REL/hotswap-demo-v1"      "/bin/hotswap-demo-v1"
add_kfs "$REL/hotswap-demo-v2"      "/bin/hotswap-demo-v2"
add_kfs "$REL/backend-supervisor"   "/bin/backend-supervisor"
add_kfs "$REL/free"                 "/bin/free"
add_kfs "$REL/bench"                "/bin/bench"
add_kfs "$REL/bench-probe"          "/bin/bench-probe"
add_kfs "$REL/hypha-core"           "/bin/hypha"
add_kfs "$REL/hypha-tool-spawn"     "/bin/tool-spawn"
if [[ "${CELLOS_INCLUDE_CAPACITY_PROBE:-0}" == "1" ]]; then
    add_kfs "$REL/capacity-probe"   "/bin/capacity-probe"
    # Heavy cells for the D5 gate's M-resident baselines; same guard as the probe
    # because only that lane spawns them.
    add_kfs "$REL/heavy-probe"      "/bin/heavy-probe"
fi

"$PYTHON_BIN" tools/mkfat32.py "${kfs_args[@]}"

# Prove the layout rather than trusting the exit code: mkfat32.py exits 0 for an
# image whose destination paths went astray, and a missing /POLICY.BIN degrades
# silently to the dev-permissive branch (an image that looks provisioned and
# enforces nothing).
"$PYTHON_BIN" tools/inspect_fat.py "$KFS" > "$KFS_TMP/fat-layout.txt"
if ! grep -q -- '--- /bin ---' "$KFS_TMP/fat-layout.txt" ||
   ! grep -q -- "LFN 'vfs'" "$KFS_TMP/fat-layout.txt" ||
   ! grep -q -- "LFN 'shell'" "$KFS_TMP/fat-layout.txt" ||
   ! grep -q -- "LFN 'bench-probe'" "$KFS_TMP/fat-layout.txt"; then
    echo "FAIL: kernel_fs.img lacks a required bootstrap cell" >&2
    cat "$KFS_TMP/fat-layout.txt" >&2
    exit 1
fi
assert_policy_in_image "$KFS_TMP/fat-layout.txt"

# ── 5. Kernel (embeds VIFS1 via include_bytes!) ───────────────────────────────
# EMBEDDED_OVERRIDE points the build script at our staging dir, so the committed
# kernel/src/embedded/init is never rewritten (gen_disk.ps1 copies app-init over
# it; the embedded bytes are identical either way). The image copy below keeps
# kernel/src/embedded/kernel_fs.img current for the lanes that read it directly.
echo "==> building kernel (PIC, VIFS1 embedded)"
cp "$REL/app-init" "$EMBED_DIR/init"
RUSTFLAGS="-C relocation-model=pic" EMBEDDED_OVERRIDE="$EMBED_DIR" \
    cargo build --release -p cellos-kernel --target "$TARGET" -Z build-std=core,alloc

# A concurrent test-hooks build clobbers the production kernel path. Both
# build-test-hooks-ci.sh and build-aarch64-test-hooks-ci.sh build the test-hooks
# kernel at $REL/cellos-kernel and only THEN copy it to `cellos-kernel-test-hooks`,
# and lanes that shell out to them (build-native-domain-test-ci.sh,
# qemu-native-domain-test.sh) inherit the clobber. The integration lanes boot
# $REL/cellos-kernel, so a clobber makes them fail on phantom causes: the
# test-hooks VIFS1 carries the test-hooks /bin/vfs, whose FAT /bin mount fails
# against a production disk, so every cell-store cell becomes "command not found".
# Catching it here costs nothing; booting it costs an hour of misdirected debugging.
if [[ -f "$REL/cellos-kernel-test-hooks" ]] && cmp -s "$REL/cellos-kernel" "$REL/cellos-kernel-test-hooks"; then
    echo "FAIL: $REL/cellos-kernel is byte-identical to cellos-kernel-test-hooks — the" >&2
    echo "      TEST-HOOKS kernel is sitting on the production path. Re-run once no" >&2
    echo "      build-*-test-hooks-ci.sh lane is running." >&2
    exit 1
fi

cp "$KFS" kernel/src/embedded/kernel_fs.img

# ── 6. disk_v3.img ───────────────────────────────────────────────────────────
# MBR layout (tools/write-mbr.py, kernel/src/loader/disk_layout.rs):
#   P1 FAT32 @2048+524288 · P2 cell table @526336 · P3 snapshot @560000
#   P4 littlefs @800000 · P6 FAT cell-store @1062144 +65536
echo "==> assembling $DISK"
DISK_SECTORS=1127680                       # P6 end: 1062144 + 65536
CELLSTORE_BASE_LBA=1062144                 # MUST match api::disk::PART_CELLSTORE_BASE_LBA
CELLSTORE_SECTORS=65536                    # MUST match api::disk::PART_CELLSTORE_SECTORS

NEW_DISK="$DISK.new"
rm -f "$NEW_DISK"
truncate -s $((DISK_SECTORS * 512)) "$NEW_DISK"
"$PYTHON_BIN" tools/write-mbr.py "$NEW_DISK"
"$PYTHON_BIN" tools/mkfat32_inplace.py "$NEW_DISK" 524288 2048

# P2 bootstrap table. Paths and order mirror gen_disk.ps1's $table_args; the
# presence guards are what make a partially-built target/ still produce a bootable
# disk (as gen_disk.ps1 does), while the required artifacts above stop the
# dangerous cases.
table_args=(
    "$NEW_DISK"
    "/bin/vfs=$REL/service-vfs"
    "/bin/config=$REL/service-config"
    "/bin/shell=$REL/app-shell"
)
add_row() { [[ -f $1 ]] && table_args+=("$2=$1"); return 0; }

add_row "$REL/lua"                    "/bin/lua"
add_row "$REL/micropython"            "/bin/python"
add_row "$REL/doom"                   "/bin/doom"
add_row "$REL/tetris"                 "/bin/tetris"
add_row "$REL/tetris-c"               "/bin/tetris-c"
add_row "$REL/tetris-lua"             "/bin/tetris-lua"
add_row "$REL/bench"                  "/bin/bench"
add_row "$REL/bench-probe"            "/bin/bench-probe"
if [[ "${CELLOS_INCLUDE_CAPACITY_PROBE:-0}" == "1" ]]; then
    add_row "$REL/capacity-probe"     "/bin/capacity-probe"
    add_row "$REL/heavy-probe"        "/bin/heavy-probe"
fi
add_row "$REL/service-input"          "/bin/input"
add_row "$REL/service-net"            "/bin/net"
add_row "$REL/service-kms"            "/bin/kms"
add_row "$REL/service-net-broker"     "/bin/net-broker"
add_row "$REL/supervisor"             "/bin/supervisor"
add_row "$REL/platform"               "/bin/platform"
add_row "$REL/driver-nvme"            "/bin/nvme"
add_row "$REL/driver-e1000"           "/bin/e1000"
add_row "$REL/driver-virtio-net"      "/bin/virtio-net"
add_row "$REL/driver-virtio-blk"      "/bin/block"
add_row "$REL/driver-virtio-gpu"      "/bin/virtio-gpu"
add_row "$REL/service-compositor"     "/bin/compositor"
add_row "$REL/fb-console"             "/bin/fb-console"
add_row "$REL/desktop"                "/bin/desktop"
add_row "$REL/ocel"                   "/bin/ocel"
add_row "$REL/ocel-js"                "/bin/ocel-js"
add_row "$REL/robot-demo"             "/bin/robot-demo"
add_row "$REL/robot-dashboard"        "/bin/robot-dashboard"
add_row "$REL/hypha-llm-gateway"      "/bin/llm-gateway"
add_row "$REL/hypha-core"             "/bin/hypha"
add_row "$REL/hypha-tool-fs"          "/bin/tool-fs"
add_row "$REL/hypha-tool-sys"         "/bin/tool-sys"
add_row "$REL/hypha-tool-spawn"       "/bin/tool-spawn"
add_row "$REL/nc"                     "/bin/nc"
add_row "$REL/curl"                   "/bin/curl"
add_row "$REL/wget"                   "/bin/wget"
add_row "$REL/service-httpd"          "/bin/httpd"
add_row "$REL/mqtt"                   "/bin/mqtt"
add_row "$REL/posix-shim-test"        "/bin/posix-shim-test"
add_row "$REL/service-ai"             "/bin/ai"
add_row "$REL/ai-test"                "/bin/ai-test"
add_row "$(pwd)/models/tiny-llama-64.gguf" "/bin/ai-model.gguf"
add_row "$REL/input-test"             "/bin/input-test"
add_row "$REL/window-policy-probe"    "/bin/window-policy-probe"
table_args+=("/bin/viui-demo=$REL/viui-demo")
add_row "$REL/audio-demo"             "/bin/audio-demo"
add_row "$REL/app-https-demo"         "/bin/https-demo"
add_row "$REL/http-smoke"             "/bin/http-smoke"
add_row "$REL/cfi-test"               "/bin/cfi-test"
add_row "$REL/wx-test"                "/bin/wx-test"
add_row "$REL/vfs-test"               "/bin/vfs-test"
add_row "$REL/hotswap-demo-v1"        "/bin/hotswap-demo-v1"
add_row "$REL/hotswap-demo-v2"        "/bin/hotswap-demo-v2"
add_row "$REL/tier2-smoke"            "/bin/tier2-smoke"
add_row "$REL/tier2-exploit"          "/bin/tier2-exploit"
add_row "$REL/backend-supervisor"     "/bin/backend-supervisor"
add_row "$REL/backend-worker"         "/bin/backend-worker"
add_row "$REL/ls"                     "/bin/ls"
add_row "$REL/cat"                    "/bin/cat"
add_row "$REL/echo"                   "/bin/echo"
add_row "$REL/ps"                     "/bin/ps"
add_row "$REL/kill"                   "/bin/kill"
add_row "$REL/free"                   "/bin/free"
add_row "$REL/hotswap"                "/bin/hotswap"

"$PYTHON_BIN" tools/write-cell-table.py "${table_args[@]}"

# P6: standalone FAT volume with every cell at the FAT root by basename, written
# into the constant-addressed window. VFS's /bin BinOverlay reads it after a VIFS1
# miss, so the raw P2 table can be retired later without losing reachability.
echo "==> assembling P6 FAT cell-store (LBA $CELLSTORE_BASE_LBA)"
CELLSTORE="$KFS_TMP/cell_store.img"
store_args=("$CELLSTORE")
for row in "${table_args[@]:1}"; do
    src="${row#*=}"
    name="${row%%=*}"
    store_args+=("$src" "/${name#/bin/}")
done
"$PYTHON_BIN" tools/mkfat32.py "${store_args[@]}"

store_bytes=$(stat -c%s "$CELLSTORE")
if (( store_bytes > CELLSTORE_SECTORS * 512 )); then
    echo "FAIL: cell_store.img ($((store_bytes / 1024 / 1024)) MB) exceeds the $((CELLSTORE_SECTORS / 2048)) MB P6 window" >&2
    exit 1
fi
dd if="$CELLSTORE" of="$NEW_DISK" bs=512 seek="$CELLSTORE_BASE_LBA" conv=notrunc status=none

# ── 7. Verify the disk, then publish it ──────────────────────────────────────
# Capture the listing once: `read-cell-table.py | grep -q` would SIGPIPE the
# producer, and with `set -o pipefail` that non-zero status reads as a missing
# cell. Grepping a file keeps the exit status about the match.
"$PYTHON_BIN" tools/read-cell-table.py "$NEW_DISK" > "$KFS_TMP/cell-table.txt"
cat "$KFS_TMP/cell-table.txt"

expected_rows=$(( ${#table_args[@]} - 1 ))
actual_rows=$(sed -n '1s/^Cell table: \([0-9]*\) entries$/\1/p' "$KFS_TMP/cell-table.txt")
if [[ "$expected_rows" != "$actual_rows" ]]; then
    echo "FAIL: cell table has $actual_rows entries, expected $expected_rows" >&2
    exit 1
fi

# The lane-critical cells must be present AND be the binaries signed above (a
# presence-only check would pass for a stale table).
verify_row() {
    local path=$1 src=$2
    if ! grep -qF -- "$path" "$KFS_TMP/cell-table.txt"; then
        echo "FAIL: $path is missing from the cell table" >&2
        exit 1
    fi
    local on_disk source
    on_disk=$("$PYTHON_BIN" - "$NEW_DISK" "$path" <<'PY'
import struct, sys
SECTOR=512; BASE=526_336; MAGIC=0x5649_4F53_5F43_454C; LEN=64
import hashlib
disk, want = sys.argv[1], sys.argv[2]
with open(disk, "rb") as f:
    f.seek(BASE * SECTOR)
    magic, count = struct.unpack_from("<QI", f.read(512))
    assert magic == MAGIC, "bad cell-table magic"
    for _ in range(count):
        e = f.read(512)
        p = e[:LEN].split(b"\x00", 1)[0].decode()
        lba, size = struct.unpack_from("<QQ", e, LEN)
        if p == want:
            f.seek(lba * SECTOR)
            print(hashlib.sha256(f.read(size)).hexdigest())
            sys.exit(0)
sys.exit(f"{want} not in table")
PY
)
    source=$(sha256sum "$src" | cut -d' ' -f1)
    if [[ "$on_disk" != "$source" ]]; then
        echo "FAIL: $path on disk is not $src (stale cell in the table)" >&2
        exit 1
    fi
}
verify_row "/bin/vfs"        "$REL/service-vfs"
verify_row "/bin/vfs-test"   "$REL/vfs-test"
verify_row "/bin/tier2-smoke" "$REL/tier2-smoke"
verify_row "/bin/tier2-exploit" "$REL/tier2-exploit"
verify_row "/bin/posix-shim-test" "$REL/posix-shim-test"
verify_row "/bin/bench"      "$REL/bench"

rm -rf "$KFS_TMP"
mv -f "$NEW_DISK" "$DISK"

if (( ${#FAILED_OPTIONAL[@]} > 0 )); then
    echo "WARN: ${#FAILED_OPTIONAL[@]} optional cell(s) omitted: ${FAILED_OPTIONAL[*]}" >&2
fi

cat <<EOF

==> done
    disk:    $DISK  ($(stat -c%s "$DISK") bytes, sha256 $(sha256sum "$DISK" | cut -c1-16))
    kernel:  target/$TARGET/release/cellos-kernel
             sha256 $(sha256sum "target/$TARGET/release/cellos-kernel" | cut -c1-16) — the lanes boot this path, so
             re-check it if another lane builds a test-hooks kernel concurrently
    VIFS1:   kernel/src/embedded/kernel_fs.img  (sha256 $(sha256sum kernel/src/embedded/kernel_fs.img | cut -c1-16))

Run the RV64 integration lanes with:
    CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test \\
        --manifest-path tests/integration/Cargo.toml --test launch-profile -- --test-threads=1
    CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test \\
        --manifest-path tests/integration/Cargo.toml --test tier2-fault-isolation -- --test-threads=1
EOF
