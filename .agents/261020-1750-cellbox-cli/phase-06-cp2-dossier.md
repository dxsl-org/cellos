# Phase 06 CP2 dossier: implementation review request

**Status: CP2 APPROVED by the user on 2026-10-24 for all nine items (S6-1, S6-3, S6-2, S6-4, S6-5, V, U, Z, Q), option A of §19; approval record in §20.** Five slices (S6-1, S6-3, S6-2, S6-4, S6-5) and four extra fixes landed inside the Phase 06 window (V: VFS idle busy-loop with the terminal-SDK `CwdLease`; U: kernel UART console `USER+ ` continuation; Z: `sleep` real seconds; Q: `$(…)` status propagation and `$(top)` refusal). Each has its own approval line. An approval makes **only that item's listed regions**, at the exact file hashes in §3, part of the Phase 06 implementation baseline. Quarantine, canonical packaging, release signing and rollout stay **BLOCKED** under [containment](reports/rollout-containment.md).

Prepared 2026-10-24 from the current disk bytes (HEAD `3471028bc`, nothing committed) and the saved evidence. The only commands run were `sha256sum`/byte counts, read-only `git status`/`ls-tree`, an mtime scan, greps of the archived logs and of the `/tmp` run directories' timestamps, and a scan of the Phase 06 implementer transcripts (to attribute edits). Nothing was built, tested, signed or booted. No source file was changed. The only other doc edit is in [phase-06-cell-top.md](phase-06-cell-top.md): the four criteria checked, status `implemented-private-evidence`, and one pointer line.

Approval recorded 2026-10-24 (§20). At recording time all 62 paths in §3 (re-hashed) still matched their listed SHA-256, and the final-tree evidence gap was closed (§5). After recording, `CHANGELOG.md` received the doc-only reconciliation edits of §18; those later bytes are outside the approved §3 hash.

## 1. Authority chain

| Item | CP1 / decision record | Approval wording | Gaps this CP2 must close |
|---|---|---|---|
| **S6-1** telemetry provider | [Entry dossier](phase-06-entry-dossier.md) §5, §6 | User accepted every §8 recommendation 2026-10-23: D60-1..6, D61-1..8 (K1, A1, P1, S1, E1, syscall 430 on bit 55), **DV-1..DV-8 ratified** (D61-7), CP1 approved as written (D61-8) | Implementation deviations 1–5 (entry §10) have no ratification line. The §9 K1 before/after switch-overhead measurement was replaced by the D65-10 absolute bench (approved) |
| **S6-3** engine Frames | [S2-S3 proposal](phase-06-s2-s3-proposal.md) §3, §6 | D60-7 (S6-3 before S6-2), D63-4 (**DV-12**), D63-5 accepted 2026-10-23; D63-1..3 accepted at entry | Implementation deviations 1–2 (§10.2) have no ratification line |
| **S6-2** model + cutover | [S2-S3 proposal](phase-06-s2-s3-proposal.md) §4–§6 | D62-1..4 accepted 2026-10-23, **DV-9..DV-11 ratified** (D62-3); (A) P03 kill victim = `init` **ratified** | Implementation deviations 3–9 (§10.2) have no ratification line |
| **S6-4** interactive `top` | [S4 proposal](phase-06-s4-proposal.md) §2–§7 | D64-5..D64-18 accepted 2026-10-23; after the runs the user **ratified** deviation 9 (picker `cell_id`), the 500-tenth idle threshold (part of deviation 12) and Ctrl-C → 130 | Deviations 1–8, 10, 11 and the other runner choices of 12 have no ratification line; the driver `TUI_ENTRY` fix after run `JSM1uq` is a recorded fact only |
| **S6-5** fixture + runtime proof | [S5 proposal](phase-06-s5-proposal.md) §2–§7 | D65-3..D65-12 accepted 2026-10-23; **DV-13 ratified** | All 13 implementation deviations (§10.3, “for ratification with the run”) have no ratification line |
| **V** VFS idle busy-loop | [Phase log](phase-06-cell-top.md) 2026-10-23 | “User approved fixing the `vfs` idle busy-loop (~98% CPU at idle in `top`)” | No CP1 text. The external-child follow-up changed the terminal SDK (`CwdLease`) inside two Phase 05 CP2-approved files; that SDK change has no separate approval line |
| **U** UART console prefix | [Phase log](phase-06-cell-top.md) (S6-4 CP1 entry) | “the user approved fixing the kernel console defect … as a separate change” | No CP1 text. The `USER+ ` marker format is recorded as the parent's decision; the rejoin edits in four non-runner scripts are not named in the approval |
| **Z** `sleep` real seconds | [S2-S3 proposal](phase-06-s2-s3-proposal.md) §10.3 | (B) “the `sleep` fix stays landed as an independent defect fix despite D60-3” — **ratified** | None for `sleep`. `uptime`/`date` stay in D60-3 (own CP1) |
| **Q** `$(…)` status + `$(top)` refusal | [S4 proposal](phase-06-s4-proposal.md) §2 runtime gate (capture depth ≤ 1, so `x=$(top)` refuses with 2) | The gate is CP1-approved (D64-9, D64-18) | Making the gate reachable changed `run_capture` (a `top` arm) and made every failed substitution carry its own status (was always 1) and skip the assignment. These general shell changes are outside any CP1 |

Earlier CP2 baselines that these items modify: Phase 02 CP2, S3-output, S2-T/W and Phase 05 CP2 (see §3). Their approvals cover their recorded hashes only; none extends to the current bytes.

## 2. Method and attribution

- **Phase 06 window.** A file belongs to the review set if its mtime is after the Phase 05 CP2 approval records edit (2026-10-23 13:21–13:22 +07; the first Phase 06 source write is `libs/api/src/abi/telemetry.rs` at 13:25). A repository-wide mtime scan (excluding `target/`, `.git/`, `.agents/`) lists **67** paths: **62 source paths** and 5 gitignored `__pycache__/*.pyc` files (§17).
- **Phase 05 baseline (D60-2).** All 65 Phase 05 CP2 paths were re-hashed. **15 drifted**, and every one of them is in the Phase 06 window and attributed below: `CHANGELOG.md`, `Cargo.lock`, `Cargo.toml`, `cells/tests/phase04-terminal-driver/src/main.rs`, `cells/tools/cellbox/src/{commands,executor,typed}.rs`, `kernel/src/task/cell_session.rs`, `libs/terminal/src/{pipe_child,session}.rs`, `libs/vitui/src/{session,tests}.rs`, `scripts/phase04_gui_drive.py`, `scripts/qemu-phase02-session-test.sh`, `scripts/qemu-phase04-terminal-test.sh`. The other 50 equal their Phase 05 hashes. `cells/tools/cellbox/src/main.rs` has a Phase 06 mtime but **equals** its Phase 05 hash (a `touch` by `S64Fix` to force a rebuild).
- **Earlier baselines outside the window.** Of the other paths with a recorded hash in any CP2 dossier, 158 still equal it. One differs: `kernel/Cargo.toml` (Phase 02 CP2 `bc59c142…`), last written 2026-10-21 20:10, before Phase 05; it carries the Phase 03/04 private-profile features and is not a Phase 06 change. 80 recorded paths no longer exist (the `cells/tools/shell` → `cellbox` move and relative evidence paths), none removed in Phase 06.
- **Cross-check, transcripts.** Every one of the 62 paths was written by a Phase 06 implementer's `edit`/`write` (`S61Impl`, `S63S62Impl` and its children `S62Model`/`S62Runners`, `P03Fix`, `P03Fix2`, `P03Fix3`, `VfsIdle`, `VfsIdle2`, `UartPrefix`, `S64Impl` and its child `TopTuiRunners`, `S64Fix`, `S65Impl`). Exceptions, both explained: `Cargo.lock` (written by Cargo) and `cellbox/src/main.rs` (the `touch`). Main made no direct source edit in the window.
- **Limits.** Most files are untracked or already carry Phase 02–05 changes, so per-slice diffs **cannot be reconstructed**. The regions below are pointers to current line numbers, not exhaustive diffs. **The whole-file hashes are the reviewed bytes.** A shared file's whole-file hash becomes a baseline only when every item that touches it is approved (§19).

## 3. Master inventory (current SHA-256)

Items: S6-1, S6-3, S6-2, S6-4, S6-5, V, U, Z, Q.

