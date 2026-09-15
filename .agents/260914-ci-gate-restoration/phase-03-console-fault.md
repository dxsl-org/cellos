# Phase 03 — The `console_drv::poll` kernel fault: mechanism proven by measurement

**Status**: measurement applied and run — the fault reproduces with a private cell root live;
the fix is designed in [phase-04-trap-root-discipline.md](phase-04-trap-root-discipline.md)
**Ceiling**: qemu (riscv64) — local repro, no board needed

## The fault

`cargo test --test srv-cellosfs riscv64_cellosfs_srv_basic` after `scripts/build-srv-test-ci.sh`
reproduces the CI failure. Before this phase the panic named only the registers:

```
USER: Cellos > posix-shim-test
[KERNEL PANIC] Critical failure.
Cellos: Kernel exception: scause=13 sepc=0x8023d708 stval=0x10000005 sstatus=0x8000000200006100
```

- `scause=13` — load page fault. `sstatus.SPP = 1` — the fault came from **S-mode** (kernel code).
- `sepc` — inside `console_drv::viConsole::poll` (+0x210), at `lbu a0, 5(a1)` with
  `a1 = 0x10000000`: a load of the **8250 UART's line-status register** on the riscv-virt machine.
- So the kernel executed an MMIO read while the live address space did not map the UART.

## The mechanism this points at

`hal/arch/riscv/src/rv64/domain.rs::activate_address_space` installs a **private native-domain root**
(`csrw satp` + `sfence.vma`), and the module documents that the scheduler is its only caller. Both
`asm/switch.S` and the trap entry matter here:

- `switch.S` writes `satp` only when the incoming context carries a non-zero root PPN, and its own
  comment says a zero PPN "preserves the SAS/same-domain path" — so the address space is whatever the
  previous context left behind unless someone explicitly switches.
- `asm/trap.S` (`__trap_entry`) saves the frame and calls into Rust with the **interrupted context's
  `satp` still live**. There is no restore-to-kernel-root step on entry, and `__trap_exit` never
  restores one either.
- A timer-driven path that touches the console runs in the *cell's* address space whenever the
  interrupted context was a private native domain. `vi_timer_tick` polls the console directly
  (`kernel/src/task.rs:832` → `console_drv::CONSOLE.lock().poll()`), before the scheduler switch that
  would have installed another root. A domain root exists to contain a cell, so it does not map the
  UART: exactly the fault above.

## The measurement (applied, and what it printed)

The panic path now names both roots, and the kernel records its own root when it activates its page
table:

- `hal/arch/riscv/src/rv64/paging.rs::PageTable::activate` calls
  `domain::record_kernel_satp(satp_val)` — the only caller of `activate` is
  `kernel/src/memory/paging.rs` for the kernel root, and the cell path
  (`domain::activate_address_space`) deliberately does not touch it.
- `hal/arch/riscv/src/rv64/domain.rs` keeps that value in `KERNEL_SATP` and exposes it as
  `kernel_satp()`.
- `hal/arch/riscv/src/rv64/trap.rs` reads the live `satp` (`csrr satp`) before the kernel-fault panic
  and prints it beside `kernel_satp()`.

Reproduced by driving the kernel directly — `qemu-system-riscv64 -machine virt -m 256M -nographic
-bios default -kernel target/riscv64gc-unknown-none-elf/release/cellos-kernel-srv-test -drive
file=<copy of build/disk_srv.img>,format=raw,if=none,id=hd0 -device virtio-blk-device,drive=hd0
-monitor none`, typing `posix-shim-test` into the serial stream after `[srv-test] ALL TESTS PASSED`,
and keeping the transcript. First attempt:

```
USER: Cellos > posix-shim-test

[KERNEL PANIC] Critical failure.
panicked at hal/arch/riscv/src/rv64/trap.rs:193:21:
Cellos: Kernel exception: scause=13 sepc=0x80242cd2 stval=0x10000005 sstatus=0x8000000200006100 satp=0x80001000000827c0 kernel_satp=0x800000000008076f
```

