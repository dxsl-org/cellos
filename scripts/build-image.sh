#!/usr/bin/env bash
# One entry point for building a Cellos image with options, so a test image is a
# command instead of a remembered combination of builder flags.
#
# Cellos is embedded-first: the base image is storage + net + a shell on a serial
# console, and every other service is an option. This script resolves the board,
# checks that the options make sense together, calls the board's builder and
# prints the gate that qualifies the result.
#
# Usage:
#   bash scripts/build-image.sh [options]
#
#   --board <slug>        board from boards/*/*/board.rs (default raspberry-pi/3-model-b)
#   --ui / --no-ui        display path: compositor, fb-console, KMS where it applies
#   --input / --no-input  keyboard/mouse event routing (default: on for boards with USB)
#   --ai / --no-ai        Spec 24 inference service plus the config service it reads
#   --supervisor          hotswap supervisor cell
#   --tier3 / --no-tier3  guest-hosting cell (default: on where the board can host a guest)
#   --autostart           preload the VM at boot so the first Tier-3 app starts fast
#   --volatile-disk       no persistent guest disk (the QEMU lanes)
#   --guest <profile>     guest image profile for the builders that take one
#   --skip-fetch          reuse .alpine-cache instead of downloading
#   --drivers <set>       default | minimal (minimal = the board's bring-up drivers)
#   --out <dir>           copy the built artifacts into <dir>
#   --list                list the boards and what each one supports, then exit
#   --dry-run             print the resolved plan and the gate command, then exit
#
# Examples:
#   bash scripts/build-image.sh --list
#   bash scripts/build-image.sh --board raspberry-pi/3-model-b --no-tier3
#   bash scripts/build-image.sh --board raspberry-pi/3-model-b --autostart
#   bash scripts/build-image.sh --board qemu/virt-aarch64 --dry-run
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BOARD="raspberry-pi/3-model-b"
UI=0
INPUT=1
AI=0
SUPERVISOR=0
TIER3=1
TIER3_SET=0
AUTOSTART=0
VOLATILE=0
GUEST=""
SKIP_FETCH=0
DRIVERS="default"
OUT=""
LIST=0
DRY_RUN=0

usage() { sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        --board) BOARD="${2:?--board needs a slug}"; shift 2 ;;
        --ui) UI=1; shift ;;
        --no-ui) UI=0; shift ;;
        --input) INPUT=1; shift ;;
        --no-input) INPUT=0; shift ;;
        --ai) AI=1; shift ;;
        --no-ai) AI=0; shift ;;
        --supervisor) SUPERVISOR=1; shift ;;
        --tier3) TIER3=1; TIER3_SET=1; shift ;;
        --no-tier3) TIER3=0; TIER3_SET=1; shift ;;
        --autostart) AUTOSTART=1; shift ;;
        --volatile-disk) VOLATILE=1; shift ;;
        --guest) GUEST="${2:?--guest needs a profile}"; shift 2 ;;
        --drivers) DRIVERS="${2:?--drivers needs a set}"; shift 2 ;;
        --skip-fetch) SKIP_FETCH=1; shift ;;
        --out) OUT="${2:?--out needs a directory}"; shift 2 ;;
        --list) LIST=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "ERROR: unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if (( AUTOSTART && !TIER3 )); then
    echo "ERROR: --autostart needs --tier3 (nothing to preload otherwise)" >&2
    exit 2
fi

# ── Boards ───────────────────────────────────────────────────────────────────
# The board decides the target, the image kind and which builder owns it. A board
# with no builder is named rather than approximated: an image that cannot be
# built is a gap, not a default.
board_row() {
    case "$1" in
        raspberry-pi/3-model-b)
            echo "aarch64-unknown-none-softfloat|raw kernel8.img + VIFS1 FAT|make-hypervisor-fs-rpi3.sh|scripts/qemu-rpi3-tier3.sh|ui,ai,input,supervisor,tier3,autostart,volatile,drivers,guest|alpine,alpine-wide,alpine-gui" ;;
        qemu/virt-aarch64)
            echo "aarch64-unknown-none-softfloat|kernel ELF + VIFS1 FAT|make-hypervisor-fs.sh|scripts/qemu-hypervisor-smoke.sh|input,ui,ai,supervisor,tier3,autostart,guest|alpine" ;;
        qemu/q35-x86_64)
            echo "x86_64-unknown-none|Limine ISO|make-hypervisor-fs-x86.sh|scripts/qemu-hypervisor-smoke-x86.sh|input,ui,ai,supervisor,tier3,autostart,guest,volatile|alpine,alpine-wide,ubuntu" ;;
        *) return 1 ;;
    esac
}

