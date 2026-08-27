# BENCHMARK-CONTRACT-APPROVAL

Artifacts: `artifacts/workload-parity-spec.md`, `artifacts/benchmark-validator-contract.md`
Canonical approval input: `artifacts/approval-input-manifest.json`
Approval-input-manifest SHA-256: `ddbc1c293416bbd8c73a3e72e81c7b9a09a82db5209f6e6228a151ea40105a8f`

| Named signer role | Decision | Approval-input-manifest digest | Date | Independence |
|---|---|---|---|---|
| Performance owner | NOT GRANTED | `ddbc1c293416bbd8c73a3e72e81c7b9a09a82db5209f6e6228a151ea40105a8f` (package and GetRandom technical backing verified; human signature absent) | — | workload owner permitted |
| Independent measurement reviewer | NOT GRANTED | `ddbc1c293416bbd8c73a3e72e81c7b9a09a82db5209f6e6228a151ea40105a8f` (package and GetRandom technical backing verified; human signature absent) | — | must not author validator/fixtures |

Both named human signers must explicitly approve this same independently verified manifest digest only after the `PAL-019` production entropy tuple and `PAL-031` hostile direct-syscall pointer cases are part of the required live evidence plan. Approval covers fixture behavior only; synthetic reports remain non-promotional and cannot replace authenticated live evidence.
