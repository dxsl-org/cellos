# COMPILER-INTEGRATION-APPROVAL

Artifact: `artifacts/compiler-strategy-decision.md`
Canonical approval input: `artifacts/approval-input-manifest.json`
Approval-input-manifest SHA-256: `f33b902c0e38a19782d9d8d7785feda829e46fc2a91dbde8cf02e510b4a6a405`

| Named signer role | Decision | Approval-input-manifest digest | Date | Independence |
|---|---|---|---|---|
| Compiler/toolchain owner (maintainer) | APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT | `f33b902c0e38a19782d9d8d7785feda829e46fc2a91dbde8cf02e510b4a6a405` | 2026-09-16 | implementation owner permitted |
| Independent PAL reviewer (maintainer approval recorded) | APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT | `f33b902c0e38a19782d9d8d7785feda829e46fc2a91dbde8cf02e510b4a6a405` | 2026-09-16 | ratified under solo-first governance |

This record approves nothing until both rows name a human signer, say `APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT`, bind this same independently verified manifest digest/date, satisfy independence, and confirm the exact kernel security-backing path set plus the no-`dev-weak-rng` production tuple are bound into the compiler/sysroot/kernel evidence tuple. It never authorizes target publication or promotion.

Digest re-bound 2026-09-29 under `PAL-IMPLEMENTATION-CHECKPOINT` condition 6: covered inputs `kernel/Cargo.toml`, `kernel/src/task/syscall.rs`, `libs/api/src/abi/syscall.rs`, `libs/ostd/src/syscall.rs`, `libs/ostd/src/startup.rs` changed after the 2026-09-16 binding. This line records only the digest re-bind; it is not a new signer decision. Re-bound a third time the same day for `.agents/260821-0642-app-tiers-completion/plan.md` and `phase-06-tier1-rust-std-pal.md`, whose Phase 06 text now records the bounded implementation (PAL, target specs, sysroot overlay, QEMU lane) while promotion and approvals stay blocked. Re-bound again the same day for `tests/rust-std-promotion/test_validator.py` and `test_validator_rejections.py`, which now resolve the pinned rust-src through the installed toolchain instead of the maintainer's absolute path (both files are pinned approval inputs).
