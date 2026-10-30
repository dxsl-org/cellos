# KSSH K3..K8 consolidated CP1 bundle (2026-10-30; documents only; nothing approved, no source edited)

Requested by the user ("xử lý luôn K3 -> K8"). Each slice keeps its own CP1/CP2, Law-1 checkpoints and lock handoff; this bundle sequences them, lists the cross-slice decisions and shows where the drafts depart from earlier decisions. Drafts: [K3](k3-cp1-draft.md), [K4b](k4b-cp1-draft.md), [K5](k5-cp1-draft.md), [K6](k6-cp1-draft.md), [K7](k7-cp1-draft.md), [K8](k8-cp1-draft.md). Each draft has its own "not checked" list.

## Order and gates
| Order | Slice | Touches | Gate before code | Proves |
|---|---|---|---|---|
| 1 | K3 kernel-attested flag(s) | `libs/api` (Law 1), kernel permission/launch/policy files, `scripts/sign-policy.py`, boot self-test | Law-1 confirmation 1 on the K3 text, lock handoff (row `260603-1803-phase-h-vfs-hardening`, controlling P05) | host/regression lanes: grant table, trailer bit, policy v8 + signer parity, edge ceiling |
| 2 | K5 KMS behaviour (T0 volatile) | `cells/services/kms/**`, `Cargo.toml`, shared `Cargo.lock` (three-package K1 diff) | K1 done, K3 constant, lock handoff (conflicts C-K5-1..3 with `260826-1605`), crate availability authorization | host tests: parser, import/unlock/sign, refusal matrix rows 2-34 |
| 3 | K6a/K6b Cellbox client | `libs/ostd`, `libs/vissh`, `cells/tools/cellbox`, K6b `libs/terminal` completion slot | none locked, Cellbox single owner | host tests, then L5 at K8 |
| 4 | K7 admin import | new admin cell `/bin/kms-ssh-admin`, flag bit 1<<7 (needs to be in K3's change) | K3, K5; Law-1 only if a new flag changes `libs/api` | import path |
| 5 | K4b T1 persistence (DEV_POSTURE) | new files under `cells/services/kms/src/ssh/` + feature wiring | after K5 and the T0 lane; design approved now, built later | survives QEMU reboot; rollback shown unprotected |
| 6 | K8 QEMU evidence | private fixture images only; no canonical promotion; private `CARGO_TARGET_DIR` | K3, K5, K6, K7 done (+K4b for T1) | CBX-24 publickey half at DEV_POSTURE ceiling |

## Cross-slice decisions that need the user (recommendations in brackets)
1. **K3 O-11** legacy policy decode of the new right: false [yes, false-closed]. **H-1** also strip the right when a loaded policy has no row for the path [yes]. **D-K3-4** carry both flag bits (`1<<6` sign, `1<<7` admin) in one policy bump now [yes if K7 option (b) is accepted; avoids a second v9 bump]. **O-K3-6** stop at host+regression lanes [yes; the end-to-end flag proof happens at K8].
2. **K5 deviations from approved wording:** (a) drop `base64ct` from the three-package lock diff [yes: KMS receives binary blobs only; deviates from "no other line changes"]; (b) golden vectors (from `ssh-key` fixtures + OpenSSL signatures) instead of `ssh-key` as a dev-dependency differential test, which would add lock edges and `cargo deny` warnings [yes; deviates from D-K4 wording]; (c) opt-in feature `ssh-dev-custody`, off by default, `compile_error!` with `hardware-relay-provider`, `build.rs` refuses `CELLOS_PRODUCTION` [yes]; (d) KDF acceptance threshold ≤10 s at 64 rounds on QEMU/TCG, to be measured at CP2 [yes]; (e) explicit authorization to copy/fetch the three crates (they exist only in `/tmp/kssh-k1`) [needs your network/offline-copy permission at K5 CP2].
3. **K6:** split K6a (client + publickey login) and K6b (pipeline exec with a completion slot in `libs/terminal`) with separate gates [yes]; KMS identity check before connecting [yes].
4. **K7:** option (b) dedicated admin cell `/bin/kms-ssh-admin` with its own flag [recommended]; your reading that contract line 142 ("no import command ... in the CLI") means the cellbox command surface, so an external admin program does not violate it [needs your confirmation].
5. **K4b:** IN, sequenced after K5 and the T0 lane; own `SshBlobIo` trait, no `KmsProviderKind` append, no `storage.rs` edit (avoids the Phase 06 stub conflict) [yes: approve the design now, build later].
6. **Run budgets:** K8 one evidence run per set, no rerun, classes A-E; every K3-K8 bare-metal build uses a private `CARGO_TARGET_DIR` because the S10 census includes 52 files under `target/x86_64-unknown-none/release`.

## What stays true whatever is approved
No canonical or promoted byte is touched; no production, hardware, remote or crash-durability claim; T0/T1 are `DEV_POSTURE`; `cellbox ssh --auth publickey` stays refused until K6 lands and K8 evidence exists; CBX-24 stays partial until a later explicit disposition.