| Path | Git | Bytes | SHA-256 (current) | Last hash in a CP2 dossier → now | Items |
|---|---|---:|---|---|---|
| `CHANGELOG.md` | `M` | 314324 | `3962321cbe2ebe67fc8980b4eb793187be22f576f22dd0332b3645bc1e36c3a1` | [Phase 05 CP2](phase-05-cp2-dossier.md) `2203b5e8…` → changed | S6-1, S6-3/S6-2, S6-4, S6-5, V, U, Z |
| `Cargo.lock` | `M` | 83130 | `793edafae7d15bc23eb1fd4ec71b3272f3168aca9eeae33d20679b464ffbb401` | [Phase 05 CP2](phase-05-cp2-dossier.md) `63bf3d8c…` → changed | S6-4, S6-5 |
| `Cargo.toml` | `M` | 8842 | `eaa3ce2a918e5a50217894a24fa7393de8b083c5d6e7ad1147ce1ffcf5d5184b` | [Phase 05 CP2](phase-05-cp2-dossier.md) `9d5122d5…` → changed | S6-5 |
| `cells/services/vfs/src/dir_admission.rs` | `M` | 3274 | `aa852af6b069ce129e9f1763ba6b2d38cdeaae097f83341de238bac1319a0015` | [Phase 02 CP2](phase-02-cp2-dossier.md) `3fccb72d…` → changed | V |
| `cells/services/vfs/src/main.rs` | `M` | 9683 | `17c3ccf534a32b63364f53655813ba0dfe5c20702321d38f560dbcf54b58c7a8` | [Phase 02 CP2](phase-02-cp2-dossier.md) `141362e5…` → changed | V |
| `cells/services/vfs/src/manager/owned_state.rs` | `M` | 2934 | `56e44ae6695350dee0ec8c5321d6e55786b20659d9a9b68d0421d396b7077578` | none | V |
| `cells/services/vfs/src/prepared_cwd.rs` | `??` | 47428 | `6291277e3b0df968be531d7acd89408b4c1bf3d98efc8bce592cb39c8e4f4749` | [Phase 02 CP2](phase-02-cp2-dossier.md) `60bf1610…` → changed | V |
| `cells/tests/phase04-terminal-driver/src/main.rs` | `??` | 103522 | `862499775d9907c6cdb7607aae55d173da37c79a30e6274af3f13f663f108bfb` | [Phase 05 CP2](phase-05-cp2-dossier.md) `216668fe…` → changed | S6-1, S6-2, S6-4, S6-5 |
| `cells/tests/phase04-terminal-driver/src/records.rs` | `??` | 24598 | `737a7a010f31172adb8f77e38d3e73c4976000cd431ec14a70e28c73d076da1c` | none | S6-5 |
| `cells/tests/phase04-terminal-driver/src/screen.rs` | `??` | 22351 | `b546f43d43f7ecf89c5189f6d098db9ed93fa516835fb30be2ec81edd16fd1cd` | none | S6-4, S6-5 |
| `cells/tests/phase04-terminal-driver/src/top_load.rs` | `??` | 40432 | `5e57a522641e72a110e5b7b9d4ce39106b5a2acce9635f63f9c97f1c5ab0e2f7` | none | S6-5 |
| `cells/tests/s2tw-probe/src/main.rs` | `??` | 12273 | `bf0958f0754b8cde839a6afdc48f609565086fc25445d9fe68ebbd7c933a70c6` | [S2-T/W CP2](phase-04-s2tw-cp2-dossier.md) `ec269474…` → changed | S6-1, S6-5 |
| `cells/tests/top-load/Cargo.toml` | `??` | 603 | `5b5da8d926aa194ae1db9c2dd7ef83f3a7f604e0f2fea47fb2e8ff2971db42e1` | none | S6-5 |
| `cells/tests/top-load/build.rs` | `??` | 52 | `309c7f94ddf64d2c2d1fce2f9472d674869c6feb2218ebcadbf4bab5626d5f90` | none | S6-5 |
| `cells/tests/top-load/src/command.rs` | `??` | 7894 | `ced03e506b81995bd602f55393718c6aa953eea6642e31379b3efabd075e4d4d` | none | S6-5 |
| `cells/tests/top-load/src/main.rs` | `??` | 12777 | `14731931916787954b11cf63d88b1e4069a574aa4593332707e333c2939123de` | none | S6-5 |
| `cells/tools/cellbox/Cargo.toml` | `??` | 1094 | `afc374e5c9f3fac15a545e6b9f16b17158d5938443c9878eec137678f88c469d` | none | S6-4 |
| `cells/tools/cellbox/src/cmd_sys.rs` | `AM` | 11107 | `1a9a3180eff37113a85363ca1b36f41d3b43d26643e59621546f0a7e37ce6bd4` | none | Z, S6-4 |
| `cells/tools/cellbox/src/commands.rs` | `AM` | 26631 | `8efcb7ce028ba1b8c500eb6faec261be55cf6a0fbb95ead2ffe49d49ffc0c516` | [Phase 05 CP2](phase-05-cp2-dossier.md) `ae2b6ed7…` → changed | S6-2 |
| `cells/tools/cellbox/src/executor.rs` | `AM` | 153078 | `db3ff92e4494942594cf7c16bb6a1c3f5568b42fd02ffb84accfd89e2b631bd2` | [Phase 05 CP2](phase-05-cp2-dossier.md) `daa6f064…` → changed | S6-3, S6-2, S6-4, Q |
| `cells/tools/cellbox/src/main.rs` | `AM` | 7673 | `9a923bd1e08af140a26d0e79d07b6aa55ae08a88406973c148c2ecdc308acc0a` | [Phase 05 CP2](phase-05-cp2-dossier.md) `9a923bd1…` → **equal** | none (touched, bytes = Phase 05) |
| `cells/tools/cellbox/src/native_rows.rs` | `??` | 10718 | `de097aaaa03bc85e2ffc78afb83cba00f1978f409899cb546977ce1e6d5d5029` | none | S6-3, S6-2, S6-4 |
| `cells/tools/cellbox/src/shell_test.rs` | `AM` | 41332 | `1af74e4773d57127cd7e112edc95b989bea891b2b9510888f12fc788ab31254b` | none | S6-2, S6-4, Q |
| `cells/tools/cellbox/src/top.rs` | `AM` | 6343 | `fff763b7ab9c7a3debabd4a400f486e968f1d058d9d040595c39bb4e4760cf7a` | none | S6-2, Z, S6-4 |
| `cells/tools/cellbox/src/top/render.rs` | `AM` | 640 | `e9259ed88c57f21e32368cb0d371293d69d72eb5e9595e36c08cf3554a206d60` | none | S6-2, S6-4 |
| `cells/tools/cellbox/src/top/tests.rs` | `??` | 25900 | `7d84a49a19e5ddcd74f0a5d7fac4ca7f7507dbab1ed43bce297d5f3c3b46add1` | none | S6-4 |
| `cells/tools/cellbox/src/top/tui.rs` | `??` | 11030 | `682bbf719c81f4ac9252de592092fd76dafdff32e6065de7409f179fba24a731` | none | S6-4 |
| `cells/tools/cellbox/src/top/view.rs` | `??` | 20578 | `0dcaefdad80fea3f5f2d65dd6809d9d1c579880667c51183d4a12a937c4eb2d1` | none | S6-4 |
| `cells/tools/cellbox/src/typed.rs` | `??` | 95633 | `d1e30c222d5f96d7b4ccd465bb85a1871ca1623239aa15753f50d06d1073073b` | [Phase 05 CP2](phase-05-cp2-dossier.md) `b5cb932e…` → changed | S6-3 |
| `kernel/src/memory/cell_quota.rs` | `M` | 19143 | `b5eb91f93ac6fc6353a4facdf2224a37731a85482bfe4fc1719c703e2583fb36` | [Phase 02 CP2](phase-02-cp2-dossier.md) `feab58ac…` → changed | S6-1 |
| `kernel/src/task.rs` | `M` | 123826 | `a27bc5d110f03cb3db428d6013ea3c9ac3a1bf0426cc099283944a3ba06d512d` | [Phase 02 CP2](phase-02-cp2-dossier.md) `70885cb4…` → changed | S6-1 |
| `kernel/src/task/cell_session.rs` | `??` | 127334 | `3035e4291da7b177bc01594b601604b08114b86edceca5a5dac684ca68fd6328` | [Phase 05 CP2](phase-05-cp2-dossier.md) `2676534f…` → changed | S6-1 |
| `kernel/src/task/drivers/uart.rs` | `M` | 26445 | `f716e9641c94513d53e49066d40deb82a7fd4e2a661914182950851f55fdff31` | [Phase 02 CP2](phase-02-cp2-dossier.md) `b70d23ff…` → changed | U |
| `kernel/src/task/scheduler.rs` | `M` | 133680 | `2065efd47e19277b6dd22a6bbeb5665cf1426015720388165d1507f88cf1b82a` | [Phase 02 CP2](phase-02-cp2-dossier.md) `e891b336…` → changed | S6-1 |
| `kernel/src/task/smp.rs` | `M` | 48676 | `dd21390398ec2da66c825192095ccbf5ea1e3978f6602576197661f89597e93e` | none | S6-1 |
| `kernel/src/task/syscall.rs` | `M` | 421770 | `c56d500f72ac1adcf68a3cd422cf7c77702ba01449c1992391f757714f170af4` | [S2-T/W CP2](phase-04-s2tw-cp2-dossier.md) `79cd3ec3…` → changed | S6-1 |
| `kernel/src/task/tcb.rs` | `M` | 41023 | `78202648db04c7065b1e9255020648a67d03d6b7d29989b9a6818681aa868e01` | [S2-T/W CP2](phase-04-s2tw-cp2-dossier.md) `65effec5…` → changed | S6-1 |
| `kernel/src/task/telemetry.rs` | `??` | 28130 | `a67465a16198d4c198e66caa9af63f884f7e8c8bbdbbaf3cce388cac0f0797fb` | none | S6-1 |
| `libs/api/src/abi.rs` | `M` | 1116 | `08c1d815fd5e40a9bd3b19a60186afc578f500275c6853913a6bc57229e0e66c` | [Phase 02 CP2](phase-02-cp2-dossier.md) `87c6eb42…` → changed | S6-1 |
| `libs/api/src/abi/syscall.rs` | `M` | 76350 | `b202dfa28f461b2c8f9940901294187e65b6ef4ff94638dc1004c01d78325957` | [Phase 02 CP2](phase-02-cp2-dossier.md) `6589f11c…` → changed | S6-1 |
| `libs/api/src/abi/syscall_tests.rs` | `M` | 24003 | `cb6cdfe1db704fa929f6239dd92445f5a4849e1a190b45d71de7bb0875c7ca4c` | [Phase 02 CP2](phase-02-cp2-dossier.md) `2d32657a…` → changed | S6-1 |
| `libs/api/src/abi/telemetry.rs` | `??` | 21716 | `9e90d932369bd65c216161656b89ae2347d19a8b895b07f4777e6778e6b38cbf` | none | S6-1 |
| `libs/ostd/src/syscall.rs` | `M` | 103035 | `95e4beb7e60561a061a6aed841bf2fc981a7a99e0c54716fcd3db4df7708c944` | [Phase 02 CP2](phase-02-cp2-dossier.md) `0053200d…` → changed | S6-1 |
| `libs/terminal/src/pipe_child.rs` | `??` | 32162 | `52aaf1ee47a897bea055a95ff2cbfa677fe74ded261c9294ae7874ff1ec82c7c` | [Phase 05 CP2](phase-05-cp2-dossier.md) `be90dd97…` → changed | V |
| `libs/terminal/src/session.rs` | `??` | 31002 | `a38aed295ad1fa7d79ed58becfd9cc902ad220ea24ad982078e541c12e03fced` | [Phase 05 CP2](phase-05-cp2-dossier.md) `b964ec1a…` → changed | V |
| `libs/text-engine/Cargo.toml` | `M` | 1058 | `9d6e91faf0b92e1bda79993b338875754a8f6d7151ce2d8e00ba098881f6edb0` | none | S6-2 |
| `libs/text-engine/src/pipeline.rs` | `??` | 121690 | `713b5402807b563fd46155b6c2eb823035afc869720d9c23aaf8d8e6845247dc` | none | S6-3 |
| `libs/text-engine/src/top.rs` | `M` | 10749 | `6c0f59b2b9accf7d661588db2928ac5298644f4d1c486579a40ff507d6919e5b` | none | S6-2 |
| `libs/text-engine/src/top/records.rs` | `??` | 6550 | `2362f67768aa06c63a60e370778ff50e0301eb253fd1d17f3585fc930ec23c01` | none | S6-2 |
| `libs/text-engine/src/top/sample.rs` | `??` | 21274 | `7243a1d7a119fd414198e03bc8f4bafd8edbf14ccdce3d4299de17843d20a5f7` | none | S6-2 |
| `libs/text-engine/src/top/tests.rs` | `M` | 42464 | `03a0aee93a3c0bf2d0aad0d8ee0c8314f5672443c004a58a8e8aeac2a04e865b` | none | S6-2, S6-4 |
| `libs/text-engine/src/top/text.rs` | `??` | 15299 | `eedce1f8bb133df060d9a7c593d399ccf5a022e5a71ee6771887844452a3cd06` | none | S6-2, S6-4 |
| `libs/vitui/src/session.rs` | `??` | 7357 | `ae73bcf1cb608c9f8fbcb428d92ca8e59ad8941c4a621a2373354948c16624ff` | [Phase 05 CP2](phase-05-cp2-dossier.md) `985eaa12…` → changed | S6-4 |
| `libs/vitui/src/tests.rs` | `??` | 17096 | `0be0b1c650070599a33173e87cfd733b7d4997b78f78ce5a06d167cbe008c1d1` | [Phase 05 CP2](phase-05-cp2-dossier.md) `b9f583a1…` → changed | S6-4 |
| `scripts/bench_results.py` | `M` | 53274 | `fdea6feae918159022dc64ffe3eed45bcc83703e09c2758879b296ba2f71b13f` | none | U |
| `scripts/phase04_gui_drive.py` | `??` | 74578 | `25dad37e15db9c95ffd57719f30e5510f16cc3fc2342b2eb09270f1e8b497a04` | [Phase 05 CP2](phase-05-cp2-dossier.md) `6065f034…` → changed | S6-4 |
| `scripts/qemu-phase02-session-test.sh` | `??` | 25415 | `6db71f47c6e11564757acac6f6a9f94adc3a58d933103cb85bae9dab56077840` | [Phase 05 CP2](phase-05-cp2-dossier.md) `42b363e1…` → changed | S6-4 |
| `scripts/qemu-phase03-pipeline-test.sh` | `??` | 53447 | `0a3c222de9bd8547fcac552997d04a4c9d42d2e30c29f435d635a3cec40ccd57` | [S3-output CP2](phase-04-s3-output-cp2-dossier.md) `166bce2a…` → changed | S6-2, U |
| `scripts/qemu-phase04-terminal-test.sh` | `??` | 56262 | `9fcd2507f46bfa3abaa2be99fc4ee47ef66d87ef991c94456ce5bf95cfaa451d` | [Phase 05 CP2](phase-05-cp2-dossier.md) `62d4b402…` → changed | S6-1, S6-4, U, S6-5 |
| `scripts/test_compare_bench_results.py` | `M` | 18586 | `7edc170e4e9293e6fd1e977bf649f5820043b9e643224c1f9d1fc866a48a3775` | none | U |
| `scripts/x86/run-c2c-render-smoke.py` | `M` | 19495 | `65041a9bce72aa94552e75242ceb7c382b3bdb524b937e8b846e4a227c82d935` | none | U |
| `scripts/x86/test-native-vmware-matrix.py` | `M` | 11183 | `0bce87014155648a1e30db359225d8ef4cb17002a3d5b1b083cb409743cb5996` | none | U |

Q touches only `executor.rs` and `shell_test.rs` (listed under their other items). No Phase 06 item touched `kernel/src/loader/launch_profile/*` or `libs/api/src/abi/cell_session.rs`: the five terminal-runner authority pins still equal their recorded hashes, so nothing was re-pinned.

Plan documents (context, not source; not under this CP2):

| Plan document | Bytes | SHA-256 | Role |
|---|---:|---|---|
| [phase-06-entry-dossier.md](phase-06-entry-dossier.md) | 45041 | `01abc820eccc9903ea1d8635128e6b8b8b8fe9ba986d15e462d9af3467f0968c` | Phase entry; D60/D61/D63/D64/D65 decisions (§8, all accepted); S6-1 CP1 §5, DV-1..8 §6, S6-1 Results §10 |
| [phase-06-s2-s3-proposal.md](phase-06-s2-s3-proposal.md) | 44970 | `da65dde9073f1983da51118c9c8d7da18eb96e820bbe816de6b5f01b86d7fe09` | S6-3/S6-2 CP1 (approved), DV-9..12 §7, Results §10 (deviations §10.2, P03 follow-ups and ratifications A/B §10.3) |
| [phase-06-s4-proposal.md](phase-06-s4-proposal.md) | 44387 | `85159a2fe8b1349fe2250ce4bda0348c1e8f7d28f466d9c9c32b37dfd7184f9d` | S6-4 CP1 (approved), Results §10 (deviations §10.2 with three ratifications) |
| [phase-06-s5-proposal.md](phase-06-s5-proposal.md) | 36360 | `692568b6f74214125e1bc3f9d1b1bc6826cdc496b4e9d53a0aa0f165fef900db` | S6-5 CP1 (approved; CP1 digest `88602bb0…0fce` before its Results), criteria map §6, Results §10 (13 deviations §10.3) |
| [spec.md](spec.md) | 26626 | `cde241787ca4ab01e75bb057a5af0ca94dd50755816e1bfc2a71b1ab6dd3906a` | §1 Begin `meta` and §2 `to jsonl` envelopes amended (D63-2/D63-3); Phase 05 CP2 recorded `7e6e11c7…` → changed |
| [command-catalog.md](command-catalog.md) | 63892 | `6f6766b93a2261d855351605b6df7413533e0a88de1e6b87943f7ec4611467b0` | `top` row gains the S6-4 note (S64Impl); the `ps` row is stale (§18) |
| [phase-06-cell-top.md](phase-06-cell-top.md) | 21664 | `507828901dbe977fa036ffd0f3a8bd560a87f16dd31330f210cb9c349c7fa666` | Phase file; criteria checked and the pointer line to this dossier (2026-10-24) |

## 4. Deleted and moved paths

**No file was deleted or moved in Phase 06.** `git ls-tree HEAD` and the working tree agree that every pre-Phase-06 `text-engine/src/top*` and `cellbox/src/top*` path still exists (rewritten in place).

Removed inside surviving files:
- S6-2: the old `text_engine::top` model (`TIMER_HZ`, `MAX_ROWS`, `build_rows`, the 100% `cpu_tenths` clamp, the fabricated `build_cell_records`/`CellRecord`); the `GetProcs2` sampler with its silent 64-row caps in `top`, `ps` and `--records`; `state_order` and the 64-row `GetProcs` buffer in `commands.rs`.
- S6-4: the interim interactive path (`CLEAR_SCREEN` redraw, `shell_live`, the fd-0 `q` poll, `quit_requested`), the session-locking `TerminalKeys::read`, and `wait_ns`'s `stop` callback.
- U: the P03 runner's `relayed()` reassembly helper.

## 5. Final evidence (shared by all items)

The final runs are on the current tree. Logs are in `reports/s6-5-evidence/`:

| Log | Bytes | SHA-256 | Total |
|---|---:|---|---|
| `reports/s6-5-evidence/phase04-terminal.log` | 44448 | `1fca74c4c953119f9cb0675e94189b49096762849b8ff8f8e809faa1ade30c29` | 430/430, 148 markers |
| `reports/s6-5-evidence/phase03.log` | 23105 | `44cf28ff16031bc81f133dd3cdbc927cae568eeb4e107bd23390be12091afadf` | 62 PASS |
| `reports/s6-5-evidence/phase02.log` | 21363 | `ea0aa01e5a4470397f44d3093f94d034709bf639985f289d35f3857bbe75b805` | passes: 48 |
| `reports/s6-5-evidence/gui-click1.log` | 42571 | `7ea9db6454352869d147760f0c5d60b996fb551859795ede8f4cfc57129d23d0` | 279/279 GUI |

**Gap closure (2026-10-24, same tree).** Five more GUI runs and the kernel AP lane on the final tree, also in `reports/s6-5-evidence/`:

