# Roadmap Dependency Scout Report

**Status:** Reconciled to the red-teamed plan on 2026-08-27. `../plan.md` and its phase files are authoritative where this discovery report records an earlier candidate scope.

## Finding

G1–G5 are product-stage overlays, but current execution is often read as a serial dependency. The authoritative roadmap already separates QEMU regression evidence from physical qualification (`docs/project-roadmap.md:91-107`) and parks G3 until hardware/vendor evidence exists (`docs/research/g3-accelerator-evidence.md:4-15`). The safe change is scheduling, not gate removal.

## Capability Map

| Lane | Action available now | Canonical classification |
|---|---|---|
| Desktop/ViUI/SDK | Record the existing host/RV64 QEMU ceiling; define no code until an approved child names a missing behavior | `execution_class=scope-gated`; `evidence_ceiling=qemu` |
| Local Cell-to-Cell | Adjudicate the newer direct-only code against the pending relay-first recovery plan | `execution_class=contract-gated`; `evidence_ceiling=host`; leases/relay deferred |
| Kernel security | Prepare separately approved ELF-signature, pointer-validation, and entropy children | `execution_class=governance-gated`; `evidence_ceiling=host` |
| RPi3 HDMI | Implement ABI, cache/pin lifecycle, framebuffer authority, and driver checks | `execution_class=ready`; `evidence_ceiling=host`; physical mailbox/coherency/visual gate remains |
| Tier 3 hostile evidence | Run QEMU hostile VirtIO, bounds, reset, budget, and supervisor recovery | `execution_class=ready`; `evidence_ceiling=qemu` |
| Tier 3 ARM64 persistent storage | Replace volatile heap disk with a bounded persistent QEMU backend | `execution_class=ready`; `evidence_ceiling=host`; physical qualification remains `external-gated` |
| Tier 3 x86 VirtIO parity | Pin and wire the shared block/network device personality after the backend contract lands | `execution_class=scope-gated`; `evidence_ceiling=host`; physical qualification remains `external-gated` |
| Admission evidence | Inventory runner trust and seek evidence-class approval | `execution_class=governance-gated`; `evidence_ceiling=host` |
| G3 accelerator | Procurement/license preparation only | `execution_class=external-gated`; `evidence_ceiling=contract` |
| Protected relay | Existing software harness maintenance only | `execution_class=external-gated`; `evidence_ceiling=host` |

## Existing Evidence

- Product stages are overlays with distinct goals: `docs/project-roadmap.md:25-37`.
- Hardware evidence is explicitly separate from QEMU/compile: `docs/roadmap/hardware-tracks.md:17-22`.
- Desktop window policy already has real QEMU input/scanout coverage but is not a shell: `docs/project-roadmap.md:68-89`.
- SDK delivery has independent completed ViUI, VFS, and compositor slices: `.agents/260825-sdk-delivery/plan.md:5-14`.
- Local net-broker starts an authenticated beacon runtime, while gossip/enrollment/routing modules remain disconnected; the competing recovery plan is relay-first and explicitly defers distributed leases. Contract adjudication precedes code: `cells/services/net-broker/src/main.rs:99-146`, `.agents/260819-1409-cell-to-cell-anywhere-core/plan.md:18,30-31,47`.
- App-tier production admission remains blocked by floor, evidence retention, anchors, physical hostile cases, and approvals: `.agents/260821-0642-app-tiers-completion/plan.md:30-35,46-52`.
- G4 feasibility is non-promotional and blocked by `PAL-019`/`PAL-031`, approvals, and Phase 03: `docs/roadmap/runtime-and-platform-tracks.md:60-89`.
- RPi3 HDMI has a complete software design with a physical-only final gate: `.agents/260823-rpi3-hardware-completion/phase-04-hdmi-framebuffer.md:98-128`.
- Tier 3 permits QEMU-first hostile/fuzz/reset work but requires physical qualification. Persistent disk/VFS scale and x86 VirtIO block/network personality wiring are confirmed in-repo software gaps, now owned by Phase 09 rather than parked externally: `.agents/260821-0642-app-tiers-completion/phase-04-tier3-qualification.md:12-33`, `.agents/TODO.md:64-75`.
- G3 forbids an accelerator ABI or placeholder probe before hardware: `docs/research/g3-accelerator-evidence.md:14-15,121-126`.

## Selected Ordering

1. Make the evidence ladder and lane status explicit.
2. Open ready implementation lanes and process scope, contract, and governance gates independently.
3. Close each executable slice at its truthful software evidence ceiling.
4. Project each lane transition immediately; never wait for unrelated lanes.

## Rejected Alternatives

- Strict G1→G2→G3: leaves implemented/QEMU-testable work idle behind procurement.
- Declare QEMU equivalent to hardware: produces false security and qualification claims.
- Build generic NPU/security abstractions now: freezes contracts without vendor or physical evidence.
- Keep expanding only documentation/harnesses: low value after existing predesign packages; prioritize executable product and correctness paths.
