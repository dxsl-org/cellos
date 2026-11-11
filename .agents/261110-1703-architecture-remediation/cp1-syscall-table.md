# CP1 — one declarative syscall table, generated lists, single allowlist gate (Phase 08)

**Date:** 2026-11-11 · **Slice:** S4 (architecture remediation) · **Law 1:** confirmation 1 requested · **Finding:** architecture review H-5 (+ Medium "C runtime tables hand-copied")

## 1. Problem (measured on `50a4f06e5`)

- `kernel/src/task/syscall.rs` is 14 295 lines. Each syscall is listed by hand in up to seven places:
  - `libs/api/src/abi/syscall.rs`: the `ViSyscall` enum (:8), `allowlist_bit` (:1196) and the `From<usize>` decode;
  - the kernel `Syscall` enum, the raw-id decode (`map_syscall`, ~:11 560), the variant → `ViSyscall` map (`syscall_to_vi`), and `supports_typed_oom`.
- The allowlist is checked twice. `check_allowlist` checks the raw id at trap entry; the variant check sits inside `handle_syscall_inner`. The code records a full-day defect from that split. 62 of 64 bits are used.
- 17 kernel files call `handle_syscall(tid, Syscall::…)` directly. Most are selftests; `gpu-scanout.rs` is a production in-kernel path. These callers skip the raw-id gate.
- The C and Zig syscall numbers are hand copies in three places: `libs/port-platform/include/cellos_syscall.h`, `libs/zig-syscall/src/syscall.zig` and `third_party/mlibc/sysdeps/vicell/include/vicell/syscall.h`.
- `scripts/check-{ai,lookupservicebound,multi-source-wait}-law1-digests.sh` match literal text in `libs/api/src/abi/syscall.rs`, so moving that text breaks them.

## 2. Proposed delta

**No number, allowlist bit, register convention, return encoding or wire byte changes.** The Law-1 surface touched is the source form of `libs/api/src/abi/syscall.rs`:

| Item | Change |
|---|---|
| `libs/api/abi/syscalls.toml` (new) | One row per syscall: `name`, `number`, `allowlist_bit` (or none), `args`, `result` (generic, typed OOM or broken pipe), `kernel_only`, and `raw_alias` for 3/100/110/111/255/500–504. |
| `libs/api/build.rs` (new) | Generates `ViSyscall` (same discriminants), `From<usize>` and `allowlist_bit`. `syscall.rs` `include!`s the generated file; hand-written items such as `ProcessInfo` stay in place. |
| `kernel/build.rs` | Generates the raw-id → `Syscall` decode skeleton, the reverse map and the encoder selector (`supports_typed_oom`). |
| `scripts/gen-syscall-headers.py` (new) | Generates the C and Zig headers. They are checked in and diffed in CI. |
| `libs/api` shared error encoding | `SyscallError` ↔ sentinel moves to `api`, shared by the kernel `encode_syscall_result` and the `ostd` decoders. The same values are kept. |

## 3. Proof obligations

1. **Golden dump before any edit.** Dump `(name, number, allowlist_bit, encoder)` for every syscall from the current code into `libs/api/abi/syscalls.golden.json`. A host test compares the generated values to the dump row by row, so the generated discriminants and bits are byte-identical to today's.
2. **Single gate.** Allowlist enforcement lives in exactly one place, `handle_syscall`. Every in-kernel caller is classified:
   - A **selftest or test** gets an explicit `KernelOrigin::Test` token.
   - **`gpu-scanout`** gets `KernelOrigin::Kernel`, documented as trusted.
   - Every other caller goes through the gate.
   A unit test per caller class checks that a denied syscall is denied identically from user entry and from that path.
3. **Digest scripts.** The three Law-1 digest scripts are re-pinned to the generated text, or to the TOML row, with the same semantic patterns. Each re-pin is listed in CP2.
4. **Move-only commits.** Splitting the dispatcher into `task/syscall/{mod,ipc,grant,spawn,fs,dev,vm,authority,misc}.rs` is done in commits that contain no logic edits, checked with `git diff --color-moved`.

## 4. Allowlist widening

This is design only, not implemented: 62 of 64 bits are used. Options for a later CP1 (manifest ABI):
- (a) a second 64-bit word in `__ViCell_syscalls`, versioned;
- (b) group bits per domain.

Phase 08 implements neither.

## 5. Risks and rollback

- A `build.rs` in `libs/api` must not break `no_std` cell builds or the patched-std sysroot lane. This is prototyped first on one cell per arch plus `std-smoke`; if it fails, the generator runs as a checked-in script instead, still CI-diffed.
- Rollback: revert the series. Numbers never move, so no ABI residue remains.

## 6. Evidence planned for CP2

- The golden-dump host test.
- The gate test for each caller class.
- A negative control: editing a header number by hand fails the CI diff.
- Kernel builds for 3 arches and the kernel host tests.
- One boot lane per arch, the x86 integration lanes, two-node C2C and the domain lane.
- The three digest scripts, passing.

## 7. Question to the owner

Approve CP1 (Law-1 confirmation 1) as scoped in §2–§3. Or choose the script-only generator (no `libs/api/build.rs`) from the start.

## 8. Owner decision

2026-11-11: **approved** (Law-1 confirmation 1). The api side uses `libs/api/build.rs`, with a fallback to a checked-in script if the build breaks.
