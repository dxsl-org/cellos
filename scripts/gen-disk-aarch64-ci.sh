#!/usr/bin/env bash
# gen-disk-aarch64-ci.sh — assemble the production-feature AArch64 QEMU images on
# Linux, the way `scripts/gen-disk-ci.sh` does for RV64 and
# `scripts/build-aarch64-cells.ps1` does on a Windows dev box.
#
# Produces (all gitignored build artifacts, nothing in the shipping tree):
#   target/aarch64-prod-ci-embedded/kernel_fs.img  VIFS1 — the bootstrap ramdisk
#                                                  embedded in the kernel via
#                                                  include_bytes! (FAT)
#   target/aarch64-prod-ci-embedded/init           the init ELF, embedded separately
#   target/aarch64-prod-ci/aarch64-unknown-none-softfloat/release/cellos-kernel
#                                                  production-feature kernel
#   target/aarch64-prod-ci/disk_arm_virt.img       MBR disk: P1 FAT32, P2 cell
#                                                  table, P6 FAT cell-store
#
# Boot the integration suite against it with:
#   CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test \
#       --manifest-path tests/integration/Cargo.toml --test aarch64-boot -- --test-threads=1
#
# WHY THIS EXISTS
# ---------------
# The AArch64 integration lanes boot `target/aarch64-unknown-none-softfloat/release/
# cellos-kernel` plus `disk_arm_virt.img`, both of which are hand-assembled by the
# Windows recipes. On Linux there was no script that produced either, so
# `tests/integration/tests/aarch64-boot.rs` could only ever run against whatever
# artifact happened to be lying in `target/` — and `aarch64_periph_demo_gpio`,
# `aarch64_uart_input_delivery` and `aarch64_httpd_web_server_serves_requests`
# depend on cells (`periph-demo`, `input-test`, `httpd`/`virtio-net`) that only
# exist in an image assembled with the right cell list. `.github/workflows/ci.yml`
# (`qemu-aarch64-boot`) carries that list, and its own comment records the failure
# mode when it drifts: a missing cell surfaces as `shell: command not found`,
# which reads as a shell fault rather than as a packaging gap.
#
# WHAT THIS SCRIPT OWNS THAT THE SHARED PATHS CANNOT
# --------------------------------------------------
#   * Isolation. `target/aarch64-unknown-none-softfloat/release/cellos-kernel` is
#     written by BOTH the production lane and `build-aarch64-test-hooks-ci.sh`
#     (which builds the test-hooks kernel there and only THEN copies it aside).
#     A test-hooks kernel sitting on that path enables Tier-2 admission, so the
#     boot suite cannot use it as its production witness. Everything here lands
#     under `target/aarch64-prod-ci*`, which no other lane touches — the same
#     convention `scripts/build-aarch64-prod-refusal-ci.sh` uses.
#   * Features. No `--features test-hooks` on any cell or on the kernel, so the
#     image is the production posture (the `switch_ordering_qualified()` branch is
#     `aarch64 && test-hooks` is const-asserted false for this build).
#
# FLAVOUR RULE: only `build-aarch64-test-hooks-ci.sh` may build service-vfs /
# app-vfs-test with `--features test-hooks`. Every cell here is the default
# (production) flavour, and the assertions below turn a violation into a build
# failure rather than a mysterious lane failure.
#
# The cell list mirrors `.github/workflows/ci.yml`'s `Assemble aarch64 kernel_fs.img`
# step (which is itself kept in sync with `build-aarch64-cells.ps1`) and adds the
# two cells the integration suite needs on top of the CI boot job's prompt check:
# `driver-virtio-net` + `service-httpd`, without which
# `aarch64_httpd_web_server_serves_requests` has nothing to serve from.
#
# Usage:
#   bash scripts/gen-disk-aarch64-ci.sh [--no-cells] [--disk <path>] [--help]
#
# Requirements: python3, cargo + nightly (rust-src), clang, aarch64-linux-gnu-objcopy,
# a cross readelf, libclang (littlefs bindgen), python `cryptography` (signing).

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

