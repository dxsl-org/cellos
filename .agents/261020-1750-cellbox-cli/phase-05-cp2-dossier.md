# Phase 05 CP2 dossier: implementation review request

**Status: CP2 APPROVED by the user on 2026-10-23 for all six slices (S5-1, S5-2, C, S5-3, S5-4, R), option A of §15; approval record in §16.** Each slice below has its own approval line. Each approval makes **only that slice's listed regions**, at the exact file hashes below, part of the Phase 05 implementation baseline. Quarantine, canonical packaging, release signing and rollout stay **BLOCKED** under [containment](reports/rollout-containment.md).

Prepared 2026-10-23 from the current disk bytes (HEAD `3471028bc`, nothing committed) and the saved evidence. The only commands run were `sha256sum`/byte counts, read-only `git status`/`diff`/`ls-tree`/`show`, greps of the archived logs, a listing of Cargo dep-info files under `target/` (to recover deleted paths), and reads of the Phase 05 implementer transcripts (to attribute edits). Nothing was built, tested, signed or booted. No source file was changed; the only other doc edit is one pointer line in [phase-05-ratatui-terminal.md](phase-05-ratatui-terminal.md).

Approval recorded 2026-10-23 (§16). At recording time all 64 source files in §3 still matched their listed SHA-256. `CHANGELOG.md` alone differed, by the post-approval records edits (Phase 05 entries reconciled per §14); later `CHANGELOG.md` edits are doc-only and outside the approved bytes of §3.

## 1. Authority chain

| Slice | CP1 / decision record | Approval wording | Gaps this CP2 must close |
|---|---|---|---|
| **S5-1** Ratatui backend | [Entry dossier](phase-05-entry-dossier.md) §3–§4 | “chốt” 2026-10-22: D5-1 pins `=0.1.0`/`=0.3.0`, D5-2 order, D5-3 S5-1 CP1 as written | Mode-guard deviation (§6 of the entry dossier) has no ratification line |
| **S5-2** VT screen + soft reset | [S5-2 proposal](phase-05-s2-vt-screen-proposal.md) §2–§5 | “chốt quyết định” 2026-10-22: D52-1 full profile, D52-2 soft reset, D52-3 `vt-sample.bin`. Option A (tracker instead of raw-lease observation) approved by the user during implementation. LF = newline ratified by the [spec](spec.md) §4 amendment (user decision, change log line 116) | Deviations 3–5 are recorded facts with no ratification line |
| **C** S5-3 option C frontend capability | [Annex](phase-05-s3c-frontend-annex.md) §3–§10 | Text approved with changes (D-C1…D-C4); “duyệt CP1” 2026-10-23 for SHA-256 `41e7f0b1903f418df8d677c008cde8363fe3d1fc427018e1d3cffe889b05dea6` (digest before its status amendment). Deviations 1 (R) and 2 (A) approved during implementation; 5 and 7 ratified (“duyệt”) | Deviations 3, 4, 6, 8, 9, 10 are recorded facts with no ratification line |
| **S5-3** proof consumer + selective TUI | [S5-3 proposal](phase-05-s3-proposal.md) §3 | Its status line still reads “PROPOSED”. D53-1/D53-2 were superseded by option C; the annex §7 schedules “S5-3 proper per the S5-3 proposal §3”; the phase log records “D53-3 taken as recommended” | **No explicit user approval of the S5-3 proposal text is recorded.** The unrequested `/bin/x > FILE` change is outside any CP1 |
| **S5-4** secrets, hostile output, glyphs, theme, grid fix, `tui-proof` removal | [S5-4 proposal](phase-05-s4-proposal.md) §1–§6, §9 | D54-1…D54-6 recorded as decided in its status line; grid fix “user-approved 2026-10-23” (§9); D54-4 = removal after evidence | The status line still says **“awaiting ‘go’ for implementation”**; no “go” is recorded in the plan docs. `${NAME}`/`${#NAME}` expansion and the `cat` fix are product changes outside the named closure |
| **R** renames | [S5-4 proposal](phase-05-s4-proposal.md) §5 (D54-6), `docs/code-standards.md` Law 6 | User decision “viTUI = one crate `vitui`…”, recorded 2026-10-23 | `httpd` output string change (§11) not called out in the decision |

Earlier CP2 baselines that these slices modify: Phase 02 CP2, S3-gui, S3-output, S2-K, S2-N, S2-T/W, OOM, S4a and C5 (see the master inventory, §3). Their approvals cover their recorded hashes only; none extends to the current bytes.

## 2. Method and attribution

- **Phase 05 window.** A file belongs to the review set if its mtime is after 2026-10-22 21:52 (+07), the last Phase 04 write (the C5/S4a dossiers and the multicall phase file). `git status --porcelain -uall` lists **70** such paths: 63 source paths and 7 plan documents. Two pure git renames with older mtimes (`libs/vimio/src/lib.rs`, `libs/vipolling/src/lib.rs`) are added, so **65 source files** are reviewed.
- **Cross-check 1, transcripts.** Every one of the 65 files was edited by a Phase 05 implementer (`S5Backend`, `S5VtScreen` and its two children, `KernelFrontend`, `InputFrontend`, `FrontendConsumers`, `TuiProof`, `ViRename`, `S54Impl` and `S54Impl.ThemeGui`, `GridFix`, `RemoveTuiProof`) or by the parent's direct edits. Exceptions, all explained: `Cargo.lock` (written by Cargo), the two renamed `lib.rs` files (`git mv`), and the driver `Cargo.toml` (one `sed -i` by `FrontendConsumers`). No Phase 05 implementer edited a file that is not in this set or in the deleted list (§4). Two scratch test modules (`zz_tmp_dump.rs`, `zz_s54_tmp.rs`) were created and removed within the same runs; neither exists now.
- **Cross-check 2, repository scan.** A repository-wide mtime scan (excluding `target/`, `.git/`, `.agents/`) found no other file modified in the window. Every file that carries a Phase 04 CP2 hash and lies outside the window still equals that hash (0 mismatches). Under `.agents/`, only the seven Phase 05 plan documents changed.
- **Limits.** Transcript attribution covers `edit`/`write`, Python `eval` writes and shell commands that use `rm`/`mv`/`cp`/`sed -i`/heredoc redirects. Most files are untracked or already carry Phase 02–04 changes, and no pre-Phase-05 copy exists, so per-slice diffs **cannot be reconstructed**. The regions below are pointers to current line numbers, not exhaustive diffs. **The whole-file hashes are the reviewed bytes.** A shared file's whole-file hash becomes a baseline only when every slice that touches it is approved (§15).

## 3. Master inventory (current SHA-256)

Slices: S5-1, S5-2, C (option C), S5-3, S5-4 (a–c, grid fix, e), R (renames).

