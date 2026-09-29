# RUNTIME-CONTRACT-APPROVAL

Artifact: `artifacts/runtime-api-contract.md`
Canonical approval input: `artifacts/approval-input-manifest.json`
Approval-input-manifest SHA-256: `f33b902c0e38a19782d9d8d7785feda829e46fc2a91dbde8cf02e510b4a6a405`

| Named signer role | Decision | Approval-input-manifest digest | Date | Independence |
|---|---|---|---|---|
| SDK/runtime owner (maintainer) | APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT | `f33b902c0e38a19782d9d8d7785feda829e46fc2a91dbde8cf02e510b4a6a405` | 2026-09-16 | contract owner permitted |
| Security owner (maintainer approval recorded) | APPROVED_FOR_LATER_IMPLEMENTATION_CHECKPOINT | `f33b902c0e38a19782d9d8d7785feda829e46fc2a91dbde8cf02e510b4a6a405` | 2026-09-16 | ratified under solo-first governance |

Both named human signers must explicitly approve this same independently verified manifest digest. PAL-019 production zero/error evidence and PAL-031 bounded caller-owned writable/hostile direct-syscall evidence are complete and bound; approval accepts only the frozen contract and does not authorize PAL/runtime work. Any frozen ABI change separately requires 2× explicit confirmation.

Digest re-bound 2026-09-29 under `PAL-IMPLEMENTATION-CHECKPOINT` condition 6: covered inputs `kernel/Cargo.toml`, `kernel/src/task/syscall.rs`, `libs/api/src/abi/syscall.rs`, `libs/ostd/src/syscall.rs`, `libs/ostd/src/startup.rs` changed after the 2026-09-16 binding. This line records only the digest re-bind; it is not a new signer decision. Re-bound a third time the same day for `.agents/260821-0642-app-tiers-completion/plan.md` and `phase-06-tier1-rust-std-pal.md`, whose Phase 06 text now records the bounded implementation (PAL, target specs, sysroot overlay, QEMU lane) while promotion and approvals stay blocked. Re-bound again the same day for `tests/rust-std-promotion/test_validator.py` and `test_validator_rejections.py`, which now resolve the pinned rust-src through the installed toolchain instead of the maintainer's absolute path (both files are pinned approval inputs).
