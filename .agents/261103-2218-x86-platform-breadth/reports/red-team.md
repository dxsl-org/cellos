# Red team — 2026-11-03

The reviewer subagents could not run (`No model selected` / usage limit), so the session model ran the four lenses itself (Security Adversary, Assumption Destroyer, Failure Mode Analyst, Dependency Trap Hunter) and checked claims against the repo. All findings were classified **Accept → mitigate in plan**. None required a scope or product trade-off, so they were applied without a user checkpoint; the owner may reclassify any of them.

| # | Lens | Sev | Finding | Evidence | Resolution |
|---|---|---|---|---|---|
| 1 | Security | Major | The MSI-X table carve-out does not stop a cell from making its device DMA into the MSI window (`0xFEE0_0000`), which spoofs interrupts without IR | phase 04a §Security | 04a states the limit; phase 12 makes IR mandatory under `Required` and rewords HCL R8 |
| 2 | Failure | Major | Demoting TSC after an AP skew check could make `now_ns()` go backwards or jump | phase 02 §Requirements (sync check) | Monotonic source handoff + unit test added to phase 02 |
| 3 | Dependency | Major | Phase 10 routes SPCR/COM2 IRQs through `route_gsi` from phase 03 but depended only on 01 | phase 10 frontmatter | Dependency `[1, 3]`; plan graph updated |
| 4 | Assumption | Major | x86 `MAX_HARTS = 64` would leave dual-socket Xeons (>64 threads) partly offline | phase 07a §Requirements | Target 256, 64 only if the size audit forces it |
| 5 | Assumption | Minor | `MAX_IRQ = 64` (`kernel/src/task/drivers/irq_wait.rs:26`) is smaller than the MSI vector range | phase 04a §Key Insights | Grow to 256 in 04a (u8 namespace, no ABI change) |
| 6 | Failure | Minor | Orderly shutdown could hang on a stuck cell, and the power button would then do nothing | phase 06 §Requirements | Bounded shutdown (30 s) + second-press force |
| 7 | Dependency | Minor | Phase 20 captures reboot/poweroff but did not depend on phase 06; the graph also lacked 02 → 07b | phase 20 frontmatter, plan graph | Dependencies added |

Verified while red-teaming (moved from Assumptions to facts):
- `acpi::parse` runs before heap init (`kernel/src/main.rs:435` vs `:734`).
- virtio `Transport::queue_set` has no vector argument (`kernel/third_party/virtio_drivers/src/transport/mod.rs:45`).
- QEMU 8.2.2 q35 has `hpet=<bool>`.
- `kernel-irqchip` is set on `-accel kvm` in the lane text.

Rejected: none. Deferred: none.

Consistency sweep: dependency numbers valid (00-20, 04a/04b, 07a/07b); renamed lane flags updated in phase 12; no stale file paths found. Consistency: OK.