| Path | Git | Bytes | SHA-256 (current) | Last hash in an approved CP2 dossier → now | Slices |
|---|---|---:|---|---|---|
| `CHANGELOG.md` | `M` | 298128 | `2203b5e8b69a8e95dcbe3065a748f919fb2ceac3e0a62aea1a890daa650db74c` | [C5 CP2](phase-04-c5-cp2-dossier.md) `ee7bde17…` → changed | S5-2, C, S5-3, S5-4, R |
| `Cargo.lock` | `M` | 82999 | `63bf3d8c41b6fc46eb578cc537b4dab4bb141da88a5552b1e33e503277d83a56` | [S2-T/W CP2](phase-04-s2tw-cp2-dossier.md) `e58e780a…` → changed | S5-1, S5-4, R |
| `Cargo.toml` | `M` | 8814 | `9d5122d530298500c4bd057d912fc689c85e540e54b9baa10e5532411e35a4e1` | [S2-T/W CP2](phase-04-s2tw-cp2-dossier.md) `ec5800c0…` → changed | S5-3, S5-4, R |
| `cells/apps/haily-agent/Cargo.toml` | `M` | 870 | `a0a2df97c6e9bebab7d412bb997be8dfd38f000506198af08319baa449e47fbb` | none | R |
| `cells/apps/haily-agent/src/main.rs` | `M` | 9125 | `74abc8d6032587e9b00aa7bc92296a43fddac87c4f48c5dc8409faa8bbb26a41` | none | R |
| `cells/apps/terminal/Cargo.toml` | `M` | 920 | `0f2c5d8632d4588d4018a48ec5fa10e49ca50ab54a9c8ec54f57e25947fbda33` | none | S5-2, S5-4, R |
| `cells/apps/terminal/src/grid.rs` | `M` | 29075 | `a794dfb3989fde6ff6233bc2ed1899c926809a2fdf2cfdfdc5db18ccd5e1634c` | none | S5-2 |
| `cells/apps/terminal/src/main.rs` | `M` | 17434 | `fc30687de060f52f3140f95eacb52dad9fd848f085369d33a689720b8c3adabc` | none | S5-2, C, S5-4 |
| `cells/apps/terminal/src/roundtrip.rs` | `??` | 10518 | `e6a74695c87fb07a959bee6d4fa26805bd392931bcf7b4e6f359d750859692bf` | none | S5-2, S5-4, R |
| `cells/apps/terminal/src/session.rs` | `M` | 19036 | `aaa5b0b6c7bf8792df9c2c3fabbba51bed3614fe0f31aba5f073a1a051de0fc8` | [S3-gui CP2](phase-04-s3-gui-cp2-dossier.md) `bb9ee497…` → changed | S5-2, C, S5-4 |
| `cells/apps/terminal/src/sink.rs` | `??` | 37538 | `77dda6e0dc549362db3a11ef0b4d63ec07fecc775e6873e1ebda17489ed62e6d` | none | S5-2 |
| `cells/demos/async-echo/Cargo.toml` | `M` | 494 | `fdc7725fb2c1ed8d14603ac55f1f90cef554429467eb2b6bed8a0240b3dee3a8` | none | R |
| `cells/demos/async-echo/src/main.rs` | `M` | 4161 | `326dc54d95e5838d7a36e24f792d67a86325b6fe8765f83547bb96bf2f68a403` | none | R |
| `cells/demos/tokio-smoke/Cargo.toml` | `M` | 488 | `9750e208b5842e92e055c30fca1f11921a5cbd6b8ee93475f2e4c544fd2e6326` | none | R |
| `cells/demos/tokio-smoke/src/main.rs` | `M` | 3937 | `ebc46094f3e461a25867371a6aabf343b9927491218b9a5f02ac6471215fa65d` | none | R |
| `cells/services/httpd/src/handlers.rs` | `M` | 10774 | `5cce6f2b25b350ab0d9ff6169dbe84e3ecba06aa407aaf325e66520bef88aee0` | none | R |
| `cells/services/input/src/session.rs` | `??` | 104196 | `906412286be2008bdab314bcaccd6a9ef2b5cdcb0c251d2ee29ca616ecd881cf` | [Phase 02 CP2](phase-02-cp2-dossier.md) `457ac5af…` → changed | C, S5-4 |
| `cells/services/input/src/session_tests.rs` | `??` | 32060 | `d462dbd1e62006d2a43b4cd8ebc71b2076566fc836931013334378b48f2ac73d` | none | C |
| `cells/tests/phase03-filter/src/main.rs` | `??` | 12299 | `4f622b4539561e402e6b7afe1e8bde2b4eec9fb2e6f17ea5f9ce645a42a2855a` | [S2-N CP2](phase-04-s2n-cp2-dossier.md) `8971d385…` → changed | S5-2, R |
| `cells/tests/phase04-terminal-driver/Cargo.toml` | `??` | 623 | `09488365e9404cba5fdb463ccc328f528f628b4b2093e102de383956c55eb404` | none | C |
| `cells/tests/phase04-terminal-driver/src/main.rs` | `??` | 76255 | `216668fef8be68c2af4ecda75e461e7a6d0fb5b4abf0d9e6fd9a98c7e98a58c1` | [C5 CP2](phase-04-c5-cp2-dossier.md) `00324c71…` → changed | S5-2, C, S5-3, S5-4, R |
| `cells/tools/cellbox/src/commands.rs` | `RM` | 25305 | `ae2b6ed75c50468f11764b7a52008f06f1c83bc3eeb12bba89b858f2589eb32c` | [S4a CP2](phase-04-s4a-cp2-dossier.md) `2e2fe670…` → changed | S5-4 |
| `cells/tools/cellbox/src/context_session.rs` | `??` | 8924 | `c4f713fb0638a2e40eee712b5513b01006decd03c7e54db81f542e38469233f6` | [S3-gui CP2](phase-04-s3-gui-cp2-dossier.md) `c2bc83b8…` → changed | C |
| `cells/tools/cellbox/src/executor.rs` | `RM` | 149060 | `daa6f064f0266418b64f5d443fdae80b4988ea4190a0bd29ed6c1c3361360f40` | [OOM CP2](phase-04-oom-cp2-dossier.md) `2ef99fa1…` → changed | S5-2, C, S5-3, S5-4 |
| `cells/tools/cellbox/src/main.rs` | `RM` | 7673 | `9a923bd1e08af140a26d0e79d07b6aa55ae08a88406973c148c2ecdc308acc0a` | [S4a CP2](phase-04-s4a-cp2-dossier.md) `0396292e…` → changed | S5-2, C |
| `cells/tools/cellbox/src/parser.rs` | `RM` | 32046 | `5d4d3eda3ebadf5a5c17e6bdc797cc1deb056c0604c9c7600c1d94be4b24d20c` | none | S5-4 |
| `cells/tools/cellbox/src/screen_reset.rs` | `??` | 12577 | `aedfc7ff3c5f3962b260a8b1d018912a65086330b9c6bd7fe04dc676a5d1a45b` | none | S5-2 |
| `cells/tools/cellbox/src/typed.rs` | `??` | 83084 | `b5cb932e20b014b2991466fac8828f2d8b985f6f08a0176d7afab59b5deffd99` | [OOM CP2](phase-04-oom-cp2-dossier.md) `6c8eb69b…` → changed | S5-3, S5-4 |
| `docs/cellbox-cli.md` | `M` | 21913 | `c7c7b43e1493079e3709fad812af76fbd5bb674311af16678ce31b38a6b2e22d` | none | R |
| `docs/code-standards.md` | `M` | 33644 | `9f87c3246074d5da7e7140e79c5108107189f6d0bade5fd664ec75ca2cde5424` | none | R |
| `docs/roadmap/haily-portability-review.md` | `M` | 8575 | `b7ac3a6a836aafe829fa5a86e3a2d7ada151cf5775d6e3155479257d028df9c5` | none | R |
| `kernel/src/loader/atomic_publication_tests/denials.rs` | `M` | 61388 | `9008724f3555009a76257bad47f9fb559d03644f599b22124752afaa015f7fab` | [Phase 02 CP2](phase-02-cp2-dossier.md) `01eb9f25…` → changed | C |
| `kernel/src/loader/launch_profile/mod.rs` | `M` | 4459 | `b8db3b800292ad8ebb72c849fdce4cfad1d731b15184bc47f500cb4ab20ec04c` | [S2-T/W CP2](phase-04-s2tw-cp2-dossier.md) `b8db3b80…` → **equal** | S5-3, S5-4 |
| `kernel/src/loader/launch_profile/profiles.rs` | `M` | 13851 | `3140c61b0765100bdf14eea8b8455e294c069abbffd71aa86885367377f0b0ca` | [S2-T/W CP2](phase-04-s2tw-cp2-dossier.md) `3140c61b…` → **equal** | S5-3, S5-4 |
| `kernel/src/loader/launch_profile/tests.rs` | `M` | 33844 | `a0f6aa7bbb7937f237301395de8398b31140ecfc16bb5d8b767ec51410ecee86` | [S2-T/W CP2](phase-04-s2tw-cp2-dossier.md) `a0f6aa7b…` → **equal** | S5-3, S5-4 |
| `kernel/src/task/cell_session.rs` | `??` | 127933 | `2676534fcbc72e2d05079a4e62b466e137ec206e1585d4145c18c715d865ad1d` | [Phase 02 CP2](phase-02-cp2-dossier.md) `57aeb7d4…` → changed | C |
| `kernel/src/task/child_context.rs` | `??` | 9063 | `66d97119eac381199cf7717807fe4c692a812e5c904bbddb243860c02c40e561` | [Phase 02 CP2](phase-02-cp2-dossier.md) `473cc6e6…` → changed | C |
| `libs/api/src/abi/cell_session.rs` | `??` | 20590 | `8924dc15855311943ff84aac8304eb2c003b8ec6eb0338626ff65b027b9ab6b5` | [Phase 02 CP2](phase-02-cp2-dossier.md) `7c5530ee…` → changed | C |
| `libs/api/src/abi/cell_session/frontend.rs` | `??` | 14852 | `06bd42a112e0b99f632e55a8fb99074e99971b99fc400982b16673e5df15e7df` | none | C |
| `libs/api/src/services/session.rs` | `??` | 32530 | `32e0a9af90160bc1a97a281e8659c97984a17b7aeb554c341fb27975256a3711` | [Phase 02 CP2](phase-02-cp2-dossier.md) `b8f902ea…` → changed | C |
| `libs/ostd/src/child_context.rs` | `??` | 13098 | `620d57f8a7145e3e456c8fd3b7e136a34e74b2d65fe3b2def1e4f0564354557a` | [Phase 02 CP2](phase-02-cp2-dossier.md) `db3529fd…` → changed | C |
| `libs/terminal/src/host.rs` | `??` | 33591 | `b6ef72703f0f00a37d7f961b52cf25fd3a9cf2c01db229015cc405ae34136b2a` | none | C |
| `libs/terminal/src/lib.rs` | `??` | 1281 | `bc01a7e5c3d35282069d17619fa203520a2d8c01bc99c1851d3aa28cc013a8d7` | [Phase 02 CP2](phase-02-cp2-dossier.md) `ba75fbda…` → changed | C |
| `libs/terminal/src/pipe_child.rs` | `??` | 30781 | `be90dd9795feb21a01069fb4b1c39419fc4a032d64fca0d584e722e3cc997b97` | none | C |
| `libs/terminal/src/session.rs` | `??` | 31308 | `b964ec1ac6de4563805cfc3dc14f5e0f55438f7dd970a0e6d44998871644ecf4` | [Phase 02 CP2](phase-02-cp2-dossier.md) `1208e5fc…` → changed | C |
| `libs/terminal/src/stream.rs` | `??` | 10921 | `fb70200fd3a7971c0c3f342a64a50c897d2eac9ebea48d8afdb30083cf37c6a2` | [Phase 02 CP2](phase-02-cp2-dossier.md) `a05fc305…` → changed | C |
| `libs/terminal/src/tests.rs` | `??` | 28368 | `a035fed6ba1e6c7d74117eff2c7f47f5dca1eec4600a20419e7180dbfd8a292c` | [Phase 02 CP2](phase-02-cp2-dossier.md) `213aa04f…` → changed | C |
| `libs/vimio/Cargo.toml` | `RM` | 313 | `40fe148314b1e92f6899192522586720b1fd0bd5e5e20f66a9cd69f6e0a483f9` | none | R |
| `libs/vimio/src/lib.rs` | `R` | 10950 | `6d76670bc97c9ab828ade77dbbc0eeee7ae7a36c40363855d9c2ffe03fba4729` | none | R |
| `libs/vipolling/Cargo.toml` | `RM` | 304 | `af3f4b4ae3ebcd3b04afac7f6a205423a7149b2937c8ad483d64f72475c50dcc` | none | R |
| `libs/vipolling/src/lib.rs` | `R` | 11367 | `add124c7c41a8478a0e7d396258eec6512df868377742956689dfe38657a4edc` | none | R |
| `libs/vitui/Cargo.toml` | `??` | 691 | `c8f0473d6a411d5623a68953115e88485ef4a1f73917521fbc252b4f0a377865` | none | S5-1, R |
| `libs/vitui/src/backend.rs` | `??` | 16291 | `89bbf18ef4f4761ab990ce75e67bafd87595f423d369342717cea364b0039648` | none | S5-1 |
| `libs/vitui/src/event.rs` | `??` | 4324 | `7fe74edf0c06e68542e40981ebd87754c5d69f2d870ffced0e6db7ec3feb0277` | none | S5-1 |
| `libs/vitui/src/lib.rs` | `??` | 994 | `9bbc2b2b3c0852b705a027e27afdf3f75fb82a00816d7323904b8f528e3e7e37` | none | S5-1, S5-4, R |
| `libs/vitui/src/session.rs` | `??` | 4940 | `985eaa12a3f72e890bb734f2295e44f27fa53b6619ddca99bdb86070bf4328d4` | none | S5-1 |
| `libs/vitui/src/tests.rs` | `??` | 13623 | `b9f583a1cac2112c3e2541bf3f939ab6755ca74646b2a481752ecda0aea167f2` | none | S5-1, R |
| `libs/vitui/src/theme.rs` | `??` | 10407 | `0ab1d1b9f99b82793e9bbe73168f3154557fd22da88e77257a03ad3eb761a7c1` | none | S5-4 |
| `libs/vitui/src/theme/tests.rs` | `??` | 8116 | `cd224c4ec70e3a4b9f3ab31c02a1a6641ffd281e66b611b0d2c2519fa7d41e88` | none | S5-4 |
| `scripts/phase04-gui-glyphs/src/main.rs` | `??` | 16228 | `1ddc8ea2c340ef827fa8a98a18fa3370ee5ba152b826bdb37f08168e24e891ce` | [C5 CP2](phase-04-c5-cp2-dossier.md) `a355dbaf…` → changed | S5-2, S5-4 |
| `scripts/phase04_gui_drive.py` | `??` | 64559 | `6065f034258c7aefdd8ee0c4e2233c93e3a5b0558ec3a4ab09eeac5a38336d14` | [C5 CP2](phase-04-c5-cp2-dossier.md) `ad3bf449…` → changed | S5-2, C, S5-4, R |
| `scripts/phase05_vt_hostile.py` | `??` | 4131 | `709bd4e3863764f9002d536c2bd29cd368ae117fde5f90bd03c805216b5c9ae2` | none | S5-4 |
| `scripts/qemu-phase02-session-test.sh` | `??` | 22513 | `42b363e1e17791dba2d8aec262102f39d9bc50e533f8281bf73fdae7b6372b9e` | [Phase 02 CP2](phase-02-cp2-dossier.md) `d0861d10…` → changed | C, S5-4 |
| `scripts/qemu-phase04-gui-test.sh` | `??` | 31112 | `1ad33576e6c11086df8d5a968c156c6414af15bdced355bccb1e3d2478d3c0f1` | [C5 CP2](phase-04-c5-cp2-dossier.md) `286a81be…` → changed | S5-2, C, S5-4, R |
| `scripts/qemu-phase04-terminal-test.sh` | `??` | 51511 | `62d4b402710c60f67a2d6c2552bc8edada0e422d691027520b8a002fc70cf643` | [C5 CP2](phase-04-c5-cp2-dossier.md) `dfaf1baa…` → changed | S5-2, C, S5-3, S5-4, R |

The three `launch_profile` files were changed by S5-3 (private `/bin/tui-proof` row) and restored by S5-4e. Their bytes now **equal** the approved S2-T/W (and S2-N) CP2 hashes, so Phase 05 leaves no net change in them.

Plan documents (context, not source; not under this CP2):

| Plan document | Bytes | SHA-256 | Role |
|---|---:|---|---|
| [phase-05-entry-dossier.md](phase-05-entry-dossier.md) | 8669 | `22636420b0728f6bf8b7632fe64d1a10d28ca8a4e3207ad53db18af3bd2bf502` | Phase entry; S5-1 CP1 (approved “chốt”) and S5-1 results §6 |
| [phase-05-s2-vt-screen-proposal.md](phase-05-s2-vt-screen-proposal.md) | 8429 | `f475940df5f02cf4b5e37287fc10a94f79439ee75644ce140c1a078c049c360b` | S5-2 CP1 (approved “chốt quyết định”) and Results |
| [phase-05-s3c-frontend-annex.md](phase-05-s3c-frontend-annex.md) | 18481 | `8f1b928d30a099cac5d9c5711c3ef0aa4e54fcbaf97e3002d93b770e4ba82cde` | Option C annex (CP1 approved “duyệt CP1” for `41e7f0b1…dea6`, digest before its status amendment) and §11 Results/ratifications |
| [phase-05-s3-proposal.md](phase-05-s3-proposal.md) | 8299 | `826bfa658278255bfeb68b9bd5b585e438863c5cc4683433e84bf6754e6774a0` | S5-3 proposal (status line still PROPOSED) and Results |
| [phase-05-s4-proposal.md](phase-05-s4-proposal.md) | 14073 | `7d06a9c4d337dfa85dbac0dc317e439d43ae23abc18d69aaae8b818e7b9b7a46` | S5-4 decisions, implementation record §7-§9 |
| [spec.md](spec.md) | 25835 | `7e6e11c7c383e7f0f46e9c7dae5e4b034cec68d265a01f00d8dc811e60f3b1a8` | §4 LF amendment 2026-10-23 (change log line 116) |
| [phase-05-ratatui-terminal.md](phase-05-ratatui-terminal.md) | 8702 | `7349da8254e5e285f376d2f77c3023a8b81f360eb51190a4a5125fa81f69a8c8` | Phase file; includes the pointer line to this dossier |

## 4. Deleted and moved paths

