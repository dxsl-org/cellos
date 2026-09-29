# BENCHMARK-CONTRACT-APPROVAL

Artifacts: `artifacts/workload-parity-spec.md`, `artifacts/benchmark-validator-contract.md`
Canonical approval input: `artifacts/approval-input-manifest.json`
Approval-input-manifest SHA-256: `e30a38266146c443744d515631a7f823a6818b2995223827eec44ceffc5e9d0c`

| Named signer role | Decision | Approval-input-manifest digest | Date | Independence |
|---|---|---|---|---|
| Performance owner (maintainer) | APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT | `e30a38266146c443744d515631a7f823a6818b2995223827eec44ceffc5e9d0c` | 2026-09-16 | workload owner permitted |
| Independent measurement reviewer (maintainer approval recorded) | APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT | `e30a38266146c443744d515631a7f823a6818b2995223827eec44ceffc5e9d0c` | 2026-09-16 | ratified under solo-first governance |

Both named human signers must explicitly approve this same independently verified manifest digest only after the `PAL-019` production entropy tuple and `PAL-031` hostile direct-syscall pointer cases are part of the required live evidence plan. Approval covers fixture behavior only; synthetic reports remain non-promotional and cannot replace authenticated live evidence.

Digest re-bound 2026-09-29 under `PAL-IMPLEMENTATION-CHECKPOINT` condition 6: covered inputs `kernel/Cargo.toml`, `kernel/src/task/syscall.rs`, `libs/api/src/abi/syscall.rs`, `libs/ostd/src/syscall.rs`, `libs/ostd/src/startup.rs` changed after the 2026-09-16 binding. This line records only the digest re-bind; it is not a new signer decision.
