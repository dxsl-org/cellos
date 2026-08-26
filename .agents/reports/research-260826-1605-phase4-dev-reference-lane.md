# Research: Phase 4 Concrete DEV_REFERENCE Lane

**Mode:** architecture evaluation · **Depth:** deep · **Date:** 2026-08-26

## Verdict

Select the **VisionFive 2 v1.3B UART-root-stream + STM32H573I-DK + OPTIGA TPM SLB 9672 + project-operated AWS signed-time service** as the only concrete lane worth implementing. Its architecture is conditionally coherent, but the Phase 4 entry remains **NO-GO** until hardware, firmware, service, and AC-001..AC-011 evidence exist.

No evaluated board or service satisfies the contract as sold. This report selects an implementation candidate, not a qualified authority, procurement approval, or production root.

## Recommended Lane

### Application processor

- **Board:** StarFive VisionFive 2, revision 1.3B, already represented by Cellos `board-vf2`.
- **Boot mode:** permanently strap JH7110 `RGPIO_1:RGPIO_0 = 11` for BootROM UART0/XMODEM boot.
- **Root path:** the external authority controls board power/reset and is the sole electrical sender to UART0 RX. The immutable JH7110 BootROM loads the first mutable program into SRAM at `0x08000000` and executes it after transfer.
- **Boot bundle:** a reviewed SRAM loader initializes DRAM, then accepts only an authority-authenticated, bounded bundle containing OpenSBI, DTB, Cellos, and VIFS. It must not read QSPI, SD, eMMC, USB, network, or an AP-provided measurement.
- **Physical enforcement:** fixed UART straps, root-owned load switch/reset supervisor, removal or isolation of any competing USB-UART TX source, and no normal alternate boot mode.

StarFive documents both the UART BootROM mode and the UART0 header pins. The repository already maps the VF2 v1.3B/JH7110 target, but currently expects OpenSBI and a firmware DTB; therefore the root-stream boot bundle is new work, not existing evidence.

### Protected Relay Authority

- **Controller:** exact orderable `STM32H573I-DK`, MCU `STM32H573IIK3Q`.
- **Root firmware:** STiRoT-provisioned, debug-locked firmware exposes only versioned boot, state, time, enrollment, commit, CSR, and TLS CertificateVerify commands. It exposes no raw digest, generic sign, generic TPM, arbitrary NV, or firmware-update command in normal mode.
- **Protected key/state anchor:** Infineon `OPTIGA TPM SLB 9672` SPI evaluation kit, OPN `TPM9672FW1523PCEBTOBO1`, electrically reachable only by STM32. The TPM holds the stable authority key, relay active/pending keys, and a non-orderly `TPMA_NV_COUNTER` floor.
- **State journal:** authenticated dual-slot records in STM32-controlled flash bind the complete `PERSIST-003` record to the TPM counter. Advance counter, write and verify the next slot, then expose the new state. A torn edge may seal; it may never infer or roll back state.
- **Provider binding:** STM32 reads the pending TPM public area itself, validates the exact certificate/profile, persists the single-use receipt, and performs the `PREPARED → TPM/provider CAS receipt → COMMITTED` protocol. KMS opcodes 9–14 remain unchanged.
- **Signing:** the AP can request only the frozen typed TLS operation. STM32 reconstructs the exact TLS 1.3 CertificateVerify input before invoking TPM signing internally. The TPM's generic primitive is never reachable from the AP transport.

ST verifies MCU-local secure-boot, protected-key, transport, and lifecycle primitives. TCG specifies TPM counter semantics. Neither source proves this Cellos authority implementation; its status remains unverified until built and fault tested.

### Signed-time authority

- **Ingress:** one regional API Gateway endpoint, `POST /v1/time`, before relay mTLS.
- **Signer:** one AWS KMS asymmetric `ECC_NIST_P256`, `SIGN_VERIFY`, `ECDSA_SHA_256` key. The authority firmware pins protocol version, source ID, key ID, and the DER SPKI digest; TLS is transport only.
- **State:** one regional DynamoDB transaction allocates strict `{source_epoch, source_sequence, unix_seconds}` and records the request tuple before signing. No multi-region allocator or failover source is allowed.
- **Request binding:** deterministic CBOR binds `{device_id, authority_id, boot_epoch, request_id, purpose, nonce[32]}` and an appliance signature.
- **Response binding:** deterministic CBOR binds the request tuple plus `{source_id, source_epoch, source_sequence, unix_seconds, expires_at<=60s, key_id, algorithm}` under the KMS signature.
- **Acceptance:** the appliance requires exact outstanding-request equality, increasing protected source sequence and Unix floor, known source epoch, and valid expiry, then atomically persists all floors before issuing a KMS fact.
- **Outage:** endpoint, upstream clock, DynamoDB, KMS, signature, freshness, or floor failure returns no fact and seals. No cached fact crosses expiry or boot epoch.

