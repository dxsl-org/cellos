---
phase: 3
title: "VF2 UART Root-Stream Boot"
status: blocked
priority: P1
dependencies: [2]
tier: thinking
---

# Phase 3: VF2 UART Root-Stream Boot

## Context Links

- [Parent plan](./plan.md) · [Phase 2 private protocol](./phase-02-private-protocol-and-dev-separation.md) · [Phase 6 integration](./phase-06-frozen-abi-kms-authority-integration.md) · [ADR-0010 root-stream manifest](../../docs/decisions/0010-use-canonical-cbor-cose-for-vf2-root-stream-manifests.md)
- [Approved entry contract](../260825-1726-kms-silo-production-root/spec.md) (`LANE-001..005`, AC-002/009/010)
- [Candidate research](../reports/research-260826-1605-phase4-dev-reference-lane.md) · [Scout report](./scout-report.md)
- Existing VF2 assumptions: `boards/starfive/visionfive-2/board.rs`, `boards/starfive/visionfive-2/starfive-visionfive-2.dts`, `scripts/vf2-build.ps1`, `scripts/vf2-flash.sh`

## Overview

Prove, on the exact VF2 v1.3B/JH7110, that immutable BootROM UART/XMODEM can load a bounded SRAM first stage at `0x08000000`, and that an STM32-controlled physical path is the only source of every later mutable byte. This is a feasibility gate: software harness results prepare experiments but never count as hardware or AC evidence.

## Key Insights

- STiRoT secures the controller, not the AP boot; authority starts only if JH7110 BootROM receives the first mutable AP stage solely from the root.
- Existing SD/Limine scripts are a separate lane and must neither build nor recover the root-stream image.
- BootROM limits, reset races, and competing USB-UART behavior are unresolved until captured on acquired hardware.

## Requirements

- After Phase 2, consume `libs/authority-protocol/` for post-boot typed operations only; do not create a second wire contract there. The BootROM/XMODEM boot-stream framing is a separate pre-runtime protocol owned and frozen by this phase — it is explicitly not part of the Phase 2 closed operation set.
- Permanently select `RGPIO_1:RGPIO_0 = 11`; authority owns AP power/reset and UART0 RX, with onboard/external competing TX sources physically removed or isolated.
- BootROM loads only the reviewed SRAM loader; the loader accepts exactly one ADR-0010 `u32be length || tagged COSE_Sign1 || component region` stream carried by a second bounded XMODEM-1K transfer. It requires canonical final-block padding and successful EOT before handoff. COSE is Ed25519-only with an embedded RFC 8949 core-deterministic CBOR payload, fixed external AAD, empty unprotected headers, one compiled key ID, and no optional or unknown fields.
- The signed manifest binds fixed offsets, lengths, load addresses, SHA-256 digests, `DEV_REFERENCE`, device/authority identity, nonzero boot epoch/request ID, approved-loader digest, entry address, and exact component-region length. Before requesting any bundle byte, immutable physically frozen staging and manifest limits define initialized usable DRAM, quarantine, and worst-case component/entry windows; quarantine and every final window must be contained in usable DRAM, mutually disjoint as required, and pre-cleared without a decoded manifest. The loader then receives only into quarantine, authenticates COSE, checks actual signed ranges inside the pre-admitted windows, completes the transfer, verifies all components, copies exact slices, and performs visible cleanup.
- The STiRoT-approved STM32 sender image/policy embeds the exact SRAM-loader bytes and manifest-verification key; the sender verifies the approved-loader digest before emitting any XMODEM byte, and that digest is persisted in the Phase 4 authority record/OpenBoot fact.
- Bundle order is exactly OpenSBI, firmware DTB, Cellos, VIFS. Builders reject overlap, integer overflow, trailing bytes, duplicate or reordered components, non-deterministic metadata, wrong lane, and any admitted size/address limit violation.
- Loader has no QSPI, SD, eMMC, USB, network, shell, recovery, or AP-measurement path; loss, corruption, replay, or timeout leaves execution sealed and reset-controlled.
- Public KMS opcodes/payloads 9–14 remain byte-for-byte unchanged.

