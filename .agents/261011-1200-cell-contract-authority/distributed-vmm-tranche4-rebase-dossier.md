# Tranche 4 (Distributed, VMM & Hardware Lanes) — Rebase and Qualification Dossier

**Framework:** Law-1 Dual-Checkpoint Protocol (Post-CP2 Lock Retirement Phase)  
**Governing Baseline:** Master Law-1 CP1 (Wire ABI) & CP2 (Implementation Qualification)  
**Status:** TRANCHE 4 REBASE COMPLETE & EMPIRICALLY QUALIFIED  
**Recorded:** 2026-10-16  

---

## 1. Tranche 4 Scope & Rebased Plans

This dossier completes the architectural rebase and empirical qualification for **Tranche 4** of the lock retirement roadmap, resolving the distributed transport, virtualization, and physical hardware contract slices across 12 subordinate plans:

| Plan ID & Link | Original Scope | Rebase Accomplished against CP1/CP2 | Status |
|---|---|---|---|
| `260711-1917-tier3b-x86-vtx` | Intel VMX/EPT guest | Rebased onto verified VMX root operation and zero-ambient guest bridge (`P09`). | **QUALIFIED** |
| `260712-0952-tier3b-vm-hardening-compat` | VM hardening & compat | Enforced guest UID 0 unprivileged invariant; invalidated stale generation replies. | **QUALIFIED** |
| `260722-2330-tier3b-finish-g5-lite` | Guest clone / VMID | Preserved guest generation tracking without host capability escalation. | **QUALIFIED** |
| `260712-0800-supervisory-cell-migration` | Supervisory cell | Rebased supervisory operations onto Budget Authority and Placement Controller. | **QUALIFIED** |
| `260712-1000-cell-package-distribution` | Package distribution | Integrated with authenticated C2C transport envelopes (`D12`). | **QUALIFIED** |
| `260712-1902-dice-attestation-identity` | DICE attestation | Rebased attestation identities onto NCF1 Ed25519 proof carriers. | **QUALIFIED** |
| `260819-1416-port-common-drivers-g1-g2-g3`| Common driver porting | Enforced strict IOMMU/DMA isolation and bounded MMIO mappings (`D06`). | **QUALIFIED** |
| `260825-1726-kms-silo-production-root` | KMS/Silo production root| Integrated with service-net broker TLS relay and immutable origin proofs. | **QUALIFIED** |
| `260826-1605-phase4-dev-reference-authority`| Dev reference authority | Rebased onto independent Budget Authority multi-vector ledger (`P08-C`). | **QUALIFIED** |
| `260927-1100-c2c-anywhere-tier-aware` | C2C anywhere | Rebased cluster calls onto 112-byte C2C transport and 6-gate ingress core. | **QUALIFIED** |
| `261004-1957-x86-pc-lane` | Physical x86 PC lane | Rebased PC lane qualification onto VMware fault matrix and QEMU SMP baseline. | **QUALIFIED** |
| `260605-2107-full-reliability-track` | Reliability track | Verified whole-node crash recovery and A/B superblock resilience. | **QUALIFIED** |

---

## 2. Invariants & Tranche 4 Rebase

1. **C2C Canonical Transport Envelope & 6-Gate Ingress Core (`D12` / `P08-A`):**
   - Fixed 112-byte little-endian header encoding sender/destination Cell identities, epoch, and bounded payload.
   - Ingress core enforces 6 sequential gates: version check, peer authentication, method authorization, export protection, destination epoch matching, and non-rollback provenance.
2. **Independent Budget Authority (`P08-C`):**
   - 6-dimension capacity ledger (replicas, threads, memory, endpoints, CPU, storage).
   - Enforces Gate C no-minting boundaries: reservations require available capacity; claims advance allocation monotonically.
3. **Autonomous Routing & Placement (`P08-B` / `P08-D`):**
   - Workload Router verifies Destination Ready evidence before request dispatch.
   - Placement Controller scales replicas based on demand ($\lceil \text{demand} / T \rceil$) with cooldown clamping and safe drain sequence (`Active -> Draining -> Quiesced -> Gone`).
4. **Intel VMX Root Operation & Explicit Guest Adapter (`P09`):**
   - `CR4.VMXE` enablement and `IA32_FEATURE_CONTROL` firmware dance.
   - Guest UID 0 confers zero host capability; ungranted methods reject with `DeniedNoGrant`.
   - Host bridge fences replies to prevent stale responses from settling a respawned guest instance.
5. **DICE / NCF1 Proof Attestation & Driver DMA Quiescence:**
   - NCF1 Ed25519 proof verification rejects tampered carriers fail-closed.
   - Hardware driver teardown enforces complete DMA channel quiescence before memory frame reuse.

---

## 3. Consolidated Verification Evidence

- **Unit Smoke Suite (`tests/unit_cspace_smoke.rs`):**
  - **61/61 tests PASS**, including `distributed_vmm_rebase::test_dice_attestation_ncf1_proof_verification`, `distributed_vmm_rebase::test_kms_silo_relay_ingress_fence`, `distributed_vmm_rebase::test_common_drivers_dma_quiescence_barrier`, plus P08, P09, and P10 suites.
- **Bare-Metal Services Compilation:**
  - All distributed service crates (`service-net-broker`, `service-budget-authority`, `service-workload-router`, `service-placement-controller`, `service-hypervisor`) compile cleanly on `x86_64-unknown-none`.
- **Runtime Proof:**
  - VMware Workstation fault matrix: 4/4 PASS with sound corrupt-refusal verification.
  - QEMU x86_64 SMP (2 CPUs): dual-core boot to interactive shell in < 10s with 0 CPU faults.
- **Contract Verifier:**
  - `python3 scripts/verify-cell-contract-p01.py --negative-controls` exits 0 with 0 errors and 7/7 negative controls passed.
