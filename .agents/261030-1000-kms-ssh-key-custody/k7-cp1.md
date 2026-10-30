# KSSH K7 CP1 (final text, amendment form after K3/K5/K6 landed; HEAD 297a89a9e)

Base: [k7-cp1-draft.md](k7-cp1-draft.md) (SHA-256 9fcf576264fdd25cdc955ce377bda7a4655c86aaf79c7349d99acbafcb00f277) is the design of record (sections 1-3 and 5-9: option (b), grammar, frame sequence, armor rules, outputs, risks, tests). This text records only what changed since K3 CP2 (47a9d5bda), K5 CP2 (236b79915) and K6 landed, and the exact remaining closure.

## 1. Decisions carried
O-2 = (b) dedicated admin cell `/bin/kms-ssh-admin`; O-K7-1 reading A of command-contracts.md:142 confirmed by the user (cellbox command surface); O-K7-2 satisfied: K3 already carries CALLER_FLAG_KMS_SSH_ADMIN (1<<7) in policy v8. O-K7-3..7 as recommended in the draft (console-only import, 98,304 B cap, base64ct, (c) outside program, typed-word delete).

## 2. Delta against the draft (what already exists)
- NO Law-1 surface: libs/api flag constants, CapSet field, TCB field, attested_identity_of, policy v8 and audit bit already landed in K3 (confirmation 2 recorded in k3-cp2-approval.md). K7 edits no libs/api or libs/types file.
- K5 admission already refuses admin to everyone but a caller holding the K3 admin flag; admin flag never co-held with sign.
- K3 left the admin path ungrantable: no launch edge, boot row or policy row. K7 adds exactly that.
- K6 landed admin-capable KmsClient methods in libs/ostd/src/clients/kms/ssh.rs (no edit by K7 unless an admin method is found missing, then STOP and amend).

## 3. Exact file closure (hashes re-taken at CP2 start; any other path stops the slice)
| Path | Action |
|---|---|
| kernel/src/loader/launch_profile/profiles.rs | edit: Path-only edge row for /bin/kms-ssh-admin with a KMS_SSH_ADMIN ceiling (LEGACY + admin right, no sign), constant kept in this file as K3 did (targets.rs and mod.rs stay byte-identical) |
| kernel/src/loader/launch_profile/tests.rs | edit: only the shell edge launches it; init/desktop/terminal/ELF route refused; admin and sign never on one ceiling |
| kernel/src/loader/boot_ceiling.rs, boot_ceiling/selftest.rs | edit only if the boot-ceiling table needs the admin ceiling literal (else byte-identical); decided and reported at CP2 |
| scripts/sign-policy.py | edit: v8 row and request helper for /bin/kms-ssh-admin |
| scripts/qemu-phase04-terminal-test.sh | digest pins of the changed files only |
| kernel/src/task/cap.rs | edit only if a path request helper for the admin path is missing (K3 named /bin/kms-ssh-admin in with_path_kms_ssh_rights); else byte-identical |
| Cargo.toml (workspace members) | edit: add cells/tools/kms-ssh-admin |
| Cargo.lock | edit: one new local package stanza; no external package (ostd, api, sha2, zeroize, base64ct already locked); dev-dependency on service-kms (feature ssh-dev-custody) for the real-KMS integration test (K6 deferral, O-K6-9) is a local-path edge only |
| cells/tools/kms-ssh-admin/** | new: Cargo.toml, build.rs, src/{main,armor,import,delete,status}.rs, tests |
| CHANGELOG.md | entry at CP2 |
Not in closure: cells/services/kms/** (K5, closed), libs/api, libs/types, libs/ostd, cells/tools/cellbox, assemble-x86-image.py canonical profiles, kernel/src/main.rs, images, signing, promoted POLICY.BIN (v7 stays).

## 4. Lock handoff (amendment to K3, Phase 02 A1 pattern)
Slice: KSSH K7, admin import surface for the single ed25519 SSH identity; serves Cellbox publickey, not an Intel C2C outcome. Locked relations: row 260603-1803-phase-h-vfs-hardening and controlling P05 for the kernel/script paths only (as K3). File-bounded exception for the section 3 paths; host and regression lanes only, DEV_POSTURE; no network, no key generation (test seeds only), no QEMU/VM/image; WIP one slice; rollback = source reversal. The new crate path is in no inventory row [INFERENCE: not exhaustively grepped; re-checked at slice start].

## 5. Tests (host)
As draft section 6: armor vectors (own synthetic fixtures; the OpenSSH reference blobs are not copied into the repo), chunk driver frames, delete confirmation, output text, error mapping, class guard (no ops 21-24), launch-profile tests, plus the integration test of the admin driver and the K6 client against the real service-kms state machine (feature ssh-dev-custody). Baselines re-captured at CP2 start. Lane markers belong to K8.

## 6. Entry gates
This CP1 + handoff approved; start-of-slice overlap re-check; K8 for any runtime claim.
