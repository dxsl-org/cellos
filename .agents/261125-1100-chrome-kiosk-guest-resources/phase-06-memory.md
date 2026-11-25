---
phase: 6
title: "More memory"
status: completed
priority: P2
effort: "2-3d"
dependencies: [3]
tier: medium
checkpoint: false
---

# Phase 6: More memory

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

The Chrome guest has 1536 MiB (1475 MiB visible), with its profile, overlay and `/tmp` in RAM
too, and its nested page tables map 4 KiB at a time. This phase gives it 3072 MiB when the
machine has it and maps guest RAM with 2 MiB leaves.

## Requirements

- Functional: the Chrome profile asks for 3072 MiB and falls back to 2048, then 1536 MiB when
  the kernel has no contiguous run that large; the size used is logged and shown in the
  `CHROME_GUEST_DEVICES` marker.
- Functional: `/bin/hypervisor` `guest_memory` budget 1544 MiB -> 3080 MiB (catalog row; CP1).
- Functional: guest RAM is carved 2 MiB-aligned and mapped with 2 MiB leaves where a whole
  2 MiB range is mapped with one permission; `unmap`, `translate` and the device-frame path
  split or refuse a large leaf correctly. Same code for EPT and NPT.
- Functional: `run-desktop.sh` gives the machine 6 GiB when the Chrome disk is attached; the
  KVM lanes follow.
- Non-functional: the 128-512 MiB console profiles and the SVM lanes are unchanged in size.

## Architecture

RAM stays one run at GPA 0 below the virtio hole (`0xd000_0000`), so 3072 MiB fits without a
second region. `NestedPageTable::map` writes a PD leaf when GPA, host address and remaining
length are 2 MiB-aligned; `table_bound` stays the 4 KiB upper bound.

## Assumptions

- **Claim:** a 3072 MiB contiguous run is free on a freshly booted 6 GiB machine.
  **Confidence:** medium. **Verify:** lane log; the fallback covers the rest.
- **Claim:** 2 MiB leaves shorten Chromium's start measurably here. **Confidence:** low.
  **Verify:** phase-02 bench with only this change.
- **Claim:** nothing maps or unmaps part of guest RAM after boot (only the hostile runner's
  reset path). **Confidence:** medium. **Verify:** callers of `unmap` and `map_device_frame`.

## Related Files

- Modify: `kernel/src/arch/x86_64/memory/ept.rs`, `kernel/src/memory/frame.rs` (aligned run),
  `kernel/src/catalog.rs`, `cells/services/hypervisor/src/boot_x86_profile.rs`, `boot_x86.rs`,
  `run-desktop.sh`, `scripts/tier3_chrome_check.py`, `scripts/chrome-guest/init.sh` (marker)
- Regenerate: `scripts/catalog.generated.json`; record in `docs/abi-boundary-records.md`

## Implementation Steps

1. CP1 for the budget row.
2. Aligned carve and 2 MiB leaves with host tests for map, unmap-split and translate.
3. Profile sizes and fallback; machine sizes in the scripts.
4. Measure; lanes (KVM and the SVM CI jobs).

## Success Criteria

- [x] The guest reports at least 2900 MiB on a 6 GiB machine and still starts on a 3 GiB one.
      (2983 MiB on 6 GiB; on 3 GiB it falls back to 1536 MiB and Chromium's UI came in
      20.5 s, after the allocator fix in the deviation log.)
- [x] Nested-table pages for the Chrome guest are reported before and after.
      (5 pages for 3072 MiB measured; 1541 with 4 KiB leaves, computed; see the log.)
- [x] Bench rows before and after; ept host tests; Chrome lane, display lane, x86 Tier 3 CI jobs pass.
      (Phase 04e image, 1536 MiB, against the current default build in the same rounds:
      `evidence/phase05-vcpus.md`, first table.)

## Security Considerations

A large leaf must never cover a hole: the existing `MMIO_HOLES` check runs before leaf size is
chosen. The budget charge is still taken before the carve.