if (( LIST )); then
    echo "Profiles are not a thing here: every image is the embedded-first base"
    echo "(vfs + net + shell on a serial console) plus options."
    echo
    printf '%-28s %-32s %-34s %s\n' BOARD TARGET IMAGE BUILDER
    for slug in raspberry-pi/3-model-b qemu/virt-aarch64 qemu/q35-x86_64; do
        IFS='|' read -r target image builder gate options guests <<<"$(board_row "$slug")"
        printf '%-28s %-32s %-34s %s\n' "$slug" "$target" "$image" "$builder"
        printf '%-28s options: %s\n' "" "$options"
        printf '%-28s gate:    %s\n' "" "$gate"
        printf '%-28s guests:  %s\n' "" "$guests"
    done
    echo
    echo "Boards with no builder yet (a gap, not a default): raspberry-pi/4-model-b,"
    echo "starfive/visionfive-2, milk-v/pioneer, qemu/virt-riscv64, qemu/q35-x86_32,"
    echo "qemu/virt-aarch32."
    exit 0
fi

if ! row="$(board_row "$BOARD")"; then
    echo "ERROR: no image builder for board '$BOARD'" >&2
    echo "Run 'bash scripts/build-image.sh --list' for the boards that have one." >&2
    exit 2
fi
IFS='|' read -r TARGET IMAGE_KIND BUILDER GATE SUPPORTED GUESTS <<<"$row"

# ── Option validation ────────────────────────────────────────────────────────
supports() { [[ ",$SUPPORTED," == *",$1,"* ]]; }
want() {
    local name="$1" on="$2"
    if (( on )) && ! supports "$name"; then
        echo "ERROR: board '$BOARD' does not support --$name yet ($BUILDER)" >&2
        exit 2
    fi
}
case "$DRIVERS" in
    default|minimal) ;;
    *) echo "ERROR: --drivers takes 'default' or 'minimal'" >&2; exit 2 ;;
esac
if [[ "$DRIVERS" == "minimal" ]] && ! supports drivers; then
    echo "ERROR: board '$BOARD' has no trimmed driver set yet ($BUILDER)" >&2
    exit 2
fi
want ui "$UI"
want ai "$AI"
want tier3 "$TIER3_SET"
want autostart "$AUTOSTART"
want volatile "$VOLATILE"
if [[ -n "$GUEST" ]]; then
    if ! supports guest; then
        echo "ERROR: board '$BOARD' has no --guest option ($BUILDER)" >&2
        exit 2
    fi
    if [[ ",$GUESTS," != *",$GUEST,"* ]]; then
        echo "ERROR: board '$BOARD' has no guest profile '$GUEST' (have: $GUESTS)" >&2
        exit 2
    fi
fi

# The Pi has no display driver in its Tier-3 image path yet; asking for the UI
# bundle there builds the cells but there is nothing for the compositor to draw on.
if (( UI )) && [[ "$BOARD" == "raspberry-pi/3-model-b" ]]; then
    echo "WARN: the Pi 3 image path has no display driver; --ui adds compositor/fb-console only" >&2
fi

# ── Plan ─────────────────────────────────────────────────────────────────────
BASE_OPTIONS=(input)   # every image that has a keyboard source routes its events
(( UI )) && BASE_OPTIONS+=(ui)
(( AI )) && BASE_OPTIONS+=(ai)
(( SUPERVISOR )) && BASE_OPTIONS+=(supervisor)
(( TIER3 )) && BASE_OPTIONS+=(tier3)
(( AUTOSTART )) && BASE_OPTIONS+=(tier3-autostart)