| Log | Bytes | SHA-256 | Total |
|---|---:|---|---|
| `reports/s6-5-evidence/gui-autoload.log` | 42609 | `adc5548517c374735822fdc595835e04c24ef88b54dc9243762333ec0a35a7c8` | 279/279 GUI |
| `reports/s6-5-evidence/gui-click2.log` | 42572 | `1847823fec3af311315057d8c1ddb44945210c4322f9923979ceba8e6d0c6fd2` | 279/279 GUI |
| `reports/s6-5-evidence/gui-click3.log` | 42571 | `a1b47a9850e5a9039700dd145bfd1f3b86cdcb05917efc93df90e0409151ccee` | 279/279 GUI |
| `reports/s6-5-evidence/gui-click4.log` | 42571 | `e65e92087f5d91df2a839bc30f956a29ca244e99f2d285784e89659ef59d3c32` | 279/279 GUI |
| `reports/s6-5-evidence/gui-click5.log` | 42571 | `bc12d7de37f99a29f9061ff603fcd25772bd685b91d2e39573154ae681ef120d` | 279/279 GUI |
| `reports/s6-5-evidence/phase02-kernel-serial.log` | 22188 | `9786cb4186ed8c72c7317a66edea2fc8990175e5308f9387ddabcf60b443e17a` | ATOMIC_PUBLICATION 38 PASS, 0 FAIL |

With `gui-click1.log` above, the final tree has GUI 6/6 at 279/279 and kernel AP 38/38. Binding to the §3 bytes (`[INFERENCE]`, as for the other final logs): every GUI log records `cargo_lock_sha256` `793edafa…b401` (line 567) and `PASS: 279 GUI checks` plus `PASS: canonical cellbox/terminal/kernel_fs.img hashes unchanged`; the runs started 2026-10-24 05:23–05:45 +07 (kernel AP `/tmp/cellbox-phase02-TzzY9D` 05:23:36; GUI `HDLP` 05:25 → click2, `VJaO` 05:30 → click3, `u40b` 05:35 → click4, `vVJu` 05:40 → click5, `NVXm` 05:44 → autoload); each GUI log is byte-identical to its directory's `run.log`, and the kernel log equals `TzzY9D/evidence/serial.log` (that `run.log` ends `KERNEL REGRESSION CASES PASS (test-hooks image; not CP2, not acceptance)`); all 62 §3 hashes were re-verified equal after these runs. GUI `G-S1a FAIL` lines are the two informational ones described below.

Six-GUI-run and kernel AP evidence on the S6-4 tree (`reports/s6-4-evidence/`), which differs from the final tree only by the S6-5 files:

| Log | Bytes | SHA-256 | Total |
|---|---:|---|---|
| `reports/s6-4-evidence/gui-autoload.log` | 42609 | `8381ac81af4774e7bbf87b7388d6779f451efe41d33049c5ffbdf50486bd2719` | 279/279 GUI |
| `reports/s6-4-evidence/gui-click1.log` | 42571 | `92a5f480ed23b4b2e64e5725699c80e4b37edf1a224db85a92c5a850097015d3` | 279/279 GUI |
| `reports/s6-4-evidence/gui-click2.log` | 42572 | `6be46fde13f23093ce2122c5b4582ef72db7fb3aee9d9c48bbaa412002575c3a` | 279/279 GUI |
| `reports/s6-4-evidence/gui-click3.log` | 42572 | `3738ab62f1992417face950329b0418d672befc4e795ce0b3338973e4b237376` | 279/279 GUI |
| `reports/s6-4-evidence/gui-click4.log` | 42571 | `fe9be9078a3f52ac14287d48cd0c71f1210b8c77b536ba43fda44acb397d2a98` | 279/279 GUI |
| `reports/s6-4-evidence/gui-click5.log` | 42571 | `d5f6c333724ade40517b786f88d4a3f3f49db869a914cc60a7c52c99308de492` | 279/279 GUI |
| `reports/s6-4-evidence/phase02-kernel-serial.log` | 22157 | `c00e9f8c745e8bb28ac29816c0c9f9f0604a726bd2a899b240bd1fcbc44234e2` | ATOMIC_PUBLICATION 38 PASS, 0 FAIL |
| `reports/s6-4-evidence/phase02.log` | 21363 | `2c74d01d1c0c2096a77ed2a56726fbe5f367e650a18b9619d0de51c569b4f7ce` | passes: 48 |
| `reports/s6-4-evidence/phase03-serial-uartfix.raw.log` | 48362 | `96d0d5766328421ddf573015f6acc99c9c5d2373d7e5e6f4a662c7d88590965c` | raw serial of a P03 run on the UART-fix tree before the `$(…)` fix (run `ccwgjh`); 0 mid-line `USER: `, 281 line-start `USER: `, 1 `USER+ ` |
| `reports/s6-4-evidence/phase03.log` | 23105 | `3b3c527184190feb8ee89df5d8530aacfa9cfc6868c89850031f04dab557c38e` | 62 PASS |
| `reports/s6-4-evidence/phase04-terminal.log` | 39609 | `e7bba68313c4557cfb856ee491a527832759c9b08fa87146bb571982f9768e7b` | 410/410, 128 markers |

**`FAIL` text in the logs.** Apart from a check named `no-FAIL-markers`, the only `FAIL` lines are informational and not results: every terminal log has one and every GUI log two `G-S1a FAIL: ceiling differs from the baseline` lines (the Terminal `SetTimer` and Files `VfsMutate` deltas, both approved earlier; cellbox itself is `G-S1a PASS`, terminal log line 6), and the raw P03 serial logs carry the historical boot self-test `COMPLETION-QUEUE: FAIL` (3 lines), disclosed since Phase 04 and not attributed to Phase 06. No log has a failed check. The same informational lines are in `reports/phase-05-final-evidence/`, so the Phase 05 dossier's “0 `FAIL` lines in every log” is inaccurate as worded (it holds for failed checks only).

**Binding the evidence to the source.** The logs record binary hashes (signed per run with throw-away keys), not source hashes. That they match the bytes in §3 is `[INFERENCE]` from four facts:
1. All four S6-5 logs record `cargo_lock_sha256` `793edafa…b401`, the current `Cargo.lock` (terminal 523, P03 403, P02 416, GUI 567). The S6-4 logs record `36688198…`, the lock after S6-4 and before the `app-top-load` entry.
2. The terminal runner fails unless five authority files hash to its pins (lines 266-270), and all five equal the current files.
3. Run start times (key-directory mtimes in `/tmp`): S6-5 terminal `cellbox-phase04-YhwJDl` 22:35:26, P03 `rxjW9L` 22:41, P02 `dua2y3` 22:49, GUI `oGFb` 22:51, all after the last source write (22:32, `s2tw-probe`/`top-load` `command.rs`). S6-4 terminal `cePdVo` started 19:59:11, after the last S6-4-tree write (19:58); its P03 (`ix9dHo`), P02 (`noIjTY`) and six GUI runs followed. Each archived S6-5 and S6-4 runner log is byte-identical to the `run.log` of the named directory. The S6-4 kernel AP log equals `/tmp/cellbox-phase02-gxi3Ma/evidence/serial.log` (19:40-19:41); the kernel last changed at 19:00 (`uart.rs`).
4. No source file changed after 22:32.

