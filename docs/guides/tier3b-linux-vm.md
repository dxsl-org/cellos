# Tier 3 `linux-guest` Profile — Full Kernel Guest

> Legacy name: Tier 3b. Run unmodified Linux binaries in a hypervisor-isolated
> VM. Use this for legacy code, fork-heavy apps, or untrusted workloads while
> Tier 2 native domains are not implemented.

---

## Overview

Tier 3 lets you run a full Linux kernel (e.g., Alpine, Busybox) inside a lightweight hypervisor. From the app's perspective, it's a normal Linux environment:

- Standard libc — **musl today** (Alpine guest, shipped). glibc (Debian) guest is **planned** (see roadmap: broadens binary compatibility for glibc-only software).
- Full POSIX (fork, mmap, signals, pthreads)
- Package manager — `apk` on the Alpine guest; `apt` applies to the **planned** Debian glibc guest, not the current Alpine one.
- Any unmodified Linux binary built for the guest's libc (musl-built today).

**Trade-off** *(estimates — not yet benchmarked; see Performance Characteristics)*: ~10–15% performance overhead vs Tier 1; ~2–10 second boot time.

---

## Platform Support

| Platform | Status | Hypervisor | Notes |
|----------|--------|-----------|-------|
| **ARM64** | ✅ Working (G2) | EL2 (non-VHE) | Cortex-A72+; boots Alpine (musl) to a shell; virtio-blk/net/console. Under QEMU-TCG only the machinery half is asserted (`[hv] vCPU ready`); the strict `/ #` boot needs KVM/real hardware |
| **x86_64** | ✅ Working in QEMU (G2) | SVM (AMD, TCG-testable) | Boots Alpine to a shell under QEMU-TCG 10.2.0 (`scripts/qemu-hypervisor-smoke-x86.sh boot`); the primary application gate `scripts/qemu-x86-python-gate.sh` installs CPython in a 256 MiB Alpine guest and processes JSON into CSV in separate Linux processes. Nginx remains a secondary regression in the same CI job. Intel VT-x guest execution is not implemented |
| **RISC-V** | ❌ Not implemented | H-ext (too new) | Deferred beyond G1 |

**G2-only**: requires real hardware or advanced QEMU (not basic RISC-V). Both the ARM64 and x86_64 paths boot a Linux guest; RISC-V has no hypervisor.

### Raspberry Pi 3 (Cortex-A53, QEMU-first)

`board-rpi3` now retains EL2 as a narrow monitor and runs the Cellos host
at EL1, Cells at EL0 and one 128 MiB ARM64 Linux guest at EL1 with Stage-2
isolation. BCM2836 has no GICH/GICV: the hypervisor cell models GICD/GICC in
software, delivers guest IRQs via `HCR_EL2.VI`, and the physical host timer
preempts a non-yielding guest. The boot-time capability gate runs a real
HVC/Stage-2 MMIO/virtual-IRQ/preemption smoke before admitting any VM.

```bash
# One front end for images: it resolves the board, checks the option combination
# and prints the gate. `--list` shows the boards and their options.
bash scripts/build-image.sh --list

# Default: prompt-first. Cellos boots to its shell and the guest is started on
# demand with `hv`; no VM exists until then.
bash scripts/build-image.sh --board raspberry-pi/3-model-b --skip-fetch --volatile-disk
RPI3_GATE=host BOOT_WINDOW=180 bash scripts/qemu-rpi3-tier3.sh \
  target/rpi3-hv-embedded/kernel8.img

# Server flavour: preload the VM at boot so the first Tier-3 app starts fast.
# The machinery/boot gates below need this build.
bash scripts/build-image.sh --board raspberry-pi/3-model-b --skip-fetch --volatile-disk --autostart
RPI3_GATE=machinery BOOT_WINDOW=90 bash scripts/qemu-rpi3-tier3.sh \
  target/rpi3-hv-embedded/kernel8.img

# Strict gate: only PASS when the Alpine guest reaches its own shell.
RPI3_GATE=boot BOOT_WINDOW=2400 bash scripts/qemu-rpi3-tier3.sh \
  target/rpi3-hv-embedded/kernel8.img

# Tier 3 off (a machine that must be safer than it is capable): the guest cell
# and the guest files are not packaged, so no VM can be started at all.
bash scripts/build-image.sh --board raspberry-pi/3-model-b --skip-fetch --volatile-disk --no-tier3
```

