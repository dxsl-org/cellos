# RUNTIME-CONTRACT-APPROVAL

Artifact: `artifacts/runtime-api-contract.md`
Canonical approval input: `artifacts/approval-input-manifest.json`
Approval-input-manifest SHA-256: `85a1aebe52ae15a396a69b6fca6b5fe6eb2fb66b9cb16bf3f66ac5d1aecff8a7`

| Named signer role | Decision | Approval-input-manifest digest | Date | Independence |
|---|---|---|---|---|
| SDK/runtime owner | NOT GRANTED | `85a1aebe52ae15a396a69b6fca6b5fe6eb2fb66b9cb16bf3f66ac5d1aecff8a7` (package verified; security backing and human signature absent) | — | contract owner permitted |
| Security owner | NOT GRANTED | `85a1aebe52ae15a396a69b6fca6b5fe6eb2fb66b9cb16bf3f66ac5d1aecff8a7` (package verified; security backing and human signature absent) | — | independent authority review required |

Both named human signers must explicitly approve this same independently verified manifest digest only after `PAL-019` production entropy/no-`dev-weak-rng` evidence and `PAL-031` bounded caller-owned writable/hostile direct-syscall evidence are complete. Approval accepts only the frozen contract; it does not authorize PAL/runtime work. Any frozen ABI change separately requires 2× explicit confirmation.