TARGET="aarch64-unknown-none-softfloat"
# One isolated target dir for the cells and the kernel: they share the triple, so
# the `-Z build-std` core/alloc artifacts build once, and no other lane can read
# or clobber any of it.
CARGO_TARGET_DIR_OVERRIDE="target/aarch64-prod-ci"
REL="$CARGO_TARGET_DIR_OVERRIDE/$TARGET/release"
EMB="target/aarch64-prod-ci-embedded"
KERNEL="$REL/cellos-kernel"
DISK="target/aarch64-prod-ci/disk_arm_virt.img"
BUILD_CELLS=1

usage() {
    cat <<'EOF'
Assemble the production-feature AArch64 QEMU images on Linux: VIFS1 (kernel_fs.img)
plus the embedded init, the production kernel and an MBR disk image, all under
target/aarch64-prod-ci*.

Usage: bash scripts/gen-disk-aarch64-ci.sh [--no-cells] [--disk <path>] [--help]

  --no-cells      skip the cargo cell builds; assemble from the current target/
  --disk <path>   disk image to (re)write (default: target/aarch64-prod-ci/disk_arm_virt.img)
  --help          this text

Every cell is built with its DEFAULT (production) features; the `test-hooks`
flavour belongs to scripts/build-aarch64-test-hooks-ci.sh and is asserted absent
before the image is assembled.
EOF
    exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --no-cells) BUILD_CELLS=0; shift ;;
        --disk)     DISK="${2:?--disk needs a path}"; shift 2 ;;
        -h|--help)  usage 0 ;;
        *) echo "unknown argument: $1" >&2; usage 1 ;;
    esac
done

export CARGO_TARGET_DIR="$CARGO_TARGET_DIR_OVERRIDE"

# ── Toolchain probe ───────────────────────────────────────────────────────────
if command -v python3 >/dev/null 2>&1 && python3 -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN=python3
elif command -v python >/dev/null 2>&1 && python -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN=python
else
    echo "FAIL: a working Python 3 interpreter is required" >&2
    exit 1
fi

# littlefs2-sys compiles a vendored C core through cc-rs, which probes
# <target>-gcc unless CC is set. The bare-metal aarch64 target has no libc, so the
# C core is cross-compiled with clang against the vendored freestanding headers
# (same values as .cargo/config.toml's [env] block and the CI job env).
export CC_aarch64_unknown_none_softfloat="${CC_aarch64_unknown_none_softfloat:-clang}"
export CFLAGS_aarch64_unknown_none_softfloat="${CFLAGS_aarch64_unknown_none_softfloat:---target=aarch64-unknown-none-elf -ffreestanding -mgeneral-regs-only -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat="${BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat:---target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu}"

# Resolve a cross readelf for the section-level assertions below.
resolve_readelf() {
    local candidate
    for candidate in "${READELF:-}" aarch64-linux-gnu-readelf llvm-readelf readelf; do
        [[ -n "$candidate" ]] || continue
        if command -v "$candidate" >/dev/null 2>&1; then
            READELF="$(command -v "$candidate")"
            export READELF
            return 0
        fi
    done
    echo "FAIL: no readelf found (tried: aarch64-linux-gnu-readelf, llvm-readelf, readelf)" >&2
    return 1
}
resolve_readelf || exit 1

echo "==> toolchain: python=$PYTHON_BIN cc=$CC_aarch64_unknown_none_softfloat readelf=$READELF"

