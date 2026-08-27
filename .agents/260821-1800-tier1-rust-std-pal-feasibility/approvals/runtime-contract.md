# RUNTIME-CONTRACT-APPROVAL

Artifact: `artifacts/runtime-api-contract.md`
Canonical approval input: `artifacts/approval-input-manifest.json`
Approval-input-manifest SHA-256: `ddbc1c293416bbd8c73a3e72e81c7b9a09a82db5209f6e6228a151ea40105a8f`

| Named signer role | Decision | Approval-input-manifest digest | Date | Independence |
|---|---|---|---|---|
| SDK/runtime owner | NOT GRANTED | `ddbc1c293416bbd8c73a3e72e81c7b9a09a82db5209f6e6228a151ea40105a8f` (package and GetRandom technical backing verified; human signature absent) | — | contract owner permitted |
| Security owner | NOT GRANTED | `ddbc1c293416bbd8c73a3e72e81c7b9a09a82db5209f6e6228a151ea40105a8f` (package and GetRandom technical backing verified; human signature absent) | — | independent authority review required |

Both named human signers must explicitly approve this same independently verified manifest digest. PAL-019 production zero/error evidence and PAL-031 bounded caller-owned writable/hostile direct-syscall evidence are complete and bound; approval accepts only the frozen contract and does not authorize PAL/runtime work. Any frozen ABI change separately requires 2× explicit confirmation.