Public Roughtime and NTS may discipline the service clock, but neither directly carries the required device, authority, boot, purpose, persistent epoch, and strict sequence contract.

## Comparison Matrix

| Candidate | Independent first mutable AP stage | Stable identity | Rollback state | Typed authority | Signed time | Result |
|---|---|---|---|---|---|---|
| VF2 UART stream + STM32H573 + SLB9672 + AWS authority | **VERIFIED primitive**; full bundle unimplemented | **VERIFIED primitive** | **VERIFIED primitive**, journal unimplemented | **Design only** | **Design only** | **Selected candidate; NO-GO now** |
| CM4 Lite + custom SD/eMMC emulator + same authority | Conditional; requires custom carrier/FPGA | Same | Same | Design only | Design only | Runner-up; higher hardware cost |
| Stock RPi4 or stock VF2 media boot | Root cannot observe/control exact first mutable bytes | External only | External only | Incomplete | Incomplete | **NO-GO** |
| STM32H573I-DK alone | No independent AP boot path | MCU primitive | No proven irreversible application floor | Design only | Absent | **NO-GO** |
| LPC55S69-EVK alone | No independent AP boot path | PUF primitive | 17 image-key revocations are not an authority database | Design only | Absent | **NO-GO** |
| OpenTitan CW310/CW340 | FPGA ROM/OTP overrideable through JTAG loader | Fixture state | Fixture state | Mutable bitstream | Absent | **Disqualified** |
| Public Roughtime | N/A | Pinned server key | No source epoch/sequence | No device/boot/purpose binding | Partial | **NO-GO as direct source** |
| NTS/NTP | N/A | TLS/cookie session | Stateless server model | No typed signed assertion | Partial | **NO-GO as direct source** |

## Active Refutation

### “Secure boot on STM32 proves the Cellos boot”

False. STiRoT authenticates STM32 firmware only. The VF2 lane is viable only because immutable JH7110 BootROM receives the first mutable AP stage exclusively from the root-controlled UART path. An AP-supplied measurement remains forbidden.

### “VisionFive 2 UART mode alone closes boot authorization”

False today. The official source verifies BootROM XMODEM loading and execution, not a complete OpenSBI/DTB/Cellos stream, maximum image behavior, competing UART sources, reset race freedom, or absence of fallback. These require a dedicated loader and physical captures.

### “STM32 protected flash is rollback-resistant authority state”

False. Secure boot, encrypted flash, and a double buffer do not detect restoration of all mutable pages. The selected design adds a TPM NV counter and deliberately seals on any counter/journal mismatch. Its actual power-loss semantics must still be demonstrated.

### “TPM generic signing violates the typed contract”

It would if reachable by the AP. In this lane the TPM bus is authority-private; only reviewed STM32 firmware may call it, after reconstructing or validating the complete typed content. Any AP-visible TPM, digest, or generic signing command invalidates the lane.

### “Roughtime or NTS directly satisfies authenticated time”

False. Roughtime binds a nonce and rough time but lacks Cellos device, authority, boot, purpose, persistent epoch, and strict source sequence. NTS authenticates NTP through TLS-derived symmetric state but does not produce the required pinned, typed signed fact.

### “CM4 eMMC can be intercepted on a carrier”

False for an eMMC CM4: its eMMC bus is internal. Only CM4 Lite exposes the SD/eMMC interface to a carrier. The corrected CM4 Lite interposer remains a custom-hardware runner-up.

## Gate Mapping

| Gate | Candidate design | Current status |
|---|---|---|
| PERSIST-001..008 | STM32 secure firmware + TPM identity/counter + authenticated dual-slot record | **NO-GO:** firmware, provisioning, journal, and fault evidence absent |
| TIME-001..008 | AWS KMS/DynamoDB nonce-bound signed-time service + appliance floors | **NO-GO:** endpoint, key pin, implementation, upstream-clock policy, and fault evidence absent |
| BIND-001..009 | STM32-private TPM provider, direct pending-SPKI validation, single-use receipt, typed TLS signing | **NO-GO:** protocol/firmware and opcode compatibility evidence absent |
| LANE-001..005 | VF2 UART-root-stream and compile/package DEV separation | **NO-GO:** physical sole-sender, boot bundle, and production rejection evidence absent |
| AC-001..011 | Full hardware/service/fault/security matrix | **NO-GO:** none executed |

## Minimum Evidence Program

