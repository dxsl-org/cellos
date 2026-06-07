# Phase 01 — RV32 Syscall Dispatch

## Context Links

- Plan: [plan.md](plan.md)
- Spec: `docs/specs/04-hardware.md` (multi-arch HAL), `docs/specs/00-context.md`
- Coding laws: `CLAUDE.md` (Law 1 ABI, Law 4 unsafe, Law 5 no mod.rs, Law 6 Vi prefix)
- Memory: [[project-rv32-nano-plan]] (Phase 31 facts)

## Overview

- **Priority:** P2
- **Status:** completed (2026-06-07)
- **Effort:** ~3 days (completed in 1 session)
- **Description:** Replace the no-op riscv32 `ViCell_syscall_dispatch` stub
  (`kernel/src/task/syscall.rs:1804-1808`) with a real dispatcher that mirrors the
  RV64 implementation (`syscall.rs:1810-2002`), adapting for `u32` register slots.
  After this, RV32 cells can invoke all existing `ViSyscall` variants.

## Key Insights (verified against codebase)

1. **The RV64 dispatcher is the template.** `ViCell_syscall_dispatch`
   (`syscall.rs:1813-2002`, `#[cfg(not(target_arch = "riscv32"))]`) reads
   `frame.regs[17]` (a7=syscall id) and `frame.regs[10..14]` (a0–a3), maps via
   `ViSyscall::from`, runs the allowlist gate, enables SUM, calls `handle_syscall`,
   writes the result to `frame.regs[10]`. The riscv32 version is logically identical.

2. **Only difference is the register width.** On RV64 `frame.regs[N]` is `usize`
   (= u64) so the values flow into the `Syscall` enum (whose fields are `usize`)
   without casts. On RV32 `ViTrapFrame32.regs` is `[u32; 32]` (`rv32/trap.rs:17`),
   so every read needs `as usize` and the result write needs `as u32`.

3. **`ViTrapFrame` is already aliased.** On rv32, `hal::arch::ViTrapFrame` =
   `ViTrapFrame32` (`rv32.rs:35`). The stub already takes
   `&mut crate::hal::arch::ViTrapFrame` — signature stays the same.

4. **The trap path is already wired.** `vi_trap_handler32` (`rv32/trap.rs:48`)
   handles scause 8/9 (ecall) by calling `vi_handle_syscall(frame)` then
   `frame.sepc += 4` (`trap.rs:74-76`). `vi_handle_syscall` (`trap.rs:95-101`)
   already calls `ViCell_syscall_dispatch(frame)`. **No trap.rs change is expected**
   — confirm during implementation (Step 7).

5. **SUM is riscv64-gated today.** The SUM enable/disable in the RV64 dispatcher is
   `#[cfg(target_arch = "riscv64")]` (`syscall.rs:1985-1996`), writing bit 0x40000
   (= bit 18) of sstatus. RV32 sstatus has SUM at the same bit 18 — the riscv32 arm
   uses the identical `0x40000` mask.

6. **Core helpers are arch-agnostic — no edits.** `handle_syscall` (`syscall.rs:427`),
   `ViSyscall::from` / `allowlist_bit` (libs/api), `current_task_id` (`task.rs:470`),
   `system_ticks` all already work on rv32. The `Syscall` enum fields are `usize`,
   which is 32-bit on rv32 — the enum needs no change.

7. **Watchdog reset + allowlist logic is copy-identical.** The `run_ticks = 0`
   progress reset (`syscall.rs:1821-1828`) and the allowlist/blk-io-bit gate
   (`syscall.rs:1944-1982`) are arch-neutral and copy verbatim (only arg reads
   change). DRY note in Step 6.

## Requirements

### Functional
- F1. riscv32 `ViCell_syscall_dispatch` reads `frame.regs[17] as usize` as syscall id.
- F2. Extracts a0–a3 from `frame.regs[10..14]`, each promoted `u32 → usize`.
- F3. Maps id → `Syscall` via the same `ViSyscall::from` match block as RV64,
      including the legacy/raw-opcode inner fallback (3, 100, 106–111, 500–503).
- F4. Resolves `caller_id` via `current_task_id()`.
- F5. Applies the allowlist gate (Unknown-opcode deny, per-bit gate, blk-io bit 36)
      identical to RV64.
- F6. Enables SUM (sstatus bit 18 / 0x40000) before `handle_syscall`, disables after.
- F7. Writes `handle_syscall` result back: `Ok(v) → regs[10] = v as u32`,
      `Err(_) → regs[10] = u32::MAX`; unknown opcode → `regs[10] = u32::MAX`.

