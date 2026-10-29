---
phase: 7
title: "Ripgrep-derived VFS search and structured matches"
status: completed-private-evidence
priority: P1
tier: thinking
dependencies: [3, 4]
---
# Phase 07: Ripgrep-based search

## Overview and requirements

Deliver CBX-19/20 using upstream search components, a bounded Cellos I/O path and direct records. Keep existing grep's contract distinct; renaming bounded ERE-lite matching is not this phase.

## Architecture and related files

Add search modules/dependencies under `cells/tools/cellbox`; retain reusable pure matcher code only where compatible. Adapt admitted VFS clients/read/directory-handle APIs in `libs/ostd` or the selected runtime under P01's reviewed scope. Inspect `libs/text-engine/src/grep*` for reuse boundaries, not as a substitute for the selected ripgrep functionality. Update Cargo lock/notices and command docs for pinned imports.

## Implementation steps

1. Use P01's pinned upstream matcher/searcher/ignore feature set and runtime route. Do not require mmap/PCRE2 or unconstrained parallel walks. If target dependencies fail, resolve the exact approved prerequisite rather than silently switching to custom grep.
2. Implement directory/file traversal with scoped handles, bounded chunks, explicit ignore/hidden/binary/symlink semantics and cancellation. Do not cross an unauthorized mount/handle boundary or loop through links.
3. Emit match/context/summary data from search callbacks into typed records. Define path representation, 1-based line number, absolute/submatch byte offsets and byte-versus-UTF-8 content without colon or color reparse.
4. Retain the P01-frozen flags/status surface and truthful access/partial traversal errors. Encode non-UTF-8 content explicitly at JSON boundaries; text rendering may safely escape data but cannot silently alter machine records.
5. Feed record and byte inputs only according to declared signatures; reject unsupported input types. On early `take`/consumer close, stop reading/traversing and release handles.
6. Exercise admitted VFS fixtures larger than the old 64 KiB aggregate bound, mixed ignore/hidden/binary/UTF-8 cases, inaccessible entries and symlink cycles where supported. Compare selected upstream-compatible behavior against a pinned host fixture with identical semantics.

## Success criteria

- [x] Actual target search finds correct byte ranges/lines over multi-chunk files with bounded memory. (S7-4: `rg-fx-big-exact`, a 7 MiB file above the 6 MiB arena, exact lines/offsets; private TCG.)
- [x] Hidden/ignore/binary/link behavior matches the published surface and access denial is reported without privilege bypass. (Hidden/ignore/binary, denied root and a denied child inside a backed directory (`rg-fx-denied-child`, seeded P5 `/srv`) are proven in S7-4. Links: no VFS backend has a link kind; no-follow/cycle behaviour is accepted on vigrep host-fake evidence by user decision 2026-10-24.)
- [x] Match/no-match/error statuses follow documented 0/1/2 policy and partial errors are not silently successful. (S7-3 `rg-*` and S7-4 `rg-fx-binary`/`-denied-root`.)
- [x] Typed output has no ANSI or lossy byte conversion; cancellation stops actual producer work. (S7-3 `rg-escape`/`rg-typed-roundtrip`; S7-4 `rg-fx-enc`, `rg-fx-take-no-leak`, P02 Ctrl-C 130 mid-walk.)

## Verification

Use deterministic matcher/traversal/encoding/error tests for meaningful edges and a throwaway real target tree/pipeline smoke. Measure memory and time on identical fixtures; do not import upstream speed claims. Clean fixture files only after preserving required evidence.

## Assumptions

Pinned std/no_std/runtime boundaries and VFS stream traversal fit the admitted profile. Verify target compilation and actual I/O; host rg JSON smoke is only reference behavior.

## Security and risk assessment

Bound pattern/input/retained result complexity and enforce per-resource scope. Escape control characters only in display to prevent terminal injection. Rollback restores matched code/catalog/dependency/image; search itself is read-only but output may disclose file contents, so authorization is checked before traversal.

## Deviation log

2026-10-20 — **Completion withdrawn; incomplete/blocked.** Custom search/record source was added in `libs/text-engine::search`; the historical 46-host-test snapshot is retained only as unit evidence. Review found no selected ripgrep component import: custom matching/record code is not the required upstream-derived VFS search, even if isolated chunk/offset tests pass. No actual target traversal, ignore/hidden/binary/link, denial/status or producer-cancellation proof closes CBX-19/20. Design consent is preserved; dependency/runtime and owner/allocation admission remain incomplete and CP2 pending/not eligible. All criteria remain unchecked and rollout is blocked under [reports/rollout-containment.md](reports/rollout-containment.md).