| Path | Fate | Evidence of the path |
|---|---|---|
| `cells/tests/tui-proof/{Cargo.toml, build.rs, src/main.rs, src/model.rs, src/frame_screen.rs, src/theme_screen.rs, src/tests.rs}` | Created by S5-3 (`frame_screen.rs` by the grid fix, `theme_screen.rs` by S5-4c); **deleted by S5-4e**, with its workspace member and lock entry. Untracked, so no hash exists | Cargo dep-info `target/*/deps/tui_proof-*.d`; build-script dirs `target/*/build/app-tui-proof-*`; transcripts `TuiProof`, `RemoveTuiProof` |
| `cells/tests/phase04-terminal-driver/src/tui.rs` | Created by S5-3; **deleted by S5-4e**. `oneshot-no-tui-bytes` and `top-pipe-batch` moved into the driver's `main.rs` (`selective_tui`) | `target/x86_64-unknown-none/release/deps/phase04_terminal_driver-*.d`; `RemoveTuiProof` |
| `libs/tui-cellos/**` → `libs/vitui/**` | Moved by R (`mv`; untracked, no pre-move hash). S5-1 had already replaced its pre-Phase-05 home-made buffer (`backend.rs` 160 lines, `lib.rs` 60, [entry §1](phase-05-entry-dossier.md)); no hash of those bytes was recorded | `target/*/deps/tui_cellos-*.d`; `S5Backend`, `ViRename` |
| `libs/mio-cellos/{Cargo.toml, src/lib.rs}` → `libs/vimio/…` | `git mv` by R. HEAD bytes: `Cargo.toml` 318 B `2f234852fdb954941afab6114beed01ad110673bcaa77dba96a1f9b10e837a2a`; `src/lib.rs` identical to the current file | `git status` (R/RM) |
| `libs/polling-cellos/{Cargo.toml, src/lib.rs}` → `libs/vipolling/…` | `git mv` by R. HEAD `Cargo.toml` 309 B `449e6c0e027fa027c0dc11dd5c7a5caeaed6ac6d7f8e5fca6f955a8006937691`; `src/lib.rs` identical | `git status` (R/RM) |

Removed inside surviving files: the Phase 04 `TerminalHost` (replaced by `FrontendHost` in `libs/terminal/src/host.rs`); cellbox's non-controller context mode (`start_context_session`, the raw-input lease, `spawn_context_external`); the private `cellbox -> /bin/tui-proof` launch row and its test; the GUI driver's `TUI-*`/`THEME-*`/`FRAME-*` scenarios; and the terminal runner's two proof UART checks. A search for `tui-proof|tui_proof|tui-cellos|tui_cellos|mio-cellos|mio_cellos|polling-cellos|polling_cellos|TerminalHost` under `cells libs kernel scripts docs config tests Cargo.toml deny.toml` finds only the retirement note at `docs/code-standards.md:321`.

## 5. Final evidence (shared by all slices)

All final runs are on the post-S5-4e tree. Logs are in `reports/phase-05-final-evidence/`:

| Log | Bytes | SHA-256 | Total (0 `FAIL` lines in every log) |
|---|---:|---|---|
| `reports/phase-05-final-evidence/gui-autoload.log` | 40753 | `cf1fa4c67839bbadead5de20cafc5872d417a63b5aad2b4d5a61fd11377e4044` | 259/259 GUI |
| `reports/phase-05-final-evidence/gui-click1.log` | 40715 | `48142d60dafecadd1e90422d00fd3b43da3ed0edcbd25fb34c3b60e4f8161724` | 259/259 GUI |
| `reports/phase-05-final-evidence/gui-click2.log` | 40715 | `6c1b3df5fd9ca677546f8aa197095e689d8abca4b3f7431e7be39127ac5e932e` | 259/259 GUI |
| `reports/phase-05-final-evidence/gui-click3.log` | 40716 | `eaaa289ae21670f75bc3bdfb595943fdee0aee12b660f64211c3c6fd30209324` | 259/259 GUI |
| `reports/phase-05-final-evidence/gui-click4.log` | 40715 | `5f8be96c250974972b293dab5be6105d15e8ddc8443e776824b2c3c81d97ce54` | 259/259 GUI |
| `reports/phase-05-final-evidence/gui-click5.log` | 40715 | `3464e322f02b4a82637ca2192cbc80dace7f16ab3bf8b64a1905fd7a1f523629` | 259/259 GUI |
| `reports/phase-05-final-evidence/phase02-kernel-serial.log` | 22153 | `62861c355b99ad48294e1c3e8e8720065732edc8b3b878a6104d373bf1bc8287` | ATOMIC_PUBLICATION 38 PASS, 0 FAIL |
| `reports/phase-05-final-evidence/phase02.log` | 20238 | `2890a4bec16c60c07f315e10383c1e67b3c1d71526d2d98cc7a0af7ffcdd29c7` | passes: 48 |
| `reports/phase-05-final-evidence/phase03.log` | 21924 | `2b5b5321df8a1bd4efab2a04af6207d17da53a698fcae6efb45f630d6cd85895` | 60 PASS |
| `reports/phase-05-final-evidence/phase04-terminal.log` | 38121 | `77bd7fcb1732cd2486186ce6d390ba04a49003b29cf5cc6351d74a91ee5405e3` | 397/397, 115 markers |

**Binding the evidence to the source.** The logs record binary hashes, not source hashes. That they match the bytes in §3 is `[INFERENCE]` from four facts:
1. Every runner log records `cargo_lock_sha256` `63bf3d8c…3a56`, which equals the current `Cargo.lock` (terminal log 488, GUI click1 533, phase02 381, phase03 368).
2. The terminal runner fails unless five authority files hash to its pins (`qemu-phase04-terminal-test.sh` 243-256), and all five pins equal the current files (`launch_profile/{mod,profiles,targets,tests}.rs`, `libs/api/src/abi/cell_session.rs`).
3. The logs were archived at 12:44, after the last source write at 12:00 (`CHANGELOG.md`, `qemu-phase04-gui-test.sh`).
4. No source file changed after that.

**Other facts from the logs.**
- **Canonical artifacts are unchanged.** GUI click1 820-825 has `PASS: 259 GUI checks` and `PASS: canonical cellbox/terminal/kernel_fs.img hashes unchanged`. The values are cellbox `64eb28f3…`, terminal `636497ac…` and `kernel_fs.img` `447d0348…`, the same as recorded for C5.
- **Ceilings.** Terminal log 6 has `G-S1a PASS` for cellbox. Lines 8-12 show the Terminal's informational `SetTimer` delta, which was approved earlier.
- **Exit codes are not in the logs.** No host-test log is archived, so the host counts below rest on the Results text and the implementer reports only.

The GUI total is 259 = the C5 baseline's 173 plus 86 Phase 05 lines: `VT-*` 22, `S53C-*` 27, `HOSTILE-*` 27, seven `no-command-uart:vt-*/hostile-*` checks and three `uart-s53c-*` checks.

## 6. Slice S5-1: Ratatui backend (`vitui`, formerly `tui-cellos`)

**Scope.** The workspace admits the pinned `ratatui-core =0.1.0` and `ratatui-widgets =0.3.0` with defaults off (no std, crossterm, termion, serde, macros, layout-cache or calendar). `CellosBackend` implements `Backend` as CUP/ED/EL/SGR bytes over the SDK stdout endpoint. `event` adapts the SDK's typed input and control frames. `TuiSession` scopes the raw lease and the screen mode. The home-made buffer is deleted (clean cutover).

**Files.**

| Path | Bytes | SHA-256 | Phase 05 regions (current lines) and sharing |
|---|---:|---|---|
| `libs/vitui/src/backend.rs` | 16291 | `89bbf18ef4f4761ab990ce75e67bafd87595f423d369342717cea364b0039648` | Whole file (S5-1 only): `ByteSink` (512-byte chunks over the stdout `Endpoint`), `CellosBackend` implementing `ratatui_core::backend::Backend` (CUP/ED/EL/SGR, diff-tracked cursor/style, no queries; `append_lines` Unsupported). Moved unchanged from `libs/tui-cellos` by R |
| `libs/vitui/src/event.rs` | 4324 | `7fe74edf0c06e68542e40981ebd87754c5d69f2d870ffced0e6db7ec3feb0277` | Whole file (S5-1 only): `Event`/`EventAdapter`/`Key`; paste coalesced and redacted in `Debug` |
| `libs/vitui/src/session.rs` | 4940 | `985eaa12a3f72e890bb734f2295e44f27fa53b6619ddca99bdb86070bf4328d4` | Whole file (S5-1 only): `TuiSession` (raw lease + TUI screen mode, restore on exit/drop) |
| `libs/vitui/src/tests.rs` | 13623 | `b9f583a1cac2112c3e2541bf3f939ab6755ca74646b2a481752ecda0aea167f2` | S5-1 host tests (exact bytes: first draw, diff redraw, style, cursor, resize, erase, wide char, sanitising, chunking, write-error stop, mode restore, event mapping); module import paths changed by R |
| `libs/vitui/src/lib.rs` | 994 | `9bbc2b2b3c0852b705a027e27afdf3f75fb82a00816d7323904b8f528e3e7e37` | Crate root and docs (S5-1); module-only exports and `pub mod ratatui { core, widgets }` (R); `pub mod theme` and doc line (S5-4c) |
| `libs/vitui/Cargo.toml` | 691 | `c8f0473d6a411d5623a68953115e88485ef4a1f73917521fbc252b4f0a377865` | Pins `ratatui-core =0.1.0`, `ratatui-widgets =0.3.0`, defaults off; `types`/`api` removed (S5-1); `name = "vitui"` (R) |
| `Cargo.lock` | 82999 | `63bf3d8c41b6fc46eb578cc537b4dab4bb141da88a5552b1e33e503277d83a56` | Ratatui set added by Cargo for S5-1 (28 lock additions per [entry §6](phase-05-entry-dossier.md); list in entry §5; `ratatui-core 0.1.0`, `ratatui-widgets 0.3.0`, `unicode-width 0.2.2`); renamed packages `vitui`/`vimio`/`vipolling` (R); `app-tui-proof` added (S5-3) and pruned (S5-4e). Also Phase 02–04 entries (443 packages now, 407 at HEAD) |

**Deviations.**
- **Mode guard.** The input service admits only Raw/Echo/BracketedPaste leases (`MODE_MASK = 1|2|16`). The alternate screen and the hidden cursor are therefore emitted as bytes through the SDK `ModeGuard`/`TerminalMode::transition_to` and restored on exit or drop. A killed TUI leaves them set until S5-2's soft reset ([entry §6](phase-05-entry-dossier.md)). **Ratified by the user 2026-10-23 (with this CP2).**
- The unused `types`/`api` dependencies were dropped, and `append_lines` returns `Unsupported` (recorded facts).

**Evidence.**
- Host `vitui`: 15/15 at S5-1 ([entry §6](phase-05-entry-dossier.md), re-run by the parent), 15/15 after R, and 22/22 after S5-4c and after S5-4e (implementer reports). Bare-metal check/build for `x86_64-unknown-none` passes.
- `cargo deny` licenses/advisories pass for the crate. The workspace-wide check fails only on the pre-existing `cells/services/ocel-pdf` AGPL licence.
- **Runtime:** only through the removed `tui-proof` consumer, in archived logs: `s5-3-evidence` terminal `tui-frame/keys/resize/reread/exit-restores/ctrl-c-130/abandon-recovered/two-sessions` and GUI 6×252; `s5-4-evidence` GUI `THEME-*`; `s5-4-gridfix-evidence` GUI `FRAME-*`.
- **Current tree:** no runtime consumer. The S5-2 round-trip host test (`roundtrip.rs`) exercises the backend bytes against the emulator.

**Known limits and open defects.**
- No live consumer until Phase 06 interactive `top`. Frame/key/resize regressions are not re-runnable.
- The newer `ratatui-core 0.1.2` / `ratatui-widgets 0.3.2` were not reviewed.
- Heap use is not measured.
- Session capabilities advertise no colour mode.

**Residual risks.** If a `TuiSession` holder dies on a path the S5-2 tracker does not see, the alternate screen and hidden cursor persist (see S5-2 limits). Pin updates need a fresh source/feature review.

**What CP2 would authorize.** `backend.rs`, `event.rs`, `session.rs`, the S5-1 tests and the S5-1 lines of `lib.rs`/`Cargo.toml`, plus the Ratatui lock entries, as the backend baseline.

[x] CP2 approved by user — date: 2026-10-23 (mode-guard deviation ratified)

## 7. Slice S5-2: `cellos-vt1` screen in the GUI Terminal and cellbox soft reset

**Scope.**
- The GUI Terminal's sink interprets the full spec §4 output profile within its bounds.
- `grid.rs` is an attributed, dual-buffer screen model with `unicode-width =0.2.2` widths.
- The renderer draws colours, bold, inverse and the hidden cursor.
- Resizes outside the bounds are refused with state preserved, and modes reset on session start, exit and failure.
- cellbox tracks `?1049/?25/?2004` in relayed child output and writes the 26-byte soft reset `ESC[?1049l ESC[?25h ESC[?2004l ESC[0m` only if a mode was left on.
- Fixtures: `p03-filter tui-abandon`; GUI `vt-sample.bin` and `vt-alt-{enter,exit}.bin`.

**Files.**