# ── 1. Build the cells ────────────────────────────────────────────────────────
# The package list mirrors `.github/workflows/ci.yml`'s `Build aarch64 cells` step
# (app-init, app-shell, service-vfs, service-config, service-net, service-input,
# service-compositor, periph-demo, input-test, app-sys-tools) minus the cells the
# image below does not carry, plus `driver-virtio-net` and `service-httpd`, which
# the integration suite's HTTP row needs and the CI prompt-only boot check does
# not. Fail-fast: a core failure aborts before a stale artifact can be signed and
# shipped.
build_cells() {
    local what=$1; shift
    echo "==> building $what"
    cargo build --release --target "$TARGET" -Z build-std=core,alloc "$@"
}

if [[ $BUILD_CELLS -eq 1 ]]; then
    build_cells "bootstrap + core services" -p app-init -p app-shell -p service-vfs -p service-config
    build_cells "sys tools (ls, cat, echo, ps, kill)" -p app-sys-tools
    build_cells "input service + input-test" -p service-input -p input-test
    build_cells "network stack + httpd" -p service-net -p driver-virtio-net -p service-httpd
    build_cells "peripheral demo (aarch64_periph_demo_gpio)" -p periph-demo
fi

# ── 2. Flavour + shape assertions ─────────────────────────────────────────────
# The image must carry exactly the cells it claims, and carry them in their
# PRODUCTION flavour. A test-hooks service-vfs would mount its test FAT against the
# production disk and turn every cell-store cell into "command not found"; an
# unsigned cell is DENIED at spawn under `signing-required` and the guest never
# reaches a shell. Fail here, where the cause is obvious.
CELL_BINARIES=(
    "$REL/app-shell"
    "$REL/service-vfs"
    "$REL/service-config"
    "$REL/service-input"
    "$REL/input-test"
    "$REL/periph-demo"
    "$REL/ls"
    "$REL/cat"
    "$REL/echo"
    "$REL/ps"
    "$REL/kill"
    "$REL/service-net"
    "$REL/driver-virtio-net"
    "$REL/service-httpd"
)
CELL_IMAGE_PATHS=(
    /bin/shell
    /bin/vfs
    /bin/config
    /bin/input
    /bin/input-test
    /bin/periph-demo
    /bin/ls
    /bin/cat
    /bin/echo
    /bin/ps
    /bin/kill
    /bin/net
    /bin/virtio-net
    /bin/httpd
)

echo "==> verifying ${#CELL_BINARIES[@]} cell binaries..."
for binary in "${CELL_BINARIES[@]}"; do
    if [[ ! -s "$binary" ]]; then
        echo "FAIL: expected nonempty cell binary not found: $binary" >&2
        echo "      build it with: bash scripts/gen-disk-aarch64-ci.sh" >&2
        exit 1
    fi
done

# `strings` markers, not sizes: the test-hooks flavour adds `access/selftest.rs`
# to service-vfs and the `rdir-quota` / `quota:` scenarios to vfs-test, and both
# are absent from the production binary.
#
# Capture the dump rather than piping it into `grep -q`: under `set -o pipefail`,
# `grep -q` exits at its first match, `strings` then dies on SIGPIPE (141), and
# the pipeline's non-zero status makes the `if` false — the guard would silently
# pass with the marker present (measured 2026-10-02 on the RV64 sibling builder).
vfs_strings=$(strings -n 8 "$REL/service-vfs" || true)
if grep -qF -- "access/selftest.rs" <<<"$vfs_strings"; then
    echo "FAIL: $REL/service-vfs carries the test-hooks marker 'access/selftest.rs'." >&2
    echo "      service-vfs must be built WITHOUT --features test-hooks for a" >&2
    echo "      production image (scripts/build-aarch64-test-hooks-ci.sh owns that flavour)." >&2
    exit 1
fi

# ── 3. Sign every cell (Ed25519 dev key) ──────────────────────────────────────
# One cellos-sign invocation for the whole set: the signature attests that F1/F5
# held for the tree, which is a per-tree claim, not a per-binary one. Under the
# `signing-required` feature (a kernel default) an unsigned cell is DENIED at spawn.
echo "==> signing cells"
# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh
sign_cells "${CELL_BINARIES[@]}" "$REL/app-init"

