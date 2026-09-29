# COMPILER-INTEGRATION-APPROVAL

Artifact: `artifacts/compiler-strategy-decision.md`
Canonical approval input: `artifacts/approval-input-manifest.json`
Approval-input-manifest SHA-256: `e30a38266146c443744d515631a7f823a6818b2995223827eec44ceffc5e9d0c`

| Named signer role | Decision | Approval-input-manifest digest | Date | Independence |
|---|---|---|---|---|
| Compiler/toolchain owner (maintainer) | APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT | `e30a38266146c443744d515631a7f823a6818b2995223827eec44ceffc5e9d0c` | 2026-09-16 | implementation owner permitted |
| Independent PAL reviewer (maintainer approval recorded) | APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT | `e30a38266146c443744d515631a7f823a6818b2995223827eec44ceffc5e9d0c` | 2026-09-16 | ratified under solo-first governance |

This record approves nothing until both rows name a human signer, say `APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT`, bind this same independently verified manifest digest/date, satisfy independence, and confirm the exact kernel security-backing path set plus the no-`dev-weak-rng` production tuple are bound into the compiler/sysroot/kernel evidence tuple. It never authorizes target publication or promotion.

Digest re-bound 2026-09-29 under `PAL-IMPLEMENTATION-CHECKPOINT` condition 6: covered inputs `kernel/Cargo.toml`, `kernel/src/task/syscall.rs`, `libs/api/src/abi/syscall.rs`, `libs/ostd/src/syscall.rs`, `libs/ostd/src/startup.rs` changed after the 2026-09-16 binding. This line records only the digest re-bind; it is not a new signer decision.
