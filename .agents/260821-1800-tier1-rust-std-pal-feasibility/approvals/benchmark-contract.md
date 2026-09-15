# BENCHMARK-CONTRACT-APPROVAL

Artifacts: `artifacts/workload-parity-spec.md`, `artifacts/benchmark-validator-contract.md`
Canonical approval input: `artifacts/approval-input-manifest.json`
Approval-input-manifest SHA-256: `99cf7d24cd14c3b862959d17b499053735bbefded850202fa72b9eb8509129b3`

| Named signer role | Decision | Approval-input-manifest digest | Date | Independence |
|---|---|---|---|---|
| Performance owner | NOT GRANTED | `99cf7d24cd14c3b862959d17b499053735bbefded850202fa72b9eb8509129b3` (package and GetRandom technical backing verified; human signature absent) | — | workload owner permitted |
| Independent measurement reviewer | NOT GRANTED | `99cf7d24cd14c3b862959d17b499053735bbefded850202fa72b9eb8509129b3` (package and GetRandom technical backing verified; human signature absent) | — | must not author validator/fixtures |

Both named human signers must explicitly approve this same independently verified manifest digest only after the `PAL-019` production entropy tuple and `PAL-031` hostile direct-syscall pointer cases are part of the required live evidence plan. Approval covers fixture behavior only; synthetic reports remain non-promotional and cannot replace authenticated live evidence.