has_section() {
    "$READELF" -S "$1" 2>/dev/null | grep -q "__ViCell_$2"
}

# The artifacts the suite drives must be signed, and the image must NOT contain a
# domain-class cell: this is the production image, not the refusal witness, and a
# `/bin/tier2-*` here would make the refusal tests pass against the wrong image.
for signed in "$REL/app-shell" "$REL/periph-demo" "$REL/service-net" "$REL/service-httpd"; do
    if ! has_section "$signed" sig; then
        echo "FAIL: $signed must carry a __ViCell_sig section to be admitted at all" >&2
        exit 1
    fi
done

# Class check via the same tool the refusal lane uses. app-shell must stay a
# non-domain cell (a domain-class shell would be refused and no boot test could
# pass), and periph-demo must stay non-domain too.
check_class() {
    (cd "$SCRIPT_DIR/../tools" && "$PYTHON_BIN" check_elf.py "$1" 2>/dev/null || true) \
        | sed -n 's/^Protection class: //p'
}
SHELL_CLASS="$(check_class "$REPO_ROOT/$REL/app-shell")"
PERIPH_CLASS="$(check_class "$REPO_ROOT/$REL/periph-demo")"
if [[ "$SHELL_CLASS" != "legacy (no explicit class)" ]]; then
    echo "FAIL: $REL/app-shell must stay a non-domain cell (got: '$SHELL_CLASS')" >&2
    exit 1
fi
if [[ "$PERIPH_CLASS" == "untrusted" ]]; then
    echo "FAIL: $REL/periph-demo must not be a domain-class (UNTRUSTED) artifact: a" >&2
    echo "      production image refuses those at admission, so the demo could never run." >&2
    exit 1
fi
echo "    app-shell: signed + $SHELL_CLASS   periph-demo: signed + $PERIPH_CLASS"

# ── 4. VIFS1 (kernel_fs.img) ──────────────────────────────────────────────────
# VIFS1 carries the bootstrap chain, the cells whose launch edge needs the kernel
# loader, and (because this IS the whole image for the boot suite) every cell the
# suite launches from the shell: `aarch64_periph_demo_gpio` drives /bin/periph-demo,
# `aarch64_uart_input_delivery` drives /bin/input-test, and
# `aarch64_httpd_web_server_serves_requests` drives /bin/httpd over /bin/virtio-net.
# The disk below carries the same cells, but VIFS1 is the copy that cannot be lost
# to a stale or hand-built disk.
echo "==> assembling kernel_fs.img (VIFS1)"
rm -rf "$EMB" "$CARGO_TARGET_DIR_OVERRIDE/kfs.tmp"
mkdir -p "$EMB" "$CARGO_TARGET_DIR_OVERRIDE/kfs.tmp"
KFS_TMP="$CARGO_TARGET_DIR_OVERRIDE/kfs.tmp"

printf 'ViCell-aarch64' > "$KFS_TMP/hostname"

# shellcheck source=scripts/lib-bake-policy.sh
source scripts/lib-bake-policy.sh
bake_policy "$KFS_TMP/POLICY.BIN"

KFS="$EMB/kernel_fs.img"
kfs_args=("$KFS")
for index in "${!CELL_BINARIES[@]}"; do
    kfs_args+=("${CELL_BINARIES[$index]}" "${CELL_IMAGE_PATHS[$index]}")
done
kfs_args+=(
    "$KFS_TMP/hostname"   "/etc/hostname"
    # Root-level 8.3-uppercase: kernel/src/policy.rs reads exactly /POLICY.BIN.
    "$KFS_TMP/POLICY.BIN" "/POLICY.BIN"
)

"$PYTHON_BIN" tools/mkfat32.py "${kfs_args[@]}"
if [[ ! -s "$KFS" ]]; then
    echo "FAIL: mkfat32.py did not produce a nonempty kernel_fs.img" >&2
    exit 1
