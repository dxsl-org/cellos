# KSSH K5 CP1 approval record (CP1 + lock handoff; no Law-1 object)

Recorded 2026-10-30. The accountable owner (the user) approved **K5 CP1 and its lock handoff**, accepting D-5.2 (stored budget: wrapped ciphertext <= 2,048 B, Plain stores only the seed, a larger wrapped container answers `SshStoreFull`, narrowing the stored set of the contract's 64 KiB import), D-5.10 (public `ssh-key` test blobs embedded as hex with attribution after a licence glance) and D-5.3 fail-closed (admin requires the K3 admin flag; the supervisor is not an admin caller until K7); plus the bundle decisions: opt-in `ssh-dev-custody` (default off), golden vectors instead of an `ssh-key` dev-dependency (recorded deviation from D-K4 wording), `base64ct` dropped from the lock diff (recorded deviation from "no other line changes"), KDF acceptance <= 10 s at rounds 64 on QEMU/TCG (measured at CP2), `ed25519-dalek 3.0.0`, fuzz target included and run only when authorized, the three crates copied **offline from `/tmp/kssh-k1`** (no further network; if missing, stop and ask).

Rulings: **C-K5-1** K5 lands first on the five shared files (`Cargo.toml`, `build.rs`, `lib.rs`, `dispatch.rs`, `tests.rs`) and `260826-1605` Phase 06 rebases on them (reciprocal note recorded in that plan); **C-K5-2** later provider/stub replacement keeps opcodes 15-24 unchanged; **C-K5-3** `Cargo.lock` is a named exception admitting exactly the amended K1 diff (three packages, no `base64ct` line) and any plan with an uncommitted lock edit rebases first.

**Gate:** K5 CP2 starts only after K3 CP2 lands the `libs/api` constants (D-5.7) and a baseline is captured; start-of-slice overlap re-check of the four unexamined plans; stop on any claim of a closure path. **Does not authorize:** files outside `k5-cp1.md` section 3, network, key generation, VM, images, K6/K7 edits, any production/hardware claim.

Source HEAD at binding: `580ff68a8df1488190721d78e9629800bb53a933`. Digest form: SHA-256 of whole files incl. final newline.

## Bound documents
| Path | SHA-256 |
|---|---|
| `.agents/261030-1000-kms-ssh-key-custody/k5-cp1.md` | `6ff24401d391681b59a21610f6b8e6cb845c48caa21f182f69c960853e4d3d3e` |
| `.agents/261030-1000-kms-ssh-key-custody/k4b-cp1-draft.md` | `bd117f72ca32bd11b9f63aa9c6c816f760c5351bf9e217990d8f20527888e1ff` |
| `.agents/261030-1000-kms-ssh-key-custody/plan.md` | `1713321e69a3231c0d15105dc013003d1698c70a9246192223a284b8fced8160` |
| `.agents/261030-1000-kms-ssh-key-custody/k2-cp1.md` | `8c2421a6ad6cc3462ee20b5328f6abe6d635f8d5cea23927a4ff7b6947f1ace1` |
| `.agents/261030-1000-kms-ssh-key-custody/k2-cp1-approval.md` | `c5dfdb9d5525f6cdae914c2a7434c1a04a84cf63c69d914473a61d0e9d97b33b` |
| `.agents/261030-1000-kms-ssh-key-custody/k2-cp2-approval.md` | `aa00ae484fd407647255412250ba48648148c9e64f76da7909fa6d41750b4846` |
| `.agents/261030-1000-kms-ssh-key-custody/k3-k8-bundle.md` | `62f587466e64ad7b3abc284dc39f098a50ca4ebf20c961a43b5c31c9e08b1d84` |
| `docs/code-standards.md` | `f7081cc1b41a417913f0f6111767b10410690f4c999ff3cab475f5db90c0769e` |
