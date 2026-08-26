# Phase 01 — Kernel measurement-aggregate read syscall

## Context Links
- Plan: [plan.md](plan.md)
- Aggregate source: `kernel/src/measurement_log.rs:82-85` (`aggregate()` — already the value a token signs)
- Syscall enum: `libs/api/src/abi/syscall.rs:70-115` (234–238 used; **239 is the next free number**)
- Cap precedent: `ReadLog` gating via `ReadLogCap` (allowlist bit 54) — `syscall.rs:99`

## Overview
- **Priority**: P1 (unblocks binding the token to real boot state)
- **Status**: pending
- **Testability**: G1 / CI, all arches — the syscall returns a deterministic 32-byte value.
- **⚠ LAW 1**: adds one syscall to `libs/api`. Requires explicit **2x user confirmation** before code.
- Expose the rolling boot-measurement aggregate to a userspace attestation producer via a new
  read-only syscall `ReadMeasurement = 239`.

## Key Insights
- The aggregate is **non-secret** (SHA-256 over public ELF images) — the sensitivity is *integrity of
  the read path*, not confidentiality. Still gate it (least privilege) with a new manifest cap
  `AttestReadCap` so only the attestation/enrollment producer can read it.
- `aggregate()` already exists and is lock-guarded (`measurement_log.rs:83-85`); the syscall handler is
  a ~5-line copy-out. No new kernel state, no `unsafe` (Law 4 preserved).
- The value is stable after boot (spawns are bounded, `MAX_ENTRIES=256`, `measurement_log.rs:22`), so a
  producer reads it once at enrollment.

## Requirements
- Functional: `sys_read_measurement(buf_ptr, 32) -> bytes_copied(32)`; copies `aggregate()` into a
  caller buffer. ABI mirrors `ReadLog` (`syscall.rs:98`): `a0 = buf_ptr, a1 = max → bytes_copied`.
- Gate: `AttestReadCap` new allowlist bit; unauthorized caller → `ViError::PermissionDenied`.
- Non-functional: handler adds no allocation, no lock beyond the existing `LOG.lock()`; per-arch
  dispatch wired for RISC-V, ARM64, x86 (three trap tables).

## Architecture
`attestation producer cell` → `sys_read_measurement(buf)` → kernel dispatch → `measurement_log::
aggregate()` → copy 32 bytes to user buffer → return 32. One syscall, no reply IPC.

## Related Code Files
- **Modify (Law 1)**: `libs/api/src/abi/syscall.rs` — add `ReadMeasurement = 239` + doc-comment;
  add `AttestReadCap` bit to the cap/manifest allowlist definitions.
- **Modify (kernel)**: syscall dispatch (the match arm that handles `ReadLog=237` — same file region)
  + per-arch trap glue; `kernel/src/task/cap.rs` (add the cap bit + manifest decode).
- **Reference**: `kernel/src/measurement_log.rs:82-85`.

## Implementation Steps
1. **[GATE: 2x user confirmation for Law 1]** Add `ReadMeasurement = 239` to the syscall enum with a
   doc-comment matching the house style (see `ReadLog` at `syscall.rs:91-100`).
2. Add `AttestReadCap` allowlist bit + manifest keyword; thread through `CapSet` decode.
3. Implement the kernel handler beside the `ReadLog` arm: cap-check → `aggregate()` → copy-out.
4. Add an `ostd` wrapper `sys_read_measurement() -> [u8;32]` for cells.
5. Add a smoke cell / extend an existing attestation test cell to read the aggregate and assert it is
   non-zero after boot; add to the 3-arch CI suite.

## Todo List
- [ ] `ReadMeasurement = 239` + `AttestReadCap` defined (Law-1 confirmed)
- [ ] Kernel handler + cap-check implemented (no `unsafe`)
- [ ] Per-arch trap dispatch wired (rv64, aarch64, x86)
- [ ] `ostd` wrapper added
- [ ] CI cell reads aggregate; asserts non-zero + stable across two reads

## Success Criteria
- A capability-bearing cell reads a 32-byte non-zero aggregate on all three arches in CI.
- A cell WITHOUT `AttestReadCap` gets `PermissionDenied` (negative test).
- Two consecutive reads post-boot return identical bytes.

## Risk Assessment
- **Law 1 churn (High × Med)**: mitigation — additive enum value at a free number (239), no existing
  ABI value changes; gated 2x confirmation; doc-comment + allowlist bit are the only surface.
- **Cross-arch dispatch miss (Med)**: mitigation — the 3-arch CI cell is the gate; a missing arm shows
  as `NotSupported` on that arch.

## Security Considerations
- Read-only, non-secret data; the cap exists for least-privilege hygiene and audit, not secrecy.
- Do NOT expose individual `MeasureEntry` paths/hashes via this syscall (only the aggregate) — keeps
  the surface minimal and avoids leaking the boot cell inventory.

## Next Steps
- P02 consumes this aggregate to derive the real `CDI_final`.
- P05 (K3) reads it at enrollment so the ticket binds the node's measured boot state.