### Stop Conditions

- Stop the lane if exact BootROM cannot load/execute a safely bounded loader, its transfer semantics cannot be made deterministic, or documented SRAM/DRAM limits cannot be frozen.
- Stop if any non-authority sender can drive UART0 RX, straps can reach a normal media boot, or any alternate medium executes after absent/corrupt/truncated input. A substituted, rolled-back, or truncated loader must fail with no execution; inability to demonstrate those negatives stops the lane.
- Stop if logic-analyzer coverage cannot distinguish reset, UART, and alternate-media activity; do not substitute host, simulator, QEMU, compile, or unit output.

## Architecture

`STM32 sender → isolated UART0 RX → immutable JH7110 BootROM/XMODEM → SRAM loader → authenticated bounded bundle → OpenSBI/DTB/Cellos/VIFS`

- `bundler` emits the exact ADR-0010 outer stream and byte-reproducible deterministic CBOR/COSE object inside the second bounded XMODEM-1K transfer; an independent host verifier parses emitted transfer bytes rather than trusting builder state.
- Shared `manifest-core` owns the closed no-allocation CBOR/COSE profile, checked arithmetic, component descriptors, and injectable immutable limits. `LogicalQuarantine::prepare` consumes only immutable staging/manifest limits and worst-case final windows, so validation and pre-clear finish before any bundle byte; actual manifest ranges are checked only after staged receive and signature verification. Loader cleanup uses evidenced uncached/device-visible accesses or an exact cache clean-to-coherency primitive plus `fence rw,rw`; compiler ordering alone is insufficient. Host limits remain `SOFTWARE_HARNESS`.
- Root-owned load switch/reset supervisor holds reset until straps and sole-sender routing are stable. AP UART TX and all AP-provided status are diagnostic only, never authorization.

### Evidence Boundary

| Evidence class | May establish | Must not claim |
|---|---|---|
| Host harness | deterministic bytes, parser/range rejection, logical zero writes and cleanup-hook order, replay state model, linker size | cache/store-buffer/DRAM visibility, BootROM behavior, electrical exclusivity, no media fetch |
| SRAM/FPGA/QEMU model | loader control-flow and logical cleanup diagnostics only | named-hardware, physical zeroization, or AC evidence |
| VF2 + STM32 + analyzer/coherency observer | actual XMODEM limits, cleanup visibility, sole sender, reset/power/media negatives | production qualification |

### Hardware Failure Matrix

| Scenario | Required physical observation |
|---|---|
| Normal cold boot | one STM32 XMODEM sender; loader then exact bundle; expected OpenSBI handoff |
| No sender / disconnected UART | no mutable execution and no alternate-media bus activity |
| Corrupt, substituted, truncated, oversized, replayed bundle | no Cellos handoff; reset remains authority-controlled |
| Substituted or rolled-back loader bytes offered by the sender, or truncated loader transfer | no XMODEM byte emitted for a digest mismatch; mid-transfer cut leaves no mutable execution and reset stays authority-controlled |
| Power/reset cut before, during XMODEM, loader auth, or component copy | restart from BootROM; never resume unauthenticated state |
| Success and post-validation software-failure cleanup | pre-validation invalid-range/profile failures perform no quarantine write; after validation, the platform clean/uncached path and `fence rw,rw` complete before a cache-bypassing or non-coherent observer reads zero throughout quarantine/scratch and handoff or reset release occurs |
| Populated/poisoned SD, QSPI, eMMC, USB, or network | no fetch or execution from that medium |
| Onboard USB-UART TX driven concurrently | isolation prevents edges from reaching AP UART0 RX |

## Assumptions

- **Claim:** Acquired VF2 v1.3B BootROM executes XMODEM payload at `0x08000000` with a usable bounded size. **Confidence:** medium. **How to verify:** run the staged size sweep and capture PC-visible marker plus UART traffic on the admitted board.
- **Claim:** Onboard USB-UART TX can be isolated without leaving another electrical sender. **Confidence:** low. **How to verify:** schematic/net continuity review, powered contention test, and analyzer capture at both sides of the isolation point.
- **Claim:** Authority can initialize DRAM before accepting the four-part bundle. **Confidence:** medium. **How to verify:** execute the smallest reviewed DRAM bring-up loader and destructive bounded memory test on hardware.

