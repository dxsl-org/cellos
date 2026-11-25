# Phase 03 — VM entry path (2026-11-25)

Raw records: `phase03-bench.log` (lane output with every run), `phase03-soak.log`.
Same machine and method as `phase02-baseline.md`; these rounds ran on a quiet host
(`host_ms` 183-196).

## What changed

1. `INVEPT` before an entry only when this CPU's record is not the table's epoch, or the vCPU
   last ran on another CPU (`svm_registry::validate_ept_on_this_cpu`, `NestedPageTable::epoch`).
2. A VPID per vCPU where the CPU offers it: VM entries and exits stop flushing the guest's and
   the host's translations.
3. The VMCS host-state area is rewritten only when CR0, CR3, CR4, FS.base or GS.base moved
   (`host_state::Stamp`): 17 `VMWRITE`s and a trapped `RDMSR` less per call.

## Bench, two rounds, medians

`head` is this tree with 1-3; `baseline` is the phase-02 image.

| metric | head | baseline | reference |
|---|---|---|---|
| launch -> Chromium UI (s) | 23.4 | 24.9 | 8.2 |
| navigation -> report (s) | 19.7 | 20.0 | 17.4 |
| `raf` frames in 5 s | 254.5 | 267 | 376.5 |
| `timer` lateness p95 (ms) | 6.9 | 9.1 | 0.2 |
| `js1` (ms) | 525 | 577 | 292 |
| `jsN` (ms) | 1031 | 1081 | 850 |
| `scroll` frames in 5 s | 275 | 271 | 395.5 |
| `paint` frames in 5 s | 276.5 | 274 | 418 |
| `RunVcpu` calls | 93,807 | 84,464 | |
| in the run scope (ms) | 35,864 | 37,630 | |
| in the guest (ms) | 32,290 | 30,547 | |
| **around the guest, per call (µs)** | **38** | **84** | |

The per-call cost more than halves; what the user sees moves by 6 % (launch) to 9 % (`js1`).
The calls themselves, not their bookkeeping, are the larger cost: phase 04.

## Soak: 20 launches each, in turns (`--launch-only`)

| | reported | failed | median launch -> UI (s) |
|---|---|---|---|
| head | 19 | 1 | 22.4 |
| baseline | 19 | 1 | 24.4 |

Both failures are the same: `CHROME_GUEST_EXITED:139` (Chromium killed by SIGSEGV while it
starts; 13-50 s after cage). It is in the phase-02 image too, so it is not caused by this
phase, and it is not explained. Since then the guest's init restarts Chromium when a signal
kills it (twice at most) and prints the kernel's line about the fault.

## Not done, and why

- **Pending port read carried by the next `RunVcpu`.** The Cell spends 0.19 s on 21,915 8259
  port exits in a launch; the cost of those trips is the kernel's, and phase 04 removes the
  traffic rather than making each trip cheaper.
- **Serving host-interrupt exits in the kernel.** They are 3-4 % of the trips and cost the
  Cell 0.11 ms each (`preempted_away_ms` 332 ms for 2,908). x86 syscalls run with interrupts
  off, so the kernel would need an interrupt window inside `RunVcpu`: a new place where a
  syscall can be preempted, for that 3 %.

## Found on the way (needs an owner decision, see the plan)

The guest's disk is the largest single cost of a launch on the Cell's side: 7,823 virtio-blk
exits took 27.3 s of a 65 s launch on the loaded host, about 6 s of 25 s on the quiet one. The
VFS reads the Chrome image from its block driver 7 sectors (3.5 KiB) per IPC round trip
(`types::block_wire::MAX_RUN_SECTORS`), each one NVMe command.
