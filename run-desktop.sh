#!/usr/bin/env bash
# Run CellOS Desktop & Interactive Shell in QEMU with KVM
# Keystrokes typed in this terminal go directly to Cellos > via serial.
# The QEMU window displays the graphical screen (Compositor + Ocel / Chrome).

set -euo pipefail
REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

ISO="build/cellos-x86_64-desktop.qcow2"

if [[ ! -f "$ISO" ]]; then
    echo "ERROR: image $ISO not found." >&2
    exit 1
fi

echo "=========================================================================="
echo "  Starting CellOS Interactive Desktop (Intel VT-x KVM)"
echo "=========================================================================="
echo "  [Terminal] Gõ lệnh trực tiếp tại dấu nhắc 'Cellos > ' trên terminal này."
echo "  [Window]   Cửa sổ đồ họa GTK hiển thị trực tiếp (Desktop, Taskbar, Ocel, Chrome, Files, Terminal)."
echo "  [VNC]      VNC server sẵn sàng tại localhost:5900 (127.0.0.1:0)."
echo "  [Thoát]    Nhấn Ctrl+A rồi nhấn X để thoát QEMU bất cứ lúc nào."
echo ""
# Khởi động dịch vụ thời gian xác thực độc lập cho HTTPS nếu chưa chạy
if ! pgrep -f "qemu-dev-time.py" > /dev/null 2>&1; then
    echo "  [Time]     Khởi động qemu-dev-time.py (dịch vụ Roughtime xác thực cho HTTPS)..."
    python3 scripts/qemu-dev-time.py &
    TIME_PID=$!
    trap 'kill $TIME_PID 2>/dev/null || true' EXIT INT TERM
fi

exec qemu-system-x86_64 \
  -enable-kvm \
  -cpu host \
  -machine q35,vmport=on \
  -m 2048M \
  -drive file="$ISO",format=qcow2,if=virtio \
  -netdev user,id=net0 -device e1000,netdev=net0 \
  -device virtio-keyboard-pci \
  -device virtio-mouse-pci \
  -display gtk \
  -vnc 127.0.0.1:0 \
  -serial mon:stdio