fi

# Prove the layout rather than trusting the exit code: mkfat32.py exits 0 for an
# image whose destination paths went astray, and a missing /POLICY.BIN degrades
# silently to the dev-permissive branch (an image that looks provisioned and
# enforces nothing).
"$PYTHON_BIN" tools/inspect_fat.py "$KFS" > "$KFS_TMP/fat-layout.txt"
awk '/--- \/bin ---/ { capture = 1; next } capture && (/dir \(SFN=/ || /^--- /) { exit } capture' \
    "$KFS_TMP/fat-layout.txt" > "$KFS_TMP/bin-layout.txt"
BIN_FILE_COUNT=$(grep -c -- ' attr=20 ' "$KFS_TMP/bin-layout.txt" || true)
if [[ "$BIN_FILE_COUNT" -ne "${#CELL_IMAGE_PATHS[@]}" ]]; then
    echo "FAIL: kernel_fs.img contains $BIN_FILE_COUNT /bin cells; expected ${#CELL_IMAGE_PATHS[@]}:" >&2
    cat "$KFS_TMP/fat-layout.txt" >&2
    exit 1
fi
for image_path in "${CELL_IMAGE_PATHS[@]}"; do
    image_name="${image_path#/bin/}"
    if ! grep -Fq -- "-> LFN '$image_name'  attr=20" "$KFS_TMP/bin-layout.txt"; then
        echo "FAIL: kernel_fs.img is missing exact path $image_path:" >&2
        cat "$KFS_TMP/fat-layout.txt" >&2
        exit 1
    fi
done
assert_policy_in_image "$KFS_TMP/fat-layout.txt" || exit 1
echo "    kernel_fs.img: $(du -sh "$KFS" | cut -f1)"

# INIT_ELF is embedded separately from kernel_fs.img.
cp "$REL/app-init" "$EMB/init"
echo "    init: $(du -sh "$EMB/init" | cut -f1)"

# ── 5. Kernel (embeds VIFS1 via include_bytes!) ───────────────────────────────
# EMBEDDED_OVERRIDE points the build script at our staging dir, so the in-tree
# `kernel/src/embedded-aarch64/**` is never rewritten. RUSTFLAGS is deliberately
# NOT set: .cargo/config.toml already carries `-C relocation-model=pic` and the
# +bti,+paca,+pacg target features, and setting the env var would REPLACE them.
echo "==> building the production-feature kernel (no test-hooks)"
rm -f "$KERNEL"
EMBEDDED_OVERRIDE="$EMB" \
cargo build --release \
    -p cellos-kernel \
    --target "$TARGET" \
    -Z build-std=core,alloc

if [[ ! -s "$KERNEL" ]]; then
    echo "FAIL: kernel not produced at $KERNEL" >&2
    exit 1
fi

# The feature set is the point of the image: assert it structurally rather than by
# reading the command line. `S22-AARCH64-DOMAIN-LIVE` exists only in the test-hooks
# build of the admission fixtures; the posture string exists in both, so it alone
# would not discriminate.
if grep -qa "S22-AARCH64-DOMAIN-LIVE" "$KERNEL"; then
    echo "FAIL: $KERNEL carries test-hooks fixtures — it is not a production image" >&2
    exit 1
fi
if ! grep -qa "Tier 2 admission: DISABLED (development profile, phase-02 switch-ordering gate)" "$KERNEL"; then
    echo "FAIL: $KERNEL does not carry the phase-02 disabled-posture message" >&2
    exit 1
fi

