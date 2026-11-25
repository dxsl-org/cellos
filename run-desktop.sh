#!/usr/bin/env bash
# Run CellOS Desktop & Interactive Shell in QEMU
# Keystrokes typed in this terminal go directly to Cellos > via serial.
# The QEMU window displays the graphical screen (Compositor + Ocel / Chrome).

set -euo pipefail
REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

ISO="build/cellos-x86_64-desktop.qcow2"

# `--rebuild`: dev fixture image from the current tree — assembles the desktop
# image from source (`assemble-x86-image.py --profile desktop`: the desktop cells,
# Chrome's launcher and hypervisor, the Chrome guest's kernel and initramfs),
# builds + signs Ocel (desktop profile) and NET (qemu-dev-https), replaces them in
# that image, builds the kernel around it, and makes the disk from an *explicitly
# selected* kernel copy (`X86_KERNEL`, the fixture route that stays open while the
# default rollout route is blocked). This is a local development image, not a
# release or qualification artifact. Signing needs a passing F1/F5 for the whole tree.
# The image is assembled in CELLOS_DESKTOP_WORK (default /tmp/cellos-desktop-fixture):
# the assembler's dev-key route refuses an output or cargo target directory inside
# the repository.
if [[ "${1:-}" == "--rebuild" ]]; then
    WORK="${CELLOS_DESKTOP_WORK:-/tmp/cellos-desktop-fixture}"
    KFS="$WORK/embedded/kernel_fs.img"
    mkdir -p build "$WORK"
    echo "  [Build]    Desktop image ..."
    CARGO_TARGET_DIR="$WORK/cells-target" python3 scripts/assemble-x86-image.py \
        --profile desktop --out-dir "$WORK/embedded"
    echo "  [Build]    Ocel (desktop) ..."
    RUSTFLAGS="-C relocation-model=pic" cargo build --release -p ocel --no-default-features --features desktop \
        --target x86_64-unknown-none -Z build-std=core,alloc
    echo "  [Build]    NET (qemu-dev-https) ..."
    RUSTFLAGS="-C relocation-model=pic" cargo build --release -p service-net --no-default-features --features qemu-dev-https \
        --target x86_64-unknown-none -Z build-std=core,alloc
    python3 scripts/cellos-sign --sign target/x86_64-unknown-none/release/ocel target/x86_64-unknown-none/release/service-net
    python3 scripts/package-ocel-profile.py "$KFS" target/x86_64-unknown-none/release/ocel
    python3 scripts/package-ocel-profile.py "$KFS" target/x86_64-unknown-none/release/service-net --dest /bin/net
    # The assembler's manifest no longer describes the image, and the kernel build
    # refuses an image that differs from the manifest beside it.
    rm -f "$WORK/embedded/manifest.json"
    echo "  [Build]    Kernel ..."
    EMBEDDED_OVERRIDE="$WORK/embedded" cargo build --release -p cellos-kernel --target x86_64-unknown-none \
        --features "dev-policy-key,dev-signing-key,dev-weak-rng,signing-required,native-domains,qemu-dev-https,large-cell-va"
    cp target/x86_64-unknown-none/release/cellos-kernel build/cellos-kernel-desktop-fixture
    X86_KERNEL="$REPO_ROOT/build/cellos-kernel-desktop-fixture" X86_ISO_ROOT="$REPO_ROOT/build/desktop-fixture-isoroot" \
        bash scripts/x86/make-iso-ci.sh build/cellos-x86_64-desktop-fixture.iso
    limine/limine-8.7.0/bin/limine bios-install build/cellos-x86_64-desktop-fixture.iso
    if [[ -f "$ISO" ]]; then cp "$ISO" "${ISO%.qcow2}.prev.qcow2"; fi
    qemu-img convert -f raw -O qcow2 build/cellos-x86_64-desktop-fixture.iso "$ISO"
    echo "  [Build]    Disk written: $ISO (previous kept as ${ISO%.qcow2}.prev.qcow2)"
    shift
fi

if [[ ! -f "$ISO" ]]; then
    echo "ERROR: image $ISO not found. Build the dev fixture image: bash run-desktop.sh --rebuild" >&2
    exit 1
fi

# Tự động giải phóng lock ổ đĩa nếu tiến trình cũ chưa thoát
if command -v fuser >/dev/null 2>&1; then
    if fuser "$ISO" >/dev/null 2>&1; then
        echo "  [Disk]     Giải phóng tiến trình đang chiếm ổ đĩa $ISO..."
        fuser -k "$ISO" >/dev/null 2>&1 || true
        sleep 1
    fi
fi

