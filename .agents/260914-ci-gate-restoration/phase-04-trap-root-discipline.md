# Phase 04 — Restore the kernel root across the trap boundary

**Status**: implemented and measured on qemu (riscv64) — the fault window is closed on the
phase-03 instrument, and the hosted job's own suite is green 10/10 consecutive runs
**Ceiling**: qemu (riscv64), then the hosted CI gate
**Evidence for the defect**: [phase-03-console-fault.md](phase-03-console-fault.md) — the panic fires
with `satp` naming a cell root (ASID 1) instead of the kernel's (ASID 0) while S-mode code loads the
UART's line-status register.

## The invariant established

> S-mode kernel code always executes with `satp == KERNEL_SATP` (ASID 0). A private domain root is
> installed only while the Cell's own code is executing.

Both halves now hold: trap entry installs the kernel root, and every plan that leaves a private root
live re-programs it before that context resumes.

## What was implemented

### A. Trap entry installs the kernel root — `hal/arch/riscv/src/rv64/asm/trap.S`

After the frame is saved and before `call vi_trap_handler`: read `satp`, park it in the frame, and —
unless the live value already equals the recorded kernel root — `fence rw, rw`, `csrw satp`,
`sfence.vma zero, zero`.

The kernel root is read from a fixed-name symbol, `VI_KERNEL_SATP`
(`hal/arch/riscv/src/rv64/domain.rs`, written by `record_kernel_satp` from
`PageTable::activate`), because trap entry runs before any Rust. A zero value means "not recorded
yet" and keeps the live root, which is the pre-fix behaviour; the guard is the value comparison, not
a `cfg` or an ASID test, so configurations without private domains stay on one assembly path. (The
ASID form is also sound — `AsidLease::acquire` never leases 0 — but the comparison additionally
covers any future non-ASID-tagged root.)

### B. The interrupted root lives in the trap frame — RV64 only

`ViTrapFrame` gains `satp` plus one padding word on `target_arch = "riscv64"`: 304 bytes, i.e.
19 × 16, so the frame base stays 16-byte aligned for the handler call. The non-RV64 size assertion is
untouched at 36 words, so x86_64 and aarch64 layouts are unchanged (their own asm keeps its hardcoded
288).

Why the frame rather than the options in the previous draft:

| candidate | verdict |
|---|---|
| per-hart slot (asm-owned) | **unsound** — a handler that yields lets another trap overwrite the slot; the draft already said so |
| per-task field | needs the value to reach Rust before the root changes; the frame already is per-trap storage, so the task field would only mirror it |
| frame slot (chosen) | one writer (entry), one reader (`__trap_exit`), no cross-arch layout change, and it survives a mid-handler yield because it belongs to the frame, not the hart |

`__trap_exit` restores it only when the live `satp` differs from the frame's value:

- trap returned straight from a Cell trap → live is the kernel root, frame value is the Cell's root → restore (`sfence.vma zero, asid-of-target`);
- handler yielded and the task resumed → the switch plan already programmed that root, so the values compare equal and nothing is written;
- fresh task entering user mode through `__trap_exit` (`kernel/src/task.rs::prime_user_mode_entry`) → the frame carries 0, the scheduler already installed the root, nothing is written.

The fault record now names all three roots (`satp`, `kernel_satp`, `interrupted_satp`), and the
test-hook snapshot walks the page tables of the interrupted root — the one the faulting access ran
under.

### C. Same-domain resume must program the root — `kernel/src/task/domain_switch.rs`

`DomainTransition::SameDomain` now carries its `DomainRef` and returns `(root_ppn, asid)`, counting
one activation. `(0, 0)` is left to exactly one transition: `SasToSas`, where the kernel root is
already live. Without this, change A would resume a Cell under the kernel root with the kernel's
mappings visible to its S-mode code — strictly worse than the fault being fixed.

`is_sas_fast_path()` became `writes_no_root()`, and `S22-RV64-RESUME-ROOT` (new, in
`kernel/src/task/domain_switch_tests.rs`) requires: not a no-write plan, the same root this domain
programmed on activation, both counters advanced by exactly one, and the published domain identity
untouched. `scripts/qemu-native-domain-test.sh` gained a matching `resume-root` case so the lane's
own runner fails if that invariant regresses.

## One interaction found while implementing

Private-domain user copies already branch on the resident root
(`kernel/src/task/user_copy/copy.rs::PinnedCopy::commit`): direct VA copy when the Cell's root is
live, otherwise a PTE walk plus `phys_to_virt` alias copy. With the entry switch, a private domain
takes the alias path — the stricter one, which re-walks the Cell's Sv39 tables and cross-checks the
ledger under the reader pin. Sas cells keep the direct-VA path because their root is the kernel root.
`validate_kernel_range` also now walks the kernel root, which refuses user addresses passed as kernel
buffers instead of accepting them because a domain root happens to map them.

## Evidence (2026-09-15, this workstation, kernel `cellos-kernel-srv-test`, QEMU 10.2)

