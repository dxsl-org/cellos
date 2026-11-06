# S3 fix S3 evidence: EFI ≥ 1 GiB boot panic at the bitmap/early-paging registration

Branch `feat/x86-s3-fix-efi-memory` from `464e168a8` (the V0 branch). Commit `bdf742de9`. Scope per [grant](../s3-v0-findings-grant.md) row S3: the boot memory registration path. No `libs/api` or `libs/types` byte changed; no Law-1 delta. `SystemPower = 436` untouched.

**Ceiling: VMware local + QEMU.** VMware Workstation 25.0.0 (driven from WSL2) and QEMU q35 with OVMF (`/usr/share/OVMF/OVMF_CODE_4M.fd`). Not CI, ESXi or physical hardware.

**Verdict: fixed.** VMware EFI 1024/2048/4096 MiB and QEMU OVMF 1/2/4 GiB reach `Cellos >`. BIOS lanes and the riscv64/aarch64 default lanes still pass.

## Root cause

`FrameAllocator::new_from_map` puts the bitmap at the start of the **largest** managed range (`plan_managed_ranges`). The boot heap was reserved with `reserve_contiguous_run`, a **first fit over every range**. `main.rs` then registers `[bitmap start, heap start)` as a shared `StaticWritable` supervisor range. That span assumes the heap follows the bitmap in the same range. When the largest range is not the first range with 32 MiB free, the heap lands below the bitmap. `register` then sees `start >= end` and returns `Err(())`, and the `.expect` panics.

| Map | Bitmap range (largest) | First range with 32 MiB free | Result before the fix |
|---|---|---|---|
| VMware EFI 512 MiB (V0 `efi-m512`) | `0x100000..0xdf2d000` (222 MiB) | same | boots |
| VMware EFI 1024 MiB (V0 `efi-m1024`) | `0xfbff000..0x39ab4000` (670 MiB) | `0x100000..0xdf2d000` | panic |
| VMware EFI 2048 / 4096 MiB (V0 probe) | `0xfbff000..` (1694 / 2718 MiB) | `0x100000..0xdf2d000` | panic |
| VMware BIOS 4096 MiB | `0x100000..0xbda55000` (3033 MiB) | same | boots |
| QEMU OVMF 1024 / 2048 MiB | `0x900000..` (887 / 1911 MiB) | same | boots |
| QEMU OVMF 4096 MiB | `0x100000000..0x180000000` (2048 MiB) | `0x900000..0x78005000` | panic |

Reproduced on QEMU OVMF 4096 MiB (KVM, baseline ISO `6baad0b0…`): `panicked at kernel/src/main.rs:1219:10: frame allocator bitmap and early paging registration must succeed`. A probe build (one temporary log line, not committed) printed the arguments of the failing call:

```
[ INFO] [S3FIX-PROBE] bitmap_range=(0xffff800100000000, 0xffff800100020000) heap_virt=0xffff800000900000 heap_phys=0x900000
```

At 1024 MiB the same probe printed `bitmap_range=(0xffff800000900000, 0xffff800000908000) heap_virt=0xffff800000908000`: the heap sits directly after the bitmap. The x86 early frames (firmware HHDM tables, kernel PML4) come from the next-fit cursor at index 0, so they land in the low range, not between the bitmap and the heap. V0's `[INFERENCE]` is confirmed: the heap was below the bitmap. The two did not overlap.

## Fix (`kernel/src/memory/frame.rs`, `kernel/src/main.rs`)

- `FrameAllocator` records `bitmap_range`, the index of the range that hosts the bitmap. `new_from_map` now builds through `from_plan(plan, storage)`, which clears the bitmap and marks its frames used. The behaviour is unchanged; host tests use the same constructor with an owned bitmap.
- `allocate_contiguous` is split into a per-range `allocate_contiguous_in`. The first fit over all ranges is unchanged.
- New `memory::frame::reserve_boot_heap(allocator, frames)` reserves the first free run inside the bitmap's range. `None` refuses boot. It does not fall back to another range, because that would break the registered span.
- `main.rs` reserves the heap with `reserve_boot_heap`. The OOM message names the bitmap's range. One comment at the registration names the dependency. The registration itself is unchanged.

The change applies to every architecture. Where the heap already followed the bitmap (every map that booted before), its address is the same. The host test asserts this for a BIOS map and a single-range map.

## Checks

