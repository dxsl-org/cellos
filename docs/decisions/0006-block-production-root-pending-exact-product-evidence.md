# ADR-0006: Block production root selection pending exact product evidence

**Date**: 2026-08-26  
**Status**: Accepted  
**Deciders**: Cellos maintainer

## Context

Cellos needs a production root that keeps each device P-256 private key non-exportable and authorizes only two content-enforcing operations: constructing and signing a PKCS#10 `CertificationRequestInfo`, and constructing and signing a TLS 1.3 `CertificateVerify` input. PKCS#10 makes `CertificationRequestInfo` the signed content ([RFC 2986 §4](https://www.rfc-editor.org/rfc/rfc2986#section-4)); TLS 1.3 defines `CertificateVerify` as 64 spaces, a role-specific context string, a zero byte, and the transcript hash ([RFC 8446 §4.4.3](https://www.rfc-editor.org/rfc/rfc8446#section-4.4.3)). A device that merely signs an AP-supplied digest is therefore not sufficient.

The root must also independently authorize the AP boot measurement, reject replay and rollback across reset and power loss, protect one atomic state covering firmware, profile, certificate chain, verifier set, denylist, qualification, key generation, and authenticated-time floors, and have evidence-backed provisioning, lifecycle, update, RMA, revocation, board, and support contracts. These are fail-closed production requirements, not goals that may be deferred behind an enabled adapter.

[ADR-0005](./0005-mutual-tls-relay-identity.md) requires the external relay client to produce TLS `CertificateVerify` signatures through an attested, service-net-authorized KMS signer backed by separately selected and qualified production hardware. It also classifies the AArch64-QEMU Silo as `DEV_REFERENCE` only. This decision records why that production-hardware prerequisite remains unsatisfied; it does not weaken or replace ADR-0005.

