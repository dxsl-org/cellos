# Validation Log

## 2026-11-03

### Verification Results

- **Tier:** Full (23 phase files: Fact Checker, Flow Tracer, Scope Auditor, Contract Verifier)
- **Claims checked:** 245
  - 194 path references: 160 existing, 28 correctly marked as files to create, 6 bare-name matches that pointed at the wrong file and were rechecked by hand.
  - 37 line-content spot checks.
  - 5 behaviour traces.
  - 5 consumer enumerations.
  - 4 state-lifetime checks.
- **Verified:** 239 | **Failed:** 4 | **Unverified:** 2 (AcpiInfo stack size, phase 01; Limine x2APIC flag semantics, phase 03; both already listed as phase assumptions)

#### Failures (all resolved by owner decision, see Q1-Q4)

1. **[Flow Tracer] Phase 02.** The claim "cpu_accounting only needs monotonic ns" is false. The journal admission records store the HPET identity tuple: `period_fs`, `base`, `capabilities`, `configuration` (`kernel/src/task/cpu_accounting.rs:258-268`).
2. **[Flow Tracer] Phases 11 and 14.** Multiple identical devices cannot be claimed. `find_class`/`find_vendor_device` return the first match, and `offer_pcie_device` returns 0 when a live driver already owns it (`kernel/src/task/syscall.rs:2428-2470,8722-8743`).
3. **[Contract Verifier] Phase 07a.** These `MAX_HARTS`/`MAX_CPUS` consumers were missing from the plan:
   - `task/scheduler.rs:265,1533,2002,3010`
   - `task/quiesce.rs`
   - `memory/deferred_release.rs:70,297-307`
   - `task/telemetry.rs:35-557` (internal only, no wire exposure)
   - `hal/arch/x86/src/x86_64.rs:255`, `syscall.rs:78`
   - `apic.rs:17` (`assert!(MAX_CPUS == 2)`)
   - the cell `cells/apps/c2c-render/src/main.rs:462-466`, which caps processor IDs at 2
4. **[Fact Checker] Phase 09 and scout report.** The VMware NIC/SCSI-disabled settings are at `scripts/x86/vmware-boot-test.py:109-113`, not in `test-native-vmware-matrix.py`.

#### Verified highlights

- The timer gate also gates UART RX IRQ (`kernel/src/main.rs:820-823`).
- A non-`Required` profile needs the hypervisor bit (`kernel/src/board.rs:279`), so `x86_64-vm` inherits the ADR-0029 rule with no new check.
- `acpi::parse` runs before heap init (`main.rs:435` vs `:734`).
- virtio `Transport::queue_set` takes no vector argument (`transport/mod.rs:45`).
- `MAX_IRQ = 64` (`irq_wait.rs:26`).
- QEMU 8.2.2 provides `hpet=<bool>`, `e1000e`, `vmxnet3`, `pvscsi`, `intel-iommu`, `virtio-iommu-pci`, and `usb-hub`.
- Two virtio cells with distinct device IDs do not race (exact-ID lookup + BDF claim).

### Questions & Answers

1. **[Assumptions]** CPU accounting journal is bound to HPET identity; what happens without HPET?
   - Options: Version the journal (Recommended) | Accounting only with HPET | Always require HPET
   - **Answer:** Version the journal.
   - **Impact:** Phase 02 adds journal v2 with a source-tagged identity and a v1 reader. Phase 00 adds a durable-charge review. The plan's rollback now names this as an irreversible effect.
2. **[Architecture]** How does a cell get the second identical device?
   - Options: Kernel skips owned devices (Recommended) | New indexed syscall | First device only
   - **Answer:** Kernel skips owned devices.
   - **Impact:** 418/424 semantics change (no wire change), listed in the phase 00 CP1 items and implemented in phase 04a. Phases 11 and 14 rely on it.
3. **[Scope]** c2c-render caps processor IDs at 2.
   - Options: Raise bound in 07b (Recommended) | Keep smoke at `-smp 2`
   - **Answer:** Raise bound in 07b.
   - **Impact:** Phase 07b requirements and files now include the cell and its smoke scripts.
4. **[Tradeoffs]** D-VM: may a VM without a vIOMMU run DMA drivers?
   - Options: Add `x86_64-vm` profile (Recommended) | vIOMMU only
   - **Answer:** Add `x86_64-vm`.
   - **Impact:** Phase 00 records the decision; phase 05 is no longer conditional.
5. **[Architecture]** Source of `SLP_TYP` for poweroff.
   - Options: Static `\_S5_` bytecode scan (Recommended) | AML interpreter crate
   - **Answer:** Static scan. Phase 06 unchanged.
6. **[Risks]** When to enable x2APIC.
   - Options: Whenever the CPU supports it (Recommended) | Only when forced
   - **Answer:** Whenever supported. Phase 03 unchanged.
7. **[Scope]** First physical machine.
   - Options: Intel mini PC I219/I226 (Recommended) | Intel server | Both in parallel
   - **Answer:** Intel mini PC.
   - **Impact:** Phase 20 names phase 13 or 15 as the NIC prerequisite.

### Decisions

- Clock identity: journal v2 with a source tag, still reading v1. The owner's "accounting only with HPET" option is the documented fallback if the review rejects v2.
- Device lookup: skip devices owned by live drivers. No new syscall.
- c2c-render: raised to N CPUs in 07b.
- VM DMA policy: `x86_64-vm` profile (Optional, explicit cmdline, hypervisor only); production uses a vIOMMU.
- Poweroff: static `\_S5_` scan.
- x2APIC: on whenever supported.
- First machine: Intel mini PC.

### Phase Updates

- Phase 00:
  - The ABI items now include the 418/424 semantic change.
  - A durable-charge review is added.
  - D-VM is recorded as decided.
  - `cpu_accounting.rs`, `syscall.rs` and `telemetry.rs` are added to the handoff file set.
- Phase 02: journal v2 requirement, files and tests; the failed assumption is recorded.
- Phase 04a: skip-owned lookup requirement and test.
- Phase 05: the `x86_64-vm` profile is no longer conditional.
- Phase 07a: full consumer lists.
- Phase 07b: c2c-render requirement and files.
- Phase 09: corrected citations (also corrected in the scout report).
- Phase 11: multi-controller discovery uses skip-owned lookup.
- Phase 14: multi-port uses skip-owned lookup.
- Phase 20: first-machine decision.
- plan.md: rollback names the journal v2 irreversible effect.

No answer rejected a previously planned approach outright: the phase 02 assumption was wrong, but the approach (multi-source clock) stands. No `.agents/failure-history.jsonl` entry is warranted.

### Consistency sweep

- Dependency numbers are valid.
- The new references (`cpu_accounting.rs:258-268`, `syscall.rs:2428-2470,8722-8743`, `c2c-render/src/main.rs:462-466`, `vmware-boot-test.py:109-113`) were re-read and match.
- `x86_64-vm` is named the same way in phases 00 and 05.

Consistency: OK.

## 2026-11-04 addendum (hc-cook phase 00 recon)

Failure 1 was itself scoped wrongly. `mod cpu_accounting` is `#[cfg(cellos_g1_cpu)]` (`kernel/src/task.rs:104`). The G1 lane already asserts HPET (`kernel/src/main.rs:604-649`), and the "journal" is an in-memory capture printed as `G1-*` log lines (`cpu_accounting.rs:1410-1445`), not a persisted durable-charge record. Q1's "journal v2" answer is therefore withdrawn; the owner is asked to confirm this at the phase 00 sign-off. Phase 02 now leaves the G1 lane on HPET, and the durable-charge review and irreversible-effect note are removed.
