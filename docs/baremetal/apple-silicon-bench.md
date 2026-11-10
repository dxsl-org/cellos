# Apple Silicon Bring-Up Bench & Fact Capture Guide

**Applies to:** Mac mini M1 (`t8103, J274`, 2020) development bench (Phase 01, ADR-0032 §5–6).

---

## 1. Bench Hardware Topology

The development loop uses **m1n1 proxy mode** over a single USB-C cable, eliminating the need for a physical COM1 serial cable or monitor/keyboard on the target Mac.

```
┌─────────────────────────────────┐                 ┌─────────────────────────────────┐
│        Development Host         │                 │       Target: Mac mini M1       │
│      (Linux or WSL2/Win11)      │                 │         (t8103, J274)           │
│                                 │                 │                                 │
│  tools/apple-host/              │  USB-C to USB-C │  Type-C Port 1 (DFU / m1n1):    │
│  run-m1n1-proxy.sh              │◄───────────────►│  - USB CDC-ACM (Proxy Control)  │
│                                 │   (vUART/Proxy) │  - USB CDC-ACM (Virtual Serial) │
│  Terminal Output:               │                 │                                 │
│  [Cellos > Interactive Shell]   │                 │  Internal ANS2 NVMe Storage     │
└─────────────────────────────────┘                 └─────────────────────────────────┘
```

---

## 2. Dev Host Environment Setup

### A. Native Linux Host
When the Mac mini M1 enters m1n1 proxy mode, two USB CDC-ACM endpoints appear:
- `/dev/ttyACM0`: m1n1 proxy control protocol.
- `/dev/ttyACM1`: Virtual UART (vUART) serial console.

Grant user permission:
```bash
sudo usermod -aG dialout "$USER"
```

### B. Windows 11 + WSL2 Host
On Windows, use `usbipd-win` (open source) to pass the Mac mini's USB endpoints into WSL2:
```powershell
# 1. On Windows PowerShell (Administrator):
usbipd list
# Look for device: 1209:316d (m1n1 USB CDC-ACM gadget)
usbipd bind --busid <BUSID>
usbipd attach --wsl --busid <BUSID>
```
Inside WSL2, verify with `ls -l /dev/ttyACM*`.

---

## 3. Preparing m1n1 Proxy Client Tooling

In accordance with [ADR-0032 §7](docs/decisions/0032-apple-silicon-secondary-lane.md) and [ADR-0030](docs/decisions/0030-license-model-and-copyleft-boundary.md), external tools are run unmodified outside the Cellos build tree:

```bash
# Clone and build m1n1 (pinned to permissive MIT tag v1.9.9); the build yields
# the stage-2 build/m1n1.macho the hypervisor runner chainloads.
git clone --recursive --depth 1 --branch v1.9.9 https://github.com/AsahiLinux/m1n1.git /tmp/m1n1-tools
make -C /tmp/m1n1-tools
pip3 install --user construct pyserial
```

---

## 4. Hardware Fact Capture Procedure

When first establishing the bench with a physical Mac mini M1, run the automated capture script to freeze hardware facts into `.agents/261104-0711-apple-silicon-group/evidence/`:

```bash
bash tools/apple-host/dump-m1-facts.sh
```

This captures:
1. `devicetree.dtb`: The device tree the running Linux received (from `/sys/firmware/devicetree/base`).
2. `pci-ids.txt`: Output of `lspci -nn` (Broadcom Ethernet BCM57762, PCIe root ports).
3. `cpuinfo.txt`: `/proc/cpuinfo`.
4. `evidence-sha256.txt`: SHA-256 hashes of all raw artifacts.

---

## 5. Loading & Running Cellos via m1n1 Hypervisor

Build the Cellos Apple Silicon kernel image:
```bash
cargo build --release -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-apple-t8103
bash scripts/build-aarch64-image.sh target/aarch64-unknown-none-softfloat/release/cellos-kernel build/cellos-apple-t8103.Image
```

Push and run under the m1n1 hypervisor (`M1N1_DTB` is the Linux `apple/t8103-j274.dtb`;
`M1N1_DIR` defaults to `/tmp/m1n1-tools`, `M1N1_TTY` to `/dev/ttyACM0`):
```bash
M1N1_DTB=path/to/t8103-j274.dtb bash tools/apple-host/run-m1n1-proxy.sh build/cellos-apple-t8103.Image
```

The hypervisor intercepts MMIO accesses, logs instruction faults cleanly to the host, and forwards console output to stdio.