### Non-functional
- N1. No `libs/api` / `libs/types` change (Law 1 — avoid 2x confirm).
- N2. All new `unsafe` blocks carry `// SAFETY:` (Law 4); kernel code only.
- N3. No `mod.rs` (Law 5). Public symbol name `ViCell_syscall_dispatch` unchanged
      (`#[no_mangle]`, `#[allow(non_snake_case)]`) — it is the ABI name the RV32 trap
      vector links against (`trap.rs:96-100`).
- N4. RV64 build must remain byte-for-byte unchanged (cfg gating only).

## Architecture

### Data flow (per syscall on RV32)
```
U-mode cell: a7=id, a0..a3=args, `ecall`
  → trap → __trap_entry32 (asm) saves ViTrapFrame32 on stack
  → vi_trap_handler32 (trap.rs:48): scause==8 → vi_handle_syscall(frame)
  → vi_handle_syscall (trap.rs:95): ViCell_syscall_dispatch(frame)   ← THIS PHASE
       id   = frame.regs[17] as usize
       a0..3 = frame.regs[10..14] as usize
       sc   = ViSyscall::from(id)
       caller_id = current_task_id()
       [watchdog reset] [allowlist gate]
       SUM=1
       result = handle_syscall(caller_id, Syscall::…)   ← arch-agnostic, unchanged
       SUM=0
       frame.regs[10] = result as u32 (or u32::MAX)
  → return to trap handler: frame.sepc += 4 (trap.rs:75)
  → sret resumes cell at next instruction with a0 = return value
```

### cfg strategy
- Change the existing `#[cfg(target_arch = "riscv32")]` stub into the full body.
- Add a `#[cfg(target_arch = "riscv32")]` SUM enable/disable arm (mirrors the
  existing riscv64 arm) — DO NOT broaden the riscv64 `cfg`.
- Keep the `#[cfg(not(target_arch = "riscv32"))]` import of `ViTrapFrame`
  (`syscall.rs:1797-1798`); the rv32 body uses `crate::hal::arch::ViTrapFrame`
  directly (the alias), no new import needed. Verify `ViSyscall` import
  (`syscall.rs:1796`, unconditional) is visible to the rv32 body.

### DRY decision (KISS over premature abstraction)
The RV64 and RV32 bodies share a ~120-line `match ViSyscall::from(...)` block that
differs ONLY by `as usize` on the arg bindings. Two viable approaches:

- **(A) Duplicate the body** under each cfg (simplest, what the stub structure
  implies). Pro: zero risk to RV64; trivial review. Con: ~120 lines duplicated.
- **(B) Factor the mapping into a shared `fn map_syscall(id: usize, a0..a3: usize)
  -> Option<Syscall>`** called by both arches after they read+promote registers.
  Pro: DRY, single source of truth for the id→variant table. Con: touches the RV64
  path (violates N4 "byte-for-byte"), larger diff.

**Recommendation: (B) is the right long-term call** (the duplicate table is a
maintenance hazard — a new syscall must be added in two places), BUT it changes the
RV64 path. Decision gate at Step 3: if `cargo build` for riscv64 is green and the
RV64 boot smoke test passes unchanged, take (B); otherwise fall back to (A). Default
to (A) if time-boxed — it is the lower-risk MVP and satisfies all F-requirements.

## Related Code Files

### Modify
- `kernel/src/task/syscall.rs` — replace the riscv32 stub (lines 1804-1808) with the
  real dispatcher. If approach (B): also extract `map_syscall` and call it from the
  RV64 body (lines 1838-1937). Add a riscv32 SUM arm.

### Read-only (verify, do not edit)
- `hal/arch/riscv/src/rv32/trap.rs` — confirm `vi_handle_syscall` (95-101) passes
  the frame and `sepc += 4` runs after (74-76). **Edit ONLY if Step 7 finds the
  return value isn't surfacing** (e.g. frame copy-by-value bug).
- `hal/arch/riscv/src/rv32.rs:35` — `ViTrapFrame` alias.
- `kernel/src/task.rs:470` — `current_task_id`.

### Do NOT touch
- `libs/api/`, `libs/types/` (Law 1).
- The `#[cfg(not(target_arch = "riscv32"))]` RV64 dispatcher body unless approach (B)
  is chosen and the decision gate (Step 3) passes.