# Phát hiện phần cứng ảo hóa KVM
ACCEL_OPTS=()
if [[ -w /dev/kvm ]]; then
    ACCEL_OPTS=(-enable-kvm -cpu host)
else
    echo "  [Accel]    /dev/kvm không khả dụng, sử dụng phần mềm giả lập TCG (qemu64)..."
    ACCEL_OPTS=(-cpu "qemu64,+pdpe1gb")
fi

echo "=========================================================================="
echo "  Starting CellOS Interactive Desktop (Intel x86-64 QEMU)"
echo "=========================================================================="
echo "  [Terminal] Gõ lệnh trực tiếp tại dấu nhắc 'Cellos > ' trên terminal này."
echo "  [Window]   Cửa sổ đồ họa GTK hiển thị trực tiếp (Desktop, Taskbar, Ocel, Files, Terminal)."
echo "  [VNC]      VNC server sẵn sàng tại localhost:5900 (127.0.0.1:0)."
echo "  [SSH]      Cổng SSH chuyển tiếp vào máy host: ssh -p 2222 root@localhost."
echo "  [QMP]      Socket quản lý QMP sẵn sàng tại /tmp/qmp-desktop.sock."
echo "  [Thoát]    Nhấn Ctrl+A rồi nhấn X để thoát QEMU bất cứ lúc nào."
echo "=========================================================================="
echo ""

# Khởi động dịch vụ thời gian xác thực độc lập cho HTTPS nếu chưa chạy
TIME_PID=""
if ! pgrep -f "qemu-dev-time.py" > /dev/null 2>&1; then
    echo "  [Time]     Khởi động qemu-dev-time.py (dịch vụ Roughtime xác thực cho HTTPS)..."
    python3 scripts/qemu-dev-time.py >/dev/null 2>&1 &
    TIME_PID=$!
fi

cleanup() {
    if [[ -n "$TIME_PID" ]]; then
        kill "$TIME_PID" 2>/dev/null || true
    fi
    rm -f /tmp/qmp-desktop.sock
}
trap cleanup EXIT INT TERM

# Dọn dẹp QMP socket cũ nếu còn tồn tại
rm -f /tmp/qmp-desktop.sock

# Chrome (Tier 3): the guest root disk from scripts/make-chrome-guest-disk.sh, when present.
# VFS mounts the lowest-addressed disk as /mnt/sd, so the guest disk is created before
# the boot disk; the guest takes up to 3072 MiB of the machine's RAM in one run.
CHROME_DISK="build/chrome-guest-disk.img"
# Chrome's Downloads: a FAT32 data disk (scripts/make-chrome-files-disk.sh), created once and
# kept; VFS mounts it under /mnt/disk/ and the hypervisor shares its Chrome folder. It sits on
# q35's AHCI controller: each block driver Cell drives one controller (NVMe and virtio-blk
# are taken).
CHROME_FILES="build/chrome-files.img"
MEMORY=2048M
DISK_OPTS=(-drive file="$ISO",format=qcow2,if=virtio)
if [[ -f "$CHROME_DISK" ]]; then
    echo "  [Chrome]   Đĩa khách Chrome: $CHROME_DISK (gắn làm /mnt/sd)."
    MEMORY=6144M
    DISK_OPTS=(-drive file="$CHROME_DISK",if=none,id=sd,format=raw
               -device nvme,drive=sd,serial=CELLOSSD
               -drive file="$ISO",format=qcow2,if=none,id=boot
               -device virtio-blk-pci,drive=boot,bootindex=0)
    bash scripts/make-chrome-files-disk.sh "$CHROME_FILES"
    echo "  [Chrome]   Đĩa dữ liệu (Downloads của Chrome): $CHROME_FILES."
    DISK_OPTS+=(-drive file="$CHROME_FILES",if=none,id=files,format=raw
                -device ide-hd,drive=files,bus=ide.1)
else
    echo "  [Chrome]   Chưa có $CHROME_DISK: chạy bash scripts/make-chrome-guest-disk.sh $CHROME_DISK"
fi

exec qemu-system-x86_64 \
  "${ACCEL_OPTS[@]}" \
  -machine q35,vmport=on \
  -m "$MEMORY" \
  -smp 2 \
  "${DISK_OPTS[@]}" \
  -netdev user,id=net0,hostfwd=tcp::2222-:22 \
  -device e1000,netdev=net0 \
  -device virtio-keyboard-pci \
  -device virtio-tablet-pci \
  -device virtio-mouse-pci \
  -display gtk,zoom-to-fit=on \
  -vnc 127.0.0.1:0 \
  -qmp unix:/tmp/qmp-desktop.sock,server,nowait \
  -serial mon:stdio
