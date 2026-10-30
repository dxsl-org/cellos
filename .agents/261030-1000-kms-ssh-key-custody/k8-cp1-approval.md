# K8 CP1 (T0 lane) + ownership handoff approval

Date: 2026-10-30. Approver: accountable owner (user), "Duyệt K8 CP1 + handoff theo khuyến nghị" (D8-1..7 as recommended). HEAD a12fd2d06.
- Text: k8-cp1.md SHA-256 5366add455ac9b7766f0f645eaba25afb3c8a0edbddf5e83d546ba55c34517e0. Authorizes exactly section 18 A-D and nothing in E: private lane edits (section 4), H0, B0 (<=3 boots per variant), one RT0 and one RT0f evidence boot, no rerun; no image build/pull, no network, no KVM, no promotion, no T1/RT1, no RG; TEST keys under /tmp/cellbox-phase09-kssh-*; no commit of keys/images; DEV_POSTURE; QEMU/TCG only.
- No locked relation in the edit closure (grep, [INFERENCE]); no Law-1 file.
- CBX-24 disposition ("publickey half met at DEV_POSTURE ceiling") is made only by an explicit user disposition after K8 CP2.

## Amendment 2026-10-30 (user): Docker
The user authorized starting Docker Desktop (and using docker.exe / WSL integration) on this machine so the existing local image can be used. Still not authorized: image build, pull, any network fetch beyond what starting Docker Desktop itself does.

## Amendment 2026-10-30 (user): F1 signing gate
The repo-wide F1 gate of `scripts/cellos-sign --sign` refuses at HEAD (9 violations; also for the stock L5/L5f lane). Decision: (1) the K7 defect (`app-kms-ssh-admin`: missing `#![forbid(unsafe_code)]`, `unsafe` keyword in `src/tests/armor.rs`) is fixed in the K7 closure (additive amendment to k7-cp1-approval.md); (2) the other 8 items (cells/tests/phase02-session-child, phase02-session-parent, phase03-filter, phase04-terminal-driver, top-load; service-input src/session.rs; app-cellbox src/rg/tests.rs) are pre-existing and outside the slice: K8 images are signed with a private per-run allowlist `$W/unsafe-allowlist.toml` (repo allowlist + explicit entries for exactly those items, kssh mode only, diff saved in evidence). The repo allowlist, crate roots and signing code stay unchanged. The dossier states "F1 not satisfied by the tracked tree at HEAD; K8 images signed under a private per-run waiver for these items".