## Related Code Files

- Created software-harness workspace: `authority/vf2-root-stream/Cargo.toml`
- Created: `authority/vf2-root-stream/manifest-core/` — no_std/no-allocation deterministic CBOR/COSE codec, Ed25519 verifier/signer, range/staging validator, logical quarantine lifecycle, component verifier, and XMODEM transcript codec.
- Created: `authority/vf2-root-stream/bundler/` — strict deterministic bundler and independent verifier host binaries.
- Create after physical bounds freeze: `authority/vf2-root-stream/loader/{Cargo.toml,linker.ld,src/main.rs,src/limits.rs,src/uart.rs,src/dram.rs}`
- Create after authority hardware admission: `authority/vf2-root-stream/sender/{Cargo.toml,src/lib.rs}` and `authority/vf2-root-stream/manifests/dev-reference.toml`
- Create for real hardware evidence: `authority/vf2-root-stream/hardware/{run-gate.py,failure-matrix.toml,capture-schema.json}`
- Modify only after physical contract is frozen: `boards/starfive/visionfive-2/board.rs`. Root `Cargo.toml` workspace registration belongs solely to Phase 6 as serialized owner; hand DEV marker names (`DEV_REFERENCE`, lane tags) to the Phase 2-owned production checker instead of editing `scripts/check-production-relay-image.py` or its tests here.
- Keep unchanged: `scripts/vf2-build.ps1`, `scripts/vf2-flash.sh`, `libs/types/src/kms/{model.rs,payload/enroll.rs,payload/tls.rs}`
- Record real outputs under `.agents/260826-1605-phase4-dev-reference-authority/evidence/phase-03/`; never check in fabricated captures.

## Implementation Steps

1. Inventory exact board/BootROM/strap/UART revisions from Phase 1; approve a reversible wiring plan before powering hardware. No purchase or board modification is implied.
2. Build a marker-only SRAM loader; sweep admitted payload sizes and XMODEM timing with `python3 authority/vf2-root-stream/hardware/run-gate.py bootrom-feasibility --capture-dir <real-dir>`; freeze measured-safe limits or stop.
3. **Completed 2026-08-29 at `SOFTWARE_HARNESS` ceiling.** ADR-0010 shared manifest core, canonical bundler, and independent verifier cover exact COSE/deterministic-CBOR, identity, digest, framing, XMODEM, usable-DRAM containment for quarantine and final windows, immutable worst-case windows, post-signature actual ranges, pre-validation-no-write, pre-receive logical clear without a manifest, cleanup-hook order, and bounded host input. The no_std RV64 core checks; 28 focused host tests and an actual bundler→verifier smoke pass. This freezes no physical bound and proves no physical visibility.
4. After physical limits are frozen, implement the bounded loader and STM32 sender library over this phase's pre-runtime boot-stream framing; embed the frozen loader bytes and manifest-verification key in the sender image, verify the loader digest before the first XMODEM byte, authenticate the complete manifest before any component handoff, and reject every forbidden transport/source.
5. Install fixed straps, root-owned power/reset, and UART isolation only at the operator-approved hardware checkpoint; document continuity and component identities.
6. Run `python3 authority/vf2-root-stream/hardware/run-gate.py full-matrix --matrix authority/vf2-root-stream/hardware/failure-matrix.toml --capture-dir <real-dir>` with analyzer channels on UART RX/TX, straps, reset, power, and relevant media clocks/chip-selects.
7. Update the VF2 descriptor and DEV production-rejection inventory only from the frozen physical contract; hand hashes, captures, and unresolved failures to Phase 8.

## Todo List

