# CP2 — Cell initial stack + auxv and the libc start-up hook (slice RE)

Status: **CP2 APPROVED by the owner on 2026-11-04 ("duyệt") — [record](cp2-cell-initial-stack-auxv-approval.md); "Cell initial stack v1" frozen, slice RE closed, WIP lane released.** CP1: [package](cp1-cell-initial-stack-auxv.md), AX-1…AX-8 [approved 2026-11-04](cp1-cell-initial-stack-auxv-approval.md). Slice record with acceptance and deviations: [slice-runtime-enablement.md](slice-runtime-enablement.md).

## 1. Delta as implemented

| AX | Implemented as | Where |
|---|---|---|
| AX-1 | every ELF Cell's root task on 64-bit targets; threads (`Spawn`) unchanged; 32-bit kernels keep the bare stack top | `kernel/src/task/elf_prepare.rs` (`write_initial_stack`, `#[cfg(target_pointer_width = "64")]`) |
| AX-2 | layout and 16-byte `sp` from `cell_initial_stack::build`; entry registers unchanged, `sp` = block | `libs/cell-initial-stack/src/lib.rs`; `kernel/src/task.rs` (`prime_user_mode_entry_at`) |
| AX-3 | `argc` = 0, empty envp | same |
| AX-4 | `AT_PHDR` (stack copy), `AT_PHENT` 56, `AT_PHNUM`, `AT_PAGESZ` 4096, `AT_BASE` = load base, `AT_ENTRY`, `AT_RANDOM` when `get_random` fills 16 bytes (RDSEED/RDRAND on x86; none on RV64/AArch64 today), `AT_NULL` | same |
| AX-5 | > 16 headers or `e_phentsize` ≠ 56 → spawn refused with `InvalidInput` and a `[loader]` warning; block ≤ 1072 bytes | `elf_prepare.rs`; `MAX_BYTES` |
| AX-6 | ostd `_start` → `__cellos_libc_start(entry_sp)` → `main` → `Exit(0)`; ostd's weak default runs `.init_array`; mlibc's strong definition replaces it, kept in the link by `mlibc-shim` | `libs/ostd/src/startup.rs`, `libs/ostd/src/startup/libc_hook.rs`, `libs/mlibc-shim/{build.rs,src/lib.rs}`, `third_party/mlibc/sysdeps/vicell/generic/startup.cpp` |
| AX-7 | written through the shared-address-space copy view (`TaskCopyView::sas`) into the root user stack before the task exists; no mapping, no authority | `elf_prepare.rs` |
| AX-8 | this package; ABI record row `docs/abi-boundary-records.md` §8 | — |

Also in the slice: launch rows `/bin/mlibc-smoke`, `/bin/mlibc-posix-x86` (`boot_ceiling.rs`, `launch_profile/targets.rs`; LEGACY, capability-free), the witness image and runner (`scripts/build-x86_64-mlibc-witness-ci.sh`, `scripts/x86/qemu-mlibc-witness.sh`), and two R3 fixes the run exposed (mlibc pin patch 0001, 16 MiB arena).

## 2. Deviations needing approval

1. **mlibc pin patch 0001** (`third_party/mlibc/patches/0001-static-start-honours-at-phdr.patch`): static start-up uses `AT_PHDR` when present, `__ehdr_start` otherwise. Without it mlibc reads the unmapped ELF header (faulted on the first run). First patch to mlibc source; MIT; applied by the build scripts.
2. **AX-6 linkage**: weak *default* definition in ostd plus a strong anchor reference in `mlibc-shim`, instead of a weak *undefined* GOT lookup. Same observable sequence; Cells keep only relative relocations.
3. **Heap arena 4 → 16 MiB** (interim until R3b).

## 3. Evidence

- Host: `cargo test -p cell-initial-stack` 8/8; `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` 421/421 (launch-profile pins included); `cargo test -p ostd` green; `cargo check -p cellos-kernel` on riscv64, aarch64, x86_64 and x86_64 `test-hooks,native-domains`; clippy `-D warnings` on the new crates.
- x86_64 QEMU: mlibc witnesses 3/3 and 7/7, TCG and KVM — [docs/evidence/mlibc-runtime-x86.txt](../../docs/evidence/mlibc-runtime-x86.txt).
- Regression: x86 domain lane PASS (every marker; `cpp-smoke` constructors through the weak default); RV64 default kernel boots init, services and starts the shell, no fault.
- Not covered: aarch64 runtime, RV64 mlibc runtime, KVM `-cpu host` (boot selftest exit before any Cell, slice record item 7), physical hardware. Repeated threaded runs leak ≈ 56 KiB each and the 45th spawn in one boot fails (slice record item 9); single-threaded mlibc Cells and ordinary Cells do not grow — a separate kernel finding, not part of this delta.

## 4. Digests at packaging

| Path | SHA-256 |
|---|---|
| `libs/cell-initial-stack/src/lib.rs` | `bc3204674c59f94deb1815296b52a58cba8e2a515e33a83ee7981450e5680e71` |
| `kernel/src/task/elf_prepare.rs` | `5c58f0afefb5a3e052e35fe72d5542879671fc7ced56c00aae68d13de27ff790` |
| `libs/ostd/src/startup.rs` | `521fe99d1e448be1e1ad281610fa61a1640422e9def53fd9d37d53fd5051a09a` |
| `libs/ostd/src/startup/libc_hook.rs` | `ee286c71b350e510e53856a7db265c1dbeef943e4d23f48bdce20c855b111f97` |
| `libs/mlibc-shim/src/lib.rs` | `5a347643c9a1cf40a10d679183de5e56c19938099d72980f01ab35f9b27a0877` |
| `third_party/mlibc/sysdeps/vicell/generic/startup.cpp` | `831d70248e206a2f480972d78da5dff1a6fde11a1da2171aa0944715446bfba6` |
| `third_party/mlibc/patches/0001-static-start-honours-at-phdr.patch` | `59533c138865acf6d0e9d0aa18acd6cdd7ce045cef918119a43e37bd6aba3109` |

## 5. Question for the owner

Confirm CP2 (the delta in §1 with deviations §2.1–§2.3), which freezes "Cell initial stack v1" and releases the WIP lane.
