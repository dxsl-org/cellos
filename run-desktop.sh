#!/usr/bin/env bash
# Run CellOS Desktop & Interactive Shell in QEMU
# Keystrokes typed in this terminal go directly to Cellos > via serial.
# The QEMU window displays the graphical screen (Compositor + Ocel / Chrome).

set -euo pipefail
REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

ISO="build/cellos-x86_64-desktop.qcow2"

if [[ ! -f "$ISO" ]]; then
    echo "ERROR: image $ISO not found." >&2
    echo "Vui lòng build image trước: python3 scripts/package-desktop-x86.py && cargo build --release -p cellos-kernel --target x86_64-unknown-none --features \"policy-required,large-cell-va,qemu-dev-https\"" >&2
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

exec qemu-system-x86_64 \
  "${ACCEL_OPTS[@]}" \
  -machine q35,vmport=on \
  -m 2048M \
  -smp 2 \
  -drive file="$ISO",format=qcow2,if=virtio \
  -netdev user,id=net0,hostfwd=tcp::2222-:22 \
  -device e1000,netdev=net0 \
  -device virtio-keyboard-pci \
  -device virtio-tablet-pci \
  -device virtio-mouse-pci \
  -display gtk,zoom-to-fit=on \
  -vnc 127.0.0.1:0 \
  -qmp unix:/tmp/qmp-desktop.sock,server,nowait \
  -serial mon:stdio
