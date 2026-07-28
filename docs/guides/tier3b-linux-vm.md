# Tier 3b Linux VM — Full Kernel Guest

> Run unmodified Linux binaries in a hypervisor-isolated VM. For legacy code, fork-heavy apps, or untrusted workloads.

---

## Overview

Tier 3b lets you run a full Linux kernel (e.g., Alpine, Busybox) inside a lightweight hypervisor. From the app's perspective, it's a normal Linux environment:

- Standard libc — **musl today** (Alpine guest, shipped). glibc (Debian) guest is **planned** (see roadmap: broadens binary compatibility for glibc-only software).
- Full POSIX (fork, mmap, signals, pthreads)
- Package manager — `apk` on the Alpine guest; `apt` applies to the **planned** Debian glibc guest, not the current Alpine one.
- Any unmodified Linux binary built for the guest's libc (musl-built today).

**Trade-off** *(estimates — not yet benchmarked; see Performance Characteristics)*: ~10–15% performance overhead vs Tier 1; ~2–10 second boot time.

---

## Platform Support

| Platform | Status | Hypervisor | Notes |
|----------|--------|-----------|-------|
| **ARM64** | ✅ Working (G2) | EL2 (non-VHE) | Cortex-A72+; boots Alpine (musl) to a shell; virtio-blk/net/console |
| **x86_64** | 🚧 **Planned — not implemented** | SVM (AMD, TCG-testable) then VT-x (Intel) | Design plan only (`.agents/260711-1917-tier3b-x86-vtx/`); **no code yet** |
| **RISC-V** | ❌ Not implemented | H-ext (too new) | Deferred beyond G1 |

**G2-only**: requires real hardware or advanced QEMU (not basic RISC-V). Only the ARM64 path currently boots a guest.

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

---

## VirtIO Devices (What's Emulated)

| Device | Status | Notes |
|--------|--------|-------|
| Block (disk) | ✅ (volatile) | 16 MiB in-memory Vec; writes work but are not persisted; no image loaded |
| Network | ✅ | Full NIC; routed via Cellos net cell |
| Console | ✅ | Serial output to kernel log |
| Entropy (RNG) | 🚧 Planned | No virtio-rng device model exists yet (`cells/services/hypervisor/src/`); planned in the glibc-guest track since glibc TLS blocks on entropy |
| Clock (virtual timer) | ✅ | armv8 CNTV register (not MMIO); `CNTVOFF_EL2=0` keeps guest counter matching host, so `clock_gettime()` is accurate; no wall-clock RTC device yet |

---

## Example: Boot Alpine Linux

```bash
# Prerequisites
scripts/build-kernel-alpine.sh  # one-time, downloads/builds Alpine rootfs

# Start Cellos
./run-arm64.ps1

# From shell
vm create --arch arm64 --mem 64M --rootfs /alpine.squashfs
vm run 1
    # Alpine login prompt appears
login: root
```

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

| Operation | Tier 1 Rust | Tier 1 + SDK | Tier 3b Linux *(est.)* |
|-----------|-------------|--------------|--------------|
| Syscall latency | ~1 μs | ~2 μs (IPC) | ~10–20 μs (trap) |
| App startup | <1 ms | <1 ms | 2–5 s (kernel boot) |
| I/O throughput | Native | ~90% native | ~80% native (VirtIO) |
| Memory overhead | ~10 KiB | ~50 KiB | ~128 MiB guest RAM (Alpine); more for glibc guest |

**Use Tier 3b when**: boot time and startup latency don't matter, but compatibility and ease-of-deployment do.

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
cd scripts
./build-kernel-alpine.sh  # ~30 min, downloads+cross-compiles

# Output: alpine.img (FAT32 with /bin, /etc, /lib, /usr)
# Loaded as VirtIO block device by hypervisor
```

---

## When to Use Tier 3b

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

For a full Alpine Linux VM, see kernel build logs (`scripts/build-kernel-alpine.sh` output).

---

## Troubleshooting

**VM boot hangs?**  
→ Check guest ELF load address matches hypervisor's page table setup. Kernel messages usually print; check serial output.

**Disk writes don't persist?**  
→ Rootfs is read-only FAT32 (mounted via VirtIO). Write to `/tmp` (tmpfs) or request writable partition.

**Network unreachable?**  
→ Cellos net cell may not be running. Check `net-tools` in `/bin/`. Guest IP should be 10.0.2.15, Cellos host at 10.0.2.2.

**Slow network?**  
→ VirtIO performance is ~90% native on QEMU. Real hardware faster. No tuning levers exposed yet.

---

## Next Steps

- See [system-architecture.md](../system-architecture.md) § Tier 3 for hypervisor design.
- For ARM64 EL2 MMU setup: see kernel/arch/arm64/ (Stage-2 paging).
- For x86 VMX: see kernel/arch/x86_64/ (EPT).
- Build Alpine: `scripts/build-kernel-alpine.sh`.