| Path | Bytes | SHA-256 | Phase 05 regions (current lines) and sharing |
|---|---:|---|---|
| `cells/apps/terminal/src/sink.rs` | 37538 | `77dda6e0dc549362db3a11ef0b4d63ec07fecc775e6873e1ebda17489ed62e6d` | Whole file (S5-2 only): bounded `cellos-vt1` parser (ESC 7/8; CSI A–H/f, J/K, SGR, s/u, r, S/T; ?7/?25/?1049/?2004; ≤16 params, ≤256 B per control, OSC/DCS/SOS/PM/APC discarded to 4 KiB; no reply path) and its host tests. File predates Phase 05 (S3 decoder), no recorded hash |
| `cells/apps/terminal/src/grid.rs` | 29075 | `a794dfb3989fde6ff6233bc2ed1899c926809a2fdf2cfdfdc5db18ccd5e1634c` | Whole file (S5-2 only): attributed dual-buffer screen model, margins, tab stops, wide/combining cells, scrollback bounds, `reset_modes` 615-626, tests (`reset_modes_returns_to_the_profile_defaults` 814) |
| `cells/apps/terminal/src/roundtrip.rs` | 10518 | `e6a74695c87fb07a959bee6d4fa26805bd392931bcf7b4e6f359d750859692bf` | New in S5-2: backend bytes → emulator vs Ratatui `Buffer` contract test; imports via `vitui::ratatui` 14-19 (R, S5-4c) |
| `cells/apps/terminal/Cargo.toml` | 920 | `0f2c5d8632d4588d4018a48ec5fa10e49ca50ab54a9c8ec54f57e25947fbda33` | `unicode-width =0.2.2` and round-trip dev-dependency (S5-2); dev-dependency now `vitui` with no direct Ratatui crate 21-23 (R, S5-4c). `terminal-sdk` dependency is Phase 04 S3 |
| `cells/apps/terminal/src/main.rs` | 17434 | `fc30687de060f52f3140f95eacb52dad9fd848f085369d33a689720b8c3adabc` | Renderer: `render` 86-167 (bg, glyph, bold, inverse, palette, hidden cursor), `ANSI`/`resolve`/`cell_colors` 186-238 (S5-2). Shared: option C (header 2-3, `SetTimer` comment 47, re-create/re-attach 330-337; one source-text test removed by Main); grid fix (`PAD_X/PAD_Y` use, `grid_dims` 175-183, callers 68/82/333, tests 388-422) |
| `cells/apps/terminal/src/session.rs` | 19036 | `aaa5b0b6c7bf8792df9c2c3fabbba51bed3614fe0f31aba5f073a1a051de0fc8` | Mode reset on session start/exit/fail (`stop_host` 228-246 incl. `reset_modes` 245) and refused out-of-bound resize keeping state (`resize` 166-183) (S5-2). Shared: option C (whole `FrontendHost` session 1-122, `Streams for FrontendHost` 269+); grid fix (comment 81, removed local dims helper) |
| `cells/tools/cellbox/src/screen_reset.rs` | 12577 | `aedfc7ff3c5f3962b260a8b1d018912a65086330b9c6bd7fe04dc676a5d1a45b` | Whole file (S5-2 only): `RelayScreen` fixed-state tracker of ?1049/?25/?2004 over relayed child output, 26-byte `SOFT_RESET`, host tests |
| `cells/tools/cellbox/src/executor.rs` | 149060 | `daa6f064f0266418b64f5d443fdae80b4988ea4190a0bd29ed6c1c3361360f40` | Relay hooks: doc 19-21, import 30, `relay_serial` 176-199, soft-reset write 299-308, foreground drain 2923-2998, pump tracker 3082-3133 and 3384 (S5-2). Shared with C, S5-3, S5-4a and Phase 02–04 (see §3) |
| `cells/tools/cellbox/src/main.rs` | 7673 | `9a923bd1e08af140a26d0e79d07b6aa55ae08a88406973c148c2ecdc308acc0a` | `mod screen_reset;` 78 (S5-2). Shared with option C 122-179 |
| `cells/tests/phase03-filter/src/main.rs` | 12299 | `4f622b4539561e402e6b7afe1e8bde2b4eec9fb2e6f17ea5f9ce645a42a2855a` | `tui-abandon` mode: doc 4, 28-29; `TUI_ABANDON` 97-99; `tui_abandon` 179-186; dispatch 338 (S5-2); wording `vitui` 28, 97 (R). Rest: Phase 03/S2-N |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 76255 | `216668fef8be68c2af4ecda75e461e7a6d0fb5b4abf0d9e6fd9a98c7e98a58c1` | `TUI_ABANDON`/`SOFT_RESET`/`occurrences`/`no_reset`/`soft_reset` 767-829, call 329 (S5-2). Shared with C, S5-3, S5-4, R and Phase 04 |
| `scripts/qemu-phase04-terminal-test.sh` | 51511 | `62d4b402710c60f67a2d6c2552bc8edada0e422d691027520b8a002fc70cf643` | Soft-reset header 60-65 and expected markers 609 (S5-2). Shared with every Phase 05 slice and Phase 04 |
| `scripts/qemu-phase04-gui-test.sh` | 31112 | `1ad33576e6c11086df8d5a968c156c6414af15bdced355bccb1e3d2478d3c0f1` | Header 48-60; `vt-sample.bin`/`vt-alt-{enter,exit}.bin` fixtures 310-337, packaging 346-358, 419 (S5-2). Shared |
| `scripts/phase04_gui_drive.py` | 64559 | `6065f034258c7aefdd8ee0c4e2233c93e3a5b0558ec3a4ab09eeac5a38336d14` | Styled-screen oracle doc 21-23; VT constants/expected screens 97-130; `vt_*` methods 840-900+, scenario call 691 (S5-2). Shared |
| `scripts/phase04-gui-glyphs/src/main.rs` | 16228 | `1ddc8ea2c340ef827fa8a98a18fa3370ee5ba152b826bdb37f08168e24e891ce` | `styled` mode (doc 13-, SGR palette 148+) (S5-2). Shared with grid fix (`grid_dims` 57-64) and C5/S3-gui |

Plan document: [spec.md](spec.md) §4 LF sentence and change log line 116.

**Deviations** ([proposal Results](phase-05-s2-vt-screen-proposal.md)):
1. **D52-2 premise false → option A, approved by the user.** No ABI exposes a child's raw lease, so a fixed-state tracker is used instead.
2. **LF also returns to column 0.** Ratified by the spec §4 amendment (user decision 2026-10-23).
3. One combining mark per cell. **Ratified by the user 2026-10-23 (with this CP2).**
4. An over-long CSI is abandoned and its excess shown as text; OSC/DCS is discarded for 4 KiB, then the rest is text. **Ratified by the user 2026-10-23 (with this CP2).**
5. A cursor move onto a wide character's trailing half snaps to its lead cell. **Ratified by the user 2026-10-23 (with this CP2).**

The Terminal also resets its own modes when the shell exits ([CHANGELOG](../../CHANGELOG.md) line 13).

**Evidence.**
- **Slice-time** (`reports/s5-2-evidence/`): terminal 368/368 (93 markers), GUI 6×197, Phase03 60, Phase02 48. Host at the time: terminal 62, app-cellbox 97.
- **Final:**
  - terminal `soft-reset-after-tui-abandon` / `-tui-pipeline` 558-559, `host-exact:tui-*` 662-665, `host-soft-reset-once:*` 682-683, `no-tui-bytes-uart` 890;
  - GUI click1 `VT-*` 22 lines 711-732 and `no-command-uart:vt-*`;
  - host terminal 64/64 and app-cellbox 97/97 (S5-4e implementer report).

**Known limits and open defects.**
- **No soft reset** on these paths:
  - legacy `spawn_external`;
  - `hv`;
  - redirected or captured output;
  - bytes still unread after the final read rounds.
- 8-bit C1 is shown as U+FFFD (D54-5).
- Device queries (DA/DSR/DECRQM) are never answered. This is by design: no reply path.
- Per-cell heap use is bounded by spec but not measured.
- A window below the 40×12 session minimum loses its trailing cells.

**Residual risks.** The emulator parses untrusted bytes. It is bounded and host-tested on fragmented and hostile input, but no fuzzing was run. The soft-reset bytes are also visible on a serial terminal (control bytes only, gated on a mode left on).

**What CP2 would authorize.** `sink.rs`, `grid.rs`, `screen_reset.rs`, `roundtrip.rs`, and the S5-2 regions of the Terminal, cellbox, `phase03-filter`, driver and runner files listed above.

[x] CP2 approved by user — date: 2026-10-23 (deviations 3–5 ratified)

## 8. Slice C: S5-3 option C, input-issued frontend capability and shell-owned session

**Scope.**
- **ABI (append-only).** SessionV1 selectors 23–28 (`RegisterFrontend`, `BindFrontendChild`, `CreateForFrontend`, `QueryFrontend`, `ReleaseFrontend`, `ReattachFrontend`); frontend kind 1 Gui admitted (2 Ssh stays `Unsupported`); Lost reasons 4 `FrontendLost` and 5 `FrontendAttached`; context grant kind 6 `InputFrontend` in slot 8 (9-entry context, third receipt slot).
- **Kernel.** Kind-6 receipt with a 48-byte seal; commit refuses kind 6 without an exact bound receipt.
- **Input service.** Frontend table, Detached sessions, holder-bound `resume`, holder liveness.
- **Consumers.** SDK `FrontendHost` replaces `TerminalHost`. cellbox given a kind-6 grant runs the serial-controller path, and its non-controller context mode is deleted. The GUI Terminal and the Phase04 driver use `FrontendHost`.

**Files.**

