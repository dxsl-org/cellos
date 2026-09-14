# Phase 03 — The `console_drv::poll` kernel fault: mechanism and the one measurement left

**Status**: reproduced deterministically, mechanism identified, proof step prepared (not applied)
**Ceiling**: qemu (riscv64) — local repro, no board needed

## The fault, deterministically

`cargo test --test redoxfs-srv srv_basic` after `scripts/build-test-hooks-ci.sh` and
`scripts/build-srv-test-ci.sh` reproduces the CI failure byte for byte, including the faulting
address:

```
USER: Cellos > posix-shim-test
USER: [posix-shim] POSIX-FSTAT-OPEN: OK
[KERNEL PANIC] Critical failure.
Cellos: Kernel exception: scause=13 sepc=0x8023d708 stval=0x10000005 sstatus=0x8000000200006100
```

- `scause=13` — load page fault. `sstatus.SPP = 1` — the fault came from **S-mode** (kernel code).
- `sepc=0x8023d708` — inside `console_drv::viConsole::poll` (symbolized from the kernel that produced
  it, +0x210 into the function).
- The instruction there is `lbu a0, 5(a1)` with `a1 = 0x10000000`: a load of the **8250 UART's
  line-status register** (`0x10000000 + 5`) on the riscv-virt machine.

So the kernel executed an MMIO read while the live address space did not map the UART.

## The mechanism this points at

`hal/arch/riscv/src/rv64/domain.rs::activate_address_space` installs a **private native-domain root**
(`csrw satp` + `sfence.vma`), and the module documents that the scheduler is its only caller. Both
`asm/switch.S` and the trap entry matter here:

- `switch.S` writes `satp` only when the incoming context carries a non-zero root PPN, and its own
  comment says a zero PPN "preserves the SAS/same-domain path" — so the address space is whatever the
  previous context left behind unless someone explicitly switches.
- `asm/trap.S` (`__trap_entry`) saves the frame and calls into Rust with the **interrupted context's
  `satp` still live**. There is no restore-to-kernel-root step on entry.

A timer-driven path that touches the console (`viTimerTick` → executor → `console_drv::poll`) therefore
runs in the *cell's* address space whenever the interrupted context was a private native domain — and
a domain root, which exists to contain a cell, does not map the UART. That is exactly the fault above,
and it is also why the symptom only appears for cells admitted as domains.

## The one measurement that closes it

Print the live `satp` in the kernel-fault panic path. `hal/arch/riscv/src/rv64/trap.rs` already reads
it for the test-hooks snapshot; the patch used to reproduce this phase's run was:

```rust
let live_satp: usize;
unsafe {
    core::arch::asm!("csrr {satp}, satp", satp = out(reg) live_satp, options(nostack, nomem));
}
panic!("Cellos: Kernel exception: scause={} sepc={:#x} stval={:#x} sstatus={:#x} satp={:#x}",
       code, frame.sepc, frame.stval, frame.sstatus, live_satp);
```

If `satp` is non-zero and not the kernel root (`(8 << 60) | (kernel_root >> 12)`), the mechanism is
proven and the fix is in the trap entry: restore the kernel's root after the frame is saved, and
return to the interrupted root on the way out (or re-activate it in the scheduler's resume path).
Mapping the UART into every domain root is *not* an acceptable shortcut — cells run in S-mode under
that same root, so it would hand them the console's MMIO.

## Caveat for whoever runs the repro

Repeated local runs of this suite can report `ok` in ~5 s without booting: the test copies a 543 MB
disk image and its serial log handling appears to tolerate a stale log, so a run that races a
previous one may "pass" without having executed anything. Run it once, sequentially, and confirm the
serial transcript in the failure dump before believing a green result. That is worth its own look —
a suite that silently passes locally is worse than one that fails.