| Check | Result | Decisive lines |
|---|---|---|
| New host tests (`boot_heap_*`) | PASS 5/5 | `boot_heap_follows_the_bitmap_on_uefi_maps`, `boot_heap_is_unchanged_where_it_already_followed_the_bitmap`, `boot_heap_refuses_rather_than_leave_the_bitmap_range` |
| Same tests against the first-fit placement (`reserve_boot_heap` temporarily calling `allocate_contiguous`) | FAIL 2 (expected) | `registration needs start < end: bitmap 0xfbff000, heap 0x100000` |
| Kernel host suite (`cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`) | PASS 503/503 with `--test-threads=1`; the parallel run passed 2 of 4 times | the parallel failures were `task::scheduler::retirement_tests::{faulted_task_to_boot_allows_successor_selection, blocked_task_requeued_from_boot_is_selectable}`, which share hart-local state across threads. Unrelated to `frame.rs`; the retirement tests pass alone (6/6) |
| `cargo clippy -p cellos-kernel -- -D warnings` x86_64 / riscv64 / aarch64 | clean | `Finished` on all three |
| `tools/mutable-globals --check` | 48 problems (= base) | `640 declarations, 255 need review, 48 problems` |
| aarch64 `LANE=virt-vhe`, `-kernel Image`, 2 CPUs | PASS | `PASS: aarch64-virt-vhe boot markers present`, `PASS: aarch64 shell prompt reached` |
| QEMU OVMF KVM 1024 / 2048 / 4096 MiB (ISO `e070210f…`) | PROMPT | e.g. 4096: `allocator: 13 usable range(s), 1037383 frames (4052 MiB)` → `Heap initialized` → `Cellos >` |
| QEMU OVMF TCG 1024 / 4096 MiB | PROMPT | same |
| `x86_64-boot` TCG, `CELLOS_X86_SMP=4` | PASS 10/10 | incl. `x86_smp_brings_every_cpu_online`; the two tier2 refusal tests ran against `build/vicell-x86-prod-refusal.iso`, built from this tree |
| `x86_64-boot` KVM, `CELLOS_X86_SMP=4` | PASS 10/10 | same |
| `scripts/qemu-x86_64-test.sh` (BIOS, TCG) | PASS | `PASS: x86_64 shell prompt reached` |
| VMware EFI 1024 MiB, SATA, 2 vCPU, two boots (run `20261106T004638Z-29aa5d`) | PASS | boot 1 `range 0xfbff000..0x39ab6000 (670 MiB)`, `Heap initialized`, `machine pin: first boot — recorded 3741143bce3c`, typed `uname -a` → `Cellos cellos-kernel 0.2.1 x86_64`; boot 2 `machine pin: match 3741143bce3c`, `vcat` → `S3LANE-20261106T004638Z-29aa5d-s3fix-efi-sata-m1024`; `persistence=PASS cleanup=True` |
| VMware EFI 2048 MiB (run `20261106T004850Z-f1dfd4`) | PASS | `range 0xfbff000..0x79ab5000 (1694 MiB)`, `topology online=2 … qualified=yes`, `Cellos cellos-kernel 0.2.1 x86_64`, `cleanup_confirmed: true` |
| VMware EFI 4096 MiB (run `20261106T005005Z-1cb1fe`) | PASS | `range 0xfbff000..0xb9ab6000 (2718 MiB)`, same markers, `cleanup_confirmed: true` |
| VMware BIOS 4096 MiB (run `20261106T005135Z-f8a11e`) | PASS | `range 0x100000..0xbda47000 (3033 MiB)`, `cleanup=True` |
| riscv64 default lane (`build-boot-ramdisk-ci.sh` + `qemu-boot-test.sh`) | PASS | `range 0x80c7a000..0x90000000 (243 MiB)`, `Heap initialized`, `PASS: FAT16 mounted — kernel booted (no disk)` |
| aarch64 default lane (CI recipe + `qemu-aarch64-test.sh`) | PASS | `PASS: aarch64 shell prompt reached — full boot successful` |

VMware lane ISO: `build/x86-vm-lane/vicell-x86-vm.iso` (`limine-vm.conf`, sha256 `5c24a8e0…`). Evidence is under `build/x86-backend-evidence/vmware-lane/<run>/`. Every VM was created under the lane root and deleted. EFI cells end with a hard stop because of S4. Regenerated tracked `kernel/src/embedded/init` and `kernel/src/embedded-aarch64/init` were restored; `kernel/src/embedded-x86_64/*` was not regenerated.

## Deviations

- The evidence is in this file and not in `s3-v0-vmware-tooling-evidence.md`. This avoids conflicts with the parallel S1/S2/S4 fixes; the integrator updates the phase file.

## Surprise (outside S3's added scope; reported, not fixed)

- **QEMU OVMF with `-smp ≥ 2` panics: `kernel/src/task/smp.rs:1100: AP requires permanent shared kernel root`.** This happens on the baseline too (OVMF 1024 MiB `-smp 4`, before this fix). OVMF reports usable memory from physical `0x0` (`range 0x0..0xa0000`). The next-fit allocator hands out frame `0x0`, and the kernel PML4 can land there. `record_kernel_cr3(pml4_phys)` (`paging.rs:572`) is the only writer of `VI_KERNEL_CR3`, so `kernel_cr3() == 0` means `pml4_phys == 0`: a valid root that collides with the "unset" sentinel. VMware EFI (`0x3000..`) and BIOS (`0x5a000..`) maps do not start at 0, so the VMware lanes are not affected. Owner decision: exclude frame 0 from the managed ranges, or stop using 0 as the sentinel.
