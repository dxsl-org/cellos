# RUNTIME-CONTRACT-APPROVAL

Artifact: `artifacts/runtime-api-contract.md`
Canonical approval input: `artifacts/approval-input-manifest.json`
Approval-input-manifest SHA-256: `5036ea3690c3b044566bd8916cd9a7022ef751efd9fc491188b04d0f2d548514`

| Named signer role | Decision | Approval-input-manifest digest | Date | Independence |
|---|---|---|---|---|
| SDK/runtime owner | NOT GRANTED | `5036ea3690c3b044566bd8916cd9a7022ef751efd9fc491188b04d0f2d548514` (package verified; security backing and human signature absent) | — | contract owner permitted |
| Security owner | NOT GRANTED | `5036ea3690c3b044566bd8916cd9a7022ef751efd9fc491188b04d0f2d548514` (package verified; security backing and human signature absent) | — | independent authority review required |

Both named human signers must explicitly approve this same independently verified manifest digest only after `PAL-019` production entropy/no-`dev-weak-rng` evidence and `PAL-031` bounded caller-owned writable/hostile direct-syscall evidence are complete. Approval accepts only the frozen contract; it does not authorize PAL/runtime work. Any frozen ABI change separately requires 2× explicit confirmation.