## Risk Notes

More guest RAM is more host RAM held while Chrome is open; the desktop's own needs on a small
machine are why the profile falls back instead of failing.

## Deviation Log

- Deviation (order): done before phase 05. The dependency graph allows it (06 after 03), and
  it is contained where 05 is the largest change of the plan; the lanes then verify 04 and 06
  together on the uniprocessor guest before vCPUs change the picture.
- Decision: guest RAM is carved 2 MiB-aligned when a run is free (`find_free_run_aligned`,
  linear, inside one managed range), else as before. `allocate_contiguous_aligned` already
  existed but re-reads the run from every aligned candidate (quadratic over a 3 GiB carve
  with the frame-allocator lock held, IRQs masked), so it was not used.
- Decision: 2 MiB leaves only on EPT and only where `IA32_VMX_EPT_VPID_CAP[16]` says the CPU
  walks them (`invept::large_pages_supported`). NPT keeps 4 KiB leaves: its CI lanes run
  under TCG and nothing measured asks for the change there. Same code, one switch per table.
- Decision: a page mapped into, or unmapped from, a 2 MiB leaf splits it into 512 equivalent
  4 KiB leaves first. An unmap whose split finds no frame clears the whole 2 MiB leaf (takes
  away more, never leaves a page mapped). `NestedPageTable::unmap` has no caller outside
  tests today.
- Decision: the Cell tries 3072, 2048, 1536 MiB and logs `[hv-x86] guest RAM: N MiB (3072
  MiB did not fit)` for a smaller size. Machines: `run-desktop.sh` 6144 MiB with the Chrome
  disk, Chrome lane 6G, bench 6G (reference guest 3072M to match).
- Surprise (found on the way): the hostile CI corpus failed on a guest line split in two
  (`[VIRTIO_HOS` | `TILE] DONE …`): phase 04's UART change flushed a partial line after two
  quiet boundaries, and on SVM a host tick makes a boundary every 10 ms. Fixed: a partial
  line goes out at `HLT` (the old rule, which the lanes had passed with) or after 100 ms with
  no byte when the guest was preempted.

- Surprise (pre-existing bug, found by the 3 GiB start check, 2026-11-26): on a 3 GiB QEMU
  machine 3072 MiB does not fit and the 2048 MiB guest died in early boot (`BUG: kernel NULL
  pointer dereference … sparse_init`), every run; the phase 04e image (1536 MiB) booted on
  the same machine. Cause: `FrameAllocator::find_free_run`, the guest-RAM fallback, counted
  runs in index space, which concatenates the managed ranges, so a run could cross the hole
  between RAM below and above 4 GiB and the guest was mapped over memory it did not own; its
  doc and its test name claimed the opposite, and the test had a single range. Fixed: the
  fallback is the per-range `find_free_run_aligned(n, PAGE_SIZE)`, `find_free_run` is gone,
  and `a_guest_run_never_spans_the_hole_between_ranges` fails on the old search.
- Measured: the 3072 MiB guest's nested tables take 5 pages (`[hv-x86] vm 1 nested-table
  pages: 5 for 3072 MiB of guest RAM`, soak run). With 4 KiB leaves the same RAM needs 1536
  page tables + 3 directories + PDPT + PML4 = 1541 pages (computed from the table layout, not
  measured: the 1536 MiB image before this phase does not log the count).
- Soak (default build, one vCPU): 608 s of bench-page reloads in one Chromium, 27 rounds, the
  slowest report 26.9 s, nothing matching a lockup, stall, segfault or oops in the serial
  log (`/tmp/cellos-tier3-soak2/soak.log`). An earlier soak attempt ended at start-up with
  the known cage SIGSEGV; the guest then wrote 13 of the 40 lines of cage's log and nothing
  more for 285 s (`/tmp/cellos-tier3-soak/bench-head-0-serial.log`): cause not found, one
  sample.
