# CP2 — one declarative syscall table, generated lists, single allowlist gate (Phase 08)

**Date:** 2026-11-11 · **CP1:** [cp1-syscall-table.md](cp1-syscall-table.md) (approved 2026-11-11) · **Law 1:** confirmation 2 requested

## 1. Delta as implemented

- **`libs/api/abi/syscalls.toml`** is the single source. It has:
  - 142 `[[syscall]]` rows: name, number, bit, rustdoc, notes, `kernel` name override and `typed_oom`;
  - 8 `[[raw]]` kernel-only opcodes: 100/110/111/500–504;
  - 5 `[[internal]]` variants that have no opcode.
- **`libs/api/abi/table.rs`** is the parser that both build scripts `include!`. It fails the build on any of the following:
  - an unknown key or a duplicate key;
  - a bit of 64 or more;
  - a duplicate number or name;
  - a raw id that equals an api number;
  - a kernel variant claimed twice;
  - a `gate_as` that names no row.
- **`libs/api/build.rs`** generates `ViSyscall`, `allowlist_bit`, `typed_oom` and `From<usize>`. It keeps the old `rerun-if-changed` lines for `printf`. `syscall.rs` shrinks from 1 820 to about 470 lines.
- **`kernel/build.rs`** generates `opcode_of` (exhaustive), `RAW_OPCODES` and the feature-gated `RAW_GATE_AS` (504 is gated as MemInfo).
- **New in `api`:** `api::syscall::result_sentinel` (0/1/2), shared by the kernel encoder and `ostd`. The values are unchanged.
- **No number, bit, register convention, return encoding or wire byte moved.**

## 2. Proof obligations (CP1 §3)

| Obligation | Evidence |
|---|---|
| 1. Byte-identical numbers and bits | **Golden dump.** `libs/api/abi/syscalls.golden.json` was taken before any edit: 148 rows giving api decode and bit, kernel decode, variant map, result encoder and effective gate bits. The kernel host test `syscall_golden` passes against the generated code. After the single gate, the only diff is `kernel_vi` for the 23 variants the hand map left unmapped; `gate_bits`, `typed_oom` and every api field are unchanged. |
| 2. Single gate | **One check.** `check_allowlist` is the only allowlist check: the trap applies it to the raw opcode and `handle_syscall` to `opcode_of(decoded)`. The variant-path check is deleted. |
| | **Equivalence test.** `in_kernel_gate_matches_the_trap_gate` runs every decodable opcode under 66 allowlists. In-kernel and trap admission are identical, except raw 100, where in-kernel is stricter because it shares a variant with the gated `Call`. |
| | **Kernel origin.** GPU scanout uses `handle_syscall_kernel_origin`, which is trusted, `pub(in crate::task)`, and never reached from a trap. `kernel_origin_skips_the_allowlist_and_handle_syscall_does_not` covers it. |
| | **Internal variants.** `internal_variants_have_no_opcode_and_need_permit_all` covers the five `[[internal]]` variants. |
| | **Audit.** A refusal records its real cause: the bit, `0x100 \| bit` for a missing right, or `u32::MAX` for unknown. |
| 3. Digest scripts | **Re-pinned.** `check-lookupservicebound` and `check-multi-source-wait` now check the table row, which generates the enum, decode and bit arms, plus the kernel text across `syscall/*.rs`. `check-ai` needed no change. All three pass, and the first two now run in CI. |
| | **Negative control.** Editing a bit fails both the digest check and `syscall_golden`. |
| 4. Move-only split | **What moved.** `task/syscall.rs` became `task/syscall/{mod,decode,ipc,grant,spawn,fs,dev,vm,authority,misc,tests}.rs`, with `dma_export` and the grant tests moved by `git mv`. |
| | **Equivalence.** After stripping path re-anchoring, visibility and whitespace, the character multiset equals the original except for the one `}` of the removed `mod tests {}`. |
| | **Visibility.** Items that were task-visible stay `pub(in crate::task)`. |
| | **Not split yet.** By owner decision, the 5 212-line dispatcher match is extracted in Phase 09. |

## 3. Evidence (CP1 §6), local only

- **Host tests.** Kernel: 615 pass with `compile-only` and with `test-hooks`. The `retirement_tests` flake appears intermittently, as it does on the pre-phase base. `api`, `ostd` and the digest scripts pass.
- **Builds.** Clippy `-D warnings` is clean on 3 arches. `test-hooks`, `bench-counters` and `getrandom-sas-test` check.
- **std-smoke** boots to PASS on riscv64, aarch64 and x86_64. That is the CP1 prototype obligation for `libs/api/build.rs`.
- **Headers.** `gen-syscall-headers.py --check` passes. Negative control: hand-editing `VI_SYS_YIELD` fails it.
- **QEMU lanes** (`/tmp/s1-lanes/p08b`):
  - x86 boot plus integration (x86_64-boot, ahci, igb, nic, vmxnet3, virtio, pvscsi, serial, power) pass.
  - xhci: 3 hub tests fail; they also fail on the original HEAD.
  - Two-node C2C, domain 1-CPU and 2-CPU, RV64, AArch64, phase09-ssh and release posture pass.
  - The ABI refusal witness passes on a fresh ISO.
  - `local-service-lifecycle-x86` passes on rerun, with the same `multi_source_wait_holds_on_two_cpus` flake seen in Phases 06–07.
  - Boot selftests 13/13 pass on all three arches.
- **Independent review** (haily-reviewer, PASS_WITH_RISK): 7 Low findings, all addressed (see the phase Deviation Log).

## 4. Not done / residual

- The pinned sha of `kernel/src/task/syscall.rs` in the manual `qemu-tier3-ssh-guest-x86.sh` lane was already stale before this phase and is left as it is.
- The `cellos_p01_trace` configuration is not built locally because it needs the P01 embed manifest. The reviewer read its moved code and found only `crate::` paths.
- Allowlist widening (62/64 bits used) is designed in CP1 §4 and not implemented.

## 5. Question to the owner

Confirm CP2 (Law-1 confirmation 2) for the `libs/api` source-form change and `result_sentinel` as implemented in §1.

## 6. Owner decision

2026-11-11: **CP2 confirmed** (Law-1 confirmation 2).