echo "[build-image] board:   $BOARD ($TARGET, $IMAGE_KIND)"
echo "[build-image] options: ${BASE_OPTIONS[*]}"
echo "[build-image] tier3:   $( (( TIER3 )) && echo "packaged" || echo "not packaged (no guest can run)" )"
echo "[build-image] guest:   ${GUEST:-default}"
echo "[build-image] drivers: $DRIVERS"
echo "[build-image] builder: $BUILDER"
echo "[build-image] gate:    $GATE"

if (( DRY_RUN )); then
    echo "[build-image] dry run — nothing built"
    exit 0
fi

# ── Build ────────────────────────────────────────────────────────────────────
BUILDER_ARGS=()
(( SKIP_FETCH )) && BUILDER_ARGS+=(--skip-fetch)
(( VOLATILE )) && BUILDER_ARGS+=(--volatile-disk)
(( AUTOSTART )) && BUILDER_ARGS+=(--autostart)
(( TIER3 )) || BUILDER_ARGS+=(--no-tier3)
(( UI )) && BUILDER_ARGS+=(--ui)
(( AI )) && BUILDER_ARGS+=(--ai)
[[ "$DRIVERS" == "minimal" ]] && BUILDER_ARGS+=(--minimal-drivers)
[[ -n "$GUEST" ]] && BUILDER_ARGS+=(--guest "$GUEST")

case "$BUILDER" in
    make-hypervisor-fs-rpi3.sh)
        bash "scripts/$BUILDER" "${BUILDER_ARGS[@]}"
        ARTIFACTS=(target/rpi3-hv-embedded/kernel8.img target/rpi3-hv-embedded/kernel_fs.img)
        ;;
    make-hypervisor-fs.sh)
        # The virt builder owns the cell image; the kernel build and the disk
        # image are the lane's two extra steps (see the Tier-3b guide).
        export HV_INIT_MIN=1
        [[ -n "$GUEST" ]] && export HV_GUEST_PROFILE="$GUEST"
        bash "scripts/$BUILDER" "${BUILDER_ARGS[@]}"
        RUSTFLAGS="-C relocation-model=pic -C target-feature=+bti,+paca,+pacg" \
            EMBEDDED_OVERRIDE="kernel/src/embedded-hv" \
            cargo build --release -p cellos-kernel --features qemu-virt-1g \
            --target "$TARGET" -Z build-std=core,alloc
        bash scripts/format-disk-hv-arm.sh disk_hv_arm.img >/dev/null
        ARTIFACTS=("target/$TARGET/release/cellos-kernel" disk_hv_arm.img)
        ;;
    make-hypervisor-fs-x86.sh)
        export HV_VOLATILE_DISK="$VOLATILE"
        export HV_INIT_MIN=1
        [[ -n "$GUEST" ]] && export HV_GUEST_PROFILE="$GUEST"
        bash "scripts/$BUILDER" "${BUILDER_ARGS[@]}"
        RUSTFLAGS="-C relocation-model=static -C code-model=kernel -C no-redzone=yes -Z cf-protection=full" \
            EMBEDDED_OVERRIDE="kernel/src/embedded-hv-x86" \
            cargo build --release -p cellos-kernel --target "$TARGET" -Z build-std=core,alloc
        bash scripts/x86/make-iso-ci.sh build/vicell-x86-hv.iso >/dev/null
        ARTIFACTS=(build/vicell-x86-hv.iso)
        ;;
esac

if [[ -n "$OUT" ]]; then
    mkdir -p "$OUT"
    for artifact in "${ARTIFACTS[@]}"; do
        [[ -e "$artifact" ]] || { echo "ERROR: expected artifact missing: $artifact" >&2; exit 1; }
        cp "$artifact" "$OUT/"
        echo "[build-image] $artifact -> $OUT/"
    done
fi

echo "[build-image] done. Qualify it with:"
echo "  bash $GATE ${ARTIFACTS[0]}"