| value | decode |
|---|---|
| `satp=0x80001000000827c0` | MODE=8 (Sv39), **ASID=1**, root PA `0x827c0000` — a private cell root |
| `kernel_satp=0x800000000008076f` | MODE=8, **ASID=0**, root PA `0x8076f000` — the kernel's own root |
| `sepc=0x80242cd2` | `console_drv::viConsole::poll` + `0x210` (the offset phase 02 reported) |
| `stval=0x10000005` | the 8250 line-status register |

`riscv64-unknown-elf-addr2line -f -C -e <that kernel> 0x80242cd2` and `nm` put `poll` at
`0x80242ac2`, so the faulting instruction is the same `+0x210` MMIO load as phase 02, executed while
`satp` named a **cell** root (ASID 1, not the kernel's ASID 0). The mechanism is proof, not inference:
the kernel touched the UART under a root that contains the cell.

The window is "any time a private domain is running", not a specific syscall. In this capture the
panic came before any `[posix-shim]` marker — a freshly admitted domain was enough — while the hosted
capture below fires between `POSIX-MKDIR-RMDIR` and `POSIX-RENAME`. What both share is the only thing
that matters: a Cell's private root was live when the timer ISR ran kernel code that touches MMIO.

## Hosted confirmation (the gate itself)

The next push reproduced the fault on the hosted runner with the same values, so this is not a local
artifact of TCG or of this workstation — it is the `CellosFS /srv Integration Test` job's own
failure. Job `104247291496` of run `34927079141` (revision `e6d992d53`), step "Run CellosFS /srv
integration tests":

```
USER: [srv-test] ALL TESTS PASSED
USER: [posix-shim] POSIX-FSTAT-OPEN: OK
...
USER: [posix-shim] POSIX-MKDIR-RMDIR: OK
[KERNEL PANIC] Critical failure.
panicked at hal/arch/riscv/src/rv64/trap.rs:193:21:
Cellos: Kernel exception: scause=13 sepc=0x80242cd2 stval=0x10000005 sstatus=0x8000000200006100 satp=0x80001000000827c0 kernel_satp=0x800000000008076f
[KERNEL PANIC] halting...
thread 'riscv64_cellosfs_srv_basic' (7198) panicked at tests/srv-cellosfs.rs:125:13:
timeout: pattern "[posix-shim] POSIX-RENAME: OK" not seen in 60s
```

`sepc`, `stval`, `satp`, and `kernel_satp` are identical to the local capture, and the fault again
lands between `POSIX-MKDIR-RMDIR` and `POSIX-RENAME` — the mkdir/rmdir window phase 02 described. The
job is red because the kernel halts, and it stays red until phase 04 lands.

## Caveat for whoever runs the repro

The fault is **intermittent**, and the suite is a weak detector of it. Two facts, both measured on
the same kernel (the one above):

- A full boot is genuinely fast enough for a 5 s green: disk copy 0.15 s (page cache), boot to
  `[srv-test] ALL TESTS PASSED` 2.82 s, shell prompt 4.64 s, `[posix-shim] POSIX-MKDIR-RMDIR: OK`
  4.86 s. The "5 s `ok` without booting" that phase 02 flagged is explained by the boot really taking
  ~5 s — it is not a stale log.
- The same commands on the same kernel panicked on one boot (the capture above) and completed
  cleanly on the next. A green `cargo test --test srv-cellosfs riscv64_cellosfs_srv_basic` therefore
  says nothing about this fault.

Use the direct QEMU drive (as above) when measuring it: it keeps the serial transcript and stops at
the panic, so one attempt either produces the measurement or provably completes without it.

## Why the next phase is a design, not a one-line patch

- Restoring the kernel root on trap entry is only half the invariant: `switch.S` treats
  `root_ppn == 0` — which is what `SwitchPlan` returns for `SameDomain` — as "leave `satp` alone", so
  a cell resumed on the same domain would keep running under the kernel root with the kernel's
  mappings visible to it. That is a silent isolation failure, worse than the fault being fixed.
- The obvious shortcut — mapping the UART into every domain root — is not acceptable: cells run in
  S-mode under that same root, so it would hand them the console's MMIO.
- `ViTrapFrame` is a 288-byte layout shared with x86_64 and aarch64; the interrupted root has to be
  parked somewhere that does not change that ABI.

[phase-04-trap-root-discipline.md](phase-04-trap-root-discipline.md) states the invariant, the three
places that must be changed, and the verification matrix.
