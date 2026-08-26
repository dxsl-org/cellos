# Phase 03 — Residual hardening (C1 IRQ cap + virtqueue fuzz + bounds-check verify)

- **Track:** A (finish Tier 3b) · **Label:** **coding** — fully QEMU-TCG validatable · **Tier:** thinking · **Effort:** L (~1-1.5K LOC)

## Context Links
- Folds in `.agents/260712-0952` P06 (virtqueue fuzz + `process_notify` backend refactor) and the LIVE C1 bug.
- Scout: [scout-report.md](scout-report.md) — **C1 is still live at `registry.rs:513`** (`q.push_back(intid)`, no cap).

## Overview
- **Priority:** P2 · **Status:** pending (C1 sub-item IN PROGRESS via a separate implementor)
- Land the safety items both tracks assume: cap the guest-triggerable IRQ queue (**C1 — the coalescing-bitset fix is being implemented in parallel by a separate implementor; this phase only owns the fuzz + bounds-verify residue and integrates the landed C1 fix**), refactor the virtqueue backend so it is fuzzable, and **functionally verify** the guest-memory bounds-check (do not trust prior line-number claims).

## Key Insights
- **C1 is a live guest→kernel-OOM DoS**, not a planned nicety. Guest masks an IRQ + spams `QueueNotify` → `inject_irq` `push_back` grows an unbounded kernel `Vec` → SAS-wide OOM. Mitigation (per Mythos dossier-6): pending-IRQ = **bounded coalescing bitset** (1 bit/INTID, idempotent inject) — strictly correct (no legit IRQ dropped, redundant re-injections collapse) and smaller than a ring.
- Write the invariant into `docs/specs/05`: "bound every guest-triggered kernel queue; coalesce where semantics allow." IRQ set is its first instance; pair with `cur<q_size` + `avail_idx` delta bounds.
- `process_notify` currently calls syscall wrappers directly (`virtqueue.rs`) → host returns `usize::MAX` → 0 fuzz coverage. Refactor to a memory-backend seam (precedent `loader_image.rs:68`) so production AND fuzzer share one parser.

## Requirements
- **Functional:** IRQ spam cannot grow kernel memory unboundedly; virtqueue parser survives a fuzz corpus; bounds-check rejects out-of-range GPA.
- **Non-functional:** production runs through the SAME bounds-checked parser as the fuzzer.

## Architecture
`inject_irq` replaces `VecDeque<u32>` per vCPU with a fixed pending-INTID bitset; `run_vcpu` drains set bits into GICH LRs. Virtqueue backend gains a trait seam: `MemBackend` (real = guest-mem syscall; fuzz = in-memory buffer). `cur<q_size` + `avail_idx` delta clamps in the parser.

## Related Code Files
- **Modify:** `kernel/src/hypervisor/registry.rs` (`inject_irq` bitset, ~line 506-523; drain in `run_vcpu`).
- **Modify:** `cells/services/hypervisor/src/virtqueue.rs` (backend seam + clamps), `virtio_mmio.rs` (`cur<q_size`).
- **Add:** fuzz harness (host target) exercising the parser; `docs/specs/05` invariant text.
- **Verify:** guest-mem bounds path in `write_guest_memory`/`read_guest_memory` (write a fault-injection test; confirm actual guard location — scout notes `registry.rs:311-317` is the exit-conversion path, not the guard).

## Implementation Steps
1. **C1 (IN PROGRESS elsewhere):** replace per-vCPU IRQ `VecDeque` with a coalescing bitset; idempotent inject; drain in `run_vcpu`. This phase integrates the landed fix and adds the IRQ-spam regression test — it does NOT re-implement C1.
2. Virtqueue `MemBackend` seam; production + fuzzer share parser; add `cur<q_size` + `avail_idx` delta clamp. **This `MemBackend` validator is reused by P07 restore (M5) — design it as the single validation entry point for both live-notify and restore paths.**
3. Fuzz harness + seed corpus (malformed descriptor rings).
4. Fault-injection test: out-of-range GPA rejected; IRQ-spam test: kernel memory bounded.
5. Document the "bound every guest-triggered queue" invariant in `docs/specs/05`.

## Todo
- [ ] C1: coalescing IRQ bitset + idempotent inject + drain
- [ ] virtqueue backend seam + `cur<q_size`/`avail_idx` clamp
- [ ] fuzz harness + corpus
- [ ] bounds-check fault-injection test (verify, don't assume)
- [ ] specs/05 invariant text

## Success Criteria
- QEMU-TCG: IRQ-spam test shows bounded kernel memory; fuzz harness runs clean over corpus; bounds-check test rejects OOB GPA. Alpine + glibc lanes still boot.

## Risk Assessment
- **Med:** coalescing bitset changes IRQ delivery semantics (level vs edge). Mitigation: idempotent re-inject preserves level semantics; test against Alpine timer/virtio IRQs.
- **Low:** backend seam perturbs hot path. Mitigation: seam is a monomorphized trait, zero-cost.

## Security Considerations
- Closes a live SAS-wide DoS (C1). This is a **hard prerequisite for Track B**: a malicious CoW clone would otherwise weaponize the same path across every cloned guest.
- No Law 1 change (kernel-internal + cell-internal); no ABI touch.

## Next Steps
- Track B P04-P08 assume C1 is capped and the backend seam exists. Gate Track-B design baseline on this phase.