# ── 6. disk_arm_virt.img ──────────────────────────────────────────────────────
# MBR layout (tools/write-mbr.py, kernel/src/loader/disk_layout.rs). These LBA
# constants are exactly the ones format-disk-arm.sh and gen-disk-ci.sh use; they
# are load-bearing (the loader reads the table from a constant address), so a
# change here must be a change there too.
#   P1 FAT32 @2048+524288 · P2 cell table @526336 · P3 snapshot @560000
#   P4 littlefs @800000 · P6 FAT cell-store @1062144 +65536
echo "==> assembling $DISK"
DISK_SECTORS=1127680                       # P6 end: 1062144 + 65536
CELLSTORE_BASE_LBA=1062144
CELLSTORE_SECTORS=65536

mkdir -p "$(dirname "$DISK")"
NEW_DISK="$DISK.new"
rm -f "$NEW_DISK"
truncate -s $((DISK_SECTORS * 512)) "$NEW_DISK"
"$PYTHON_BIN" tools/write-mbr.py "$NEW_DISK"
"$PYTHON_BIN" tools/mkfat32_inplace.py "$NEW_DISK" 524288 2048

# P2 bootstrap table. Host-side paths and image-side paths are the same list as
# VIFS1 above: the loader's fallback reads it when a VIFS1 lookup misses, and a
# cell reachable from only one of the two is a packaging bug waiting for a lane.
table_args=("$NEW_DISK")
for index in "${!CELL_BINARIES[@]}"; do
    table_args+=("${CELL_IMAGE_PATHS[$index]}=${CELL_BINARIES[$index]}")
done
"$PYTHON_BIN" tools/write-cell-table.py "${table_args[@]}"

# P6: standalone FAT volume with every cell at the FAT root by basename, written
# into the constant-addressed window. VFS's /bin BinOverlay reads it after a VIFS1
# miss.
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

# ── 7. Verify the disk, then publish it ───────────────────────────────────────
# Capture the listing once: `read-cell-table.py | grep -q` would SIGPIPE the
# producer, and with `set -o pipefail` that non-zero status reads as a missing cell.
"$PYTHON_BIN" tools/read-cell-table.py "$NEW_DISK" > "$KFS_TMP/cell-table.txt"
cat "$KFS_TMP/cell-table.txt"

expected_rows=${#CELL_IMAGE_PATHS[@]}
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
import struct, sys, hashlib
SECTOR=512; BASE=526_336; MAGIC=0x5649_4F53_5F43_454C; LEN=64
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
verify_row "/bin/periph-demo"   "$REL/periph-demo"
verify_row "/bin/input-test"    "$REL/input-test"
verify_row "/bin/httpd"         "$REL/service-httpd"
verify_row "/bin/virtio-net"    "$REL/driver-virtio-net"
verify_row "/bin/vfs"           "$REL/service-vfs"
verify_row "/bin/shell"         "$REL/app-shell"

# A production disk carries no domain-class fixture: those belong to
# scripts/build-aarch64-prod-refusal-ci.sh's own image, and a stray /bin/tier2-*
# here would let the refusal tests deny a cell this image was never about.
if grep -qE -- '/bin/tier2-(smoke|exploit)' "$KFS_TMP/cell-table.txt"; then
    echo "FAIL: production disk carries a domain-class Tier-2 fixture" >&2
    exit 1
fi

rm -rf "$KFS_TMP"
mv -f "$NEW_DISK" "$DISK"

cat <<EOF

==> done
    kernel: $KERNEL
    VIFS1:  $KFS
    init:   $EMB/init
    disk:   $DISK  ($(stat -c%s "$DISK") bytes, sha256 $(sha256sum "$DISK" | cut -c1-16))
    kernel sha256 $(sha256sum "$KERNEL" | cut -c1-16) — production features, no test-hooks

The integration suite finds these automatically
(tests/integration/tests/aarch64-boot.rs prefers target/aarch64-prod-ci/** over the
legacy in-tree paths). Override with CELLOS_AARCH64_KERNEL / CELLOS_AARCH64_DISK.

Run the lanes with:
    CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test \\
        --manifest-path tests/integration/Cargo.toml --test aarch64-boot -- --test-threads=1
EOF