2026-10-24 — Entry dossier drafted (no source change): [phase-07-entry-dossier.md](phase-07-entry-dossier.md).

2026-10-24 — **Phase 07 entered (user decision).** The user read the [entry dossier](phase-07-entry-dossier.md) and accepted **every §10 recommendation** (recorded in dossier §13): D70-1..6, D71-1..4 (ripgrep 15.2.0 `e89fff89`, isolated network acquisition into `/tmp`, host reference `rg` built from the same tag, MIT choices), D72-1..4, D73-1..5 (S7-2 CP1 approved as written), D74-1..7 (DV7-1..9 ratified), D75-1..3. D72-5 (S7-1 CP1) is approved only after the S7-0 evidence. Status → `in_progress` (S7-0 first). Rollout containment stays in force. Evidence is private-fixture only. No source-text or wording-pinning tests.

2026-10-24 — **S7-0 admission executed** (no workspace Cargo/source change): [phase-07-s0-admission.md](phase-07-s0-admission.md), evidence `reports/s7-0-evidence/`.
- **Acquisition.** ripgrep 15.2.0 tag (`e89fff89`, good tag signature) and bstr 1.13.1 were acquired in isolation under `/tmp/p07-s0`. Tarball, per-crate-dir and crates.io SHA-256 were recorded; the packages match the tag.
- **Licences.** No in-file licence headers; MIT chosen.
- **Compile.** Stock bare-metal compile fails (exit 101, std boundary). Adapted matcher+regex+bstr subset builds on both x86 profiles.
- **Audit.** Offline `cargo deny` ok/ok (DB `640bf5a9`).
- **Reference.** Host reference `rg 15.2.0 (rev e89fff89ac)` built, SHA-256 `f1ae3ea6…`.
- **Port inventory.** 28 files / 12,183 non-test lines to adapt.
- **Correction.** Host `/usr/bin/rg` is 14.1.0, not 15.1.0 as dossier §3.4 said.

Next: user decision D72-5 (S7-1 CP1, admission record §9).

2026-10-24 — **D72-5 approved (user): S7-1 CP1 as written in admission record §9.**

2026-10-24 — **S7-1 implemented:** `libs/vigrep` ([admission record](phase-07-s0-admission.md) §10). no_std+alloc, `forbid(unsafe_code)` port of the five components plus `vigrep::io` and the serial `vigrep::walk` over `ViSearchTree`. Host tests: 547 unit (+1 upstream-ignored), 21 integration, 7 doc, all pass; 171/186 upstream tests ported, 15 dropped with removed features. Differential: 20 cases / 172 records identical to reference `rg 15.2.0 --json`; intentional differences asserted separately. Bare-metal builds pass for `x86_64-unknown-none` and the Cellos target JSON. `cargo deny`: advisories ok; licences fail only on the pre-existing `ocel-pdf` AGPL. `Cargo.lock` adds only `vigrep` + `bstr 1.13.1`. Criteria stay unchecked until S7-3/S7-4. Next: S7-3 (S7-2 in progress separately).

2026-10-24 — **S7-3 CP1 approved (user):** [phase-07-s3-proposal.md](phase-07-s3-proposal.md) as written, every §9 recommendation (D76-1..D76-18): P1 segmented restart in `vigrep::walk` (D76-1/2), M1 1 MiB arena with `RG_PROFILE` ratified as **DV7-10** (D76-3), packed pending children (D76-4), S1 record bound clarifying DV7-8 (D76-5), K2 256 KiB rg Collected bound (D76-6), latch cadence 1/16 (D76-7), status/renderer/stdin/escaping/path/argv/cache/reporting/runner/size rules D76-8..D76-17. Implementation starts.

2026-10-24 — **D76-3 revised (user): M2, 6 MiB cellbox arena** ([proposal](phase-07-s3-proposal.md) end note). Reason: globset construction for maximal ignore files measured at a 4.45 MB peak, over the M1 512 KiB target. `RG_PROFILE`/DV7-10 limits unchanged. Host: `app-cellbox` 172/172 pass. Runtime: private `qemu-phase04-terminal-test.sh` (KVM) passed 437 checks (155 driver markers) with the 6 MiB arena, no panic. GUI, P03, P02 and kernel AP regressions (§7.2) not rerun for this change.