| Path | Bytes | SHA-256 | Phase 05 regions (current lines) and sharing |
|---|---:|---|---|
| `libs/api/src/abi/cell_session.rs` | 20590 | `8924dc15855311943ff84aac8304eb2c003b8ec6eb0338626ff65b027b9ab6b5` | `pub mod frontend` 7; `GRANT_KIND_INPUT_FRONTEND = 6`, `SLOT_FRONTEND = 8`, `FRONTEND_SEAL_LEN` 23-28; selectors 23–28 99-101; 9-grant context bounds; pin test 304. Pinned by the terminal runner's authority list at this hash |
| `libs/api/src/abi/cell_session/frontend.rs` | 14852 | `06bd42a112e0b99f632e55a8fb99074e99971b99fc400982b16673e5df15e7df` | Whole file (C only): selector constants, kinds (0 Serial, 1 Gui, 2 Ssh reserved), `RegisterFrontendV1`…`ReattachFrontend` records, `LOST_FRONTEND = 4`, `FRONTEND_ATTACHED = 5` |
| `libs/api/src/services/session.rs` | 32530 | `32e0a9af90160bc1a97a281e8659c97984a17b7aeb554c341fb27975256a3711` | `validate_event` admits Lost reasons 4/5 130-134 (deviation 6); test 491 |
| `kernel/src/task/child_context.rs` | 9063 | `66d97119eac381199cf7717807fe4c692a812e5c904bbddb243860c02c40e561` | 9-grant context; kind 6 / slot 8 validation 3, 21, 80; tests 124-180 |
| `kernel/src/task/cell_session.rs` | 127933 | `2676534fcbc72e2d05079a4e62b466e137ec206e1585d4145c18c715d865ad1d` | Third receipt slot 82-83; `receipt_service`/`check_frontend_seal` 107-118; `QueryOperation` opened to the INPUT issuer 798-806 (deviation 5); `BindOwnerReceipt` index 820; `InspectPreparedGrant` kind 6 / slot 8 1063-1072 and length rule 1098 (deviation 5); kind-6 `RegisterBindReceipt` 1158-1165; commit gate 1232-1247; retained receipt-less terminal records 557-574, 889, 1491-1493 (deviation 7); test hooks 2076-2143; tests 2149-2187 |
| `kernel/src/loader/atomic_publication_tests/denials.rs` | 61388 | `9008724f3555009a76257bad47f9fb559d03644f599b22124752afaa015f7fab` | AP-PG `session_frontend_receipt_contract` registration 130 and body 612-660+ |
| `libs/ostd/src/child_context.rs` | 13098 | `620d57f8a7145e3e456c8fd3b7e136a34e74b2d65fe3b2def1e4f0564354557a` | `GrantSlot::Frontend = 8` 8; `Grant::frontend` 18-21; kind-6 validation 32; parse 41; `frontend_grant` 83-84; test 113-120 |
| `cells/services/input/src/session.rs` | 104196 | `906412286be2008bdab314bcaccd6a9ef2b5cdcb0c251d2ee29ca616ecd881cf` | Frontend table, selectors 23–28 (`frontend`/`frontend_lane`/`frontend_index` 178-200, `frontend_operation` 623+), kind-1 sessions only via sel 25 (427), Detached/`resume`, holder liveness in `maintain`, `InspectPreparedGrant` reply buffer 704 B (deviation 10), seal 669-677; `#[path = "session_tests.rs"] mod frontend_tests` 18-21. Shared with S5-4a (`retained` 106-138, 307-312, 366-369, test 1230-1310) and Phase 02 |
| `cells/services/input/src/session_tests.rs` | 32060 | `d462dbd1e62006d2a43b4cd8ebc71b2076566fc836931013334378b48f2ac73d` | Whole file (C only): host fake of the kernel/IPC and tests for selectors 23–28 incl. re-attach, wrong holder/child, limits, Lost(4) |
| `libs/terminal/src/host.rs` | 33591 | `b6ef72703f0f00a37d7f961b52cf25fd3a9cf2c01db229015cc405ae34136b2a` | Whole file rewritten (C): `FrontendHost` (register, prepare with writer + kind-6 grants, bind, wait Attached, submit, drain, release, re-attach, close) replaces the Phase 04 `TerminalHost` |
| `libs/terminal/src/session.rs` | 31308 | `b964ec1ac6de4563805cfc3dc14f5e0f55438f7dd970a0e6d44998871644ecf4` | `FrontendOutput` 16-18; `create_for_frontend` 39-60+; `create_serial` and `close_operation_bounded` adjustments; `ForegroundChild::wait_status` 214 (deviation 7) |
| `libs/terminal/src/lib.rs` | 1281 | `bc01a7e5c3d35282069d17619fa203520a2d8c01bc99c1851d3aa28cc013a8d7` | Exports `FrontendOutput` 31, `FrontendHost`/`FrontendState`/`ChildPoll` 32 |
| `libs/terminal/src/stream.rs` | 10921 | `fb70200fd3a7971c0c3f342a64a50c897d2eac9ebea48d8afdb30083cf37c6a2` | `prepare_child` context-length bound to `CONTEXT_MAX` 32 (9-grant context) |
| `libs/terminal/src/pipe_child.rs` | 30781 | `be90dd9795feb21a01069fb4b1c39419fc4a032d64fca0d584e722e3cc997b97` | `query` 131, `wait_status`/`wait_child_status` 140-164 (status from the kernel terminal record, deviation 7) |
| `libs/terminal/src/tests.rs` | 28368 | `a035fed6ba1e6c7d74117eff2c7f47f5dca1eec4600a20419e7180dbfd8a292c` | SDK record tests incl. Lost reasons 4/5 479 |
| `cells/apps/terminal/src/session.rs` | 19036 | `aaa5b0b6c7bf8792df9c2c3fabbba51bed3614fe0f31aba5f073a1a051de0fc8` | `FrontendHost` session 1-122 (open, launch with `CELLBOX_STREAMS=merged`, detach, `reattach`), `Streams for FrontendHost` 269+. Shared with S5-2 and grid fix |
| `cells/apps/terminal/src/main.rs` | 17434 | `fc30687de060f52f3140f95eacb52dad9fd848f085369d33a689720b8c3adabc` | Header 2-3, `SetTimer` comment 47, window re-create → `reattach` 330-337. Shared with S5-2 and grid fix |
| `cells/tools/cellbox/src/context_session.rs` | 8924 | `c4f713fb0638a2e40eee712b5513b01006decd03c7e54db81f542e38469233f6` | Launch-context decision: kind-6 → `CreateForFrontend` (`FrontendLaunch`, `open` 50-74, `ensure_default_cwd` 105+); non-controller branches deleted; two source-text tests removed by Main |
| `cells/tools/cellbox/src/executor.rs` | 149060 | `daa6f064f0266418b64f5d443fdae80b4988ea4190a0bd29ed6c1c3361360f40` | `FRONTEND_OUTPUT`/`frontend_mode`/`STREAMS_MERGED` 44-79, `start_frontend_session` 111-119, `close_session` 121-128, Lost notices 130+, relay to holder writers; `start_context_session`/`context_mode`/`spawn_context_external` deleted; foreground status via `wait_status`. Shared |
| `cells/tools/cellbox/src/main.rs` | 7673 | `9a923bd1e08af140a26d0e79d07b6aa55ae08a88406973c148c2ecdc308acc0a` | Frontend launch branch 122-156, comments 157, 178-179. Shared with S5-2 (78) |
| `cells/tests/phase04-terminal-driver/Cargo.toml` | 623 | `09488365e9404cba5fdb463ccc328f528f628b4b2093e102de383956c55eb404` | Description now names `FrontendHost` (sed by the implementer) |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 76255 | `216668fef8be68c2af4ecda75e461e7a6d0fb5b4abf0d9e6fd9a98c7e98a58c1` | Doc 1-4; `FrontendHost` `Term` 30, 42, 172-190; `frontend` scenario 1394-1495 (`frontend-*` markers); `external-filter-single` sends EndInput (deviation 9). Shared |
| `scripts/qemu-phase04-terminal-test.sh` | 51511 | `62d4b402710c60f67a2d6c2552bc8edada0e422d691027520b8a002fc70cf643` | Header 16-19, 52-57; `FrontendHost` source check 195-196; authority pins incl. `libs/api/src/abi/cell_session.rs` 240-248; UART checks `frontend-detach-observed-uart`/`frontend-reattach-once-uart`; limitation notes 370-372. Shared |
| `scripts/qemu-phase02-session-test.sh` | 22513 | `42b363e1e17791dba2d8aec262102f39d9bc50e533f8281bf73fdae7b6372b9e` | Kernel AP lane expects AP-PG 338 (38 cases). Shared with S5-4a and Phase 02/03 |
| `scripts/qemu-phase04-gui-test.sh` | 31112 | `1ad33576e6c11086df8d5a968c156c6414af15bdced355bccb1e3d2478d3c0f1` | S5-3 option C header 70-78; `FrontendHost` source check 194-195. Shared |
| `scripts/phase04_gui_drive.py` | 64559 | `6065f034258c7aefdd8ee0c4e2233c93e3a5b0558ec3a4ab09eeac5a38336d14` | `s53c_reattach` 774-839 and call 694-695 (`S53C-*`, UART order checks). Shared |