| # | check | command | result |
|---|---|---|---|
| 1 | the fault itself | `/tmp/rv64-root-accept.py` (phase-03 instrument, acceptance form): 10 consecutive boots of `build/disk_srv.img`, `posix-shim-test` typed at the shell | **10/10 clean** — every boot reached `[posix-shim] POSIX-RENAME: OK`, zero `[KERNEL PANIC]` / `Kernel exception` lines, serial transcript 12035 bytes on all ten boots |
| 2 | the hosted job's own suite | `cd tests/integration && cargo test --test srv-cellosfs` ×10 | **10/10**: `ok. 3 passed; 0 failed` (S1–S6 CellosFS, no-disk degrade, two-boot persistence), zero panic lines |
| 3 | RV64 domain regressions, one hart | `scripts/qemu-native-domain-test.sh --harts 1 --case switch,resume-root,sas-fastpath,user-copy,ipc-copy,admission,rollback,grant-revoke` | **suite PASS**; every marker green, including `S22-RV64-SWITCH`, `S22-RV64-RESUME-ROOT`, `S22-RV64-SAS-FASTPATH roots=0 flushes=0`, `S22-RV64-PLAN`, `S22-RV64-PIN-DYING`, `S22-RV64-COPY`, `S22-RV64-IPC-{COPY,SCATTER,NO-PEER-MAP}`, `S22-RV64-GRANT-REVOKE`, `S22-RV64-ADMISSION-{DENY,DRAIN}`, `S22-RV64-DYING-NONSCHEDULABLE`, `S22-RV64-ASID-REUSE`, `S22-RV64-ASPACE` |
| 4 | RV64 domain regressions, two harts (the interleaving cases) | `scripts/qemu-native-domain-test.sh --harts 2 --case switch,resume-root,migration,user-copy-race,ipc-copy-race` | **suite PASS** — `S22-RV64-RESUME-ROOT: PASS harts=2` in every run, plus `SWITCH`, `PLAN`, `PIN-DYING`, `MIGRATION`, `COPY`, `COPY-RACE`, `IPC-COPY`, `IPC-COPY-RACE`, `IPC-SCATTER`, `IPC-NO-PEER-MAP`, all at `harts=2`, no `FAIL` and no panic in any log |
| 5 | negative isolation after a trap round-trip | inside the same suite: `S22-RV64-COPY` (`kernel/src/task/user_copy_tests.rs`) requires a kernel-range-style pointer inside the canonical user half to be rejected with the destination untouched | PASS — and the handler now takes the stricter branch: the probe/commit walk the Cell's Sv39 tables and cross-check the mapping ledger instead of trusting the resident root |
| 6 | cross-arch containment | `cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc` and the same for `aarch64-unknown-none-softfloat` | both kernels build; the RV64-only field is not compiled there and their 36-word frame assertion still holds |

Raw serial transcripts: `/tmp/rv64-root-accept/serial-{1..10}.log`; domain-run logs:
`.logs/native-domain-qemu/h1-*` and `h2-*` (gitignored).

The two commands that reproduce the acceptance:

```bash
bash scripts/build-srv-test-ci.sh && bash scripts/mksrv-img.sh build/disk_srv.img
N=10 python3 /tmp/rv64-root-accept.py          # 10 consecutive boots, zero kernel exceptions
cd tests/integration && cargo test --test srv-cellosfs
```

## A runner defect found while measuring — not caused by this change

`scripts/qemu-native-domain-test.sh` classifies every `[fault] Cell` line against an allowlist, and it
pinned the `SMP-FAULT-RETIREMENT` canary to its literal `generation 99`. Running the suite at
`--harts 2` after this work produced the same line with `generation 133` and the runner aborted with
`FAIL: unclassified cell fault` before the remaining cases ran.

Attribution was measured, not assumed: with the change **stashed** (`git stash push` over exactly the
files in this phase), the unmodified kernel rebuilt and produced the **same `generation 133`** line on
this workstation, failing the same check. So the pin — not the fix — is the defect.

The number is `NEXT_DOMAIN` (`kernel/src/memory/address_space.rs::build`), i.e. how many private
address spaces had been built when the record was published, which depends on how far the concurrent
campaign had progressed in that boot; the phase-07 captures read 99, this workstation reads 133 today.
The allowlist now matches that canary by cell, task, injected cause (`0xdead`) and null `pc`/`addr`,
with the measurement in the comment, so a correct kernel cannot be failed by a timing artifact.

## Limits

- Software evidence on qemu/TCG, local and (pending the next push) hosted — not a board or production
  claim.
- No x86_64/aarch64 boot smoke was run here: the change is `cfg(target_arch = "riscv64")`, those two
  kernels build, and their frame layout is asserted unchanged. The doc's §B "option A/B needs a
  cross-arch review" does not apply to a cfg'd field, but a reviewer should confirm that reading.
- The fault was intermittent (the same kernel passed the suite before panicking on the next boot), so
  10 clean boots is evidence about this kernel, not a probability statement. The mechanism is what
  closes the window: entry no longer leaves a Cell root installed while kernel code touches MMIO.

## Scope notes (kept from the design)

- The `native-domains` feature gate stays.
- The domain substrate lane (`260823-phase07-rv64-domain-qemu`) still owns the ABI; this phase added
  no cross-arch ABI change and no new root-write path outside the two boundaries above.
- Mapping kernel MMIO into domain roots remains disallowed: cells run in S-mode under that root.