2026-10-24 — **S7-3 implemented** ([proposal](phase-07-s3-proposal.md) §10). Host tests: text-engine 115, vigrep 590 (1 ignored), app-cellbox 170, service-vfs 94. Private TCG lanes: terminal 437 (with the seven `rg-*` checks), GUI 279, P03 62, P02 48, kernel 39/39 plus 3/3 dir-page markers, G-S1a PASS. cellbox text 2,286,228 B (+955,720). **D76-19 (deviation, user-directed root-cause fix):** the larger ELF made init's `ReadFileGrant` of `/bin/cellbox` (a 512-byte `ReadCap` loop in `backend_bootfs::read_to_vec`) outlast its deadline under TCG and stall single-threaded VFS, so the first spawn in P02 failed. The read now uses 64 KiB chunks; P02 passes. CP2 is pending; rollout is blocked.

2026-10-24 — **S7-3 CP2 approved (user, per the recommendations):** D76-19 is ratified. The SDK single-attempt `OpenRootDir` residual stays recorded and unfixed, the size is recorded without a budget, and the out-of-graph `cargo deny` findings stay out of scope. Rollout remains blocked.

2026-10-24 — **S7-4 CP1 approved (user, D77-1..6) and implemented** ([proposal](phase-07-s4-proposal.md) §5). The fixture is FAT32 `/mnt/sd/rg` on a private NVMe disk (§9's "FAT `/data`" was not a backed FAT volume). Private TCG: terminal 447 (eight `rg-fx-*`), P03 67 (five `rg-*`), P02 48 plus `rg` Ctrl-C 130, P02 kernel 39/39. Deviations for CP2 (§5.3): D77-7 (user) makes `big.log` 7 MiB, above the 6 MiB arena, instead of above the 16 MiB quota; the D77-5 fallback was taken; and the fixture carries `/etc/cellos`, spans the full partition layout and uses names without `-`/`:`. Criteria 1, 3 and 4 are checked; criterion 2 stays open (links, partial denial). CP2 is pending; rollout is blocked.

2026-10-24 — **CP2 approved (user) for S7-1, S7-2 and S7-4**, including the S7-4 §5.3 deviations 1–8 (S7-3 was approved earlier). Every Phase 07 slice is now CP2-approved. The phase stays `in_progress` only because criterion 2 is open: there is no link-kind backend, and partial denial inside a backed directory is not exercised. Rollout remains blocked.

2026-10-24 — **Denied-child fixture (user request).** `tools/cellos-fs-provision --seed` provisions P5 `/srv` with a read-denied `cellos/` between readable `pub/a.txt` and `zz.txt`. `rg-fx-denied-child`: the walk reports `rg: /srv/cellos: permission denied`, searches both readable files and ends with status 2 (`-q` also gives 2). Private TCG: terminal 449 (164 markers), P03 67, P02 48 plus `rg` Ctrl-C 130, P02 kernel lane PASS (3/3 dir-page markers). Criterion 2 now lacks only on-target link evidence ([S7-4 proposal](phase-07-s4-proposal.md) §5.5).

2026-10-24 — **Phase 07 closed (user decision).** Host-fake evidence is accepted for link no-follow/cycle behaviour, because no VFS backend has a link kind (evidence ceiling: an on-target link check needs a link-kind backend, outside Phase 07). All four success criteria are checked and every slice S7-0..S7-4 is CP2-approved. State: completed-private-evidence. Rollout remains blocked under [rollout containment](reports/rollout-containment.md); CBX-19/20 acceptance belongs to Phase 10 qualification.

2026-10-25 — **Hash addendum: recorded 2026-10-25 at closure, post-approval state** (Phase 01 closure dossier gap C2-c). Doc-only. This addendum adds no scope or approval. The CP1/CP2 decisions above (D71–D77, CP2 2026-10-24) remain the binding records. These SHA-256 values bind the current bytes so that later drift is detectable. They are **not** the bytes on the approval date: the shared files `typed.rs` and `libs/api/src/services/ipc.rs` also carry later approved P08/P09 hunks. Tree digest command: `git ls-files --cached --others --exclude-standard DIR | LC_ALL=C sort | xargs sha256sum | sha256sum`.

| Approved proposal / record | SHA-256 |
|---|---|
| [phase-07-s0-admission.md](phase-07-s0-admission.md) | `a152acbb87a03ac5a7aae7df3b3b0ca4efc1d95f7097543f92099972bc985212` |
| [phase-07-entry-dossier.md](phase-07-entry-dossier.md) | `066362587b719074134c5cf95531c45a773d670ec5d7250e349cb0e1b8b09dc8` |
| [phase-07-s3-proposal.md](phase-07-s3-proposal.md) | `7f35cf804ff3566089523ba0f983feda3184d01f3c72f177e99e09de0f3b3ce4` |
| [phase-07-s4-proposal.md](phase-07-s4-proposal.md) | `b82e8bb5639ec789c13161f1e6ee54830ee6f0c8af5ad36bec97dc6d0477f0f5` |

| Key source file | Bytes | SHA-256 |
|---|---:|---|
| `libs/vigrep/` tree (47 files) | — | `e6eabddbe210bfc151caf09ec00bbf2ea03eb75ccb6cd2358fe719561d28223e` |
| `libs/vigrep/Cargo.toml` | 1520 | `dd0eeebc9c7574f2c9da20d154996df35b40e0d2f8095e5c3313fc95c59c2aa1` |
| `libs/vigrep/PROVENANCE` | 10362 | `42ce5cd101fb12e526e6de172124babade9a56f1b31ecd9ed16c77545ea0d5dc` |
| `libs/vigrep/THIRD-PARTY-NOTICES` | 1214 | `7c4f6cf153e08656f7b943ee953f770277001797e5939e905a59dc7b32818739` |
| `libs/vigrep/LICENSE-MIT` | 1081 | `0f96a83840e146e43c0ec96a22ec1f392e0680e6c1226e6f3ba87e0740af850f` |
| `cells/tools/cellbox/src/rg.rs` | 8660 | `9d0f112be1de8095372a998405c4351eb520914ddfbe7e474ec5bde15a033fec` |
| `cells/tools/cellbox/src/rg/args.rs` | 13607 | `e2426d3dfc6e006510fa8516835cf2cc6dfe877161e1fe523eeabbc6a0e3e939` |
| `cells/tools/cellbox/src/rg/source.rs` | 23507 | `a285866b69efc02e97f0ced694dcd98187cc7bb82d8a0e59718f04ef76ecd88b` |
| `cells/tools/cellbox/src/rg/text.rs` | 4954 | `682dd62997148f1034466a90c7f955732e7c200327d35c388b40ef1f704df66a` |
| `cells/tools/cellbox/src/rg/tree.rs` | 6396 | `369be8627c9628c562c80c4655711796d0954041f3b99beae5cec5e59e74c288` |
| `cells/tools/cellbox/src/rg/tests.rs` | 59639 | `ecc9b6fa14a0af7da398b13f74738dbc735e868dca2bf7cf3877fcfab5f1a146` |
| `cells/tools/cellbox/src/typed.rs` (shared) | 104882 | `88fe26dbe72884c97f176c9b547b11fc4835d319eeaddac39e593a1cfd12bd05` |
| `cells/tools/cellbox/src/native_rows.rs` | 11445 | `3cfc9b39bc6f37a4746278f35d11536bcfa7d98e3675c102b7e6229f4c0fc40e` |
| `libs/ostd/src/clients/vfs.rs` (shared) | 13290 | `dff8f69fd56b70eb1ac98f0fa3a80dd5410e44b23ab61a137ddbe9aa6b900f97` |
| `libs/api/src/services/ipc.rs` (shared: `ListAtPage` 31 / `DirPage` 11 / `FileState` 12) | 48085 | `ca8916570450f794c427d8519efb1e275b686a9e3c90d498a831fc5d936b0072` |
| `cells/services/vfs/src/dispatch_dirs.rs` | 7536 | `8ef1f683b95a6640614865f82b657dace0c7183fb8e2bf2cfccd16113286b60e` |
| `cells/services/vfs/src/backend_bootfs.rs` (D76-19 64 KiB reads) | 11836 | `dd3669dcb1d33d965d4392f34a70b71df62a1a903d4c68cc45c53eed7edb3bc7` |
| `cells/tests/phase04-terminal-driver/src/rg.rs` | 17033 | `fdf24cb99d4eb28d2a3be995be685795ea365b82db0a38ebf036e7a621b10d37` |
| `scripts/phase07_rg_fixture.py` | 7989 | `58c143486a87ec9a5df8d0eb42647eb3687af5b15abf6eacad9aac972fd8d67a` |