**Deviations** ([annex §11](phase-05-s3c-frontend-annex.md)):
1. Output relayed from the shell's rings to the holder's writers instead of delegating them to children (approved “R”).
2. Window close inside a live Terminal Cell releases the frontend and re-attaches automatically; a user-facing reopen of a running Terminal is not delivered (approved “A”).
3. Input on selector-25 attachments carries epoch 0, and input substitutes the current epoch. A paste spanning an epoch change fails `WrongEpoch`. **Ratified by the user 2026-10-23 (with this CP2).**
4. The `CreateForFrontend` result is 144 B (`Create` is really 128 B). **Ratified by the user 2026-10-23 (with this CP2).**
5. Kernel reads: `InspectPreparedGrant` (op 18) for kind 6 / slot 8 by the current INPUT issuer, and `QueryOperation` (op 9) opened to the INPUT issuer. Read-only. **Ratified 2026-10-23.**
6. `validate_event` admits Lost reasons 4/5. **Ratified by the user 2026-10-23 (with this CP2).**
7. A foreground child's terminal record is retained for its preparer until the preparer's operation closes (exit-status race fix). **Ratified 2026-10-23.**
8. `FrontendHost` releases a closed frontend before closing the launch operation. **Ratified by the user 2026-10-23 (with this CP2).**
9. The driver's `external-filter-single` sends `EndInput`. **Ratified by the user 2026-10-23 (with this CP2).**
10. The input `InspectPreparedGrant` reply buffer is 704 B (was 256 B, below the kernel's 672-byte minimum). **Ratified by the user 2026-10-23 (with this CP2).**

Source-text tests added by the implementer were removed by the parent (executor ×2, `context_session` ×2, terminal `main.rs` ×1).

**Evidence.**
- **Slice-time** (`reports/s5-3c-evidence/`): terminal 380/380 (102 markers), GUI 6×227, Phase03 60, Phase02 48, kernel AP 38/38. Host (annex §11): api 136+2, terminal-sdk 52, app-cellbox 94, terminal 63, service-input 34, kernel 263.
- **Final:**
  - terminal `frontend-controller-session`, `-rm-interactive`, `-top-quit`, `-release-detached`, `-reattach-redraw`, `-reattach-cwd-preserved`, `-reattach-history-preserved`, `-old-attachment-stale`, `-resize-after-reattach` 619-627, plus `frontend-detach-observed-uart`/`frontend-reattach-once-uart` 919-920;
  - GUI click1 `S53C-*` 27 lines 733-759 and `uart-s53c-*` ×3;
  - kernel serial 38 `ATOMIC_PUBLICATION_*: PASS` (166-327), including AP-PI 321, AP-PQ 322, AP-PG 323 and ARMING 327;
  - Phase02 `passes: 48` (407).

**Known limits and open defects.**
- Frontend-holder death (resume revoked, `Lost(4)` then EOF) is proven by host tests only, not on an image.
- Cross-Cell re-attach is out of scope (C2C/Phase 09).
- The user-facing reopen of a running Terminal is not delivered.
- Ssh kind 2 stays `Unsupported`.
- **E4:** the annex promised a recorded input-service cost of `maintain` polling at 64+64 objects; no measurement is recorded.
- Retained terminal records count toward the 512-record cap until each command's operation closes.
- No KVM or multi-hart run.

**Residual risks.**
- New kernel authority surface: the kind-6 grant and read-only ops 18/9 for the INPUT issuer.
- Selectors 23–28 and their records become frozen once any image ships them.
- More input-service state (frontends, `resume` capabilities) sits inside the 64/8 session limits.

**What CP2 would authorize.** The ABI, kernel, ostd, input, SDK, Terminal, cellbox, driver and runner regions above as the option C baseline, including ratification of deviations 3, 4, 6, 8, 9 and 10.

[x] CP2 approved by user — date: 2026-10-23 (deviations 3, 4, 6, 8, 9 and 10 ratified)

## 9. Slice S5-3: proof consumer and selective-TUI negatives

**Scope.**
- A throwaway `cells/tests/tui-proof` (terminal-sdk + `tui-cellos`/`vitui` only) on a private `cellbox -> /bin/tui-proof` row under `phase04-driver-profile`.
- The driver's TUI module and GUI `tui_proof()` scenario.
- Negatives: one-shot commands emit no TUI bytes; `tui-proof | …` and `> FILE` refused; `top` stays batch when piped.

S5-4e later removed the consumer, row, driver module and GUI scenarios. What survives in the tree:
- the two negatives (`oneshot-no-tui-bytes`, `top-pipe-batch`);
- the **unrequested** cellbox change: a lone `/bin/x > FILE` now runs as a one-stage byte pipeline into the file instead of a terminal foreground child that left the file empty. `/bin/x > o 2> e` and `/bin/x < i > o` are refused before any effect, and `/bin/x 2> e` is unchanged.

**Files (current).**

| Path | Bytes | SHA-256 | Phase 05 regions (current lines) and sharing |
|---|---:|---|---|
| `cells/tools/cellbox/src/typed.rs` | 83084 | `b5cb932e20b014b2991466fac8828f2d8b985f6f08a0176d7afab59b5deffd99` | Admission: an external with a stdout redirect becomes a one-stage byte pipeline 339-343; test `external_byte_pipelines` 1660-1690 (stage changed from `/bin/tui-proof` to `/bin/echo` by S5-4e). Rest: Phase 03 engine + OOM streaming |
| `cells/tools/cellbox/src/executor.rs` | 149060 | `daa6f064f0266418b64f5d443fdae80b4988ea4190a0bd29ed6c1c3361360f40` | `exec_simple` doc/admission of the redirected lone external 1112-1117+. Shared |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 76255 | `216668fef8be68c2af4ecda75e461e7a6d0fb5b4abf0d9e6fd9a98c7e98a58c1` | Selective-TUI negatives `TUI_MODES`/`tui_bytes`/`selective_tui` 1496-1545 (`oneshot-no-tui-bytes`, `top-pipe-batch`; moved from the deleted `tui.rs` by S5-4e), call 343. Shared |
| `scripts/qemu-phase04-terminal-test.sh` | 51511 | `62d4b402710c60f67a2d6c2552bc8edada0e422d691027520b8a002fc70cf643` | Selective-TUI header 66-72, markers and `no-tui-bytes-uart`. Shared |
| `kernel/src/loader/launch_profile/mod.rs` | 4459 | `b8db3b800292ad8ebb72c849fdce4cfad1d731b15184bc47f500cb4ab20ec04c` | Private `cellbox -> /bin/tui-proof` dispatch added (S5-3) and removed (S5-4e): **current bytes equal the S2-T/W CP2 hash** — no net Phase 05 change |
| `kernel/src/loader/launch_profile/profiles.rs` | 13851 | `3140c61b0765100bdf14eea8b8455e294c069abbffd71aa86885367377f0b0ca` | `tui_proof_profile` added and removed: **equal to the S2-N/S2-T/W CP2 hash** |
| `kernel/src/loader/launch_profile/tests.rs` | 33844 | `a0f6aa7bbb7937f237301395de8398b31140ecfc16bb5d8b767ec51410ecee86` | `tui_proof_fixture_row_is_feature_gated` added and removed: **equal to the S2-N/S2-T/W CP2 hash** |
| `Cargo.toml` | 8814 | `9d5122d530298500c4bd057d912fc689c85e540e54b9baa10e5532411e35a4e1` | `cells/tests/tui-proof` member added (S5-3) and removed (S5-4e); no net change from these slices. Member renames are R |

Deleted: see §4 (`cells/tests/tui-proof/**`, driver `tui.rs`).

**Deviations** ([proposal Results](phase-05-s3-proposal.md)):
- The pipe negative uses `| /bin/p03-filter cat`, because stock `/bin/cat` does not read stdin.
- The “no lease” part of the one-shot negative is not observable by a frontend; only “no TUI bytes and no soft reset” is checked.
- `tui-proof` listed `/tmp` through its cwd handle.
- One UART line per frame.
- An extra `tui-reread` marker.
- **No second GUI Terminal window.** Two concurrent sessions are proven only on the terminal runner (`tui-two-sessions`, archived).
- **Unrequested redirect change.** **Accepted by the user 2026-10-23 (with this CP2):** the lone `/bin/x > FILE` byte pipeline and its `typed.rs`/`executor.rs` regions are part of the S5-3 baseline.

**Evidence.**
- **Slice-time** (`reports/s5-3-evidence/`, now history-only for TUI behaviour): terminal 393/393 (113 markers), GUI 6×252, Phase03 60, Phase02 48. Host: tui-proof 9/9, app-cellbox 94/94, launch_profile 29/29 with and without the feature.
- **Final:** terminal `oneshot-no-tui-bytes` 628, `top-pipe-batch` 629, `no-tui-bytes-uart` 890. Host `external_byte_pipelines` (app-cellbox 97/97, implementer report).
- **No current image check exercises a lone external with `> FILE`.** The archived runtime proof was `tui-proof > /tmp/x` (`tui-pipe-refused`).

**Known limits and open defects.**
- TUI frames, keys, resize and restore after `q`/Ctrl-C/abandon, as well as two concurrent TUI sessions, are archived evidence only until a real consumer exists (Phase 06 `top`).
- The redirect change has host coverage only in the current tree.

**Residual risks.** Phase 05 criteria 2 and 4 rest partly on archived runs of code that no longer exists, so regressions in `vitui` runtime behaviour would go unnoticed until Phase 06.

**What CP2 would authorize.**
- The selective-TUI negatives and the `typed.rs`/`executor.rs` redirect-admission regions (if the unrequested change is accepted).
- Recording that the launch-profile files and the workspace membership return to their earlier approved bytes.

[x] CP2 approved by user — date: 2026-10-23 (S5-3 CP1 recorded retroactively; redirect change accepted)

## 10. Slice S5-4: secrets, hostile output, glyphs, theme, grid fix and `tui-proof` removal

**Scope.**
- **S5-4a, secrets and hostile output.**
  - Input stores secret-bearing requests (`SubmitEvents`/`WritePaste`/`SubmitRaw`) only as SHA-256 at receipt.
  - cellbox gains `${NAME}`/`${#NAME}`, with the value copy wiped.
  - Builtin `cat` invalid-UTF-8 fix (bytes after an invalid byte near EOF were dropped).
  - Hostile fixture `scripts/phase05_vt_hostile.py`.
  - Driver secret/hostile/paste scenarios and the Phase02 physical-UART `read -s` refusal.
- **S5-4b, glyphs.** A font census found box drawing 128/128 and blocks 32/32, so procedural drawing was dropped. Row-gap check.
- **S5-4c, theme.** `vitui::theme`; the Terminal round trip moved onto `vitui::ratatui`.
- **Grid fix.** `grid_dims` subtracts the padding: 720×480 → 79×26.
- **S5-4e.** Removal of `tui-proof` and its row, driver module, GUI scenarios and two UART checks.

**Files.**

| Path | Bytes | SHA-256 | Phase 05 regions (current lines) and sharing |
|---|---:|---|---|
| `cells/services/input/src/session.rs` | 104196 | `906412286be2008bdab314bcaccd6a9ef2b5cdcb0c251d2ee29ca616ecd881cf` | S5-4a: secret-bearing requests (sel 13/15/20, `secret_bearing` 110) retained only as SHA-256 at receipt: `Outcome.sealed` 106, `retained` 112-127, `scrub`/`matches` 129-138, insert 307-312, replay lookup 366-369; test `secret_requests_are_sealed_when_consumed_and_still_resolve_replays` 1230-1310. Shared with C |
| `cells/tools/cellbox/src/parser.rs` | 32046 | `5d4d3eda3ebadf5a5c17e6bdc797cc1deb056c0604c9c7600c1d94be4b24d20c` | S5-4a: `${NAME}`/`${#NAME}` kept inside the word 297-300+; unclosed `${` literal |
| `cells/tools/cellbox/src/executor.rs` | 149060 | `daa6f064f0266418b64f5d443fdae80b4988ea4190a0bd29ed6c1c3361360f40` | S5-4a: braced expansion 844-861 (value copy wiped). Shared |
| `cells/tools/cellbox/src/commands.rs` | 25305 | `ae2b6ed75c50468f11764b7a52008f06f1c83bc3eeb12bba89b858f2589eb32c` | S5-4a: builtin `cat` invalid-UTF-8 fix `emit_utf8_lossy` 404-409, 435-455; tests 639+ |
| `scripts/phase05_vt_hostile.py` | 4131 | `709bd4e3863764f9002d536c2bd29cd368ae117fde5f90bd03c805216b5c9ae2` | Whole file (S5-4a only): hostile fixture generator (OSC 52/8/0/2, >4 KiB DCS/APC/PM, DA/DSR/DECRQM, 8-bit C1, 17-param CSI, embedded `ESC[201~`), shared by both runners |
| `scripts/qemu-phase02-session-test.sh` | 22513 | `42b363e1e17791dba2d8aec262102f39d9bc50e533f8281bf73fdae7b6372b9e` | S5-4a: physical-UART `read -s` refusal: header 17-21, scenario 298-312, oracle 386-390. Shared with C |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 76255 | `216668fef8be68c2af4ecda75e461e7a6d0fb5b4abf0d9e6fd9a98c7e98a58c1` | S5-4a: `secret_read` 500-581, `hostile_output` 582-621, `paste_injection` 622-696, calls 336-338. S5-4e: `tui` module/registration removed. Shared |
| `scripts/qemu-phase04-terminal-test.sh` | 51511 | `62d4b402710c60f67a2d6c2552bc8edada0e422d691027520b8a002fc70cf643` | S5-4a header 73-87, hostile fixture 318-349, packaging 385, limitation notes 373-378; S5-4e removals (proof build/image/provenance, two UART checks), expected 397/115. Shared |
| `scripts/qemu-phase04-gui-test.sh` | 31112 | `1ad33576e6c11086df8d5a968c156c6414af15bdced355bccb1e3d2478d3c0f1` | S5-4 hostile header 79-88, fixture 338-358; grid-fix oracle note 59-60; S5-4e removals (image list, build `-p`, `FEATURES` without `phase04-driver-profile`), header 117-119. Shared |
| `scripts/phase04_gui_drive.py` | 64559 | `6065f034258c7aefdd8ee0c4e2233c93e3a5b0558ec3a4ab09eeac5a38336d14` | Hostile oracle import 43-45, doc 24, `hostile` scenario (call 698); `COLS, ROWS` from padding 49-51 and 79-column expectations (grid fix); S5-4e removed `TUI-*`/`THEME-*`/`FRAME-*`. Shared |
| `libs/vitui/src/theme.rs` | 10407 | `0ab1d1b9f99b82793e9bbe73168f3154557fd22da88e77257a03ad3eb761a7c1` | Whole file (S5-4c only): truecolor palette with 256/16 fallback by explicit `ColorMode`, border sets, bars/list/hint helpers, block/half-block markers 128-132 |
| `libs/vitui/src/theme/tests.rs` | 8116 | `cd224c4ec70e3a4b9f3ab31c02a1a6641ffd281e66b611b0d2c2519fa7d41e88` | Whole file (S5-4c only): theme fallback tests |
| `libs/vitui/src/lib.rs` | 994 | `9bbc2b2b3c0852b705a027e27afdf3f75fb82a00816d7323904b8f528e3e7e37` | `pub mod theme` + doc (S5-4c). Shared with S5-1, R |
| `cells/apps/terminal/src/roundtrip.rs` | 10518 | `e6a74695c87fb07a959bee6d4fa26805bd392931bcf7b4e6f359d750859692bf` | Imports via `vitui::ratatui` (S5-4c). Shared with S5-2, R |
| `cells/apps/terminal/Cargo.toml` | 920 | `0f2c5d8632d4588d4018a48ec5fa10e49ca50ab54a9c8ec54f57e25947fbda33` | No direct Ratatui dev-dependency (S5-4c). Shared |
| `cells/apps/terminal/src/main.rs` | 17434 | `fc30687de060f52f3140f95eacb52dad9fd848f085369d33a689720b8c3adabc` | Grid fix: `grid_dims` 175-183 subtracting `PAD_X`/`PAD_Y`, used at start 68, resize 82, re-attach 333; tests 388-422 (720×480 → 79×26). Shared |
| `cells/apps/terminal/src/session.rs` | 19036 | `aaa5b0b6c7bf8792df9c2c3fabbba51bed3614fe0f31aba5f073a1a051de0fc8` | Grid fix: frontend registers the grid's own size (81); local dims helper and its test removed. Shared |
| `scripts/phase04-gui-glyphs/src/main.rs` | 16228 | `1ddc8ea2c340ef827fa8a98a18fa3370ee5ba152b826bdb37f08168e24e891ce` | Grid fix: `grid_dims` 57-64 mirrors the Terminal. Shared |
| `cells/tools/cellbox/src/typed.rs` | 83084 | `b5cb932e20b014b2991466fac8828f2d8b985f6f08a0176d7afab59b5deffd99` | S5-4e: redirect admission test stage `/bin/echo` (1684-1690). Shared with S5-3 |
| `kernel/src/loader/launch_profile/mod.rs` | 4459 | `b8db3b800292ad8ebb72c849fdce4cfad1d731b15184bc47f500cb4ab20ec04c` | S5-4e: proof row removed; equals S2-T/W CP2 hash |
| `kernel/src/loader/launch_profile/profiles.rs` | 13851 | `3140c61b0765100bdf14eea8b8455e294c069abbffd71aa86885367377f0b0ca` | S5-4e: proof profile removed; equals S2-N/S2-T/W CP2 hash |
| `kernel/src/loader/launch_profile/tests.rs` | 33844 | `a0f6aa7bbb7937f237301395de8398b31140ecfc16bb5d8b767ec51410ecee86` | S5-4e: proof test removed; equals S2-N/S2-T/W CP2 hash |
| `Cargo.toml` | 8814 | `9d5122d530298500c4bd057d912fc689c85e540e54b9baa10e5532411e35a4e1` | S5-4e: `cells/tests/tui-proof` member removed |
| `Cargo.lock` | 82999 | `63bf3d8c41b6fc46eb578cc537b4dab4bb141da88a5552b1e33e503277d83a56` | S5-4e: `app-tui-proof` pruned |

**Deviations** ([proposal §7–§9](phase-05-s4-proposal.md); S5-4e implementer report):
- **Serial `read -s`.** No token is typed: the refusal precedes any key read.
- **Hostile output.** The terminal runner judges it by exact relayed bytes and the absence of injected input; screen effects are the GUI runner's claim.
- **Glyphs.** Procedural box drawing was dropped (D54-1 analysis); JetBrains Mono kept.
- **Theme.** It takes an explicit `ColorMode`, because the capabilities advertise none.
- **Grid fix.** User-approved 2026-10-23.
- **S5-4e:**
  - two negatives moved into the driver rather than deleted;
  - the redirect test kept with `/bin/echo`;
  - the GUI kernel no longer built with `phase04-driver-profile`;
  - **the GUI last-column runtime check (`FRAME-2`) was lost**, so the grid fix now has host tests only (`main.rs` 388-422) plus archived evidence.
- **Product changes outside the named closure** (**ratified by the user 2026-10-23, with this CP2**):
  - `${NAME}`/`${#NAME}` expansion (parser + executor), which the CP1 relied on through `echo ${#SECRET}` but did not list;
  - the `cat` fix, allowed by the CP1's “product code only where a check fails”.

**Evidence.**
- **Slice-time:**
  - `reports/s5-4-evidence/`: terminal 408/408 (124 markers), GUI 6×306, Phase02 48 (+ `read -s` refusal), Phase03 60;
  - `reports/s5-4-gridfix-evidence/`: GUI 6×323, terminal 408/408;
  - host: app-cellbox 97, service-input 34, vitui 22, tui-proof 12 → 15, terminal 63 → 64.
- **Final:**
  - terminal `secret-read-*` 588-591, `hostile-*` (592-…, 6 lines incl. 901), `paste-*` 595-598;
  - GUI click1 `HOSTILE-*` 27 lines 760-786 and `no-command-uart:hostile-*` ×4;
  - phase02 (`read -s` refusal is a `problems` check in `qemu-phase02-session-test.sh` 386-390): `passes: 48` 407 and “observations complete” 409, with no `FAIL` block;
  - host after S5-4e: app-cellbox 97, terminal 64, vitui 22, launch_profile 28/28 with and without the feature (implementer report).

**Known limits and open defects.**
- Braille (U+2800–28FF) is missing from the font and renders as the missing-glyph box.
- A faint seam from the 8.4 px advance in 9 px cells.
- Windows below 40×12 lose trailing cells.
- `read -s` on the physical UART fails closed, because echo-off is unenforceable there.
- The GUI runner notes it does **not** prove a clipboard or title facility exists on the image; the proof is that nothing is replied or applied.
- `THEME-*`, `FRAME-*` and last-column runtime checks are archived only.

**Residual risks.**
- Plaintext secret bytes still transit the IPC buffers before sealing (wiped, not retained) `[INFERENCE from the design; not separately measured]`.
- The theme's colour fallback is chosen by the app, not negotiated.

**What CP2 would authorize.** The S5-4a input/cellbox/fixture/runner regions, `vitui::theme`, the grid-fix regions, and the S5-4e removals as the hardening baseline.

[x] CP2 approved by user — date: 2026-10-23 (S5-4 “go” recorded retroactively; `${NAME}`/`${#NAME}` and `cat` fix ratified)

## 11. Slice R: library renames (`tui-cellos` → `vitui`, `mio-cellos` → `vimio`, `polling-cellos` → `vipolling`)

**Scope.** A mechanical rename recorded in `docs/code-standards.md` Law 6. `vitui` exposes its parts by module only and adds `vitui::ratatui::{core, widgets}`. Consumers, docs and runner comments are updated. Behaviour is unchanged **except** two kinds of output strings: the `httpd` JSON and the demo log lines.

**Files.**

| Path | Bytes | SHA-256 | Phase 05 regions (current lines) and sharing |
|---|---:|---|---|
| `libs/vimio/Cargo.toml` | 313 | `40fe148314b1e92f6899192522586720b1fd0bd5e5e20f66a9cd69f6e0a483f9` | `name = "vimio"` (only change vs HEAD `libs/mio-cellos/Cargo.toml`) |
| `libs/vimio/src/lib.rs` | 10950 | `6d76670bc97c9ab828ade77dbbc0eeee7ae7a36c40363855d9c2ffe03fba4729` | Byte-identical to HEAD `libs/mio-cellos/src/lib.rs` (git rename) |
| `libs/vipolling/Cargo.toml` | 304 | `af3f4b4ae3ebcd3b04afac7f6a205423a7149b2937c8ad483d64f72475c50dcc` | `name = "vipolling"` (only change vs HEAD `libs/polling-cellos/Cargo.toml`) |
| `libs/vipolling/src/lib.rs` | 11367 | `add124c7c41a8478a0e7d396258eec6512df868377742956689dfe38657a4edc` | Byte-identical to HEAD `libs/polling-cellos/src/lib.rs` (git rename) |
| `libs/vitui/Cargo.toml` | 691 | `c8f0473d6a411d5623a68953115e88485ef4a1f73917521fbc252b4f0a377865` | `name = "vitui"` (directory moved from untracked `libs/tui-cellos`). Shared with S5-1 |
| `libs/vitui/src/lib.rs` | 994 | `9bbc2b2b3c0852b705a027e27afdf3f75fb82a00816d7323904b8f528e3e7e37` | Module-only exports, `vitui::ratatui::{core, widgets}`. Shared with S5-1, S5-4c |
| `libs/vitui/src/tests.rs` | 13623 | `b9f583a1cac2112c3e2541bf3f939ab6755ca74646b2a481752ecda0aea167f2` | Import paths. Shared with S5-1 |
| `Cargo.toml` | 8814 | `9d5122d530298500c4bd057d912fc689c85e540e54b9baa10e5532411e35a4e1` | Workspace members `libs/vitui`, `libs/vipolling`, `libs/vimio` (replacing `*-cellos`). Other hunks vs HEAD are Phase 02–04 |
| `Cargo.lock` | 82999 | `63bf3d8c41b6fc46eb578cc537b4dab4bb141da88a5552b1e33e503277d83a56` | Package names `vitui`, `vimio`, `vipolling` (Cargo). Shared |
| `cells/apps/haily-agent/Cargo.toml` | 870 | `a0a2df97c6e9bebab7d412bb997be8dfd38f000506198af08319baa449e47fbb` | Dependency `vimio` 15 |
| `cells/apps/haily-agent/src/main.rs` | 9125 | `74abc8d6032587e9b00aa7bc92296a43fddac87c4f48c5dc8409faa8bbb26a41` | `use vimio::{Events, Selector}` 24 |
| `cells/demos/tokio-smoke/Cargo.toml` | 488 | `9750e208b5842e92e055c30fca1f11921a5cbd6b8ee93475f2e4c544fd2e6326` | Dependency `vimio` 11 |
| `cells/demos/tokio-smoke/src/main.rs` | 3937 | `ebc46094f3e461a25867371a6aabf343b9927491218b9a5f02ac6471215fa65d` | Doc 1, `use vimio::…` 12, log string 38 |
| `cells/demos/async-echo/Cargo.toml` | 494 | `fdc7725fb2c1ed8d14603ac55f1f90cef554429467eb2b6bed8a0240b3dee3a8` | Dependency `vipolling` 11 |
| `cells/demos/async-echo/src/main.rs` | 4161 | `326dc54d95e5838d7a36e24f792d67a86325b6fe8765f83547bb96bf2f68a403` | Doc 3, `use vipolling::…` 14, log string 39 |
| `cells/services/httpd/src/handlers.rs` | 10774 | `5cce6f2b25b350ab0d9ff6169dbe84e3ecba06aa407aaf325e66520bef88aee0` | `/api/gateway/stats` JSON `"runtime_engine":"vimio"` 197 (**externally visible output change**) |
| `docs/code-standards.md` | 33644 | `9f87c3246074d5da7e7140e79c5108107189f6d0bade5fd664ec75ca2cde5424` | Law 6 naming row 317 and rule 319-324 (Main) |
| `docs/cellbox-cli.md` | 21913 | `c7c7b43e1493079e3709fad812af76fbd5bb674311af16678ce31b38a6b2e22d` | `libs/vitui` in source ownership 30. Other hunks vs HEAD are Phase 04 S3 text |
| `docs/roadmap/haily-portability-review.md` | 8575 | `b7ac3a6a836aafe829fa5a86e3a2d7ada151cf5775d6e3155479257d028df9c5` | `mio-cellos` → `vimio` in 4 places (14, 29, 63, 85) |
| `cells/apps/terminal/Cargo.toml` | 920 | `0f2c5d8632d4588d4018a48ec5fa10e49ca50ab54a9c8ec54f57e25947fbda33` | Dev-dependency path `libs/vitui`. Shared |
| `cells/apps/terminal/src/roundtrip.rs` | 10518 | `e6a74695c87fb07a959bee6d4fa26805bd392931bcf7b4e6f359d750859692bf` | `vitui::…` imports. Shared |
| `cells/tests/phase03-filter/src/main.rs` | 12299 | `4f622b4539561e402e6b7afe1e8bde2b4eec9fb2e6f17ea5f9ce645a42a2855a` | Doc wording `vitui` 28, 97. Shared |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 76255 | `216668fef8be68c2af4ecda75e461e7a6d0fb5b4abf0d9e6fd9a98c7e98a58c1` | Doc wording `vitui` 767. Shared |
| `scripts/qemu-phase04-terminal-test.sh` | 51511 | `62d4b402710c60f67a2d6c2552bc8edada0e422d691027520b8a002fc70cf643` | Comment wording. Shared |
| `scripts/qemu-phase04-gui-test.sh` | 31112 | `1ad33576e6c11086df8d5a968c156c6414af15bdced355bccb1e3d2478d3c0f1` | Comment wording. Shared |
| `scripts/phase04_gui_drive.py` | 64559 | `6065f034258c7aefdd8ee0c4e2233c93e3a5b0558ec3a4ab09eeac5a38336d14` | Comment wording. Shared |

**Deviations.** None recorded. The `httpd` `/api/gateway/stats` field `runtime_engine` changes from `"mio-cellos"` to `"vimio"`, an externally visible value. No in-repo consumer of that field exists (repository grep). **Accepted with this CP2 (user, 2026-10-23)** as one of the listed R hunks.

**Evidence.**
- Host: vitui 15/15, terminal 63/63, tui-proof 9/9; bare-metal checks for all eleven affected packages; runner syntax OK; no digest pin affected ([S5-4 §5](phase-05-s4-proposal.md)).
- The final runs build the renamed crates (`Cargo.lock` binding, §5).
- `haily-agent`, `tokio-smoke`, `async-echo` and `httpd` are **not** run by any Phase 05 runner. The only reference is `/bin/service-httpd` in a desktop ceiling list (`qemu-phase04-gui-test.sh` 206).

**Known limits and open defects.**
- Earlier CHANGELOG entries and archived logs keep the old names, by design.
- `docs/roadmap/haily-portability-review.md` describes an external Haily workspace that is not updated.

**Residual risks.** An external client of `/api/gateway/stats` (e.g. the Haily host coordinator) that matches `"mio-cellos"` would break.

**What CP2 would authorize.** The rename hunks listed above.

[x] CP2 approved by user — date: 2026-10-23 (`httpd` value change included)

## 12. Historical slice evidence (archived runs on earlier trees)

These runs were on trees that have since changed. They are cited for behaviour that no longer has a live check (TUI, theme, last column).

| Directory | Tree | Totals |
|---|---|---|
| `reports/s5-2-evidence/` | after S5-2 | terminal 368/368 (93), GUI 6×197, P03 60, P02 48 |
| `reports/s5-3c-evidence/` | after option C | terminal 380/380 (102), GUI 6×227, P03 60, P02 48, kernel AP 38/38 |
| `reports/s5-3-evidence/` | after S5-3 | terminal 393/393 (113), GUI 6×252, P03 60, P02 48 |
| `reports/s5-4-evidence/` | after S5-4a–c | terminal 408/408 (124), GUI 6×306, P03 60, P02 48 |
| `reports/s5-4-gridfix-evidence/` | after the grid fix | terminal 408/408 (124), GUI 6×323 |

| Log | Bytes | SHA-256 | Total |
|---|---:|---|---|
| `reports/s5-2-evidence/gui-autoload.log` | 37950 | `3a6f4d4a1c4e1047e9b8c5e6a84c455f759f877fb6bad5975318bfb42d1f158c` | 197/197 GUI |
| `reports/s5-2-evidence/gui-click1.log` | 37912 | `d3514d97aab6f4de11508e7d5b8900c1abd472be20236f27ac6fe9b07816a219` | 197/197 GUI |
| `reports/s5-2-evidence/gui-click2.log` | 37912 | `e6f0b1a8be4d2cacb4f814e4959fce26e3891efa667af50c98152294dcbb2174` | 197/197 GUI |
| `reports/s5-2-evidence/gui-click3.log` | 37912 | `995d69b08c349d6956283c309c5bea9e1f069d689bd1155d4ef55a85c5ac6b79` | 197/197 GUI |
| `reports/s5-2-evidence/gui-click4.log` | 37912 | `692bcecef07c601e570233341c60d4d5d832fdbea918c208a0a670a3c0a5e029` | 197/197 GUI |
| `reports/s5-2-evidence/gui-click5.log` | 37912 | `33f4d3140e9bfdd8bfbea21c9436b36a5c0fbdf8dc37c5580563a9efe8a73b3e` | 197/197 GUI |
| `reports/s5-2-evidence/phase02.log` | 20249 | `fe133012776fa4edb1309ae37b6f4869edc15552813d762aadc854873cbfad2b` | passes: 48 |
| `reports/s5-2-evidence/phase03.log` | 21935 | `bf3ac91c25a00d20b6ba857c7ffcd20e13e38d31fe808235abc93ad94f802a5b` | 60 PASS |
| `reports/s5-2-evidence/phase04-terminal.log` | 37078 | `e16b9d87857d9ba64686e79fa9ed6965a85d80da14914a9d55e7455b81af4c81` | 368/368, 93 markers |
| `reports/s5-3c-evidence/gui-autoload.log` | 39225 | `012fb8265336c70a5e4159e14ed4ca7139cd3a57a6b989b6b67650a9c4531275` | 227/227 GUI |
| `reports/s5-3c-evidence/gui-click1.log` | 39187 | `0e8473c319422c0845112dfd40b22fe49e8a3711f93fd3c22e0177d569f884ec` | 227/227 GUI |
| `reports/s5-3c-evidence/gui-click2.log` | 39187 | `9d3bc974e881f0d2788e53d0fdaac931309e222ff62da03532d4b2554202dd5e` | 227/227 GUI |
| `reports/s5-3c-evidence/gui-click3.log` | 39187 | `4a334790ebc7c256ba95e4d5fd747acdc696d3176ac562d75e5285de2b4502e6` | 227/227 GUI |
| `reports/s5-3c-evidence/gui-click4.log` | 39187 | `be7dcd04fcf39f6a880d2afdc8470be8ff811bb4ee07cc38bd1e52e87ba8e823` | 227/227 GUI |
| `reports/s5-3c-evidence/gui-click5.log` | 39187 | `f58fcd6c461520353ea69fb395017c985391a256e1335d3a0f77dabbc9766548` | 227/227 GUI |
| `reports/s5-3c-evidence/phase02-kernel-serial.log` | 22157 | `4ebcd85d2ca6280c55df1e6e409b3fc484b71029bb868dbac7722a702ada8e1a` | ATOMIC_PUBLICATION 38 PASS, 0 FAIL |
| `reports/s5-3c-evidence/phase02.log` | 20238 | `999bf72e1edb54952070a67a76cd6b6d1038ff4736122dfa55a26991e49433fa` | passes: 48 |
| `reports/s5-3c-evidence/phase03.log` | 21924 | `86ca50cd486fc0c04c4fbdc39432fe6959c2dba3d998d26550736a3ce91ed37c` | 60 PASS |
| `reports/s5-3c-evidence/phase04-terminal.log` | 37530 | `8c39c7e8f0bbff28fa9c0478a0d315e85d7533c900014de9e0a845ce00b969cb` | 380/380, 102 markers |
| `reports/s5-3-evidence/gui-autoload.log` | 41518 | `cd66122d04cb7f7c893acfcdb47a1c78b588c5a477299bd35854456b19944a8d` | 252/252 GUI |
| `reports/s5-3-evidence/gui-click1.log` | 41480 | `ee31dd7c9e9266113ffff838b21de38d6e357eb231651351beb1d3271bfc1b09` | 252/252 GUI |
| `reports/s5-3-evidence/gui-click2.log` | 41480 | `64fe1e9347b4cea1e8f636c8594031d224d5e795f4cfc629682ccc04badf295b` | 252/252 GUI |
| `reports/s5-3-evidence/gui-click3.log` | 41480 | `5b3faa20f72f6d87fc462e18abb20d9c4ee41a586d0403526b1e4a560b94b512` | 252/252 GUI |
| `reports/s5-3-evidence/gui-click4.log` | 41480 | `3f8d954068d70ca095f0c60191d18d3e561f660d7f81e35062af05c5f24e6965` | 252/252 GUI |
| `reports/s5-3-evidence/gui-click5.log` | 41480 | `c7d2028ef4ac00f1d3556b0d0d5ecf70990ca2b7dc552cebae3eb8e7c6777d42` | 252/252 GUI |
| `reports/s5-3-evidence/phase02.log` | 20238 | `5da0dacb06ee6a0523517d5d8065788a57388bb601eabe589e628f2575ab8ffe` | passes: 48 |
| `reports/s5-3-evidence/phase03.log` | 21924 | `8ac91b49e39411dc88b0d817b68cff322bd83ddba33e8b9d40d797953b29a17c` | 60 PASS |
| `reports/s5-3-evidence/phase04-terminal.log` | 27973 | `9d3e81d7197ab2f4c9a24548907d89776ba157714460f5c4fadd42e728218c51` | 393/393, 113 markers |
| `reports/s5-4-evidence/gui-autoload.log` | 43923 | `b22ae165c1b35d612d6230a66ddf600172c8388c9f699bc44d66b8f902a16230` | 306/306 GUI |
| `reports/s5-4-evidence/gui-click1.log` | 43885 | `36bf0ba5a99d4e21d8d8808c280c019573087068b24af026393ed9fccf297526` | 306/306 GUI |
| `reports/s5-4-evidence/gui-click2.log` | 43886 | `f672f8d4c668a35e31f16e55156063f0a64d143d80864b663c0e30cec7d4aaaa` | 306/306 GUI |
| `reports/s5-4-evidence/gui-click3.log` | 43886 | `b0ee9bbfc233733838fab03308b0565e817834ec531de4a4483105f2d4f550cb` | 306/306 GUI |
| `reports/s5-4-evidence/gui-click4.log` | 43885 | `ca58a9ad45e3a66bfec38666c2c9dc65e861115cdc3073a3904858331c43d6d7` | 306/306 GUI |
| `reports/s5-4-evidence/gui-click5.log` | 43885 | `3ca533a0330f07418423c0d68a2aab0a9988b8adb604258108fd3c9dfa76ab30` | 306/306 GUI |
| `reports/s5-4-evidence/phase02.log` | 20238 | `5da0dacb06ee6a0523517d5d8065788a57388bb601eabe589e628f2575ab8ffe` | passes: 48 |
| `reports/s5-4-evidence/phase03.log` | 21924 | `45e65e34b6ce04b1986354396ffd89163e9bd14add384deba66964e1dabcbe1a` | 60 PASS |
| `reports/s5-4-evidence/phase04-terminal.log` | 39670 | `f2aa5acffe086f66f217708a772d7459f42a3c1bd1827545f14073d49c75ea52` | 408/408, 124 markers |
| `reports/s5-4-gridfix-evidence/gui-autoload.log` | 44662 | `8879b246833c4c1617631f1bdd912ac7dc54677589b2cc16cd81e9b61e50931a` | 323/323 GUI |
| `reports/s5-4-gridfix-evidence/gui-click1.log` | 44624 | `effc3e1c2d8ef5f0a9b96f8056b31ee99c5c1dd5ce439db59ff99f005641dae8` | 323/323 GUI |
| `reports/s5-4-gridfix-evidence/gui-click2.log` | 44624 | `da4ab9220325b536ae9f6ea36880e553784d4481fe51baf68339825fb3b45fe8` | 323/323 GUI |
| `reports/s5-4-gridfix-evidence/gui-click3.log` | 44624 | `db11c10164ef32cdd6470f764c76791319450c282578b1a855f94f61e5ad8377` | 323/323 GUI |
| `reports/s5-4-gridfix-evidence/gui-click4.log` | 44625 | `89a3a1e415862035dd081cdc4013e7f8ce985b3cdc29005aa1520c7ef6e755c1` | 323/323 GUI |
| `reports/s5-4-gridfix-evidence/gui-click5.log` | 44624 | `64cac30ee9653e127a90b7ca7fb5223c8df12d2a1cf1b0cbcf5efc8af8ce2270` | 323/323 GUI |
| `reports/s5-4-gridfix-evidence/phase04-terminal.log` | 39670 | `b9037c341acb4142b8268aeb217a17b0113aac15562321e6b52103eae7bd755c` | 408/408, 124 markers |

Every listed log has 0 `FAIL` lines.

## 13. Unattributed changes

**None found.** All 65 source files and 7 plan documents in the Phase 05 window are attributed above, and all deletions are accounted for (§4).

Caveats:
- **Edits invisible to the method.** Transcript attribution cannot see an edit made by a shell command that matches none of the tracked patterns (§2). The mtime scan would still have listed such a file; it found none.
- **Bytes outside every Phase 05 region.** Many files carry Phase 02–04 bytes in the same whole-file hash with no earlier per-file hash (for example `typed.rs` Phase 03 code, `qemu-phase02-session-test.sh` Phase 03 AP-lane changes, and `docs/cellbox-cli.md` S3-output paragraphs). Those bytes are **not** attributed to Phase 05 and are not reviewed here.
- **Plan document digest drift.** [spec.md](spec.md) no longer matches its frozen candidate digest `d318e63f…c8a` ([contract-deltas.md](contract-deltas.md) line 19); it is now `7e6e11c7c383e7f0f46e9c7dae5e4b034cec68d265a01f00d8dc811e60f3b1a8`. The 2026-10-23 §4 amendment is a recorded user decision. This is a plan document, not source. **Acknowledged 2026-10-23 and diff-verified:** the amending command is in the parent session transcript (a scripted replace of `autowrap enabled, CR separate from LF.` in §4 plus one appended change-log line). Reversing exactly those two edits on the current bytes reproduces `d318e63fae7da4e65ba581767f08c94cf6bb054e6cd3a4aff91169970f967c8a` byte for byte, so the §4 LF amendment is the only change since the frozen digest.

## 14. Docs not reconciled at preparation (reconciled 2026-10-23 after approval; see the last bullet)

- **`CHANGELOG.md`:**
  - no S5-1 entry and no grid-fix entry;
  - lines 12 and 13 still say “host-verified only, QEMU runners not yet run”;
  - line 10 calls the 80th column an open finding and says the GUI scenarios were not yet run;
  - line 11 says the GUI scenarios were not yet run.
- **[phase-05-s3-proposal.md](phase-05-s3-proposal.md):** the status line still reads “PROPOSED”.
- **[phase-05-s4-proposal.md](phase-05-s4-proposal.md):**
  - the status line still reads “awaiting ‘go’”;
  - it has no S5-4e results section (the results are in the CHANGELOG and the phase file only).
- **[phase-05-entry-dossier.md](phase-05-entry-dossier.md):** still names `libs/tui-cellos` (historical text).
- **[acceptance.md](acceptance.md):** CBX-13/15/16/29 stay pending (Phase 10 qualification).
- **Reconciled 2026-10-23 after approval** (records edits only; no source change):
  - `CHANGELOG.md`: S5-1 entry added under Added; grid-fix entry added under Fixed; the S5-2, option C, S5-3 and S5-4a–c entries now cite their archived runs and CP2 approval instead of “not yet run” / “open finding”.
  - S5-3 and S5-4 proposals: status lines record the retroactive approval; the S5-4 proposal gains an S5-4e results section.
  - [phase-05-ratatui-terminal.md](phase-05-ratatui-terminal.md): status `completed-private-evidence` with a log line (rollout still blocked).
  - The entry dossier's `libs/tui-cellos` wording stays as historical text; acceptance rows stay pending (Phase 10).

## 15. What CP2 does not do, options, rollout and rollback

**CP2 does not:**
- lift quarantine;
- authorize canonical packaging, release signing or rollout;
- close any CBX acceptance row;
- accept the Phase 02–04 bytes inside the shared files;
- authorize KVM, physical, remote or multi-hart claims. All evidence is private QEMU/TCG.

**Shared files.** A whole-file hash in §3 becomes a baseline only when every slice that touches the file is approved. Examples:
- `executor.rs`: S5-2, C, S5-3, S5-4 and Phase 02–04;
- the driver, both Phase04 runners and the GUI driver: every slice;
- `CHANGELOG.md`: S5-2, C, S5-3, S5-4, R.

Approving one slice alone authorizes its regions, not the other slices' bytes in the same file.

**Dependencies between slices:**
- C relies on S5-2's relay tracker (deviation 1 keeps the soft reset).
- S5-4a's secret, hostile and paste checks run through C's frontend.
- S5-3's surviving regions and S5-4e are entangled in `typed.rs` and the driver.
- R touches every slice's imports.

**Options.**
- **A.** Approve each slice as listed, including the ratifications requested in §6–§11, and confirm the missing approval records: the S5-3 text, the S5-4 “go”, and the unrequested redirect change.
- **B.** Approve a subset. The non-approved slices' regions must then be reverted as matched sets (below) before any of the shared files can become a baseline.
- **C.** Not yet: keep the implementation and evidence and withhold CP2. A rerun can be requested first, for example a live last-column GUI check or a lone-external `> FILE` image check.

**Approved 2026-10-23: option A** (all six slices, every ratification requested in §6–§11, and the missing approval records; see §16).

**Rollout.** Stays **BLOCKED** under [containment](reports/rollout-containment.md). Nothing is unblocked.

**Rollback.** Every rollback is a matched set: source plus its driver/runner checks. Removing one without the other fails the runners.
- **S5-1:** cannot be reverted alone while S5-2 (round trip) and R depend on `vitui`.
- **S5-2:** revert `sink.rs`/`grid.rs`/renderer, `screen_reset.rs` and its executor hooks, `tui-abandon`, the driver `soft_reset` and the `VT-*` scenarios.
- **C:** an ABI rollback is possible only because no image has shipped selectors 23–28 (fixtures are private). Revert the api/kernel/ostd/input/SDK/consumer regions and restore `TerminalHost` and the context mode together.
- **S5-3:** revert the redirect admission and its test.
- **S5-4:** revert the input sealing, the expansion, the `cat` fix, the hostile fixture and checks, `theme`, and the grid fix (79 → 80 columns re-breaks the last column).
- **R:** revert all consumers together.

Bytes already delivered to terminals or written by private guests cannot be recalled.

## 16. Approval record (2026-10-23)

User decision, 2026-10-23: **CP2 APPROVED for all six Phase 05 slices** in this dossier (S5-1, S5-2, C, S5-3, S5-4, R), i.e. option A of §15. It covers each slice's listed regions at the §3 hashes, and also:
- **Retroactive approval records.** The S5-3 CP1 ([proposal](phase-05-s3-proposal.md) §3, D53-3 as recommended; D53-1/D53-2 superseded by option C) and the S5-4 “go” ([proposal](phase-05-s4-proposal.md) §2, D54-1…D54-6) are recorded as given; both were implemented before the record existed.
- **Ratified deviations.**
  - S5-1: the mode guard (§6).
  - S5-2: deviations 3–5 (§7).
  - Option C: deviations 3, 4, 6, 8, 9 and 10 (§8). Deviations 1, 2, 5 and 7 were approved or ratified earlier.
- **Accepted out-of-CP1 product changes.**
  - The lone `/bin/x > FILE` byte pipeline (S5-3, §9).
  - `${NAME}`/`${#NAME}` expansion (S5-4a, §10).
  - The builtin `cat` invalid-UTF-8 fix (S5-4a, §10).
  - The `httpd` `runtime_engine` value change was approved as one of the listed R hunks (§11).
- **Plan document.** The spec.md digest drift is acknowledged; it is diff-verified as the §4 LF amendment only (§13).

Unchanged by this approval: everything in §15 “CP2 does not”. Quarantine, canonical packaging, release signing and rollout stay **BLOCKED** under [containment](reports/rollout-containment.md). The CBX acceptance rows stay pending. The approved bytes are the §3 hashes. At recording time they matched the disk for all 64 source files; `CHANGELOG.md` differs only by the doc-only edits above.
