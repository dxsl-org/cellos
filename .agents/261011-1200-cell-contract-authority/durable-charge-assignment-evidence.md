# Durable charge owner/fencing/backend assignment evidence

2026-10-15. **Disposition: actual deployment assignments unavailable in inspected repository evidence; implementation admission BLOCKED.** This is an evidence register, not a roster granting authority. Engineering phase ownership and existing source mechanisms are not deployment assignments. No principal, key, endpoint, device, floor or successor has been invented.

Related [semantic contract](durable-charge-ownership-adoption-contract.md), [byte candidate](durable-charge-canonical-byte-candidate.md) and [G3 gates](g3-lifecycle-authority-gates.md). All locks/phases/CP pairs retain their existing status. No credentials/private keys were sought or are required in this document.

## 1. Inspection scope and evidence classification

Read-only assignment investigation covered issuer-persistence-candidate.md E01–E13/A1–A8; scoped-g1-g2-authorization.json; build/g2-runtime/runtime-summary.json; live source roots `libs/authority-protocol`, `authority`, `cells/services/{kms,vfs}`, `cells/drivers/nvme`, `kernel`; Spec25 and ADR0006/0008. Search terms concerned ownership/admission/fencing/backend traits, creation keys, writer/enforcement epochs, public pins, issuer roster and recovery floors. This is bounded repository evidence, not a claim about inaccessible operator records or exhaustive external/macro consumer closure. No deployment, hardware or provider query was performed.

Evidence labels: SOURCE = inspected implementation seam; RECORDED = retained run/authorization; UNAVAILABLE = required decision/producer not supplied by these sources. No source test fixture counts as assigned production authority.

## 2. Available engineering mechanisms, not deployment grants

| Surface | Available evidence | Admission ceiling |
|---|---|---|
| `libs/authority-protocol` | Typed canonical request/authenticator and ProtectedStore CAS seams; issuer candidate E08 records explicit verification, floor and conflict-sealing library behavior | Traits/injected verifier/store do not assign issuer pins, authenticated device CAS or DCO1 adapters. Concrete inspected implementations are tests/fixtures, not deployed assignments |
| `authority/stm32h573i-dk` | Protected-authority candidate/profile software using authority-protocol | Not provisioned appliance identity, deployed storage/verifier adapter or exact-product GO |
| `cells/services/kms/src/storage.rs`; `storage/provider/relay.rs` | Issuer candidate E04 records non-test protected PermissionDenied/unavailable and limited development-Silo gating | Missing protected runtime wiring, not a usable default recovery/floor authority |
| `kernel/src/task/launch.rs`; `scheduler.rs`; memory cell quota | G3 S1–S4 record local publication/retirement/RAII quota seams | No durable DCO1 custody ledger, enforceable distributed writer fence or keyed publication closure assignment |
| `cells/services/supervisor/src/{main,hotswap}.rs` | G3 S6–S7 record orchestration/restart/snapshot/local cutover | Supervisor is not budget issuer or automatically appointed successor custodian |
| `cells/services/vfs/src/{backend_cellosfs,disk_cellosfs,blk_router}.rs`; NVMe driver | Issuer E09–E11 record native open-only/error/poison, sector transport and flush command software | Does not appoint storage owner or prove physical cache/firmware/ordering/floor admission |
| `cells/services/kms/src/storage/{backend,runtime,journal}.rs` | Issuer E12 records186byte KMS two-slot/readback adapter with StoreIo lacking flush | Not DCO1/OGJ1 immutable lifecycle journal, authenticated nonregression or protected durability |
| `scoped-g1-g2-authorization.json:4–16` | RECORDED bounded G1/G2 source-edit permission under controlling P01 integration owner | Does not appoint budget issuer/recovery/fencing/backend principals; excludes downstream approval/deployment |
| `build/g2-runtime/runtime-summary.json:1–64` | RECORDED host/provisioner/VM functional/fault runs; tuple is QEMU RV64 virt,256M,OpenSBI,temporary raw host image,virtio-blk | Explicitly no physical storage/cache/floor/issuer admission or G2-J qualification; not an admitted DCO1 backend |

Primary retained reconciliation: [issuer/persistence E01–E13 and A1–A8](issuer-persistence-candidate.md), [G3 source evidence S1–S9](g3-lifecycle-authority-gates.md#1-evidence-vocabulary-and-source-boundary). Older literal source-line numbers are historical anchors, not an exact implementation-grant pin.

## 3. Actual assignment register

| Required role/tuple | Actual assignment status | Required authentic artifact before admission |
|---|---|---|
| Application budget issuer/genesis/ceiling | **UNAVAILABLE** | Operator/maintainer-authorized application-scoped principal, purpose/lane, measured artifact/tier, public verifier pin/suite/revocation and unique genesis decision |
| Fixed-home Authority custodian | **UNAVAILABLE** | Exact principal/incarnation/home, admitted ledger/genesis, permitted operations, authorized successor set and owner consent |
| Recovery/fence authority | **UNAVAILABLE** | Authorized fence installer, authentic epoch/floor producer, gate-specific enforceability and nonregression evidence; no timeout self-election |
| Destination keyed-publication/closure owner | **UNAVAILABLE as G3 assignment**; local kernel seam exists | Assigned node/boot/artifact identity and authenticated key-map/closed-key producer; exact publish/fence serializer and boot-Gone/restart evidence |
| Resource/state/effect reconciliation issuers | **UNAVAILABLE for this workload** | Exact resource/effect homes, operation methods and complete use/quiescence/watermark/fence/outcome producer roster |
| Ordinary durable backend | **UNAVAILABLE**; only development tuple recorded | Storage-owner signed/recorded decision naming actual volume/device/partition/driver/image/firmware/cache, failure/threat class, barriers and qualification evidence |
| Ledger and destination floor/nonregression | **UNAVAILABLE** | Assigned producer plus authenticated floor identity/revision/persistence, wrong-volume/old-image rejection and reset/exhaustion procedure |
| DCO1 accepted-proof verifier | **UNAVAILABLE** | Real roster/suite/key-purpose and exact authenticated carrier, actual parser/consumer adapters and admission decision |
| Finite charge/manifest/recovery profile | **UNAVAILABLE** | Owner-selected full dimensions/units/limits and independent storage/work bounds plus real high-water evidence; u32 capacity is not a profile |

P01/P02/P03/P05/P06/P08-C remain accountable engineering owners only. This register intentionally contains no fake operational principal ID/public pin/floor/device values. Actual assignment values require externally authorized records if not added to the repository by their owners. Copying a development image hash or CA configuration does not fill any row.

## 4. Admission decision and exact missing prerequisite

No inspected source artifact can authorize the above roles. Therefore the reachable deliverable is a formally reviewed semantic draft and structurally explicit byte candidate with named unresolved carriers/assignments, not a complete admitted CP1 candidate or executable durable adoption path. A real assignment packet must include decision reference/revision/digest, approving owner, principal/scope/home/purpose/lane, public pin and revocation policy, actual backend/floor tuple and evidence, producer/consumer paths and full transition/failure contract. Private keys must never be written into the plan.

Until that packet and exact byte/consumer/capacity approvals exist, canonical parsing/structural vectors may be inspected but cannot mint usable adoption, fence, claim or release authority. No runtime side effects, cloud/hardware procurement, provider credentials, production activation or lock release follow. Declining this package changes documentation only; future durable effects would require admission-closed forward recovery, not binary/floor rollback.