**Other facts from the logs.**
- Canonical artifacts unchanged: S6-5 GUI 874-878 `PASS: 279 GUI checks`, `PASS: canonical cellbox/terminal/kernel_fs.img hashes unchanged` with cellbox `64eb28f3…`, terminal `636497ac…`, `kernel_fs.img` `447d0348…` (the Phase 05/C5 values).
- Evidence ceiling: QEMU/TCG (runner default `PHASE04_ACCEL=tcg`), one vCPU, HPET ns; restricted SLIRP, no inbound network. No CPU-speed, hardware, KVM or multi-hart claim.
- **Exit codes and host tests are not in the logs.** No host-test log is archived, so the host counts below rest on the Results texts only.
- Totals: terminal 430 = 399 (Phase 05 397 + S6-1's 2) + 11 (S6-4) + 20 (S6-5); markers 148. GUI 279 = 259 + 20 `TOP-*` (S6-4). P03 62 = 60 + 2 (S6-2). P02 48 unchanged (+6 fail-closed `top` problem checks, silent on pass). Kernel AP 38/38 unchanged.
- Re-run on the final tree on 2026-10-24 (gap closure above): the other five GUI runs and the kernel AP lane. The S6-4-tree runs remain slice-time evidence.

## 6. Slice S6-1: Cell-aware telemetry provider (kernel/API/ostd)

**Scope.** `TelemetrySnapshot = 430` on allowlist bit 55 (shared with `GetProcs2`), gated by X; record layouts V1 in `api::telemetry`; exact on-CPU ns per task (K1); per-incarnation retained CPU of exited threads (A1); one-shot capture with explicit totals and truncation (P1); System scope only (S1); per-boot provider epoch (E1); `ostd` wrapper; `s2tw-probe telemetry` and two terminal markers. `GetProcs`, `GetProcs2`, `MemInfo`, `cpu_run_ticks`, CapSet/policy v6, launch profiles and cellbox's declared syscall list are unchanged.

**Files.**

| Path | Bytes | SHA-256 | Phase 06 regions (current lines) and sharing |
|---|---:|---|---|
| `libs/api/src/abi/telemetry.rs` | 21716 | `9e90d932369bd65c216161656b89ae2347d19a8b895b07f4777e6778e6b38cbf` | Whole file (new, S6-1 only): `TelemetryHeaderV1` 96 B, `TelemetryCellV1` 96 B, `TelemetryTaskV1` 88 B, flag/state constants, request bits, little-endian encode, `TelemetrySnapshot::parse` refusals, 7 tests |
| `libs/api/src/abi.rs` | 1116 | `08c1d815fd5e40a9bd3b19a60186afc578f500275c6853913a6bc57229e0e66c` | `pub mod telemetry;` 37 |
| `libs/api/src/abi/syscall.rs` | 76350 | `b202dfa28f461b2c8f9940901294187e65b6ef4ff94638dc1004c01d78325957` | `TelemetrySnapshot = 430` doc/variant 599-614; allowlist bit 55 shared with `GetProcs2` 1138-1140; number map 1344. Rest: Phase 02–04 |
| `libs/api/src/abi/syscall_tests.rs` | 24003 | `cb6cdfe1db704fa929f6239dd92445f5a4849e1a190b45d71de7bb0875c7ca4c` | Number pins 79, 135; `telemetry_snapshot_shares_the_get_procs2_authority` 302-312 |
| `libs/ostd/src/syscall.rs` | 103035 | `95e4beb7e60561a061a6aed841bf2fc981a7a99e0c54716fcd3db4df7708c944` | `sys_telemetry_snapshot` 1572-1605 (local refusals, error mapping), `telemetry_bytes` 1607+ |
| `kernel/src/task/telemetry.rs` | 28130 | `a67465a16198d4c198e66caa9af63f884f7e8c8bbdbbaf3cce388cac0f0797fb` | Whole file (new, S6-1 only): `CpuLedger` (A1), `monotonic_ns`, `provider_epoch` (E1 with E2/E3 fallbacks), `request_is_valid`, `capture` (P1/S1), 8 tests |
| `kernel/src/task.rs` | 123826 | `a27bc5d110f03cb3db428d6013ea3c9ac3a1bf0426cc099283944a3ba06d512d` | `pub(crate) mod telemetry;` 120 |
| `kernel/src/task/tcb.rs` | 41023 | `78202648db04c7065b1e9255020648a67d03d6b7d29989b9a6818681aa868e01` | `Task::cpu_ns` 449, init 680 |
| `kernel/src/task/scheduler.rs` | 133680 | `2065efd47e19277b6dd22a6bbeb5665cf1426015720388165d1507f88cf1b82a` | `cpu_telemetry: CpuLedger` 419, 449; `charge_cpu` 606-620 (K1, called first in every pass); retention in `exit_task` and quiescent root retirement |
| `kernel/src/task/syscall.rs` | 421770 | `c56d500f72ac1adcf68a3cd422cf7c77702ba01449c1992391f757714f170af4` | `supports_typed_oom` (renamed) 1613-1635; `Syscall::TelemetrySnapshot` 3717-3718; ViSyscall map 4107; dispatch 7066-7090 (X gate via `observe_right_held`, M for system memory); tests |
| `kernel/src/task/smp.rs` | 48676 | `dd21390398ec2da66c825192095ccbf5ea1e3978f6602576197661f89597e93e` | `online_hart_count` 310 no longer gated to `test-hooks` |
| `kernel/src/task/cell_session.rs` | 127334 | `3035e4291da7b177bc01594b601604b08114b86edceca5a5dac684ca68fd6328` | `super::telemetry::monotonic_ns()` 1764 (helper moved to `telemetry.rs`). Rest: Phase 02/05 option C |
| `kernel/src/memory/cell_quota.rs` | 19143 | `b5eb91f93ac6fc6353a4facdf2224a37731a85482bfe4fc1719c703e2583fb36` | `pub fn limit` 299 |
| `cells/tests/s2tw-probe/src/main.rs` | 12273 | `bf0958f0754b8cde839a6afdc48f609565086fc25445d9fe68ebbd7c933a70c6` | `telemetry` right (doc 13-22; `denied` from `/data/bin`, `other:cells-N-tasks-M` from `/bin`). Shared with S6-5 `trunc` mode (doc 31-36) and S2-T/W |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 103522 | `862499775d9907c6cdb7607aae55d173da37c79a30e6274af3f13f663f108bfb` | `S2TW_RIGHTS` incl. `telemetry` 1136-1138, admitted prefix 1205 (`sandbox-denied-telemetry`/`control-allowed-telemetry`). Shared with S6-2, S6-4, S6-5 and Phase 04/05 |
| `scripts/qemu-phase04-terminal-test.sh` | 56262 | `9fcd2507f46bfa3abaa2be99fc4ee47ef66d87ef991c94456ce5bf95cfaa451d` | S6-1 header 51 and the 2 markers. Shared with S6-4, U, S6-5 and Phase 04/05 |

Plan document: [entry dossier](phase-06-entry-dossier.md) §5, §6, §10.

**Deviations.**
- **DV-1..DV-8** (TID as task incarnation; no membership/accounting epochs; one-shot capture instead of a retained `SnapshotRef`; quota charge + limit instead of an attributed total; 32-byte names; System scope only; core-equivalent CPU only; bit 55 shared). **Ratified by the user 2026-10-23 (D61-7).**
- Implementation deviations (entry §10, recorded as “within the approved CP1”):
  1. K1 charges at every scheduler pass instead of stamp-in plus add-at-switch-out; kernel work in a pass or syscall is charged to the current task. **Ratified by the user 2026-10-24 (CP2).**
  2. E1 is drawn lazily at the first capture; if neither entropy nor RTC answers, the epoch is the constant 1 (E3 form). **Ratified by the user 2026-10-24 (CP2).**
  3. `supports_typed_spawn_oom` renamed `supports_typed_oom`; `monotonic_ns` moved from `cell_session.rs` to `telemetry.rs`; `smp::online_hart_count` no longer gated to `test-hooks`. **Ratified by the user 2026-10-24 (CP2).**
  4. Kernel-context tasks appear only as task rows; `total_tasks` counts the whole live table including Retiring members; `MEMORY_INTERVAL` is always set. **Ratified by the user 2026-10-24 (CP2).**
  5. Names are cut at a UTF-8 boundary ≤ 32 bytes and flagged (`GetProcs2` cuts at byte 32). **Ratified by the user 2026-10-24 (CP2).**

**Evidence.**
- **Slice-time** (`reports/s6-1-evidence/`): terminal 399/399 (117 markers) incl. `sandbox-denied-telemetry` 610 and `control-allowed-telemetry` 616; P03 60, P02 48, kernel AP 38/38, GUI 259 (one run). Host (entry §10): api 144, kernel 272 (also with `phase04-driver-profile`; 8 `task::telemetry` tests, 2 syscall tests), ostd pass.
- **Final:** terminal `sandbox-denied-telemetry` 671, `control-allowed-telemetry` 677, `control-telemetry-truncated` 723 (`emitted_cells=1 total_cells=8 … complete=0`, INFO 564); every `top-load-*` marker reads this provider. Kernel AP 38/38 on the final tree (gap closure, §5; also on the S6-4 tree).
- **K1 cost (D65-10):** INFO 567 `bench calls=10000 clock_ns=Some(1731) yield_ns=Some(403232)`: 1.7 µs per `GetTime(0)` (upper bound of the one added counter read per pass) and 403 µs per `Yield` round trip, TCG, recorded with no budget.

**Known limits and open defects.**
- No before/after A/B of the scheduler pass cost (D65-10 alternative not taken; the HEAD baseline is confounded by Phase 02–06 changes).
- OwnCell/GrantedSet scopes and the machine-normalized CPU view are not implemented (DV-6, DV-7).
- A Cell above 1000 tenths is host-tested only (one-vCPU runners, D60-5).
- `MemInfo` frames are read after `SCHEDULER` is released, so system memory is not atomic with the rows (flagged by `MEMORY_INTERVAL`).
- No riscv64/aarch64 runtime run; those kernels were build-checked only.

**Residual risks.** A clock read and a ledger update now run in every scheduler pass (hot path). Syscall 430 and its V1 layouts become frozen once any image ships them. The capture holds `SCHEDULER` while it walks every task (bounded by the caller buffer).

**What CP2 would authorize.** The api/kernel/ostd regions above as the provider baseline, the probe/driver/runner S6-1 regions, and ratification of implementation deviations 1–5.

[x] CP2 approved by user — date: 2026-10-24 (implementation deviations 1–5 ratified: yes)

## 7. Slice S6-3: Frames in the typed engine

**Scope.** Spec Frames (F1): Begin `meta` (M1), `take --frame N`, `sort-by --frame`, admission by producer category, an order guard on every chain, JSONL envelopes (D63-2); `to json`/`to text` refuse every Frames stream (DV-12). Spec §1/§2 amended.

**Files.**

| Path | Bytes | SHA-256 | Phase 06 regions (current lines) and sharing |
|---|---:|---|---|
| `libs/text-engine/src/pipeline.rs` | 121690 | `713b5402807b563fd46155b6c2eb823035afc869720d9c23aaf8d8e6845247dc` | `Input` 616; `validate_stage_sequence(Input, …)` 823; `encode_item` with the D63-2 envelopes 1541; `FrameBegin` 1567, `Item` 1584, `Category` 1592, `RowSource::next_item` 1600; `check_items` order guard 1733; `SortFrameSource` 1906-1972 and frame-counting `take`; 10 `frame_tests`. File is untracked and has no earlier per-file hash: Phase 03 engine bytes are inside the same whole-file hash |
| `cells/tools/cellbox/src/typed.rs` | 95633 | `d1e30c222d5f96d7b4ccd465bb85a1871ca1623239aa15753f50d06d1073073b` | `TypedPlan.category` 218; admission by `Input` 495-556; `build_chain` wraps the producer with `check_items` 698-705 (deviation 1); `run_rows`/`stream_rows`/`RowFeed::new` take the category; per-frame table (`write_tables`/`frame_footer`); `--text` redirect bounded by `MAX_SINK_BYTES`; 6 Frames tests. Rest: Phase 03–05 |
| `cells/tools/cellbox/src/executor.rs` | 153078 | `db3ff92e4494942594cf7c16bb6a1c3f5568b42fd02ffb84accfd89e2b631bd2` | Category reaches the sinks 1572, 1612-1614. Shared with S6-2, S6-4, Q and Phase 02–05 |
| `cells/tools/cellbox/src/native_rows.rs` | 10718 | `de097aaaa03bc85e2ffc78afb83cba00f1978f409899cb546977ce1e6d5d5029` | `producer_category` 30. Shared with S6-2 (`PsSource`/`TopSource`) and S6-4 |

Plan document: [spec.md](spec.md) §1/§2 amendments and change log (D63-2/D63-3, accepted decisions).

**Deviations.**
- **DV-12** `to json`/`to text` refuse every Frames stream, including `take --frame N`-bounded ones. **Approved (D63-4, 2026-10-23).**
- Implementation deviations ([proposal §10.2](phase-06-s2-s3-proposal.md)):
  1. `check_items` is applied in `typed::build_chain` (the single chain constructor) rather than at the `executor.rs` call sites; violations are attributed to the producer stage. **Ratified by the user 2026-10-24 (CP2).**
  2. `encode_item` takes the `Item` by value (no copy). **Ratified by the user 2026-10-24 (CP2).**

**Evidence.**
- **Slice-time** (`reports/s6-23-evidence/`, S6-3 + S6-2 together): P03 62/62 ×2 incl. `top-frames-closed-by-take` 409, `frames-rows-stage-refused` 410, `sort-by-frame-live` 411, `typed-rows-stream-into-external-stage` 428, `take-closes-endless-producer` 432 (`phase03-1.log`); terminal 399/399; GUI 6×259 + 1. Host (§10.4): text-engine 105 (10 `frame_tests`), app-cellbox 105 (+6 Frames tests).
- **Final:** P03 `top-frames-closed-by-take` 444, `frames-rows-stage-refused` 445, `sort-by-frame-live` 446, `typed-rows-stream-into-external-stage` 463, `take-closes-endless-producer` 467; terminal `top-pipe-batch` 692; every `top-load` records check parses these envelopes.

**Known limits.** No array/text form of envelopes exists (DV-12). The `--text` redirect is bounded by `MAX_SINK_BYTES`.

**Residual risks.** The `RowSource::next_item` trait change touches every producer; a missed migration fails loudly. JSONL recorded with envelopes is not readable as flat rows by older tools.

**What CP2 would authorize.** The `pipeline.rs` Frames regions and the cellbox plumbing regions above, with deviations 1–2.

[x] CP2 approved by user — date: 2026-10-24 (deviations 1–2 ratified: yes)

## 8. Slice S6-2: telemetry model, sampler and caller cutover

**Scope.** The `TelemetrySnapshot` model, sampler, records and text renderer replace the old `text_engine::top`; `top`, `ps`, `top --records`, `ps --records` cut over (D62-1 one-capture `ps --records`; D62-2 text `ps` on the shared renderer); blocking `SetTimer` waits; exit 2 on bad options. P03 and driver checks migrated (P03 60 → 62; terminal stays 399); P03 kill victim `init` (ratified A).

**Files.**

| Path | Bytes | SHA-256 | Phase 06 regions (current lines) and sharing |
|---|---:|---|---|
| `libs/text-engine/src/top.rs` | 10749 | `6c0f59b2b9accf7d661588db2928ac5298644f4d1c486579a40ff507d6919e5b` | Rewritten (S6-2): `View`, `RowKind`, `Field`, schemas `top.cell.v1`/`top.task.v1`, options; modules `records`, `sample`, `text` 14-16. The fabricated `build_cell_records`, `MAX_ROWS`, `TIMER_HZ` and the 100% clamp are deleted. No earlier per-file hash |
| `libs/text-engine/src/top/sample.rs` | 21274 | `7243a1d7a119fd414198e03bc8f4bafd8edbf14ccdce3d4299de17843d20a5f7` | Whole file (new, S6-2): sampler over `TelemetrySnapshot` (incarnation keys, New/Ok/Reset/Gone/Unavailable, core-equivalent tenths, no clamp) |
| `libs/text-engine/src/top/records.rs` | 6550 | `2362f67768aa06c63a60e370778ff50e0301eb253fd1d17f3585fc930ec23c01` | Whole file (new, S6-2): Frames records, Begin `meta` (15 keys, 16-hex `provider_epoch`, DV-9), Null on non-Ok |
| `libs/text-engine/src/top/text.rs` | 15299 | `eedce1f8bb133df060d9a7c593d399ccf5a022e5a71ee6771887844452a3cd06` | New in S6-2 (`render_frame`, header with `complete`/`TRUNCATED n`, `denied`/`n/a`). Shared with S6-4 (public `Column`, `columns`, `widths`, `header_parts`, `HeaderMark`) |
| `libs/text-engine/src/top/tests.rs` | 42464 | `03a0aee93a3c0bf2d0aad0d8ee0c8314f5672443c004a58a8e8aeac2a04e865b` | Rewritten (28 tests, S6-2); +2 S6-4 (byte-identical batch layout, width fit) |
| `libs/text-engine/Cargo.toml` | 1058 | `9d6e91faf0b92e1bda79993b338875754a8f6d7151ce2d8e00ba098881f6edb0` | Dependency comment only |
| `cells/tools/cellbox/src/top.rs` | 6343 | `fff763b7ab9c7a3debabd4a400f486e968f1d058d9d040595c39bb4e4760cf7a` | `TelemetryReader` 35, `capture_tasks` 95, `cmd_top` → status 123 (S6-2). Shared with Z (`wait_ns` 109, the `sleep` wait) and S6-4 (`set_view` 70, `run_batch` 152, `^C` 146, `mod tui`/`mod view` 11-12). File has no earlier per-file hash (moved from `cells/tools/shell/src/top.rs`) |
| `cells/tools/cellbox/src/top/render.rs` | 640 | `e9259ed88c57f21e32368cb0d371293d69d72eb5e9595e36c08cf3554a206d60` | Thin batch writer (S6-2); `CLEAR_SCREEN` deleted (S6-4) |
| `cells/tools/cellbox/src/commands.rs` | 26631 | `8efcb7ce028ba1b8c500eb6faec261be55cf6a0fbb95ead2ffe49d49ffc0c516` | `cmd_ps` on the shared renderer 491-596 (keeps `-p`/`-s`; bad options 2, telemetry failure 1); `cmd_top` 597-598; `state_order` and the 64-row `GetProcs` buffer deleted |
| `cells/tools/cellbox/src/native_rows.rs` | 10718 | `de097aaaa03bc85e2ffc78afb83cba00f1978f409899cb546977ce1e6d5d5029` | `PsSource` one capture 82-107 (D62-1), `TopSource` Frames 49. Shared with S6-3, S6-4 |
| `cells/tools/cellbox/src/executor.rs` | 153078 | `db3ff92e4494942594cf7c16bb6a1c3f5568b42fd02ffb84accfd89e2b631bd2` | `top`/`ps` dispatch with statuses 0/1/2 1948-1949; `ps` left the legacy table. Shared |
| `cells/tools/cellbox/src/shell_test.rs` | 41332 | `1af74e4773d57127cd7e112edc95b989bea891b2b9510888f12fc788ab31254b` | `Stage`/`EndlessRows` migration; `top` heading pins replaced by exit-status checks; `pid` → `tid`. Shared with S6-4, Q |
| `scripts/qemu-phase03-pipeline-test.sh` | 53447 | `0a3c222de9bd8547fcac552997d04a4c9d42d2e30c29f435d635a3cec40ccd57` | P03 60 → 62: `pid` → `tid`; `frames_of` 727, `cell_meta_ok`, `closed_ok`; `top-frames-closed-by-take`, `frames-rows-stage-refused`, `sort-by-frame-live`; victim `init` by name 564-577 (ratified A). Shared with U (`cell_lines`/`body_of` 529-539, 687-701) |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 103522 | `862499775d9907c6cdb7607aae55d173da37c79a30e6274af3f13f663f108bfb` | `count_cellbox`, `builtin-ps-records`, `typed-pipeline` `pid` → `tid`; `top-pipe-batch` on Frames. Shared |

**Deviations.**
- **DV-9** `provider_epoch` as a 16-hex-digit String in records; **DV-10** Gone as a per-frame count in Begin `meta`, Null after an incomplete capture; **DV-11** 4,096 rows per frame of one kind plus a 192 KiB client snapshot cap. **Ratified by the user 2026-10-23 (D62-3).**
- **(A) P03 kill victim = `init` by name**; the old “non-system cells preferred” exclusion stays dropped. **Ratified by the user 2026-10-23.**
- Implementation deviations ([proposal §10.2](phase-06-s2-s3-proposal.md)):
  3. `Cpu.ns` is `Option<u64>`: Null when the provider flags CPU unavailable, listed in `unavailable_fields`. **Ratified by the user 2026-10-24 (CP2).**
  4. A `--cell ID:GEN` frame's `dropped` is the system-wide `total_tasks − emitted_tasks`. **Ratified by the user 2026-10-24 (CP2).**
  5. `-o mem|heap` in a task view gives a dedicated usage line naming `stack_bytes`/`segment_bytes` (status 2). **Ratified by the user 2026-10-24 (CP2).**
  6. `TelemetryReader` always requests system memory (also for `ps`); other capture errors map to `Malformed`. **Ratified by the user 2026-10-24 (CP2).**
  7. `wait_ns`: a failed `SetTimer` falls back to `yield_now` for that step; an unreadable clock ends the wait. **Ratified by the user 2026-10-24 (CP2).**
  8. The interim interactive/batch view prefixed one status line (`t=`, mode, `q to quit`/`remaining`). **Ratified by the user 2026-10-24 (CP2)** (the interactive part was replaced by S6-4).
  9. `top --records` accepts `--fields` (text-only); records always carry the full schema. **Ratified by the user 2026-10-24 (CP2).**

**Evidence.**
- **Slice-time** (`reports/s6-23-evidence/`): P03 62/62 ×2 incl. `s2-kill-victim-selected` 445, `s2-kill-builtin-refused-no-ipc` 446 (`phase03-1.log`); terminal 399/399 (117); P02 48; GUI 6×259 + `gui-post-sleep` 259. The first P03 runs (59/62, then 60/62 ×4, `/tmp/cellbox-phase03-{hLN8fY,fecweV,vxgD7j,2boe79,CbvXmW}`) are not archived; their causes are in [proposal §10.3](phase-06-s2-s3-proposal.md).
- **Final:** P03 62 incl. `s2-kill-victim-selected` 480, `s2-kill-builtin-refused-no-ipc` 481; terminal `builtin-ps-records` 594, `top-load-over-64-records`/`-text` 705-706 (74 rows in records, text `top` and `ps`, status 0); host (§10.4): text-engine 105 (28 `top::tests`), app-cellbox 105; G-S1a PASS.

**Known limits and open defects.**
- `input` shows `Ready` in single captures where the old two-sample capture showed `Waiting`; the cause was not isolated ([proposal §10.3](phase-06-s2-s3-proposal.md) third follow-up). The victim change (A) avoids depending on it.
- `unavailable`/`reset` qualities and `mem denied`/`n/a` are host-tested only (not reachable on the runners).
- No heap high-water measurement; > 64-row runs prove only the absence of `OOM:` (S6-5 #4, #19).

**Residual risks.** cellbox has a 1 MiB heap; a very large population (2,048 tasks) approaches it (proposal §8). About 20 runner command strings moved `pid` → `tid`; a missed one fails loudly.

**What CP2 would authorize.** The model files, cellbox cutover and runner migration regions above, with deviations 3–9.

[x] CP2 approved by user — date: 2026-10-24 (deviations 3–9 ratified: yes)

## 9. Slice S6-4: interactive Ratatui `top`, drill-down and keys

**Scope.** Interactive `top` is a vitui/Ratatui view (keys, drill-down into `ID:GEN`, field picker, `o`/`r` sort, `q` → 0, Ctrl-C → 130 + `^C`, no lifecycle key), drawn through `executor::with_tui`/`TuiSink` (S1) on a viewport capped at 120×40 by `TuiSession::enter_bounded` (Z1). The selection gate refuses `x=$(top)`, a shell without a session and `-d` < 0.1 s with 2. The batch table model is shared and byte-identical. Runners: terminal 399 → 410, GUI 259 → 279, P02 serial `top` step extended. Release `cellbox` text 1,094,251 → 1,334,827 B (+240,576; D64-16, no budget).

**Files.**

| Path | Bytes | SHA-256 | Phase 06 regions (current lines) and sharing |
|---|---:|---|---|
| `libs/vitui/src/session.rs` | 7357 | `ae73bcf1cb608c9f8fbcb428d92ca8e59ad8941c4a621a2373354948c16624ff` | `TuiSession::enter_bounded` 72+, `Viewport::Fixed(bounded_area)` 169, `apply_resize` re-clamp 130 (Z1). `enter` unchanged. Rest: Phase 05 S5-1 |
| `libs/vitui/src/tests.rs` | 17096 | `0be0b1c650070599a33173e87cfd733b7d4997b78f78ce5a06d167cbe008c1d1` | `bounded_viewport_never_exceeds_its_cap_and_the_backend_size_is_exact` 417; `bounded_entry_clears_from_home_row_by_row` 459 (driver fix after run `JSM1uq`). Rest: Phase 05 |
| `cells/tools/cellbox/Cargo.toml` | 1094 | `afc374e5c9f3fac15a545e6b9f16b17158d5938443c9878eec137678f88c469d` | `vitui` dependency 27. File has no earlier per-file hash |
| `cells/tools/cellbox/src/top/tui.rs` | 11030 | `682bbf719c81f4ac9252de592092fd76dafdff32e6065de7409f179fba24a731` | Whole file (new): `MAX_VIEW` 120×40 28, `MIN_DELAY_NS` 30, `run` 76, `run_with` 93, `color_mode` 108, drain + `enter_bounded` ≤ 8 tries 154, event loop |
| `cells/tools/cellbox/src/top/view.rs` | 20578 | `0dcaefdad80fea3f5f2d65dd6809d9d1c579880667c51183d4a12a937c4eb2d1` | Whole file (new): `ViewState`, `Step`, `RowId`, `Notice`, field picker, `draw` |
| `cells/tools/cellbox/src/top/tests.rs` | 25900 | `7d84a49a19e5ddcd74f0a5d7fac4ca7f7507dbab1ed43bce297d5f3c3b46add1` | Whole file (new): 14 tests incl. the D64-14 constructed same-snapshot pair (TUI = batch = records with Ok/New/Reset) |
| `cells/tools/cellbox/src/top.rs` | 6343 | `fff763b7ab9c7a3debabd4a400f486e968f1d058d9d040595c39bb4e4760cf7a` | `set_view`, `run_batch`, Outcome mapping, `^C` for 130; `quit_requested` and the fd-0 poll deleted. Shared with S6-2, Z |
| `cells/tools/cellbox/src/top/render.rs` | 640 | `e9259ed88c57f21e32368cb0d371293d69d72eb5e9595e36c08cf3554a206d60` | `CLEAR_SCREEN` deleted. Shared with S6-2 |
| `libs/text-engine/src/top/text.rs` | 15299 | `eedce1f8bb133df060d9a7c593d399ccf5a022e5a71ee6771887844452a3cd06` | Table model (`Column`, `columns`, `widths`, `header_parts`). Shared with S6-2 |
| `libs/text-engine/src/top/tests.rs` | 42464 | `03a0aee93a3c0bf2d0aad0d8ee0c8314f5672443c004a58a8e8aeac2a04e865b` | +2 tests. Shared with S6-2 |
| `cells/tools/cellbox/src/executor.rs` | 153078 | `db3ff92e4494942594cf7c16bb6a1c3f5568b42fd02ffb84accfd89e2b631bd2` | `TuiSink` 338, `with_tui` 363, `interactive_terminal` 415, `terminal_is_ours` 422, `TerminalKeys::read_from` 659; `shell_live` and `TerminalKeys::read` deleted; `flush_repl_capture` public 1393. Shared with S6-3, S6-2, Q |
| `cells/tools/cellbox/src/native_rows.rs` | 10718 | `de097aaaa03bc85e2ffc78afb83cba00f1978f409899cb546977ce1e6d5d5029` | View plumbing for Frames. Shared |
| `cells/tools/cellbox/src/cmd_sys.rs` | 11107 | `1a9a3180eff37113a85363ca1b36f41d3b43d26643e59621546f0a7e37ce6bd4` | `wait_ns(delay_ns)` call without the `stop` callback (deviation 11). Shared with Z |
| `cells/tools/cellbox/src/shell_test.rs` | 41332 | `1af74e4773d57127cd7e112edc95b989bea891b2b9510888f12fc788ab31254b` | Capture-stack selection test (deviation 4). Shared |
| `cells/tests/phase04-terminal-driver/src/screen.rs` | 22351 | `b546f43d43f7ecf89c5189f6d098db9ed93fa516835fb30be2ec81edd16fd1cd` | New (S6-4): driver grid model (UTF-8, CR/LF/BS/HT, CUP, ED/EL, SGR 16/256/RGB, 1049/25/2004, save/restore, pending wrap, OSC/DCS skip). Shared with S6-5 (`Frame::status()`) |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 103522 | `862499775d9907c6cdb7607aae55d173da37c79a30e6274af3f13f663f108bfb` | `top_tui` 1781+ (11 `top-*` markers), `frontend-top-quit` on the grid 1490, `TUI_ENTRY` = `ESC[1;1H ESC[0m ESC[J` after the modes, `layout=` before the clip. Shared |
| `scripts/qemu-phase04-terminal-test.sh` | 56262 | `9fcd2507f46bfa3abaa2be99fc4ee47ef66d87ef991c94456ce5bf95cfaa451d` | S6-4 header 74-80; 11 markers. Shared |
| `scripts/qemu-phase02-session-test.sh` | 25415 | `6db71f47c6e11564757acac6f6a9f94adc3a58d933103cb85bae9dab56077840` | Serial `top` step 22-24, 296-320+ (6 fail-closed problem checks; parser drops kernel/`USER+ ` breaks 309) |
| `scripts/phase04_gui_drive.py` | 74578 | `25dad37e15db9c95ffd57719f30e5510f16cc3fc2342b2eb09270f1e8b497a04` | Doc 26; `top` scenario `TOP-0`..`TOP-5` 1011-1100+ |
| `Cargo.lock` | 83130 | `793edafae7d15bc23eb1fd4ec71b3272f3168aca9eeae33d20679b464ffbb401` | `app-cellbox` depends on `vitui` (no new crate). Shared with S6-5 |

**Deviations** ([proposal §10.2](phase-06-s4-proposal.md)):
1. `ViewState::draw(&mut self, …, clamped, buf)` records the visible page and scroll offset. **Ratified by the user 2026-10-24 (CP2).**
2. The status line is built from `text::header_parts`; `label`/`right_aligned`/`field` are `Column` methods. **Ratified by the user 2026-10-24 (CP2).**
3. The type-ahead drain runs inside `with_tui` (`TerminalKeys::read_from`) before each lease try; `TerminalKeys::read` deleted. **Ratified by the user 2026-10-24 (CP2).**
4. The selection-gate host test lives in the executor capture-stack test plus the pure `terminal_is_ours` predicate. **Ratified by the user 2026-10-24 (CP2).**
5. Interactive `-n COUNT` counts every drawn capture; after the last one the view stays one delay, then exits 0. **Ratified by the user 2026-10-24 (CP2).**
6. `FRONTEND_ATTACHED` redraws; other `Lost` reasons are ignored. **Ratified by the user 2026-10-24 (CP2).**
7. A failed restore gives status 1; `with_tui` sends `SOFT_RESET` when the tracker is dirty or any device write failed. **Ratified by the user 2026-10-24 (CP2).**
8. `TuiSink` is `Copy`. **Ratified by the user 2026-10-24 (CP2).**
9. The field picker shows and applies `cell_id` where the table shows `ID:GEN`. **Ratified by the user 2026-10-23.**
10. `Gone` notice only after a complete capture; `Empty` when the view has no rows; the clamp is shown as `clamped WxH`. **Ratified by the user 2026-10-24 (CP2).**
11. `top::wait_ns` lost its `stop` callback. **Ratified by the user 2026-10-24 (CP2).**
12. Runner choices: the `top-tui-idle-cost` threshold **< 500 tenths — ratified by the user 2026-10-23**. The rest of 12 is **ratified by the user 2026-10-24 (CP2)**: `top-tui-keys` and GUI `TOP-3` first press `o` four times; the P02 parser drops console breaks (kernel lines, `USER+ `) and `USER: ` prefixes inside the TUI segment.

Also: Ctrl-C exits **130** with `^C` after restore (D64-10 as implemented) — **ratified by the user 2026-10-23**. After run `cellbox-phase04-JSM1uq` (not archived) the driver's `TUI_ENTRY` now requires `ESC[1;1H ESC[0m ESC[J` after the modes (the bounded viewport clears row by row; pinned by the vitui test `bounded_entry_clears_from_home_row_by_row`) and prints `layout=` ahead of the 200-char clip. **Ratified by the user 2026-10-24 (CP2)** (runner expectation change, no product change).

**Evidence.**
- **Slice-time** (`reports/s6-4-evidence/`): terminal 410/410 (128) incl. `top-pipe-batch` 665, `top-subst-refused` 666, `top-tui-frame` … `top-tui-ctrl-c-130` 667-676, `frontend-top-quit` 657; GUI 6×279 incl. `TOP-0`..`TOP-5` (click1 821-840); P02 48 with the serial `top` step; P03 62; kernel AP 38/38. Host (§10.4): text-engine 107, vitui 23 (24 after `S64Fix`), app-cellbox 121 (14 `top::tests`, incl. the D64-14 constructed same-snapshot pair).
- **Final:** terminal `frontend-top-quit` 684, `top-subst-refused` 693, `top-tui-*` 694-703; `top-tui-idle-cost` INFO 563 `cpu_tenths=Some(31) threshold_tenths=500`; `top-load-over-64-tui` 707 (74 TIDs, 3 pages, `complete`, no `OOM:`), `top-load-drill-live` 712, `top-load-views-agree` 713, `top-load-no-lifecycle` 714, `top-load-tui-gone-notice` 721; GUI `TOP-*` 20 lines 821-840.

**Known limits.**
- The numeric same-snapshot comparison of TUI, batch and records is the D64-14 constructed host pair; no recorded-snapshot replay path exists. Runtime agreement covers identity and TID sets.
- The tick-polled `TuiSession` wait (W1) costs 3.1% of one core at idle on TCG (31 tenths).
- Denied and truncated rendering inside the TUI are host-tested only.
- Release `cellbox` grew by 240,576 B of text; no budget exists (Phase 10).

**Residual risks.** cellbox now links Ratatui through vitui; the 120×40 cap bounds the two buffers at 384,000 B of a 1 MiB heap. A holder that dies on an untracked path relies on the Phase 05 soft-reset net.

**What CP2 would authorize.** The vitui, cellbox view/session, executor S6-4, driver grid model and runner regions above, with deviations 1–8, 10, 11, the rest of 12 and the `TUI_ENTRY` driver change.

[x] CP2 approved by user — date: 2026-10-24 (deviations 1–8, 10, 11, rest of 12 and the `TUI_ENTRY` change ratified: yes)

## 10. Slice S6-5: `top-load` fixture and runtime proof

**Scope.** New private fixture `cells/tests/top-load` at `/data/bin/top-load` (EMPTY ceiling, no kernel or launch-profile change, no re-pin; DV-13), packaged by the terminal runner only; `s2tw-probe trunc` (T-a); driver `top_load` with 20 markers (#1–#20) and `P04 INFO top-load …` value lines (D65-9); runner boot window 1800 s. Typed lines reach the fixture as canonical session events (C1; C2 not built).

**Files.**

| Path | Bytes | SHA-256 | Phase 06 regions (current lines) and sharing |
|---|---:|---|---|
| `cells/tests/top-load/Cargo.toml` | 603 | `5b5da8d926aa194ae1db9c2dd7ef83f3a7f604e0f2fea47fb2e8ff2971db42e1` | New: package `app-top-load`, bin `top-load` |
| `cells/tests/top-load/build.rs` | 52 | `309c7f94ddf64d2c2d1fce2f9472d674869c6feb2218ebcadbf4bab5626d5f90` | New: build script (as the other fixtures) |
| `cells/tests/top-load/src/main.rs` | 12777 | `14731931916787954b11cf63d88b1e4069a574aa4593332707e333c2939123de` | New: W ≤ 31 workers (`worker` 105; park in `SetTimer` 124, syscall-free spin re-reading the command word every 4,096 iterations), line commands from session events, `bench N`, fixed 512-byte output buffer |
| `cells/tests/top-load/src/command.rs` | 7894 | `ced03e506b81995bd602f55393718c6aa953eea6642e31379b3efabd075e4d4d` | New: command parser and bounded line, host tests (6) |
| `cells/tests/phase04-terminal-driver/src/records.rs` | 24598 | `737a7a010f31172adb8f77e38d3e73c4976000cd431ec14a70e28c73d076da1c` | New: pure readers/comparisons (envelopes, `top.cell.v1`/`top.task.v1`, `TOPLOAD` acks, probe `trunc`, #8/#12-#15 evaluations); 10 host tests |
| `cells/tests/phase04-terminal-driver/src/top_load.rs` | 40432 | `5e57a522641e72a110e5b7b9d4ce39106b5a2acce9635f63f9c97f1c5ab0e2f7` | New: scenarios #1-#20, `info` 44 (`P04 INFO top-load …`) |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 103522 | `862499775d9907c6cdb7607aae55d173da37c79a30e6274af3f13f663f108bfb` | `mod records/screen/top_load` 33-35; `top_load::top_load(&mut a)` 357; `cfg_attr(not(test), no_std/no_main)`. Shared |
| `cells/tests/phase04-terminal-driver/src/screen.rs` | 22351 | `b546f43d43f7ecf89c5189f6d098db9ed93fa516835fb30be2ec81edd16fd1cd` | `Frame::status()`. Shared with S6-4 |
| `cells/tests/s2tw-probe/src/main.rs` | 12273 | `bf0958f0754b8cde839a6afdc48f609565086fc25445d9fe68ebbd7c933a70c6` | `trunc` mode (doc 31-36; one capture into header + one Cell row), `cfg_attr` for host tests. Shared with S6-1 |
| `Cargo.toml` | 8842 | `eaa3ce2a918e5a50217894a24fa7393de8b083c5d6e7ad1147ce1ffcf5d5184b` | Workspace member `cells/tests/top-load` |
| `Cargo.lock` | 83130 | `793edafae7d15bc23eb1fd4ec71b3272f3168aca9eeae33d20679b464ffbb401` | `app-top-load` package. Shared with S6-4 |
| `scripts/qemu-phase04-terminal-test.sh` | 56262 | `9fcd2507f46bfa3abaa2be99fc4ee47ef66d87ef991c94456ce5bf95cfaa451d` | S6-5 header 82-92; builds `-p app-top-load` 311, packages `/data/bin/top-load` only 340, FAT check 497, `s6_5_fixture` provenance 559, `acceptance-limits.txt` paragraph 406; 20 `EXPECTED` markers; `P04 INFO` echo (joined across `USER+ `); boot window 1200 → 1800 s 119/134. Shared |

**Deviations.**
- **DV-13** `/data/bin/top-load` with an EMPTY ceiling and no launch row. **Ratified by the user 2026-10-23 (D65-3).**
- Implementation deviations ([proposal §10.3](phase-06-s5-proposal.md), “for ratification with the run”; **all 13 ratified by the user 2026-10-24 with CP2**):
  1. `-a` added where Retiring rows matter (#2 task capture, #4 TUI, the #12/#13/#15 streams). **Ratified by the user 2026-10-24 (CP2).**
  2. #4 compares the TUI's TID set with `top --records --tasks -a` captures taken just before and after it. **Ratified by the user 2026-10-24 (CP2).**
  3. #19's UART part is covered by the existing log-wide `no-oom-heap-exhausted-uart` and `no-panic` checks plus both fixtures reaching `bye` with status 0; `CellFault` is an audit event, not a UART line. **Ratified by the user 2026-10-24 (CP2).**
  4. One TUI session (O at 40×120) serves #9–#11; #10's batch/records captures come after #11's keys. **Ratified by the user 2026-10-24 (CP2).**
  5. #12, #13 and #15 wait for the stream's first End before acting; #13's floor is Σ task `cpu_ns` in the last frame that still lists the exited TID. **Ratified by the user 2026-10-24 (CP2).**
  6. #8's busy frames are the #6/#7 Cells frames; #1, #5, #13, #15 frames are bound-checked only. **Ratified by the user 2026-10-24 (CP2).**
  7. #14: `quota_bytes` exactly once in every Cell row and in no task row; L2's value non-null and within `limit_bytes`. **Ratified by the user 2026-10-24 (CP2).**
  8. #16 takes the new incarnation from the last #15 frame and fails with the observed IDs if another launch took the slot. **Ratified by the user 2026-10-24 (CP2).**
  9. #17 statuses are recorded (INFO), not asserted. **Ratified by the user 2026-10-24 (CP2).**
  10. #20 runs first on host `a`; the probe prints only the `telemetry-trunc` line and the driver requires status 0. **Ratified by the user 2026-10-24 (CP2).**
  11. INFO format: one line per marker plus `launch`, `bench`, `identities`; a failed marker's line starts with `failed`; an unreachable scenario fails its markers as `not reached: <why>`. **Ratified by the user 2026-10-24 (CP2).**
  12. Fixture failure lines `TOPLOAD error timeout|spawn|clock`. **Ratified by the user 2026-10-24 (CP2).**
  13. The fixture, probe and driver crates use `cfg_attr(not(test), no_std/no_main)` so their pure parts have host tests. **Ratified by the user 2026-10-24 (CP2).**

**Evidence (final run; terminal log line numbers).**

| # | Marker (line) | Recorded values (INFO line) |
|---|---|---|
| 1 | `top-load-launch` 704 | `cells=["12:191/tasks=32", "11:159/tasks=32"]` (568); L1/L2 TIDs 55–85, 87–117 (565-566) |
| 2 | `top-load-over-64-records` 705 | `total_tasks=74 rows=74 complete=true dropped=0 ps_rows=74 ps_status=0` (569) |
| 3 | `top-load-over-64-text` 706 | `top_rows=74 header_complete=true`, `ps_rows=74`, statuses 0 (571) |
| 4 | `top-load-over-64-tui` 707 | `before=74 after=74 shown=74 pages=3 last=117 max=117 complete=true oom=false` (572) |
| 5 | `top-load-cpu-idle` 708 | L1/L2 4–6 tenths, `ok`, max 20 (573) |
| 6 | `top-load-cpu-busy` 709 | L1 880, busy TID 55 876 of task sum 878, L2 4, L1 shell 4 (574) |
| 7 | `top-load-cpu-two-busy` 710 | TIDs 55/56 at 495/437, L1 937 (575) |
| 8 | `top-load-cpu-sum-bounded` 711 | 31 frames, Σ ≤ 1012 (e.g. 994/1012 busy) (586) |
| 9 | `top-load-drill-live` 712 | Cell 11:159, `first_all_new=true numeric=true rows=32 tids_match=true`, busy TID 860 (576) |
| 10 | `top-load-views-agree` 713 | TUI 32 = batch 32 = records 32 (complete) (577) |
| 11 | `top-load-no-lifecycle` 714 | keys sent, still drawing, L1 quiet, `live=31`, TIDs unchanged (578) |
| 12 | `top-load-exit-gone-tasks` 715 | frames `32t 74 g0` → `31t 73 g1` → `g0`… (579) |
| 13 | `top-load-exit-retains-cpu` 716 | L1 `cpu_ns` 102.42 → 104.50 s, floor 73.44 s, never falls (580) |
| 14 | `top-load-quota-once` 717 | Cell 12:191 quota 4,617,554 ≤ limit 16,777,216; in-quota stacks 2,031,616 (581) |
| 15 | `top-load-relaunch` 718 | old 11:159 → new 11:223, `O-g0,--g1,-Nnewg0,-Nokg0…`, status 0 (582) |
| 16 | `top-load-id-reused` 719 | same Cell ID 11, generation 159 → 223 (583) |
| 17 | `top-load-gone-cell-empty` 720 | 0 rows, `complete`, statuses 0 (584) |
| 18 | `top-load-tui-gone-notice` 721 | drilled 11:223 keeps its title, no task rows, third generation 11:228 absent from the drill (585) |
| 19 | `top-load-teardown` 722 | both `bye` with status 0, shells alive, `oom=false` (587); log-wide `no-oom-heap-exhausted-uart` 992, `no-panic` 1012 |
| 20 | `control-telemetry-truncated` 723 | `emitted_cells=1 total_cells=8 … complete=0 status=0` (564) |

Host (§10.4): top-load 6, s2tw-probe 2, terminal driver 10. Fixture hash in provenance: `top-load` `96c3add3…` (549).

**Known limits.**
- One vCPU: no > 1000-tenth Cell at runtime (D60-5); `unavailable`/`reset` never occur at runtime (#12, #13, #15 add the negative: exit and restart never produce `reset`).
- `&` is still refused on every session shell; concurrency uses separate driver hosts (H1).
- `top`'s own truncation rendering is host-only (T-a: the runtime proof is the provider's).
- GUI 6/6 at 279/279 on the final tree (gap closure 2026-10-24, §5); the fixture is not in the GUI, P02 or P03 images (D65-11).
- The `bench` values are TCG numbers under the runner, not a hardware claim.

**Residual risks.** Busy phases spin a single vCPU for up to 5 s, so timing-sensitive checks elsewhere in the same run could be slowed; the run passed with the thresholds as approved. Membership drift between captures fails loudly rather than being tolerated.

**What CP2 would authorize.** The fixture, probe `trunc`, driver `records`/`top_load` and runner S6-5 regions above, with the 13 deviations.

[x] CP2 approved by user — date: 2026-10-24 (deviations 1–13 ratified: yes)

## 11. Extra fix V: VFS idle busy-loop (with the terminal-SDK `CwdLease`)

**Scope.** VFS's receive loop blocks in the attested `Recv` unless `prepared_cwd::idle_work` reports an unsettled prepared-cwd reservation; only then does it keep the nonblocking receive and the 128-turn reconcile. Follow-up: a proven-running child is settled while it or its owner is watched, and the terminal SDK's `CwdLease` contacts VFS when a reaped child is dropped (`CloseDir`, or `ResolveCwd` action 0 for a grant-derived cwd), so the receipt is settled before the Operation closes. Owner death still polls until the cancelled child ends. No kernel or ABI change.

**Files.**

| Path | Bytes | SHA-256 | Phase 06 regions (current lines) and sharing |
|---|---:|---|---|
| `cells/services/vfs/src/main.rs` | 9683 | `17c3ccf534a32b63364f53655813ba0dfe5c20702321d38f560dbcf54b58c7a8` | Receive loop: `idle_work` 121, blocking `sys_recv_attested` unless `idle_work` 147-154 (nonblocking receive + 128-turn reconcile only then) |
| `cells/services/vfs/src/prepared_cwd.rs` | 47428 | `6291277e3b0df968be531d7acd89408b4c1bf3d98efc8bce592cb39c8e4f4749` | `idle_work` 731; tests `idle_loop_blocks_only_once_every_reservation_waits_on_a_message` 858, `silent_child_of_a_live_owner_lets_the_loop_block_until_the_owner_dies` 889, `retired_and_dead_owner_state_polls_only_while_reconcile_can_progress` 912. Rest: Phase 02 |
| `cells/services/vfs/src/manager/owned_state.rs` | 2934 | `56e44ae6695350dee0ec8c5321d6e55786b20659d9a9b68d0421d396b7077578` | Owner-watch bookkeeping read by `idle_work` (`install_owner_watch` 8-30) |
| `cells/services/vfs/src/dir_admission.rs` | 3274 | `aa852af6b069ce129e9f1763ba6b2d38cdeaae097f83341de238bac1319a0015` | `prepare_contact` 23 settles a proven-running child while it or its owner is watched |
| `libs/terminal/src/pipe_child.rs` | 32162 | `52aaf1ee47a897bea055a95ff2cbfa677fe74ded261c9294ae7874ff1ec82c7c` | `CwdLease` 382-384 (drop contacts VFS: `CloseDir`, or `ResolveCwd` action 0 for a grant-derived cwd); `PipeChild._cwd` 124, lease setup 297-343. **Phase 05 CP2-approved file**; rest: Phase 02–05 |
| `libs/terminal/src/session.rs` | 31002 | `a38aed295ad1fa7d79ed58becfd9cc902ad220ea24ad982078e541c12e03fced` | `ForegroundChild` holds a `CwdLease` 209, 248; import 8. **Phase 05 CP2-approved file**; rest: Phase 02–05 |

**Deviations / approval state.**
- The fix itself: **approved by the user 2026-10-23** (phase log).
- The SDK `CwdLease` change in `libs/terminal/src/{pipe_child,session}.rs` (Phase 05 CP2-approved files) was made by the follow-up under the same approval but has no line of its own. **Confirmed by the user 2026-10-24 (CP2).**

**Evidence.**
- `reports/vfs-idle-evidence/`: P03 serial before the follow-up shows `vfs` `cpu_tenths` 0 at idle but 985–988 in 4 frames while `/bin/p03-filter` ran; after it, 0 in all 9 frames, and no close-failed lines. P03 62, terminal 399/399, P02 48, GUI 6×259 on that tree.
- Final: P03 62 and terminal 430 on the current tree; `top-load-cpu-idle` and the idle-cost check see no VFS spinner [INFERENCE: the Σ-bound frames would include it].
- Host (phase log): vfs 79, terminal-sdk 52.

**Known limits.** While an owner is dead and its cancelled child still runs, VFS polls until the child ends.

**Residual risks.** A missed wake source would make VFS sleep through work it should reconcile; the three host tests cover the known sources (request, owner-death notification).

**What CP2 would authorize.** The VFS regions and the SDK `CwdLease` regions above.

[x] CP2 approved by user — date: 2026-10-24 (SDK `CwdLease` change confirmed: yes)

## 12. Extra fix U: kernel UART console `USER: ` prefix (`USER+ ` continuation)

**Scope.** `emit_user_record` reset the console owner after an inner `\n`, so the next 512-byte relay record got `USER: ` mid-line and a kernel line could be spliced into an open Cell line. Now `emit_record` sets the owner from each fragment; a kernel line that interrupts a Cell line breaks first, and the Cell continues on a line tagged `USER+ `. Parsers rejoin `USER+ `: P03 `cell_lines`/`body_of` (`relayed()` removed), the P04 terminal UART checks, the P02 TUI parser (S6-4), `bench_results.py`, and the x86 c2c and VMware scripts.

**Files.**

| Path | Bytes | SHA-256 | Phase 06 regions (current lines) and sharing |
|---|---:|---|---|
| `kernel/src/task/drivers/uart.rs` | 26445 | `f716e9641c94513d53e49066d40deb82a7fd4e2a661914182950851f55fdff31` | `emit_record` 213 sets the console owner from each fragment; `USER_CONTINUATION = "USER+ "` 176, `USER_TAGS` 195, doc 149-154, 247; 7 `task::drivers::uart` tests |
| `scripts/qemu-phase03-pipeline-test.sh` | 53447 | `0a3c222de9bd8547fcac552997d04a4c9d42d2e30c29f435d635a3cec40ccd57` | `cell_lines` 529-539 and 687-699, `body_of` 701 (drop kernel lines, strip line-start `USER: `, join `USER+ `); `relayed()` removed; frame checks use `rows()`. Shared with S6-2 |
| `scripts/qemu-phase04-terminal-test.sh` | 56262 | `9fcd2507f46bfa3abaa2be99fc4ee47ef66d87ef991c94456ce5bf95cfaa451d` | UART reassembly 766-775. Shared |
| `scripts/bench_results.py` | 53274 | `fdea6feae918159022dc64ffe3eed45bcc83703e09c2758879b296ba2f71b13f` | `USER+ ` rejoin 153-160 (not a Phase 06 runner) |
| `scripts/test_compare_bench_results.py` | 18586 | `7edc170e4e9293e6fd1e977bf649f5820043b9e643224c1f9d1fc866a48a3775` | Test of the rejoin 411 |
| `scripts/x86/run-c2c-render-smoke.py` | 19495 | `65041a9bce72aa94552e75242ceb7c382b3bdb524b937e8b846e4a227c82d935` | `USER+ ` rejoin 25-29 |
| `scripts/x86/test-native-vmware-matrix.py` | 11183 | `0bce87014155648a1e30db359225d8ef4cb17002a3d5b1b083cb409743cb5996` | `USER+ ` rejoin 20-25 |

**Deviations / approval state.**
- The kernel fix: **approved by the user 2026-10-23** as a separate change.
- The `USER+ ` marker format (a distinct tag so parsers can rejoin exactly) is recorded as the parent's decision. **Ratified by the user 2026-10-24 (CP2).**
- The rejoin edits in `bench_results.py`, `test_compare_bench_results.py`, `run-c2c-render-smoke.py` and `test-native-vmware-matrix.py` (outside the Phase 06 runners) were not named in the approval. **Ratified by the user 2026-10-24 (CP2).**

**Evidence.**
- `reports/s6-4-evidence/phase03-serial-uartfix.raw.log`: 0 mid-line `USER: `, 281 line-start `USER: `, 1 `USER+ ` line. For comparison, `reports/vfs-idle-evidence/phase03-serial.log` (before the fix) has 19 lines with a mid-line `USER: `.
- Host (phase log): 7/7 `task::drivers::uart` tests. Builds: x86_64 (runner features), riscv64, aarch64, aarch64 `board-rpi3`.
- Final: every runner that parses UART output passed (terminal 430, P03 62, P02 48, GUI 279).

**Known limits.** The four non-runner scripts were not executed against a fresh capture. A consumer outside this repository that expects only `USER: ` sees `USER+ ` as an unknown prefix.

**Residual risks.** Console output format change on every image (serial users see `USER+ ` lines after kernel log interruptions).

**What CP2 would authorize.** `uart.rs` and the parser regions above, including the `USER+ ` format.

[x] CP2 approved by user — date: 2026-10-24 (`USER+ ` format and the four script edits ratified: yes)

## 13. Extra fix Z: `sleep` waits real seconds

**Scope.** `sleep N` waits N real seconds on every architecture and parks in one-tick `SetTimer` steps (the shared `top::wait_ns`). It previously yield-spun on raw `GetTime` ticks assumed to be 10 MHz, which on x86 (HPET ns) made `sleep 1` a 10 ms busy wait.

**Files.**

| Path | Bytes | SHA-256 | Phase 06 regions (current lines) and sharing |
|---|---:|---|---|
| `cells/tools/cellbox/src/cmd_sys.rs` | 11107 | `1a9a3180eff37113a85363ca1b36f41d3b43d26643e59621546f0a7e37ce6bd4` | `cmd_sleep` 117-130 and `sleep_delay_ns` 132: N real seconds via `top::wait_ns` (one-tick `SetTimer` steps). `cmd_uptime` 101-103 and `cmd_date` 282-287 still divide by 10,000,000 (D60-3, not changed). File has no earlier per-file hash |
| `cells/tools/cellbox/src/top.rs` | 6343 | `fff763b7ab9c7a3debabd4a400f486e968f1d058d9d040595c39bb4e4760cf7a` | `wait_ns` 109 (shared `top` wait). Shared with S6-2, S6-4 |

**Deviations / approval state.** Landed despite D60-3 (which put the 10 MHz users outside Phase 06). **Ratified by the user 2026-10-23 (B).** `uptime` and `date` remain in D60-3 with their own CP1.

**Evidence.** `reports/s6-23-evidence/gui-post-sleep.log` 259/259 and the two P03 62/62 runs on the `sleep`-fix tree; every later run (P03 uses `sleep 1;` between samples).

**Known limits and open defects.** `uptime` (`cmd_sys.rs` 103) and `date` (287) still divide raw ticks by 10,000,000 (D60-3, open).

**What CP2 would authorize.** `cmd_sleep`/`sleep_delay_ns` and the shared `wait_ns`.

[x] CP2 approved by user — date: 2026-10-24

## 14. Extra fix Q: `$(…)` status propagation and `$(top)` refusal

**Scope.** `$(…)` ran only `echo`/`cat`/`vcat`/`pwd` (`run_capture`) and substituted every other command as empty with status 0, so `x=$(top)` never reached the S6-4 gate. Now an interactive-class `top` runs under its own `CaptureGuard` (depth 2), where `tui::run` refuses (2, `-b` hint, nothing drawn). A failed substitution carries its status (`SubstFailed`, was always 1) and the assignment does not run. Batch `top` forms in `$(…)` stay unsupported and empty, like other unlisted commands.

**Files.**

| Path | Bytes | SHA-256 | Phase 06 regions (current lines) and sharing |
|---|---:|---|---|
| `cells/tools/cellbox/src/executor.rs` | 153078 | `db3ff92e4494942594cf7c16bb6a1c3f5568b42fd02ffb84accfd89e2b631bd2` | `SubstFailed(i32)` 813-826; `run_capture` 827+: interactive-class `top` arm 856-870 under its own `CaptureGuard` (depth 2) where `tui::run` refuses with 2; failed substitutions carry their status through `expand_token` 881, `expand_word` 1016, `stage_input` 1178, and the assignment does not run. Shared |
| `cells/tools/cellbox/src/shell_test.rs` | 41332 | `1af74e4773d57127cd7e112edc95b989bea891b2b9510888f12fc788ab31254b` | `st_top_subst=$(top)` → 2 and “a refused substitution assigns nothing” 301-304. Shared |

**Deviations / approval state.**
- `x=$(top)` refusing with 2 is the CP1-approved S6-4 gate (§2 of the [S4 proposal](phase-06-s4-proposal.md)).
- The `top` arm in `run_capture`, the status carried by every failed substitution, and “the assignment does not run” are general shell behaviour changes outside any CP1. **Accepted by the user 2026-10-24 (CP2).**

**Evidence.** Terminal `top-subst-refused` (S6-4 log 666; final 693). `shell_test` gains the status and no-assignment checks (built for `x86_64-unknown-none`; no archived shell-test lane run). Host (phase log): app-cellbox 121.

**Known limits and open defects.** Unlisted commands in `$(…)` (including `top -b`) still substitute as empty with status 0. The CHANGELOG entry for the substitution-status change was added after approval (§18).

**What CP2 would authorize.** The `SubstFailed`/`run_capture` regions and the `shell_test` checks above.

[x] CP2 approved by user — date: 2026-10-24 (out-of-CP1 substitution changes accepted: yes)

## 15. Historical slice evidence (archived runs on earlier trees)

| Directory | Tree | Totals |
|---|---|---|
| `reports/s6-1-evidence/` | after S6-1 | terminal 399/399 (117), P03 60, P02 48, kernel AP 38/38, GUI 259 (one run) |
| `reports/s6-23-evidence/` | after S6-3 + S6-2 (+ P03 runner fixes, `sleep` fix for the last GUI run) | terminal 399/399 (117), P03 62 ×2, P02 48, GUI 6×259 + 1 |
| `reports/vfs-idle-evidence/` | after V | terminal 399/399 (117), P03 62, P02 48, GUI 6×259, two P03 serial logs |
| `reports/s6-4-evidence/` | after S6-4, U and Q | §5 table |

| Log | Bytes | SHA-256 | Total |
|---|---:|---|---|
| `reports/s6-1-evidence/gui-click1.log` | 40715 | `74ba109d6414cad9df1150ba3a30b1bec53c5f677b3f717d690f87898746d28d` | 259/259 GUI |
| `reports/s6-1-evidence/phase02-kernel-serial.log` | 22160 | `6e2d83926dbbdb85490fe54ac7f51bec1c4a8b6119b451c034b3ca7095c43921` | ATOMIC_PUBLICATION 38 PASS, 0 FAIL |
| `reports/s6-1-evidence/phase02.log` | 20238 | `f8563b24c526d3f10722774d8e407b43c1ddedcb92779927b0518d35c6f2fff9` | passes: 48 |
| `reports/s6-1-evidence/phase03.log` | 21924 | `d953fcda156f8244ba724db4e00efc7865577ea5e796369db2b81fd5406c8248` | 60 PASS |
| `reports/s6-1-evidence/phase04-terminal.log` | 38196 | `b715e7baada9d0acce57cdf7465169b75de206aac7e3ea64cc935aa92ba1dd0a` | 399/399, 117 markers |
| `reports/s6-23-evidence/gui-autoload.log` | 40753 | `a0aa7a730be7fb10d7b0db5dfe287305afb3d908cd70330185a3fc0f3cad7f55` | 259/259 GUI |
| `reports/s6-23-evidence/gui-click1.log` | 40715 | `d5411061686e7e28742e25ee107c0cbb61038010c8bb1bc1c133074ed0aeff9b` | 259/259 GUI |
| `reports/s6-23-evidence/gui-click2.log` | 40715 | `fd25a8c1f1070fd5a26bf68dd991f96bf935a9ade7ba0f180edc6697a0e56f07` | 259/259 GUI |
| `reports/s6-23-evidence/gui-click3.log` | 40715 | `0fd6c3880152853ae80e9de7d6b8448407512bb5e341f1cad8432f804cfbf9cf` | 259/259 GUI |
| `reports/s6-23-evidence/gui-click4.log` | 40715 | `d636823a7da01b6bdfe611312ec53b93d609cbb24df4a80d34b076e2122df3e3` | 259/259 GUI |
| `reports/s6-23-evidence/gui-click5.log` | 40715 | `874d3fe273dcc11d567bc5f0469c1282d7412fe881d276cf2cf73de8c9795019` | 259/259 GUI |
| `reports/s6-23-evidence/gui-post-sleep.log` | 40715 | `7b1197ce673b7380484f7953df24bfc37294f48dc3d533caacebfe62d63a3ec0` | 259/259 GUI (after the `sleep` fix) |
| `reports/s6-23-evidence/phase02.log` | 20238 | `779a662c5a6597252d85d9cd180b670fca77d98ad98d32d668125c8be90a53c4` | passes: 48 |
| `reports/s6-23-evidence/phase03-1.log` | 21980 | `ad11882fe71ded97f5a67cb62eaa605147485608ab6bd800771d3f5f9cfdfd4c` | 62 PASS |
| `reports/s6-23-evidence/phase03-2.log` | 21980 | `ef631a3d8f8f62dcdba24a2363b33955873048c542ff3b32af6c92f391237a96` | 62 PASS |
| `reports/s6-23-evidence/phase04-terminal.log` | 38196 | `42d8a423536308a93bc2fa0df79ff7b8ecf07ecb202ad162508ee0a87e890749` | 399/399, 117 markers |
| `reports/vfs-idle-evidence/gui-autoload.log` | 40753 | `336495bb292d255d730ae33d73aa619cbed5164166589ed85aa077230db9f20f` | 259/259 GUI |
| `reports/vfs-idle-evidence/gui-click1.log` | 40715 | `da76b75ccc9e97c8e47c60bf72ae89a5637837d304d5941652931d752ac905a7` | 259/259 GUI |
| `reports/vfs-idle-evidence/gui-click2.log` | 40715 | `da1be9f59511e1b092e3134eb2b0ef1b0dd2f7a9babe65458e9fe1d5675f5cd9` | 259/259 GUI |
| `reports/vfs-idle-evidence/gui-click3.log` | 40715 | `0a0ec935e8247cd854d8ca9df290c4c4c6fa7283f838c4fe0e238d351d126aa5` | 259/259 GUI |
| `reports/vfs-idle-evidence/gui-click4.log` | 40715 | `9a79003929777b88bd7f5fc356a288eee6d1ce2128edb5aa21b4f8365767344c` | 259/259 GUI |
| `reports/vfs-idle-evidence/gui-click5.log` | 40715 | `60be2924ebbf59e4692319bde58f38b47ec64e90df5b898425e44e7394cbf7b8` | 259/259 GUI |
| `reports/vfs-idle-evidence/phase02.log` | 20238 | `575e6b901eef19c692ad33fbc808190458ce4210f9b2dd0aacfc28dcf50c9e13` | passes: 48 |
| `reports/vfs-idle-evidence/phase03-serial-before-child-fix.log` | 48484 | `7e450cd00d1da4a8a803aa96bc8fc1f247f44a7c1c837d335d4418e643765c1e` | P03 serial before the external-child follow-up: `vfs` `cpu_tenths` 0 ×4 idle, 985–988 ×4 during `/bin/p03-filter` |
| `reports/vfs-idle-evidence/phase03-serial.log` | 48473 | `ef6e0076c295bc5222ed339ef15e40969b3a2adb6770749070e27b0b1c79c4d5` | P03 serial after the follow-up: `vfs` `cpu_tenths` 0 in all 9 frames |
| `reports/vfs-idle-evidence/phase03.log` | 21980 | `a1f41850365804cb61d941cd51d770ab7d2a92b0c84cf08d7803aec163ef9b9e` | 62 PASS |
| `reports/vfs-idle-evidence/phase04-terminal.log` | 38196 | `971880b39e888a0b285a0844cc20c1d3d2e58523ccda51832c33a565b42f9dea` | 399/399, 117 markers |

Failed or superseded runs whose logs were not archived: P03 `hLN8fY` (59/62), `fecweV`, `vxgD7j`, `2boe79`, `CbvXmW` (60/62 each); terminal `JSM1uq` (`top-subst-refused`, `top-tui-frame` failed). Their causes are recorded in the proposals; only the `/tmp` directories remain.

## 16. Phase 06 success criteria and steps → evidence

| Criterion / step | Runtime evidence (private QEMU/TCG, 1 vCPU) | Host-only or recorded deviation | Outcome |
|---|---|---|---|
| **C1** incarnations distinct across restart/reuse; drill-down returns actual tasks | #15–#18 (Cell 11: 159 → 223, never both generations in one frame, gone Cell empty, TUI drill keeps its title and shows no third-generation row); #9 TIDs = fixture spawn list; `top-tui-drill` | S6-1 generation-reuse ledger test; S6-2 New/Gone; S6-4 gone notice | **Met** |
| **C2** shared memory not summed; CPU convention and unknown samples explicit | #14 quota once per Cell, never in task rows; #5–#8 core-equivalent tenths with Σ bound; #9 `new` then numeric; #13 retention without `reset`; P03 meta `cpu_unit` | `unavailable`/`reset` and > 1000 tenths host-only (D60-5) | **Met** (qualified) |
| **C3** missing/denied/truncated not shown as zero load or complete inventory | `sandbox-denied-telemetry`; #20 provider truncation explicit (1/8, `complete=0`); #2–#4 74 tasks complete in records, text, `ps`, TUI | `top`'s denied and `TRUNCATED n` rendering host-only (D65-1, D65-8) | **Met** (qualified) |
| **C4** TUI/batch/typed agree for one snapshot; no lifecycle rights | #10 same TID set in all three; `top-tui-drill`; #11 and `top-tui-no-lifecycle` (`k`/`K`/Delete change nothing); `kill` stays `InSuiteRefused` | Numeric same-snapshot = D64-14 constructed host pair (approved); runtime views are separate captures | **Met** (qualified) |
| Step 1 identity, membership, units, completeness from an admitted provider | S6-1 provider; #1, #2, #16; P03 meta | DV-1..DV-8 ratified | Done |
| Step 2 Cell vs task projection; no repeated heap; no RSS label | #14; schemas `top.cell.v1`/`top.task.v1` | DV-4 ratified (quota charge, no attributed total) | Done |
| Step 3 CPU deltas per incarnation; new/reset/stale/unavailable; no per-task clamp | #5–#9, #12, #13, #15 | multi-core and `unavailable`/`reset` host-only (D60-5) | Done (qualified) |
| Step 4 no silent 64-row ceiling; bounded frames | #2–#4, #20; P03 frame checks | DV-11 ratified | Done |
| Step 5 finite and streaming records; TUI with fields, sort, drill-down; batch without keys or ANSI | `top-pipe-batch`, `top-tui-*`, GUI `TOP-*`, #4, #9, #12, #15 | — | Done |
| Step 6 real multi-thread Cell, restart/reuse, denied, > cap, cross-mode, no double count | whole §10 table | “one recorded snapshot” is the constructed D64-14 pair (no capture-dump path) | Done (qualified) |
| Verification: provenance recorded; no CPU-speed or hardware claim | `provenance.json` in every log; `P04 INFO` values; `acceptance-limits.txt` | — | Done |
| Entry §9 K1 hot-path cost | INFO `bench` (1.7 µs `GetTime`, 403 µs `Yield`, TCG) | no before/after A/B (D65-10) | Recorded |

All four criteria are checked in [phase-06-cell-top.md](phase-06-cell-top.md) with these qualifications; its status is `completed-private-evidence` since the CP2 approval (2026-10-24; rollout blocked). CBX-17/CBX-18 stay pending in [acceptance.md](acceptance.md) (Phase 10 qualification).

## 17. Unattributed changes

**None found.** All 62 source paths in the Phase 06 window are attributed (§2), and no file was deleted (§4).

Caveats:
- **Generated files.** `scripts/__pycache__/{bench_results,test_compare_bench_results,phase04_gui_drive}.cpython-312.pyc` and `scripts/x86/__pycache__/{run-c2c-render-smoke,test-native-vmware-matrix}.cpython-312.pyc` were (re)written at 19:08/19:24 by `py_compile` checks; they are gitignored (`.gitignore:89`) and not source.
- **Touched, unchanged.** `cells/tools/cellbox/src/main.rs` (mtime 19:57) equals its Phase 05 hash.
- **Edits invisible to the method.** Transcript attribution cannot see an edit made by a shell command that matches none of the tracked patterns; the mtime scan would still have listed such a file, and it lists none beyond §3.
- **Bytes outside every Phase 06 region.** Many files carry Phase 02–05 bytes inside the same whole-file hash with no earlier per-file hash (for example `pipeline.rs` Phase 03 engine code, `cellbox/src/top.rs` moved from `cells/tools/shell`, `cmd_sys.rs`, `shell_test.rs`). Those bytes are **not** attributed to Phase 06 and are not reviewed here.
- **Plan document digest drift.** [spec.md](spec.md) changed from the Phase 05 CP2 record `7e6e11c7…` to `cde24178…`: the S6-3 §1/§2 amendments (D63-2/D63-3, accepted decisions), written by `S63S62Impl`. Not diff-verified here.

## 18. Docs not reconciled at preparation (reconciled 2026-10-24 after approval; see the last bullet)

- **`CHANGELOG.md`:**
  - the S6-3/S6-2 (line 144), S6-4 (148) and S6-5 (153) entries still say “host tests and bare-metal builds only, runner proof pending”, although the runs are archived (`s6-23`, `s6-4`, `s6-5` evidence);
  - the S6-1 (143), console (170), `sleep` (171) and VFS (172) entries cite no runtime evidence;
  - no entry describes the `$(…)` change (Q): failed substitutions now carry their status and skip the assignment (line 160 mentions only `x=$(top)`).
- **[plan.md](plan.md)** row 06 still reads “incomplete/blocked”.
- **[command-catalog.md](command-catalog.md)** `ps` row (59) still cites `-s` state order and 64 rows (`ps` moved to the provider in S6-2, D62-2); the `top` row (73) keeps its pre-Phase-06 text ahead of the S6-4 note.
- **[acceptance.md](acceptance.md)** CBX-17/CBX-18 stay pending (Phase 10).
- **[phase-06-s5-proposal.md](phase-06-s5-proposal.md)** §10 heading still says “private QEMU run pending”; [phase-06-s4-proposal.md](phase-06-s4-proposal.md) §10 and [phase-06-s2-s3-proposal.md](phase-06-s2-s3-proposal.md) §10 headings likewise say “pending”.
- **Reconciled 2026-10-24 after approval** (records edits only; no source change):
  - `CHANGELOG.md`: the S6-3/S6-2, S6-4 and S6-5 headers now record CP2 instead of “runner proof pending”, and each of those entries plus S6-1, console (U), `sleep` (Z) and VFS (V) gains an archived-runs/CP2 sub-bullet; a new Changed entry describes the `$(…)` change (Q).
  - [plan.md](plan.md) row 06: `completed-private-evidence`, rollout blocked.
  - [command-catalog.md](command-catalog.md): the `ps` row gains a Phase 06 S6-2 note (one capture, no 64-row cap, `-s` by `State`); the `top` row gains an S6-2 note ahead of the S6-4 note. The first column stays the pre-Phase-06 baseline inventory, as for every row.
  - S2-S3, S4 and S5 proposals: §10 headings name the archived runs and the CP2 approval, with one pointer line each.
  - [phase-06-cell-top.md](phase-06-cell-top.md): status `completed-private-evidence` with a log line (rollout still blocked).
  - Acceptance rows stay pending (Phase 10).

## 19. What CP2 does not do, options, rollout and rollback

**CP2 does not:**
- lift quarantine;
- authorize canonical packaging, release signing or rollout;
- close CBX-17/CBX-18 or any other acceptance row;
- accept the Phase 02–05 bytes inside the shared files;
- authorize KVM, physical, remote or multi-hart claims. All evidence is private QEMU/TCG with one vCPU.

**Shared files.** A whole-file hash in §3 becomes a baseline only when every item that touches it is approved. Examples:
- `executor.rs`: S6-3, S6-2, S6-4, Q (and Phase 02–05);
- the driver `main.rs`: S6-1, S6-2, S6-4, S6-5; the terminal runner: S6-1, S6-4, U, S6-5;
- `top.rs` (cellbox): S6-2, Z, S6-4; `cmd_sys.rs`: Z, S6-4; the P03 runner: S6-2, U;
- `CHANGELOG.md`: every item except Q.

Approving one item alone authorizes its regions, not the other items' bytes in the same file.

**Dependencies between items:**
- S6-2 needs S6-1 (provider) and S6-3 (Frames); S6-4 needs S6-2's table model; S6-5 exercises all four.
- Q exists to make the S6-4 gate reachable from `$(…)`.
- U's `USER+ ` format is read by the S6-2 P03 parsers, the S6-4 P02 parser and the S6-5 INFO echo.
- Z uses S6-2's `wait_ns`.
- V's SDK `CwdLease` changes Phase 05-approved SDK files.

**Options.**
- **A.** Approve every item as listed, with every pending ratification in §6–§14.
- **B.** Approve a subset. The non-approved items' regions must then be reverted as matched sets (below) before any shared file can become a baseline.
- **C.** Not yet: keep the implementation and evidence and withhold CP2. Reruns can be requested first, for example five more GUI runs and the kernel AP lane on the final tree, or an A/B of the scheduler pass cost.

**Rollout.** Stays **BLOCKED** under [containment](reports/rollout-containment.md). Nothing is unblocked.

**Rollback.** Every rollback is a matched set: source plus its driver/runner checks, and a cold-booted matched private image.
- **S6-1:** syscall 430 and the V1 layouts can be removed only because no image has shipped them (fixtures are private). Never reinterpret `TelemetryCellV1`/`TaskV1` bytes as another version. `GetProcs2`/`MemInfo` are untouched, so old consumers keep working. Requires reverting S6-2, S6-4 and S6-5 first.
- **S6-3:** revert the Frames engine with S6-2's consumers and the P03 Frames checks.
- **S6-2:** revert the model, cellbox cutover and runner migration together; the old `GetProcs2` sampler comes back with its 64-row cap.
- **S6-4:** revert the view, `with_tui`/`TuiSink`, `enter_bounded`, the table-model refactor (batch bytes are identical), the driver grid model, `TOP-*` and the P02 `top` step.
- **S6-5:** revert the fixture, probe `trunc`, driver scenarios and runner block; no kernel or pinned file is touched.
- **V:** revert the VFS loop and the SDK `CwdLease` together; VFS then yield-spins again.
- **U:** revert `uart.rs` and every `USER+ ` parser together.
- **Z:** revert `cmd_sleep` (back to the 10 MHz busy wait on x86).
- **Q:** revert `SubstFailed`/`run_capture`; `x=$(top)` then substitutes empty with status 0 again and `top-subst-refused` fails.

Bytes already delivered to terminals or written by private guests cannot be recalled.

## 20. Approval record (2026-10-24)

User decision, 2026-10-24: **CP2 APPROVED for all nine Phase 06 items** in this dossier (S6-1, S6-3, S6-2, S6-4, S6-5, V, U, Z, Q), i.e. option A of §19. It covers each item's listed regions at the §3 hashes, and also:
- **Ratified deviations** (every mark that read “pending” in §6–§14):
  - S6-1: implementation deviations 1–5 (§6).
  - S6-3: implementation deviations 1–2 (§7).
  - S6-2: implementation deviations 3–9 (§8).
  - S6-4: deviations 1–8, 10, 11, the rest of 12, and the driver `TUI_ENTRY` change (§9).
  - S6-5: implementation deviations 1–13 (§10).
  - Ratifications given earlier stand as recorded: DV-1..DV-13, (A) P03 kill victim `init`, (B) `sleep`, S6-4 deviation 9, the 500-tenth idle threshold and Ctrl-C → 130.
- **Accepted changes outside CP1.**
  - Q: the `top` arm in `run_capture`, general `$(…)` status propagation (`SubstFailed`) and “the assignment does not run” (§14).
  - V: the terminal-SDK `CwdLease` edits to the Phase 05-hashed `libs/terminal/src/{pipe_child,session}.rs` (§11).
  - U: the `USER+ ` marker format and the rejoin edits in the four non-runner scripts (§12).
- **Phase 05 baseline drift.** The 15 drifted Phase 05 CP2 paths (§2) are ratified: their change from the Phase 05 hashes is the Phase 06 work attributed in §2, and their §3 hashes replace the Phase 05 hashes as baseline.
- **Evidence.** The final-tree gap was closed before recording: GUI 6/6 at 279/279 and kernel AP 38/38 (§5 gap-closure table, with log SHA-256s).

Since every item is approved, the §19 shared-file rule is met and every whole-file hash in §3 is the Phase 06 implementation baseline. At recording time all 62 §3 paths matched the disk; `CHANGELOG.md` differs afterwards only by the doc-only edits of §18.

Unchanged by this approval: everything in §19 “CP2 does not”. Quarantine, canonical packaging, release signing and rollout stay **BLOCKED** under [containment](reports/rollout-containment.md). CBX-17/CBX-18 stay pending (Phase 10). The open defects stay open: `uptime`/`date` on the 10 MHz assumption (D60-3), `input` shown `Ready` in single captures (§8), no scheduler-pass A/B (§6), host-only `unavailable`/`reset`, denied and truncated TUI rendering.