- [ ] Freeze BootROM/XMODEM, SRAM, initialized usable-DRAM aperture, address, timeout, component, contained disjoint quarantine, and exact cleanup/coherency bounds from real measurements.
- [x] Produce byte-identical ADR-0010 four-part bundles and exhaustive `SOFTWARE_HARNESS` COSE/CBOR/signature/parser/range/quarantine negatives without claiming physical evidence.
- [ ] Bind exact loader bytes/key into the STiRoT-approved sender image and persist the approved-loader digest in the authority record/OpenBoot fact.
- [ ] Prove physical sole-sender/reset/strap/media behavior for every matrix row.
- [ ] Preserve DEV_REFERENCE classification and frozen KMS ABI.

## Success Criteria

- [ ] Real VF2 executes the authenticated deterministic bundle from authority UART after cold boot; analyzer traces identify every mutable byte source.
- [ ] Every negative row produces no Cellos handoff and no alternate-media execution; each result has raw capture, wiring revision, firmware/bundle hashes, and operator timestamp.
- [ ] Substituted, rolled-back, and truncated-loader negatives emit no executed mutable byte and each carries the recorded digest-mismatch outcome.
- [ ] Host-only results are labeled `SOFTWARE_HARNESS` and physical results `VF2_V1_3B_HARDWARE`; neither is promoted to production evidence.
- [x] Host tooling remains `SOFTWARE_HARNESS`/`DEV_REFERENCE`, passes 28 focused tests plus no_std RV64 check and a real bundler→verifier smoke, and exposes no default hardware limits.

## Risk Assessment

Undocumented BootROM limits, DRAM initialization complexity, UART contention, or hidden recovery behavior can invalidate the candidate. The response is a hard stop and continued parent `blocked` state—not media fallback or weaker measurement.

## Security Considerations

Trusted: immutable BootROM behavior, STM32 sender, fixed straps, root-owned power/reset, isolated UART, loader verifier. Untrusted: AP output, all media/network, VFS, supervisor, service-net, USB-UART, and analyzer workstation. Analyzer files are evidence, not authorization; secrets must not appear in captures.

## Next Steps

On pass, publish only the frozen loader/bundle/physical contract to Phase 6 and raw evidence to Phase 8. On any stop condition, keep Phases 6–8 and the parent Phase 4 blocked.

## Deviation Log

None at planning time beyond: **2026-08-26 Decision** — security and simplicity red-team reviews returned NO-GO; resolved without weakening any stop. (1) PLAN-BOOT-001: exact SRAM-loader bytes and manifest-verification key are bound into the STiRoT-approved STM32 image/policy, verified before any XMODEM byte, with the approved-loader digest persisted in the Phase 4 authority record/OpenBoot fact plus substituted/rolled-back/truncated-loader physical negatives. (2) Simplicity review: BootROM/XMODEM boot-stream framing declared a separate pre-runtime protocol owned by this phase (outside the Phase 2 closed operation set); production-checker ownership stays with Phase 2 (marker-name handoff only); root `Cargo.toml` workspace registration defers to Phase 6 as sole serialized owner. During Build append each Decision/Deviation/Surprise with trigger, contract impact, and reversal; escalate irreversible or contract-breaking changes before action.
- 2026-08-26 — Decision: software track authorized; only `SOFTWARE_HARNESS` rows (bundle format, parser/range rejection, linker size) may proceed pre-admission. BootROM/XMODEM, sole-sender, and analyzer evidence remain hardware-gated exactly as written.
- 2026-08-29 — Decision: ADR-0010 freezes the software-track root stream as a bounded tagged `COSE_Sign1` object with an embedded RFC 8949 core-deterministic CBOR manifest and Ed25519-only verification. Host implementation may proceed with injected `SOFTWARE_HARNESS` limits; loader admission, exact physical bounds, and replay resistance remain blocked on the named-hardware sole-sender gate.
- 2026-08-29 — Implementation: `authority/vf2-root-stream/` completes the ADR-0010 host tooling and logical quarantine-order harness. Security/quality review found and resolved unbounded host file allocation before configured limits. Physical loader placement, cache/coherency cleanup, XMODEM timing, sole-sender wiring, and every hardware matrix row remain blocked.
