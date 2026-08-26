# Scout Report — W^X Cross-Hart TLB Shootdown

## Relevant Files

- `kernel/src/loader/wx.rs:21` — W^X ordering contract; current text says another hart can cache writable PTEs because no shootdown exists.
- `kernel/src/task.rs:939` — relocation and `wx::enforce` run before task registration.
- `kernel/src/memory/page_protect.rs:11` — `protect_page` promises only caller-hart invalidation and names the IPI shootdown gap.
- `hal/arch/riscv/src/rv64/paging.rs:7` — RV64 local `sfence.vma` per VA.
- `hal/arch/riscv/src/common/sbi.rs:139` — SBI IPI exists; SBI RFENCE does not yet exist in the wrapper.
- `kernel/src/task/scheduler.rs:195` — RV64 cross-hart preemption currently sends SBI IPI to HART_RT.
- `hal/arch/riscv/src/rv64/trap.rs:72` — SSIP receive path clears and runs scheduler tick only; no payload/ack.
- `kernel/src/task/smp.rs:38` — SMP secondary bring-up is RV64-only; non-RV `start_secondaries()` is a no-op at `:104`.
- `hal/arch/arm/src/aarch64/paging.rs:63` — AArch64 already performs inner-shareable TLBI broadcast with barriers and optional EL2 leg.
- `hal/arch/x86/src/x86_64/paging.rs:166` — x86_64 `invlpg` is local-only and documents the missing IPI.
- `hal/arch/x86/src/x86_64/apic.rs:90` — APIC driver covers LAPIC timer/EOI/IOAPIC redirect, not ICR send.

## Patterns

- Keep bare-metal tests as boot/runtime lanes; `#[cfg(test)]` is not enough for kernel invariants.
- Fail closed on security primitives: W^X spawn errors kill/refuse the cell instead of logging and continuing.
- Use WSL-native git/grep/build commands in `/home/dmin/cellos`; repo-root `docs/coding.md` and `docs/engineering-standards.md` were not present.

## Precedents

- `8f9e3a16 feat(kernel): enforce W^X and signed cell admission` touched W^X, page protection, and all three HAL paging legs.
- `d078c1a0 feat(kernel): revoke WRITE on cell pages once relocation finishes` introduced the original W^X mechanism.
- `2d7d40fc feat(smp): Phase 32 P04 — RT hart pinning + cross-hart IPI + WaitForEvent(217)` added RV64 IPI/scheduler precedent.
- `e15af924 feat(smp): Phase 32 P01+P02 — SBI HSM hart boot + per-hart ViHartLocal via tp` added RV64 secondary-hart groundwork.

## Prior Failures

- Phase 10 originally shipped runtime-unverified; `qemu-build-unblock-260731.md` proves QEMU/toolchain were available with setup fixes.
- `wave1-critical-fixes-260730.md` records that compile/disassembly cannot substitute for runtime SMP proof.
- No `.agents/failure-history.jsonl` match was found or relied on.

## Blast Radius

- Production code: `kernel/src/loader/wx.rs`, `kernel/src/memory/page_protect.rs`, `hal/arch/riscv/src/common/sbi.rs`, `kernel/src/task/smp.rs`.
- Evidence/tests: `tests/integration/tests/wx-text-write.rs`, `cells/tests/wx-test/**`, QEMU wrapper scripts if `-smp 2` needs wiring.
- Docs/comments: `docs/specs/02-memory.md`, `docs/specs/19-hardware-isolation-layers.md`.
- Public contracts: no syscall numbers, ABI structs, manifest fields, feature flags, or user commands may change.

## Inconsistencies To Note, Not Fix Here

- AArch64 current code is stronger than the generic "no cross-hart shootdown" wording for stage-1 W^X because it uses TLBI broadcast.
- x86_64 has no current SMP/IPI send path; closing that would require a separate interrupt-controller/SMP project.
- `HART_ONLINE[0]` is not set; online-mask logic must treat hart 0 as implicit, not read it from the array.
