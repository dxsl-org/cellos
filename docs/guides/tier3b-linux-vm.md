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
# Fetch the pinned Alpine artifacts if .alpine-cache is empty; omit
# --skip-fetch on that first build.
bash scripts/make-hypervisor-fs-rpi3.sh --skip-fetch --volatile-disk
RPI3_GATE=machinery BOOT_WINDOW=90 bash scripts/qemu-rpi3-tier3.sh \
  target/rpi3-hv-embedded/kernel8.img

# Strict gate: only PASS when the Alpine guest reaches its own shell.
RPI3_GATE=boot BOOT_WINDOW=900 bash scripts/qemu-rpi3-tier3.sh \
  target/rpi3-hv-embedded/kernel8.img
```

Use the **raw** `kernel8.img`, not the linked ELF: QEMU `raspi3b` loads
these at different exception levels. The mini UART is serial1, so the runner
uses `-serial null -serial stdio`. The volatile profile has no persistent SD
disk; guest writes disappear on restart. The default builder packages an
ext4 `guest_disk.img` on the first FAT partition of `disk_rpi3_hv.img`, but
that SD-backed profile must pass its own QEMU boot/persistence gate before
it can be called qualified. See
[the Pi board instructions](../baremetal/load-cellos.md#8-tier-3-direct-firmware-boot).

**Observed here:** QEMU `raspi3b` passed the machinery gate: host
initialization, monitor smoke, VM creation, guest kernel/initrd streaming and
vCPU entry. Alpine Linux 6.12.13 printed its PSCI, CPU and early memory
initialization. The strict guest-shell gate did **not** pass in a 900-second
TCG run (last output: `software IO TLB`); do not equate Linux boot messages
or `[hv] vCPU ready` with a usable guest shell. No physical-board Tier 3 VM
execution has been observed yet.

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

The guest's virtio-blk device backing is currently a 16 MiB **volatile, zero-filled** in-memory buffer — writes are accepted (BLK_T_OUT works) but are lost on cell restart, and there is no bootable filesystem image loaded onto it today (Alpine itself boots from initramfs, not this device). Persistent, image-backed storage is planned (see `.agents/260712-0952-tier3b-vm-hardening-compat/phase-04-writable-storage.md`). Until then:

1. **Create an overlay** (writable tmpfs on top) — survives only for the VM's lifetime
2. **Write to /tmp** (ramdisk, shared with Cellos)
3. Persistent image-backed disk — **planned**, not yet shipped

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