Use the **raw** `kernel8.img`, not the linked ELF: QEMU `raspi3b` loads
these at different exception levels. The mini UART is serial1, so the runner
uses `-serial null -serial stdio`. The volatile profile has no persistent SD
disk; guest writes disappear on restart. The default builder packages an
ext4 `guest_disk.img` on the first FAT partition of `disk_rpi3_hv.img`, adds
the pinned Alpine ext4 modules to the guest initramfs, and mounts `/dev/vda`
at `/mnt/disk` before launching the guest shell. Its strict SD-backed boot
gate requires that mount marker as well as the shell prompt. See
[the Pi board instructions](../baremetal/load-cellos.md#8-tier-3-direct-firmware-boot).

**Observed here:** QEMU `raspi3b` passed the monitor/machinery gate, and
Alpine Linux 6.12.13 reached its own `~ #` prompt. The volatile-profile
guest executed `echo PI_TIER3_EXEC_OK` through the emulated PL011 UART.
The SD-backed guest mounted `/dev/vda` as ext4 at `/mnt/disk`, wrote
`PI_DATA_123456` to `/mnt/disk/pi-proof`, synced, and read the same data
after a full QEMU restart with the same SD image. Host inspection of the
backing ext4 image also returned the marker. The earlier block-probe
stall came from using DTB SPI 17 as GIC INTID 17 rather than 49; the
direct `/bin/sh` initramfs had also skipped loading virtio/ext4 drivers.
QEMU TCG emitted a guest soft-lockup warning during early boot; the
shell and persistence checks subsequently succeeded. On the physical
board the coherent-EL2 build ran on 2026-10-01: the two EL2 `first-run`
lines agree field for field with what EL1 holds
(`entry=0x40000000 insn=0xd2800540 s2=[0x406e003,0x406f003,0x406b7ff]`,
`exit=[0x5a000000,0x40000008,irq=0]` on both sides), the smoke reports
`HVC/MMIO/VI/PREEMPT smoke PASS; HypervisorCap open`, `[hv] vCPU ready —
entering run loop` follows and Linux 6.12.13 starts on the physical
Cortex-A53 with the PL011 early console. The captured board trace ended
there, so that run recorded no hardware `~ #`, and it ended
with no keyboard: the trace shows the low-speed devices on hub ports
3/5 exhausting their complete-splits inside the start-split's own
microframe. The paced-split build (the one now served as `cellos.uimg`)
fixed that on the board: the halves are one microframe apart, the
low-speed device on hub port 3 enumerates as a HID boot keyboard
(`10c4:0005`, class 3 / subclass 1 / protocol 2) and the driver reports
`driving 1 HID interface(s)`, while the device on port 5 answers every
complete-split with NAK. What is still unproven is a **keystroke
reaching the guest** — no key was pressed on the board. See
[the Pi board instructions](../baremetal/load-cellos.md#8-tier-3-direct-firmware-boot)
for the exact lines a passing board trace must show.

**Board observation (2026-10-06, volatile profile over TFTP):** the physical
board reached the guest's own `~ #` prompt. The UART trace shows
`[hv] volatile disk selected by build policy — no persistent guest disk`,
`[hv] vCPU ready — entering run loop`, Linux 6.12.13 starting, and then the
guest's `ifconfig eth0 192.168.42.50 netmask 255.255.255.0 up` and
`ping -c 3 192.168.42.1` output at `~ #`, typed through the UART because the
USB keyboard still delivers no keystrokes to the guest. Guest networking is
**not** working: the first virtio-net receive poll reports
`[hv-net-rx] first L2Recv tid=5 result=send deadline`, the net cell reports
`[net-bridge] TX not accepted: no reply from tid 4 within the timeout`, and the
guest's ping loses every packet. Both messages name host cells, not guest
resources — this board runs Cellos single-hart (`BCM2836 has no SGI path in
this kernel`), the hypervisor models GICD/GICC in software, and the Ethernet
controller and the keyboard share one USB 2.0 hub behind the single-loop
`dwc2-usb` cell.

**Practical ceiling on this board:** memory is not the limit (~928 MiB usable
against a 128 MiB default guest); CPU time is. Every cell and the guest vCPU
share one 1.2 GHz Cortex-A53, so treat the Pi as the EL2 correctness and
evidence lane for the narrow musl Alpine guest. Wide guests, fast-clone/CoW and
snapshot performance work belong on a multi-core lane.

---

## Architecture

```
┌─────────────────────────────────┐
│ Cellos Kernel (S-mode / VMX host)
│                                 │
│  ┌──────────────────────────┐   │
│  │ Hypervisor (custom, ~2.9K LOC today; ~9K planned)
│  │                          │   │  Trap device MMIO
│  │  ┌────────────────────┐  │   │  Emulate PL011, clint, etc.
│  │  │ Linux Guest (HS-mode / VM) │
│  │  │  /bin/app          │  │   │
│  │  │  fork() / mmap()   │  │   │
│  │  └────────────────────┘  │   │
│  └──────────────────────────┘   │
│                                 │
│  VirtIO devices:                │
│    disk  → Cellos VFS           │
│    net   → Cellos Net           │
│    console → kernel log         │
└─────────────────────────────────┘
```

---

## Running a Linux VM

### Create a VM

```bash
# Start shell and ask for a Linux VM
vm_id = sys_create_vm(4, 0x4000000)
    # args: mode (4=ARM64 HS-mode), mem (64 MiB)
    # → vm_id (u64)

# Load Linux kernel ELF
sys_vm_load_elf(vm_id, kernel_elf_data)

# Boot it
sys_vm_run(vm_id)
    # Blocks until VM exits or you call sys_vm_exit()
```

### From Shell

The shell has built-in hypervisor commands (planned):

```bash
vm create --arch arm64 --mem 64M --kernel /vmlinuz
vm run <vm_id>
vm exit <vm_id>
```

---

## Guest Filesystem Access

The Alpine guest boots from initramfs. Its separate virtio-blk device is
backed by either a volatile in-memory image or, with the SD-backed Pi profile,
the ext4 file `/mnt/sd/guest_disk.img` opened by the hypervisor Cell through
VFS. The persistent Pi guest's initramfs loads the pinned virtio/ext4 modules
and mounts that device at `/mnt/disk` before launching `/bin/sh`. The volatile
image is discarded at VM restart. QEMU read-back after reboot proves the
SD-backed path there; physical-board durability is not yet established.

---

## Guest Network Access

The hypervisor exposes a VirtIO net device. Guest sees a standard Linux NIC:

```bash
# Inside guest
ip addr show
eth0: inet 10.0.2.15

# Connect to host services (Cellos net cell runs at 10.0.2.2)
curl -v http://10.0.2.2:8080/

# Or use sockets normally
```

Network traffic is routed through Cellos's kernel; no direct hardware access.

> **Boot-time caveat (measured):** the guest kernel seeds its CRNG only after boot and `/dev/random`
> blocks until it does. A TLS client started immediately after the network comes up can sit inside its
> first handshake long enough — in host time — for the peer to close the connection: the capture shows
> the TCP handshake completing, then no ClientHello for ~60 s, then the peer's FIN, which the guest
> reports as `SSL routines::unexpected eof while reading`. The retry succeeds because the pool is then
> ready. Read from `/dev/random` once before the first TLS connection;
> `scripts/qemu-x86-nginx-gate.sh` does exactly that (marker `NGINX_IN_VM_CRNG_READY`).

---

## VirtIO Devices (What's Emulated)

| Device | Status | Notes |
|--------|--------|-------|
| Block (disk) | ✅ | In-memory buffer (volatile profile) or a persistent image the VFS serves at `/mnt/sd/guest_disk.img`; the `x86 VirtIO e2e + persistence` lane proves two-boot durability |
| Network | ✅ | Full NIC; routed via Cellos net cell |
| Console | ✅ | Serial output to kernel log |
| Entropy (RNG) | 🚧 Planned | No virtio-rng device model exists yet (`cells/services/hypervisor/src/`); planned in the glibc-guest track since glibc TLS blocks on entropy |
| Clock (virtual timer) | ✅ | armv8 CNTV register (not MMIO); `CNTVOFF_EL2=0` keeps guest counter matching host, so `clock_gettime()` is accurate; no wall-clock RTC device yet |

---

## Example: Boot Alpine Linux

```bash
# One-time: fetch the pinned Alpine artifacts, build the cells, assemble the
# guest filesystem the hypervisor cell boots (/vmlinuz + /initrd.gz inside it)
bash scripts/make-hypervisor-fs.sh

# Build the ARM64 hypervisor kernel with that filesystem embedded
RUSTFLAGS="-C relocation-model=pic -C target-feature=+bti,+paca,+pacg" \
  EMBEDDED_OVERRIDE=kernel/src/embedded-hv \
  cargo build --release -p cellos-kernel --features qemu-virt-1g \
  --target aarch64-unknown-none-softfloat -Z build-std=core,alloc

# Boot it. The guest is already wired to the console, so Alpine's own shell is
# what you type into (Windows: .\run-hypervisor-arm.ps1):
HV_SMOKE_MODE=boot bash scripts/qemu-hypervisor-smoke.sh   # needs KVM/real ARM64
```

The x86 walkthrough is the same shape with `scripts/make-hypervisor-fs-x86.sh` /
`scripts/qemu-hypervisor-smoke-x86.sh` and `--target x86_64-unknown-none`.

Inside the VM, you have a full Linux shell:

```bash
# Install packages
apk update
apk add curl vim

# Run C++ code
apk add g++ make
g++ -o myapp main.cpp
./myapp

# Fork works!
for i in {1..10}; do (sleep 1 & echo "background job $i") done

# exit to return to Cellos shell
exit
```

---

## Performance Characteristics

> ⚠️ **The numbers below are design estimates, not measured.** A real benchmark pass (throughput, trap latency, boot time on QEMU/TCG with its caveats) is planned; treat these as targets until then.

| Operation | Tier 1 Rust | Tier 1 + SDK | Tier 3 `linux-guest` *(est.)* |
|-----------|-------------|--------------|--------------|
| Syscall latency | ~1 μs | ~2 μs (IPC) | ~10–20 μs (trap) |
| App startup | <1 ms | <1 ms | 2–5 s (kernel boot) |
| I/O throughput | Native | ~90% native | ~80% native (VirtIO) |
| Memory overhead | ~10 KiB | ~50 KiB | ~128 MiB guest RAM (Alpine); more for glibc guest |

**Use Tier 3 `linux-guest` when**: boot time and startup latency don't matter, but compatibility and ease-of-deployment do.

---

## Limits & Constraints

❌ **No nested VMs** — guest cannot create sub-VMs.
❌ **No direct hardware access** — I/O goes through Cellos drivers.
❌ **No DMA to host memory** — disk/network buffers are copied.
⚠️ **Slow boot** — ~2–10 seconds for full Linux init *(estimate)*.
✅ **Full fork() / pthreads** — anything Unix-like works.
⚠️ **Package managers** — `apk` works on the Alpine guest; persistence across VM reboots needs writable-backing storage (**planned**, not yet shipped). `apt` requires the planned Debian glibc guest.

---

## Hypervisor Internals (Advanced)

The hypervisor is a custom minimal VMM (~2.9K lines of Rust shipped today, ~9K planned at full device coverage), not a fork of Crosvm or KVM. It:

1. **Boots the guest** — loads ELF, sets up Stage-2 page tables, enters guest mode
2. **Emulates MMIO** — traps device accesses (PL011 UART, GICv2, timer, etc.)
3. **Mediates VirtIO** — disk/net virtqueue buffers are copied through **kernel-bounds-checked** guest-memory wrappers (NOT direct DMA to host); every guest-physical address is validated against the guest RAM window
4. **Isolates faults** — guest page faults, invalid instructions trapped; host continues

For details, see [system-architecture.md](../system-architecture.md) § Tier 3 Hypervisor.

---

## Building a Custom Alpine Rootfs

```bash
# ARM64: fetch Alpine netboot artifacts + assemble kernel/src/embedded-hv/kernel_fs.img
bash scripts/make-hypervisor-fs.sh            # add --skip-fetch to reuse .alpine-cache/

# x86_64: same, into kernel/src/embedded-hv-x86 (HV_EMBEDDED_DIR moves the staging dir)
bash scripts/make-hypervisor-fs-x86.sh

# The image carries the guest kernel/initramfs plus the boot cells; the kernel
# build embeds it through EMBEDDED_OVERRIDE (see the example above).
```

---

## Evidence Lanes (what runs, and where)

Each lane builds its own guest image, boots it under the pinned QEMU-TCG 10.2.0
(`scripts/install-qemu-x86-ci.sh`), and asserts markers printed by the guest or by the
hypervisor cell. Passing is emulator evidence — it does not qualify physical hardware.

| Lane | Command | Asserts | CI job |
|------|---------|---------|--------|
| ARM64 machinery | `HV_SMOKE_MODE=machinery bash scripts/qemu-hypervisor-smoke.sh` | `[hv] vCPU ready`; tolerates only the documented TCG address-size fault | `qemu-hypervisor-machinery` |
| ARM64 boot-to-shell | `HV_SMOKE_MODE=boot bash scripts/qemu-hypervisor-smoke.sh` | Alpine `/ #` prompt (switches to KVM — needs real hardware) | `qemu-hypervisor-boot-kvm` (gated on a self-hosted runner) |
| x86 boot-to-shell | `HV_SMOKE_MODE=boot bash scripts/qemu-hypervisor-smoke-x86.sh` | Alpine `/ #` prompt | `qemu-x86-hypervisor-boot` |
| x86 in-guest CPython (primary application) | `bash scripts/qemu-x86-python-gate.sh` | 256 MiB Alpine profile, `apk add python3` via HTTPS from Alpine v3.21, exact JSON→CSV aggregation and a separate Python consumer process | `qemu-x86-tier3-python` |
| x86 in-guest nginx (secondary regression) | `bash scripts/qemu-x86-nginx-gate.sh` | `apk add nginx`, forked master + worker and in-guest HTTP fetch | `qemu-x86-tier3-python` (second step) |
| x86 VirtIO e2e + persistence | `bash scripts/qemu-x86-virtio-e2e.sh` | block/network discovery, IRQ5/IRQ6 completion, and a two-boot persistent marker read back on the host | `qemu-x86-tier3-virtio-e2e` |
| x86 hostile corpus | `BUILD_HOSTILE_ISO=1 bash scripts/qemu-tier3-hostile-runner-x86.sh` | 27 bounded malformed-input scenarios plus a host-read post-reset recovery write | `qemu-x86-tier3-hostile` |

CPython is the first useful application gate because its batch-processing result
can be checked without a display bridge. A browser is a separate, later gate:
the existing virtio-gpu/input device models do not by themselves prove usable
guest scanout, keyboard/mouse interaction, or enough RAM for Chromium/Firefox.
The Python gate runs on the volatile Alpine profile; its CSV is deliberately
not a persistence claim. Use the two-boot VirtIO lane for persistent block I/O.

The x86 persistence and hostile runners stage their embedded filesystem and ISO
root in their own work directories; a fresh evidence build does not replace
`kernel/src/embedded-hv-x86/init`. Ten fresh two-boot persistence runs passed
before `seg_max` was enabled. With `VIRTIO_BLK_F_SEG_MAX` advertising a maximum
of two data segments, the two-boot lane now checks the marker and a flushed
16 KiB payload and the adjacent unwritten region across reboot. Its fresh-build
run and 15 additional fresh-disk two-boot runs passed with `seg_max=2`, as did
all 27 hostile scenarios under QEMU-TCG 10.2.0. The persistent backend
previously handled a whole chain once *per data descriptor*, overwriting the
first read with bytes from the next offset. It now handles each whole chain
once. Larger advertised segment counts and physical boards remain unqualified;
bounded emulator runs cannot rule out unrelated intermittent short reads,
flush failures or boot flakes.

The Python lane still depends on live HTTPS. One run with a shortened
`BOOT_WINDOW=1000` passed DNS but did not reach `PYTHON_IN_VM_APK_PASS` before
the outer deadline; four runs with a 1200-second window passed. The failure had no
packet trace, so its cause is undetermined. The guest now streams `apk` progress
to the serial log; set `QEMU_NET_CAPTURE=build/<workdir>/traffic.pcap` to record
QEMU's guest-network traffic when investigating another timeout. A successful
capture received about 20 MiB over TLS. Hosted CI remains unobserved.

---

## When to Use Tier 3 `linux-guest`

✅ Existing Linux C/C++ code (no rewrite)
✅ Apps that fork() heavily (e.g., nginx, Java)
✅ Package managers essential (`apk add` today; `apt install opencv` once the Debian glibc guest ships)
✅ Untrusted code (isolated in VM)
✅ Learning Linux internals without rewriting

❌ Performance-critical (use Tier 1 Rust)
❌ Real-time (VM jitter unacceptable)
❌ Embedded systems with 4 MiB RAM (VM needs 64+ MiB)
❌ RISC-V (not implemented yet)

---

## Canonical Example

See [cells/guests/silo-guest/](../../cells/guests/silo-guest/) — the Silo guest firmware is also a micro-VM example (much smaller, ~5 KiB).

For a full Alpine Linux VM, use the lanes above: `scripts/make-hypervisor-fs.sh` (ARM64) or
`scripts/make-hypervisor-fs-x86.sh` (x86_64) build the guest image, and the matching smoke lane boots it.

---

## Troubleshooting

**VM boot hangs?**
→ Check guest ELF load address matches hypervisor's page table setup. Kernel messages usually print; check serial output.

**Disk writes don't persist?**
→ The guest's VirtIO block device is backed by an image on the Cellos side: x86 opens `/mnt/sd/guest_disk.img`
and the ARM64 lane uses a fixed persistent image. If writes vanish, check that the backing file exists and
mounted — the `x86 VirtIO e2e + persistence` lane above is the worked example (write + flush on the first
boot, read back on the second).

**Network unreachable?**
→ Cellos net cell may not be running. Check `net-tools` in `/bin/`. Guest IP should be 10.0.2.15, Cellos host at 10.0.2.2.

**Slow network?**
→ VirtIO performance is ~90% native on QEMU. Real hardware faster. No tuning levers exposed yet.

---

## Next Steps

- See [system-architecture.md](../system-architecture.md) § Tier 3 for hypervisor design.
- For ARM64 EL2 MMU setup: `kernel/src/memory/stage2.rs` (Stage-2 builder) and `hal/arch/arm/src/aarch64/`.
- For x86: SVM-first world-switch (stage-2 NPT) in `kernel/src/hypervisor/svm_registry.rs` and
  `hal/arch/x86/src/x86_64/svm.rs`, with the device models in `cells/services/hypervisor/`. Intel VT-x
  root operation is not implemented and is deliberately not attempted under a hypervisor (VMXON faults).
- Build the guest image: `bash scripts/make-hypervisor-fs.sh` (ARM64) or `bash scripts/make-hypervisor-fs-x86.sh` (x86_64);
  `scripts/fetch-alpine-artifacts.sh` / `scripts/fetch-alpine-x86.sh` pull the pinned netboot artifacts.