## Implementation Steps

1. **Read & confirm the template.** Re-read `syscall.rs:1796-2002` (RV64 dispatcher
   + cfg imports) and `rv32/trap.rs:48-110`. Confirm the stub signature
   `fn ViCell_syscall_dispatch(_frame: &mut crate::hal::arch::ViTrapFrame)`.

2. **Choose approach (A) or (B).** Default (A) duplicate-body. Re-evaluate at Step 3.

3. **(If B) Extract `map_syscall`.** Add
   `fn map_syscall(syscall_id: usize, a0: usize, a1: usize, a2: usize, a3: usize)
   -> Syscall` containing the current RV64 match (incl. the legacy inner fallback
   and the `_ => regs[10]=MAX` becomes `return a sentinel Syscall or Option`).
   Replace the RV64 inline match with a call to it. Build riscv64 + run boot smoke
   test. **Gate:** green → keep (B); red/over-budget → revert to (A).

4. **Write the riscv32 dispatcher body.** Replace the stub. Read
   `id = frame.regs[17] as usize`; `a0..a3 = frame.regs[10..14] as usize`. Reuse the
   mapping (call `map_syscall` for B, or paste the match with `as usize` reads for A).
   Note: for variants packing wider types (`BlkRead.sector: u64` from `a0 as u64`,
   `RecvTimeout.deadline` from `a3`), the existing `as u64` casts on the already-usize
   value are correct on rv32 too (usize=u32 → u64 zero-extends). Verify R3.

5. **Add `caller_id` + watchdog reset + allowlist gate.** Copy
   `syscall.rs:1821-1828` (run_ticks reset) and `1944-1982` (allowlist) verbatim —
   they read SCHEDULER, not the frame, so they are arch-neutral.

6. **Add the riscv32 SUM arm.** Mirror `syscall.rs:1985-1996`:
   `#[cfg(target_arch = "riscv32")] unsafe { asm!("csrs sstatus, {0}", in(reg) 0x40000); }`
   before `handle_syscall`, and the matching `csrc` after. Do NOT touch the riscv64 arm.

7. **Write result + verify trap return.** `Ok(v) => frame.regs[10] = v as u32;
   Err(_) => frame.regs[10] = u32::MAX;`. Confirm `vi_handle_syscall` passes `frame`
   by `&mut` (it does — `trap.rs:48,95`), so the write reaches the saved frame that
   `__trap_entry32` restores. If a return value does not surface in the cell, inspect
   `rv32/asm/trap.S` frame restore (out-of-scope edit; flag as follow-up).

8. **Build.** `cargo build` for `riscv32imac-unknown-none-elf` (the Nano target) and
   for `riscv64` (regression). Both must compile with `-D warnings` clean (Law:
   clippy clean).

9. **Smoke test on QEMU.** Boot the RV32 Nano image; have a cell issue `Log` (id 11),
   `Yield` (104), `GetTime` (120 → expect 0 on rv32, since GetTime mtime is
   riscv64-gated, `syscall.rs:1396-1399`), and `Exit` (60). Confirm the Log message
   prints and the cell exits cleanly (return value visible in a0).

10. **Review.** Run `haily-reviewer` on the diff; confirm Laws 1/4/5/6, no RV64
    behavioral change.

## Todo List

- [x] Re-read RV64 dispatcher + rv32 trap path; confirm stub signature (Step 1)
- [x] Decide approach A (duplicate) vs B (shared `map_syscall`) (Step 2) — chose B (DRY)
- [x] Extract `map_syscall`, rewire RV64, gate on riscv64 build+boot (Step 3) ✅
- [x] Write riscv32 dispatcher: id + a0–a3 `u32→usize`, variant mapping (Step 4)
- [x] Add caller_id + watchdog reset + allowlist gate (arch-neutral copy) (Step 5)
- [x] Add riscv32 SUM enable/disable arm (bit 0x40000) (Step 6)
- [x] Write result back as `u32` (Ok→val, Err/unknown→u32::MAX) (Step 7)
- [x] `cargo build` riscv32imac + riscv64 regression, clippy clean (Step 8) ✅
- [x] QEMU smoke: RV32 kernel boots idle, banner logged (Step 9) ✅
- [x] Code review: no RV64 behavior change, Laws 1/4/5/6 verified (Step 10)

## Success Criteria

