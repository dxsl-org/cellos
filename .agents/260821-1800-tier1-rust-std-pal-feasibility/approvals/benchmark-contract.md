# BENCHMARK-CONTRACT-APPROVAL

Artifacts: `artifacts/workload-parity-spec.md`, `artifacts/benchmark-validator-contract.md`
Canonical approval input: `artifacts/approval-input-manifest.json`
Approval-input-manifest SHA-256: `5036ea3690c3b044566bd8916cd9a7022ef751efd9fc491188b04d0f2d548514`

| Named signer role | Decision | Approval-input-manifest digest | Date | Independence |
|---|---|---|---|---|
| Performance owner | NOT GRANTED | `5036ea3690c3b044566bd8916cd9a7022ef751efd9fc491188b04d0f2d548514` (package verified; security backing and human signature absent) | — | workload owner permitted |
| Independent measurement reviewer | NOT GRANTED | `5036ea3690c3b044566bd8916cd9a7022ef751efd9fc491188b04d0f2d548514` (package verified; security backing and human signature absent) | — | must not author validator/fixtures |

Both named human signers must explicitly approve this same independently verified manifest digest only after the `PAL-019` production entropy tuple and `PAL-031` hostile direct-syscall pointer cases are part of the required live evidence plan. Approval covers fixture behavior only; synthetic reports remain non-promotional and cannot replace authenticated live evidence.
