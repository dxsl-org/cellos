# RUNTIME-CONTRACT-APPROVAL

Artifact: `artifacts/runtime-api-contract.md`
Canonical approval input: `artifacts/approval-input-manifest.json`
Approval-input-manifest SHA-256: `99cf7d24cd14c3b862959d17b499053735bbefded850202fa72b9eb8509129b3`

| Named signer role | Decision | Approval-input-manifest digest | Date | Independence |
|---|---|---|---|---|
| SDK/runtime owner (maintainer) | APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT | `99cf7d24cd14c3b862959d17b499053735bbefded850202fa72b9eb8509129b3` | 2026-09-16 | contract owner permitted |
| Security owner (maintainer approval recorded) | APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT | `99cf7d24cd14c3b862959d17b499053735bbefded850202fa72b9eb8509129b3` | 2026-09-16 | ratified under solo-first governance |

Both named human signers must explicitly approve this same independently verified manifest digest. PAL-019 production zero/error evidence and PAL-031 bounded caller-owned writable/hostile direct-syscall evidence are complete and bound; approval accepts only the frozen contract and does not authorize PAL/runtime work. Any frozen ABI change separately requires 2× explicit confirmation.