1. Acquire exact VF2 v1.3B, STM32H573I-DK, and SLB9672 kit; record revisions and firmware.
2. Prove from schematic and logic analyzer that cold boot in fixed UART mode executes no mutable byte not sent by the authority.
3. Implement and bound the SRAM loader and authenticated UART bundle; prove substituted, truncated, replayed, alternate-media, reset, and outage paths do not execute Cellos.
4. Provision STiRoT/debug lockdown and TPM authority/provider keys plus non-orderly NV counter; prove AP cannot address TPM or generic signing.
5. Implement the authenticated dual-slot state machine; inject power loss at every counter/write/verify/prepare/promote/finalize edge and restore old flash snapshots.
6. Deploy the single-region signed-time service and pin its KMS SPKI; test replay, freeze, fork, restored server state, clock fault, KMS denial, database conflict, and endpoint outage.
7. Implement the root-side pending-SPKI/profile validator and typed CSR/TLS operations while preserving byte fixtures for KMS opcodes 9–14.
8. Prove production builds reject all DEV board/provider/anchor/certificate/features and retain `BLOCKED_BY_ADR_0006`.
9. Complete independent security review. Only then may AC-001..AC-011 open Phase 4 Build.

## Risks

- The UART BootROM path may impose undocumented payload or recovery-loader constraints; failure returns the lane to NO-GO rather than allowing media fallback.
- TPM NV endurance and `TPMA_NV_ORDERLY` behavior require exact SLB9672 firmware evidence. The authority must use a counter configuration whose power-loss behavior cannot regress; uncertainty seals.
- STM32 and TPM compose two security boundaries. Provisioning, bus ownership, debug lockdown, firmware recovery, and key authorization are part of the TCB.
- A custom cloud service creates operational cost and a new trust root. Its unavailability intentionally stops relay identity use.
- This lane is DEV_REFERENCE only and must never be cited as production silicon or physical-attack qualification.

## Primary Sources

- StarFive, [JH7110 BootROM](https://doc-en.rvspace.org/VisionFive2/Boot_UG/JH7110_SDK/bootrom.html) and [VisionFive 2 UART0 pinout](https://doc-en.rvspace.org/VisionFive2/40-Pin_GPIO_Header_UG/VisionFive2_40pin_UG/gpio_pinout%20-%20vf2.html).
- STMicroelectronics, [STM32H5 STiRoT](https://wiki.st.com/stm32mcu/wiki/Security:STiRoT_for_STM32H5), [STM32H573II](https://www.st.com/en/microcontrollers-microprocessors/stm32h573ii.html), and [STM32H573I-DK order page](https://estore.st.com/en/stm32h573i-dk-cpn.html).
- Infineon, [OPTIGA TPM SLB 9672 kit](https://www.infineon.com/evaluation-board/OPTIGA-TPM-SLB-9672-KIT) and [SLB 9672 product family](https://www.infineon.com/products/security-smart-card-solutions/optiga-embedded-security-solutions/optiga-tpm).
- Trusted Computing Group, [TPM 2.0 Library Specification](https://trustedcomputinggroup.org/resource/tpm-library-specification/), Parts 2–3 (`TPMA_NV_COUNTER`, `TPM2_NV_Increment`).
- Raspberry Pi, [Compute Module documentation](https://www.raspberrypi.com/documentation/computers/compute-module.html) and [CM4 datasheet](https://datasheets.raspberrypi.com/cm4/cm4-datasheet.pdf).
- OpenTitan, [FPGA bitstream execution environments](https://opentitan.org/book/hw/bitstream/index.html) and [secure boot](https://opentitan.org/book/doc/security/specs/secure_boot/).
- IETF, [Roughtime draft](https://www.ietf.org/archive/id/draft-ietf-ntp-roughtime-15.html), [RFC 8915 NTS](https://datatracker.ietf.org/doc/html/rfc8915), and [RFC 8949 CBOR](https://www.rfc-editor.org/rfc/rfc8949.html).
- AWS, [asymmetric KMS keys](https://docs.aws.amazon.com/kms/latest/developerguide/symmetric-asymmetric.html), [`Sign`](https://docs.aws.amazon.com/kms/latest/APIReference/API_Sign.html), [DynamoDB transactions](https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/transaction-apis.html), and [API Gateway regional resilience](https://docs.aws.amazon.com/apigateway/latest/developerguide/disaster-recovery-resiliency.html).

## Unresolved Questions

- Does the exact JH7110 BootROM revision on the acquired VF2 v1.3B accept the planned SRAM loader size and transfer sequence without an alternate recovery path?
- Can the onboard USB-UART sender be electrically isolated so STM32 is the sole UART0 RX source?
- What exact SLB9672 firmware and NV configuration provides the required non-regressing power-loss behavior and acceptable counter endurance?
- What upstream UTC discipline and holdover bound will cause the AWS service to stop signing on clock uncertainty?
- What procurement region and cloud account own the DEV hardware and service? No purchase or deployment is authorized by this report.