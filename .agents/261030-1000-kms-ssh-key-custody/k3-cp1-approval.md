# KSSH K3 CP1 approval record (Law-1 confirmation 1 + lock handoff)

Recorded 2026-10-30. The accountable maintainer/owner (the user) answered **YES** to: *approve `k3-cp1.md` as the design for adding `CALLER_FLAG_KMS_SSH_SIGN = 1 << 6` and `CALLER_FLAG_KMS_SSH_ADMIN = 1 << 7` to `libs/api/src/abi/caller_identity.rs`, with the kernel closure of section 10 (12 source/script paths plus `CHANGELOG.md` at CP2), policy v8, O-11 = false, H-1 = yes, D-K3-4 = B, O-K3-6 = host + regression lanes, and the K5 admission design of section 7; and approve the section-11 lock handoff (bounded adopted slice, file-bounded exception, row `260603-1803-phase-h-vfs-hardening` and controlling P05; serves Cellbox publickey, not an Intel C2C outcome)*.

Rulings recorded with the approval: the accountable owner is named as the P05 owner/delegation for K3; the rows marked "Check" (`260621-0830`, `260809-0922`, `260927-0739`) and the four unexamined plans are re-checked by the integrating owner at slice start and the slice stops if one claims a closure path; the scheduling record is made in `cell-contract-0024-locks.md`, the controlling plan and `plan-portfolio.md` (same commit).

**Authorizes:** the design (Law-1 confirmation 1) and the file-bounded exception for the K3 §10 paths. **Does not authorize:** confirmation 2, any file outside §10, `Cargo.lock`, network, keys, VM, images, signing routes, K5 edits. Pinned unchanged files (`targets.rs`, `launch_profile/mod.rs`, `ipc.rs`, `libs/types/src/kms/model.rs`) stay byte-identical. Before edits: baseline capture of host tests (kernel default and `phase04-driver-profile`, `-p api`).

Source HEAD at binding: `580ff68a8df1488190721d78e9629800bb53a933` (`git status --short`: clean). Pre-edit hashes of the closure: `k3-cp1.md` section 10 (HEAD `fad076ee9`); re-taken at the baseline step.

Digest form: SHA-256 of whole files incl. final newline, no normalization, none stored inside the hashed files.

## Bound documents
| Path | SHA-256 |
|---|---|
| `.agents/261030-1000-kms-ssh-key-custody/k3-cp1.md` | `f3141cf41445921dfc6fc448c7dc231f293e7694a03aaeeec6d6ab764a67519e` |
| `.agents/261030-1000-kms-ssh-key-custody/plan.md` | `1713321e69a3231c0d15105dc013003d1698c70a9246192223a284b8fced8160` |
| `.agents/261030-1000-kms-ssh-key-custody/k2-cp1.md` | `8c2421a6ad6cc3462ee20b5328f6abe6d635f8d5cea23927a4ff7b6947f1ace1` |
| `.agents/261030-1000-kms-ssh-key-custody/k2-cp1-approval.md` | `c5dfdb9d5525f6cdae914c2a7434c1a04a84cf63c69d914473a61d0e9d97b33b` |
| `.agents/261030-1000-kms-ssh-key-custody/k2-cp2-approval.md` | `aa00ae484fd407647255412250ba48648148c9e64f76da7909fa6d41750b4846` |
| `.agents/261030-1000-kms-ssh-key-custody/k3-k8-bundle.md` | `62f587466e64ad7b3abc284dc39f098a50ca4ebf20c961a43b5c31c9e08b1d84` |
| `docs/code-standards.md` | `f7081cc1b41a417913f0f6111767b10410690f4c999ff3cab475f5db90c0769e` |