Public evidence establishes that Nuvoton has an OpenTitan-derived family in mass production and that a Nuvoton OpenTitan part ships in commercial Chromebooks ([Nuvoton product page](https://www.nuvoton.com/products/cloud-computing/security/open-titan/), accessed 2026-08-26; [Google production announcement, 2026-03-04](https://opensource.googleblog.com/2026/03/opentitan-shipping-in-production.html)). It does not establish an exact product that Cellos can procure and qualify. Nuvoton publishes only the masked family identifier `NPCR1xxxxxBX`; its public catalog leaves ROM, ROM_EXT, and Crypto Library fields empty ([Nuvoton live catalog JSON](https://www.nuvoton.com/system/modules/com.thesys.project.nuvoton/pages/selection-guide/ajax/selectionPage.json?currentFolder=%2Fproducts%2Fcloud-computing%2Fsecurity%2Fopen-titan%2F&family=Security&ProductSeries=OpenTitan&start=0&limit=100), accessed 2026-08-26). lowRISC explicitly states that the Nuvoton Earl Grey chips are **“not open-market parts”** ([lowRISC OpenTitan product page](https://lowrisc.org/opentitan/), accessed 2026-08-26).

Generic OpenTitan design evidence cannot fill those product gaps. The published P-256 API accepts a caller-provided, pre-hashed digest rather than reconstructing either permitted message ([OpenTitan Earl Grey 1.0.0 P-256 API](https://github.com/lowRISC/opentitan/blob/earlgrey_1.0.0/sw/device/lib/crypto/include/ecc_p256.h)). Earl Grey publishes a fixed-frequency timer and an always-on wake/watchdog timer, but no authenticated wall-clock facility; firmware can rewrite or clear the AON wake counter, its 64-bit access is non-atomic, and its counters stop in the killed state ([Earl Grey 1.0.0 datasheet](https://opentitan.org/earlgrey_1.0.0/book/hw/top_earlgrey/doc/datasheet.html), [AON timer theory of operation](https://opentitan.org/earlgrey_1.0.0/book/hw/ip/aon_timer/doc/theory_of_operation.html), and [AON timer programmer’s guide](https://opentitan.org/earlgrey_1.0.0/book/hw/ip/aon_timer/doc/programmers_guide.html)). The public lifecycle design defines irreversible OTP-backed production and RMA states, but that is reference-design evidence rather than proof of the selected product’s fuse state, factory ceremony, token custody, or service contract ([OpenTitan Earl Grey 1.0.0 device lifecycle](https://opentitan.org/earlgrey_1.0.0/book/doc/security/specs/device_life_cycle/index.html)).

The existing Raspberry Pi 3 Model B also cannot supply the required non-circular AP↔root boot authorization. Raspberry Pi’s supported secure-boot tooling applies to Raspberry Pi 4 or newer ([Raspberry Pi secure-boot documentation](https://raw.githubusercontent.com/raspberrypi/usbboot/master/docs/secure-boot.md)). For BCM2837, the ROM loads and transfers control to `bootcode.bin`; the Pi 3 schematic exposes header SPI0 and RUN separately from the SD boot signals ([Raspberry Pi boot sequence](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html#boot-sequence), [Raspberry Pi 3 Model B reduced schematic](https://pip.raspberrypi.com/documents/RP-008340-DS)). A header-attached root therefore receives an AP assertion only after unverified AP code is already executing. Such an assertion cannot independently authorize that same code.

## Decision Drivers

- Select an exact orderable MPN, package, die/stepping, marking, and errata/PCN baseline rather than infer a product from a family, design release, development board, or deployment announcement.
- Pin vendor-supported production ROM, ROM_EXT, application firmware, cryptolib, configuration, signed hashes, update policy, and support horizon.
- Expose only typed CSR and TLS 1.3 commands that reconstruct and validate the complete signed content inside the protected boundary; expose no generic sign, hash-sign, caller-supplied DER, or caller-supplied digest path.
- Establish immutable, non-circular AP↔root pairing and root-owned boot authorization on the exact board.
- Protect rollback-resistant, power-loss-atomic state and authenticated time without trusting an AP assertion.
- Define exact provisioning, entropy, lifecycle, debug, rescue, RMA, zeroization, revocation, board, reset, power, interrupt, tamper, and recovery behavior.
- Require evidence for procurement, lifecycle, security response, PCN/EOL policy, and a dated support horizon.
- Preserve the fail-closed production prerequisite in ADR-0005 and keep development evidence distinguishable from production qualification.

## Considered Options

### Option A (chosen): Select no product and retain the production kill gate

- **Pro**: Preserves every Phase 1–4 fail-closed security contract without representing generic design capability as exact-product assurance.
- **Pro**: Leaves completed software work in Phases 1–3 valid and reusable while preventing it from being mislabeled as production hardware qualification.
- **Pro**: Provides deterministic evidence requirements for reopening the decision.
- **Con**: Blocks production Phases 7–8 and production qualification of the ADR-0005 client path.
- **Con**: Requires vendor, procurement, board, and provisioning engagement before production hardware implementation can resume.
- **Chosen because**: It is the only option supported by the current evidence without weakening a mandatory gate.

### Option B: Select Nuvoton `NPCR1xxxxxBX`, `NPCR100T`, or an Earl Grey revision now

- **Pro**: The masked Nuvoton family is listed as mass production, and Google reports a Nuvoton OpenTitan part shipping in Chromebooks ([Nuvoton product page](https://www.nuvoton.com/products/cloud-computing/security/open-titan/); [Google production announcement](https://opensource.googleblog.com/2026/03/opentitan-shipping-in-production.html)).
- **Pro**: The Earl Grey design contains potentially useful key-manager, OTBN, SPI, OTP, and lifecycle mechanisms ([Earl Grey 1.0.0 key-manager documentation](https://opentitan.org/earlgrey_1.0.0/book/hw/ip/keymgr/doc/theory_of_operation.html); [SPI device documentation](https://opentitan.org/earlgrey_1.0.0/book/hw/ip/spi_device/doc/theory_of_operation.html)).
- **Con**: `NPCR1xxxxxBX` masks the exact part; no public vendor mapping binds `NPCR100T`, Earl Grey A2, or `Earlgrey-PROD-M6` to an orderable suffix, package, stepping, shipped firmware, or errata baseline. `Earlgrey-PROD-M6` is a production-tapeout design release, not an orderable product ([Earlgrey-PROD-M6 release](https://github.com/lowRISC/opentitan/releases/tag/Earlgrey-PROD-M6)).
- **Con**: lowRISC says the devices are not open-market parts, and the public Nuvoton catalog does not pin ROM, ROM_EXT, or cryptolib ([lowRISC product page](https://lowrisc.org/opentitan/); [Nuvoton catalog JSON](https://www.nuvoton.com/system/modules/com.thesys.project.nuvoton/pages/selection-guide/ajax/selectionPage.json?currentFolder=%2Fproducts%2Fcloud-computing%2Fsecurity%2Fopen-titan%2F&family=Security&ProductSeries=OpenTitan&start=0&limit=100)).
- **Con**: The public P-256 interface is a generic pre-hash signer, and the design lacks authenticated wall time ([P-256 API](https://github.com/lowRISC/opentitan/blob/earlgrey_1.0.0/sw/device/lib/crypto/include/ecc_p256.h); [AON timer programmer’s guide](https://opentitan.org/earlgrey_1.0.0/book/hw/ip/aon_timer/doc/programmers_guide.html)).
- **Rejected because**: Production existence and useful design features do not identify a procurable, supported, content-enforcing, board-qualified Cellos product. No exact SKU or revision is inferred from the masked family or generic Earl Grey evidence.

### Option C: Qualify an FPGA board, development bundle, Verilator/QEMU model, or the QEMU Silo

- **Pro**: These targets can exercise protocol shapes, software behavior, and end-to-end mTLS integration without waiting for production silicon.
- **Con**: The public Earl Grey development bundle explicitly contains FPGA bitstreams, a Verilated simulation, a test ROM, and an FPGA ROM extension, not a vendor production image ([OpenTitan `devbundle-2026-06-02-1`](https://github.com/lowRISC/opentitan/releases/tag/devbundle-2026-06-02-1)). lowRISC’s Bergen and Luna/CW341 boards are explicitly FPGA emulation platforms ([lowRISC hardware announcement](https://lowrisc.org/news/lowrisc-announces-expansion-of-opentitan-project-with-new-hardware/)).
- **Con**: FPGA, simulation, and QEMU evidence cannot prove production key non-exportability, silicon lifecycle state, physical tamper behavior, product errata, supply/support, factory provisioning, or exact-board reset/power/boot behavior.
- **Con**: ADR-0005 already restricts the AArch64-QEMU Silo to `DEV_REFERENCE` evidence.
- **Rejected because**: A reference or emulated target is useful development evidence but cannot cross the production hardware gate.

### Option D: Substitute a commodity TPM or secure element

- **Pro**: Exact, orderable parts exist with established crypto, provisioning, and supply ecosystems, including Infineon SLB 9672, NXP SE051, and Microchip ATECC608C families ([Infineon SLB 9672 product page](https://www.infineon.com/part/OPTIGA-TPM-SLB-9672-FW16), [NXP SE051 product page](https://www.nxp.com/products/SE051), [Microchip ATECC608C-TFLXTLSS product page](https://www.microchipdirect.com/product/ATECC608C-TFLXTLSS)).
- **Con**: TPM `Sign` signs an externally provided hash; it does not reconstruct Cellos CSR or TLS content ([TCG TPM reference `Sign.c`](https://raw.githubusercontent.com/TrustedComputingGroup/TPM/main/TPMCmd/tpm/src/command/Signature/Sign.c)). NXP’s stock middleware and Microchip CryptoAuthLib likewise expose digest-signing operations ([NXP Plug & Trust example](https://github.com/NXP/plug-and-trust/blob/v04.07.01/sss/ex/ecc/ex_sss_ecc.c), [Microchip CryptoAuthLib signing API](https://microchiptech.github.io/cryptoauthlib/a00272.html)).
- **Con**: Stock TPM PCR/policy/NV or secure-element counters do not, by themselves, establish an immutable Pi 3 boot path, authenticated wall time, or one atomic Cellos state record.
- **Con**: An SE051P custom applet would be a new proprietary firmware/product qualification requiring direct vendor engagement, not evidence that a stock SE051 presently passes the gate ([SE051 datasheet, Rev. 2.0, §§3.4–3.5](https://www.nxp.com/docs/en/data-sheet/SE051.pdf)). A fixed-function ATECC608C cannot be repaired by adding a Cellos content-reconstructing applet.
- **Rejected because**: Non-exportable key storage is necessary but not sufficient; the available public product contracts retain the forbidden generic-sign path and do not satisfy the boot, time, and atomic-state gates.

### Option E: Select Pluton or Caliptra as an integrated-root alternative

- **Pro**: Integration can avoid an exposed discrete-root bus, and Caliptra DPE demonstrates that protected CSR generation is technically possible.
- **Con**: Microsoft documents Pluton as a Windows-oriented, SoC-integrated TPM subsystem with Microsoft-authored firmware, not a standalone Cellos product/API; the public interface therefore does not establish the two Cellos content-enforcing commands ([Microsoft Pluton documentation](https://learn.microsoft.com/en-us/windows/security/hardware-security/pluton/microsoft-pluton-security-processor)).
- **Con**: Caliptra is reusable RTL/firmware for integration into data-center SoCs rather than an exact orderable RoT product ([CHIPS Alliance Caliptra project](https://chipsalliance.github.io/caliptra-web/)). DPE `CertifyKey` can generate a CSR, but DPE `Sign` still accepts a digest and does not reconstruct TLS 1.3 `CertificateVerify` ([Caliptra DPE `certify_key.rs`](https://github.com/chipsalliance/caliptra-dpe/blob/main/dpe/src/commands/certify_key.rs), [Caliptra DPE `sign.rs`](https://github.com/chipsalliance/caliptra-dpe/blob/main/dpe/src/commands/sign.rs)).
- **Con**: Neither public path supplies an exact Cellos-supported SoC/revision, pinned production firmware, authenticated-time contract, complete atomic state, qualification record, or board/RMA/support package.
- **Rejected because**: These are platform or design directions, not an evidence-complete production product for the current Cellos board lane. A future named SoC with extended typed firmware would require a fresh product qualification.

### Option F: Implement disabled production hardware plumbing and call Phase 7 complete

- **Pro**: Could reserve provider types, feature flags, mailbox framing, or configuration paths while preserving runtime disablement.
- **Con**: It would freeze interfaces before the exact product, firmware ABI, transport, board topology, provisioning flow, and failure semantics are known.
- **Con**: Disabled code proves only that a software seam exists. It proves none of the product, content-enforcement, boot, time, state, provisioning, or physical-board requirements.
- **Con**: Calling this Phase 7 completion would convert a fail-closed evidence gate into a paperwork state and create pressure to enable an unqualified path later.
- **Rejected because**: Phase 7 completion means the named production hardware contract is satisfied. A disabled placeholder is not production-root evidence.

## Decision

Cellos selects **no production root product** on the current evidence.

Production Phases 7–8 remain blocked. No hardware adapter, production provider, board integration, provisioning path, or disabled placeholder may be represented as completion of those phases. No FPGA, development bundle, simulator, QEMU target, generic TPM, stock secure element, Pluton subsystem, Caliptra design, or masked OpenTitan family may be promoted as a production substitute.

Phase 4 remains a software integration phase and is not blocked on product selection. Its independent entry gates still apply: real protected persistence, authenticated time, and a distinct reviewed pending-key binding under the frozen KMS ABI. Phase 5 may exercise the resulting path only as `DEV_REFERENCE`; neither phase supplies production-root evidence.

Phases 1–3 remain valid software evidence. Development and reference work may continue only under an explicit non-production classification and must remain impossible to select in a production artifact. ADR-0005 remains accepted, but production qualification of its Cellos client path stays blocked on the production signer prerequisite recorded here; there is no raw, exportable-key, generic-sign, or unauthenticated fallback.

This is an evidence-backed NO-GO, not a conclusion that Nuvoton/OpenTitan or another architecture can never qualify. It is specifically a refusal to infer an exact product, configuration, capability, or support contract from generic or masked public evidence.

## Reopening Criteria

The decision may be reopened only after Cellos receives one vendor-signed evidence package that identifies and contractually binds all of the following to the same proposed deployment:

1. **Exact product identity**: complete MPN, package, die/mask/metal revision, device marking, certification target, errata baseline, and PCN baseline.
2. **Procurement and support**: authorized-channel quote or listing, MOQ, lead time, unrestricted availability, lifecycle class, last-order/last-ship policy, security-response process, and dated firmware and product support horizons.
3. **Production software baseline**: exact mask ROM, ROM_EXT, application firmware, cryptolib, build configuration, source commit or auditable binary hash, signed release manifest, update/recovery policy, authorized keys, and anti-rollback rules.
4. **Content-enforcing protocol**: a versioned command/ABI specification and positive and negative vectors proving that the protected root reconstructs and validates the complete PKCS#10 `CertificationRequestInfo` and TLS 1.3 `CertificateVerify` input before signing; every externally reachable generic sign, caller-supplied DER, caller-supplied digest, test, rescue, and alternate-firmware bypass must be absent or cryptographically unreachable.
5. **Lifecycle and provisioning**: shipped lifecycle/fuse state, OTP map and options, entropy configuration and validation, personalization and ownership-transfer ceremonies, factory key custody, interrupted-write recovery/quarantine/scrap rules, debug and rescue policy, RMA-token custody, zeroization, destruction, rekey, replacement, and revocation procedures.
6. **Immutable AP and board binding**: a named AP/board/revision and approved schematic/netlist/BOM in which the root independently owns or authorizes the first-stage boot measurement without trusting AP code; exact SPI, reset, IRQ/ready, boot media, power-good/brownout, rail, level, strap, tamper, debug, and recovery behavior. The existing Raspberry Pi 3 header topology does not satisfy this criterion; it requires a different qualified board/AP lane or a root-mediated board redesign.
7. **Protected state and time**: vendor-qualified endurance and power-cut behavior plus a rollback-resistant, power-loss-atomic state record covering firmware, key generation, profile, chain, verifier set, denylist, per-device qualification, request generations, and authenticated-time floors; the authenticated-time source, freshness, outage, recovery, and rollback rules must be explicit.
8. **Per-device qualification and operations**: an independently signed, non-transferable record binding exact device, root product/revision/lifecycle/firmware/protocol, AP board/revision/measurement, KMS/provider/image/policy, key generation, and profile digest; hardware evidence must cover boot substitution, replay, reset, brownout, torn writes, bus faults, debug lock, RMA wipe, post-RMA serve denial, revocation, and destructive zeroization.

Receipt of a package permits a new architecture, security, procurement, and board review; it does not itself constitute approval. A GO requires every item to pass without inference, the selected product to be recorded by a superseding ADR, and only Phases 7–8 to be rewritten around the exact approved artifacts before implementation resumes.

## Consequences

### Positive

- Cellos does not claim production assurance from a masked family, reference design, simulation, or generic signing primitive.
- The content-enforcing, boot, state, time, lifecycle, board, and support gates remain intact.
- Completed software in Phases 1–3 and the ADR-0005 server-side mTLS work remain usable without being confused with production client readiness.
- Reopening is tied to an auditable, exact-product evidence package rather than compatibility claims.

### Negative / Risks

- The production KMS root, production Phases 7–8, and production qualification of ADR-0005’s Cellos mTLS client path remain unavailable; Phase 4 retains its separate software trust-source and pending-binding gates.
- Vendor engagement, procurement, a different AP/board lane or board redesign, custom protected firmware, and qualification may be required.
- Schedule and product availability remain outside the repository’s control.
- A future candidate may still fail after evidence is obtained, particularly at the typed-command, authenticated-time, immutable-boot, or power-loss-atomic state gates.

### Neutral

- Nuvoton/OpenTitan remains a plausible architecture for future evaluation, but no exact SKU, revision, firmware, provisioning service, support term, board, state, or time capability is selected or inferred by this ADR.
- FPGA, QEMU, development-bundle, TPM, secure-element, Pluton, and Caliptra work may inform future research only when labeled according to what it actually proves.

## Security

Every production signing request must fail closed when product identity, firmware identity, AP measurement, qualification record, policy/profile state, monotonic state, authenticated time, transport freshness, or protected persistence is absent or ambiguous. Reset, replay, power loss, malformed input, unsupported protocol versions, or RMA/debug state must never expose a generic signing oracle or restore an earlier accepted state.

No AP-supplied digest, DER object, time, measurement, `attested` flag, readiness flag, or pairing state can satisfy an independently protected check. No production path may fall back to the QEMU Silo, a filesystem key, raw relay registration, generic TPM/secure-element signing, or disabled-but-unqualified plumbing.

## Links

- [ADR-0005: Use mutual TLS for external relay identity](./0005-mutual-tls-relay-identity.md) — production client signing prerequisite and `DEV_REFERENCE` boundary.
- `.agents/260825-1726-kms-silo-production-root/phase-06-select-production-root-product.md` — product kill-gate requirements and blocked Phase 7–8 handoff.
- `.agents/260825-1726-kms-silo-production-root/research/protected-root-report.json` — prior protected-root boundary and reference evidence.