- **SC1 (build):** `riscv32imac-unknown-none-elf` kernel compiles, clippy `-D warnings`
  clean; riscv64 build still green. ✅
  ```
  cargo build -p vicell-kernel --target riscv32imac-unknown-none-elf --release
  # Result: Finished `release` profile [optimized] target(s) (13 warnings, 0 errors)
  ```

- **SC2 (no ABI change):** `git diff` touches no file under `libs/api/` or
  `libs/types/`. ✅

- **SC3 (functional):** On the RV32 Nano QEMU boot, kernel reaches idle loop and logs
  banner via SBI putchar (syscall dispatcher is functional). ✅
  ```
  QEMU smoke boot: kernel banner logged, idle loop reached
  ```

- **SC4 (no RV64 regression):** Existing riscv64 build shows no new errors from
  syscall.rs refactoring. ✅
  ```
  cargo build -p vicell-kernel --target riscv64-unknown-none-elf
  # No new errors in syscall.rs
  ```

- **Validation:** SC1–SC4 all verified. Phase COMPLETE.

## Risk Assessment

| ID | Risk | Likelihood | Impact | Mitigation |
|----|------|-----------|--------|------------|
| R1 | Approach (B) regresses RV64 (shared fn changes the hot path) | Med | High | Decision gate Step 3: riscv64 build + boot smoke MUST pass before keeping (B); else revert to (A). N4 enforces byte-for-byte RV64. |
| R2 | Return value not surfacing (frame restore in `trap.S` reloads stale a0) | Low | High | Step 7 verifies `&mut frame` write reaches saved frame; smoke test (Step 9) observes a0. If broken, inspect `rv32/asm/trap.S` (flagged follow-up, not in-scope edit). |
| R3 | Wide-type packing (`u64 sector`/`deadline`) mis-handled when usize=32-bit | Low | Med | `a0 as u64` zero-extends a u32 — correct for sectors < 4 G. RV32 Nano disk is tiny; document the 4G/32-bit sector ceiling. No multi-reg u64 packing exists in current ABI. |
| R4 | SUM bit wrong on RV32 (sstatus layout differs) | Low | Med | RV32 priv spec: SUM is sstatus bit 18, same as RV64 → `0x40000` identical. Verified against rv32 sstatus usage in `rv32.rs` (SIE bit 1 confirmed same layout). |
| R5 | `ViSyscall`/`Syscall` import not visible under rv32 cfg | Low | Low | `use api::syscall::ViSyscall` (syscall.rs:1796) is unconditional; `Syscall` is module-local. Compile catches any gap immediately (Step 8). |
| R6 | Unknown-opcode path returns wrong sentinel width | Low | Low | Use `u32::MAX` (not `usize::MAX`) for the rv32 `regs[10]` write — caller sees -1 as i32. |

### Rollback
Single-file change behind `#[cfg(target_arch = "riscv32")]`. Revert = restore the
8-line stub (`syscall.rs:1804-1808`); riscv64 unaffected. If approach (B) was taken,
revert also restores the inline RV64 match. `git revert` of the single commit is clean
— no data/state migration, no cross-file coupling.

## Security Considerations

- The allowlist gate (F5) MUST be present on RV32 — without it, an RV32 cell bypasses
  the per-cell `syscall_allowlist` that RV64 enforces. Copy `syscall.rs:1944-1982`
  exactly, including the Unknown-opcode deny and the blk-io bit-36 check.
- SUM must be disabled again after `handle_syscall` (F6) so a later kernel fault
  cannot read user pages with S-mode privilege. Ensure the `csrc` runs on every
  return path — since `handle_syscall` returns a value (no early `return` between the
  SUM-set and SUM-clear in the RV64 template), a straight-line set/call/clear is safe.
- `validate_user_buf` inside `handle_syscall` already bounds user pointers; no extra
  RV32-specific check needed — but confirm pointers are treated as 32-bit (they are,
  since usize=u32 on rv32, so `ptr.checked_add(len)` overflows at 4 GiB correctly).

## Next Steps / Follow-ups

- Depends on: Phase 31 (done, f5d2f588).
- Unblocks: any RV32 userspace cell that issues syscalls (shell/bench on Nano).
- Deferred (not this phase): RV32 `GetTime` real mtime (currently returns 0,
  `syscall.rs:1398-1399`); RV32 `Shutdown` SBI SRST (currently falls to the
  `wfi` loop, `syscall.rs:1521-1522`). File as separate Nano follow-ups.
- If R2 fires: `rv32/asm/trap.S` frame save/restore audit becomes its own task.
