# Phase 01 closure dossier: gate reconciliation against current repo evidence

**Prepared 2026-10-25. Read-only reconciliation; this file is the only write.** I edited no source, built nothing, ran no QEMU, and changed no other plan file. Commands run: `sha256sum`, `git status/diff/log`, `cargo tree --offline --locked`, `cargo deny --offline --locked … check` (reads only, no build), `readelf`, `size` and `ls`. [Phase 01](phase-01-baseline-contracts.md) stays `blocked` until the user decides on the gaps in §6. This dossier only proposes; it does not tick any checkbox.

Phase 01 was blocked on 2026-10-20 because its gate inputs were missing: owner handoffs, codec/namespace allocation, provider/profile qualification and dependency target proof. Phases 02–09 later supplied most of those inputs one delta at a time, each under its own user CP1/CP2. This dossier maps that later evidence back onto the four Phase 01 criteria and eight steps, and lists what is still missing.

## 0. Verdict summary

| Phase 01 criterion | Verdict | One-line reason |
|---|---|---|
| C1 Every suite name/caller has a disposition and an acceptance scenario; no silent deletion; no fake rg | **GAP (catalog reconciliation)** | All 60 builtins and the 17 standalone tools have a ledger disposition (host-tested), and `rg` is a real ripgrep adaptation. However, the catalog itself is stale: it has no §2 rows for `rg`/`edit`/`ssh`/`completion`, no rows for the typed stages, and no `/bin/cellbox-ssh` edge. Some of its text contradicts the code. The `export` row's denial scenario is unimplemented. |
| C2 Each delta has an exact scope, digest and owner plus a CP1 state; ABI edits wait for CP1; final cutover waits for CP2 | **GAP** | Every implemented delta has a recorded CP1, some of them retroactive. Four Phase 04 slices have CP1 but no CP2 record. `contract-deltas.md` is stale (CP2 shown as "Pending", owners shown as "Missing", old digests). P07–P09 are bound by decision IDs, not by a SHA-256 manifest. Several open deltas have no CP1, but those correctly remain refused or deferred. |
| C3 Ratatui still selected; port feasibility and SSH choice backed by exact evidence | **MET for selection/feasibility; GAP for policy closure** | Ratatui, ripgrep, Kibi and Sunset are pinned with provenance, and the license/advisory checks pass for the cellbox graph. The `cargo deny` bans check **fails** for the cellbox graph (wildcard path deps), and no phase recorded that. `dependency-review.md` §5 does not reflect the as-built Sunset profile. CBX-02 is still `pending`. |
| C4 No physical/remote/production gate or legacy lock released by a completion label | **MET** | The three rollout guards are in source. The canonical image and ISO kernel hashes are unchanged. All 38 lock relations remain. Phase 10 is blocked, and the Tier-3 guest scenario is blocked. |

Step 8 (footprint): the baseline is **MET** (provenance-limited). The before/after comparison is **open by design** in Phase 10. Portfolio registration is a **GAP** because the portfolio Cellbox section is stale.

**Update 2026-10-25 (closure pass).** The verdicts above describe the state before that pass. §7 lists which gaps are now closed and which remain.

## 1. Criterion C1 — command/caller dispositions and acceptance scenarios

### 1.1 Evidence

- **Registry.** `cells/tools/cellbox/src/executor.rs:940-948` `BUILTINS` has 60 names: the 56 legacy completion names plus `edit`, `ssh`, `completion` and `rg`. `[` and `.` are dispatched at `executor.rs:2196,2201` but are not in `BUILTINS`, the same as before.
- **Ledger.** `cells/tools/cellbox/src/ledger.rs` has 77 entries (60 builtin + 11 sys-tool + 6 net-tool), one disposition each. Host tests require the builtin entries to equal `BUILTINS` (`ledger.rs:230-234`) and require the catalog §3.1 table to be byte-equal to the ledger rendering ([catalog §3.1](command-catalog.md), lines 125-214). Every `InSuiteEffectDenied`/`InSuiteRefused` entry names a runner check.
- **Scenarios.**
  - The 56 legacy names plus `[` and `.` have catalog §2 rows with a scenario (58 rows, counted by script).
  - `rg`/`edit`/`ssh` have exact contracts and required acceptance in [command-contracts.md](command-contracts.md) §2.4/§3.4/§4.4. They were exercised by the P07/P08/P09 lanes: terminal 459 incl. `rg-*` and 10 `edit-*`; GUI 290 (EDIT-0..5); Phase 09 lane 103/103 (see [plan.md](plan.md) table).
  - The typed stages `select|where|sort-by|take|to|from` are specified in [spec.md](spec.md) §2, with acceptance in §5 AC-1..3 and P03 runner evidence.
- **No silent deletion, no fake rg.**
  - `kill`/`snapshot` are explicit refusals (`InSuiteRefused`). `shutdown`/`blktest` are observed denials. The `applets` module deletion, the removal of fake GUI responses and the removal of the desktop's fake power actions are recorded in [catalog §3.1](command-catalog.md), the [S4a CP2](phase-04-s4a-cp2-dossier.md) and the [C5 CP2](phase-04-c5-cp2-dossier.md).
  - `rg` is `libs/vigrep`, an adaptation of ripgrep 15.2.0 `e89fff89` (`libs/vigrep/PROVENANCE`). Legacy `grep` is retained as a separate builtin.
- **Callers.**
  - Init autoloads `/bin/cellbox` (`config/autoload.toml:23-24`).
  - The terminal may launch `/bin/cellbox` only (`kernel/src/loader/launch_profile/profiles.rs:151-156`).
  - cellbox may launch `/bin/hotswap`, `/bin/shutdown`, `/bin/cellbox-ssh` (NET_CLIENT) and the reviewed user targets (`profiles.rs:63-80`).
  - No `/bin/shell` reference remains in launch profiles, init, the desktop or `package-desktop-x86.py`, and `cells/tools/shell` is gone.

### 1.2 Gaps (exact)

| # | Missing item | Evidence | Smallest closing action |
|---|---|---|---|
| C1-a | Catalog §2 has no row (disposition + rights + scenario) for `rg`, `edit`, `ssh`, `completion`. `completion` has only a ledger `InSuite` entry; its scenario exists only in Phase 09 ([phase-09-ssh-client.md:107](phase-09-ssh-client.md), S9-6). | Script comparison: builtins without a §2 row = `edit, ssh, completion, rg` | Add four §2 rows that point at command-contracts §2–§4 and the P07/P08/P09 lane checks. For `completion`: `usage: completion` → 2, no record → 1, otherwise 0 (`executor.rs:3446-3457`). |
| C1-b | The typed stages (`select where sort-by take to from`) and the `--records` modes of `ps/ls/top` have no catalog or ledger disposition. They are classified in `typed.rs:181-209`, outside `BUILTINS`, so the ledger cannot catch their loss. | `typed.rs:108,181,206` | Add a catalog subsection that lists them with a reference to spec §2/§5. A ledger surface for typed stages would be a source change; propose it separately if wanted. |
| C1-c | The `/bin/cellbox-ssh` launch identity and the `cellbox → /bin/cellbox-ssh` Path edge (NET_CLIENT ceiling, D79-9) appear nowhere in catalog §3/§4.2/§6. The terminal → `/bin/cellbox` and init → `/bin/cellbox` edges are described only as "proposed". | `profiles.rs:70-75,151-156`; `grep cellbox-ssh command-catalog.md` = 0 | Add the as-built edges to §4.2. Mirror the `hv` precedent in §3.1: `ssh` is `InSuite` as a launcher whose authority is the reviewed edge. |
| C1-d | Catalog text is stale. §5 (line 280) says "`edit` and `ssh` still have **no dispatch/backend path**". §6 (lines 285-288) describes the `/bin/shell` builder array and says no source change was made. §2 "observed" columns cite the deleted `cells/tools/shell/src/*`. | [command-catalog.md](command-catalog.md) lines 280, 285-288, 25 | Doc-only revision: update §5 and §6 to the as-built state. Mark the §2 observed columns as the historical 2026-10-20 baseline. |
| C1-e | The `export` row's scenario ("denied key yields nonzero and no mutation") is **unmet and unimplemented**. The as-built `export` still discards the `ConfigClient::set` result (`cells/tools/cellbox/src/shell.rs:145-150`). `service-config` applies `Set` with no authorization (`cells/services/config/src/main.rs:86-90`). The ledger marks `export` as plain `InSuite`. | Source lines cited | Choose one: **(1)** a Q-right (Config per-key) CP1 proposal for service-config, or **(2)** an explicit user disposition: "legacy ungated `export` retained; Q enforcement deferred; scenario deferred to P10". |
| C1-f | Canonical image recipes do not install `/bin/cellbox-ssh`. Only `scripts/sign-policy.py` and the private P04/P09 runners name it. `scripts/package-desktop-x86.py:30-40` (refused at entry) also lacks it. | `grep -rl cellbox-ssh scripts` | Record it as a Phase 04 criterion 4 / Phase 10 packaging item. Not a Phase 01 source change. |
| C1-g | Catalog digest drift. The current `command-catalog.md` SHA-256 `20fda2214e4ceac668be9cc6b16d89d3de6ef11a6da9a1f44c6f08858acf2e66` is recorded nowhere. The last recorded identity is `6f6766b9…` in the [P06 CP2 dossier](phase-06-cp2-dossier.md):113; the CP1-frozen identity is `6db8f1d1…`. | `sha256sum` on 2026-10-25 | After C1-a..d, record the new digest in the reconciled delta index (§2.3) and get user acknowledgement. |

## 2. Criterion C2 — delta scope, digest, owner, CP1/CP2

### 2.1 Reconciled delta ledger

Owner: unless stated, the integrating owner is the assistant, with the user as approver. An explicit named handoff exists only where marked **H**. Dates are user approvals recorded in the cited file.

| Delta | Exact scope (as built) | Digest / binding file | Owner | CP1 | CP2 |
|---|---|---|---|---|---|
| Engine v1 | Typed Value/Rows/Frames, pre-effect pipeline validation, status rules | [spec.md](spec.md) frozen `d318e63f…`. Amended §4 LF (2026-10-23, diff-verified in [P05 CP2](phase-05-cp2-dossier.md) §13) and §1/§2 D63 (P06). Current `cde24178…` (recorded at [P06 CP2](phase-06-cp2-dossier.md):112) | implicit | design consent 2026-10-20; implementation under P03 2026-10-21 | P03 2026-10-21; amendments ratified in P05 CP2 2026-10-23 and P06 CP2 2026-10-24 |
| D01 child context / prepared cwd (P02) | Child-context publication, grant slots Stdin 0 … Cwd 4 (`libs/ostd/src/child_context.rs:11`; slot 5 `Completion` is P09, Frontend 8 is [INFERENCE] P05 S5-3c), VFS cwd binder `VfsOp` 1–3 (`libs/api/src/abi/cell_session.rs:115-118`) | Annex `73bc7483…ae67`, snapshot `2aaa6297…7e3b`, A1 `ae6be1c3…`, A2 ([CP1 record](phase-02-cp1-approval.md)) | **H** "Giao slice Phase 02" | 2026-10-21 | 2026-10-21 baseline ([dossier](phase-02-cp2-dossier.md)); 2026-10-25 phase closure |
| D02 stream/session (P02) | `CellSessionV1 = 262` (`syscall.rs:758`), session selectors 0–22 | Same annex | **H** | 2026-10-21 | 2026-10-21 / 2026-10-25 |
| P03 A3 external byte stages | Shell external byte adapters | [phase-03-external-bytes-amendment.md](phase-03-external-bytes-amendment.md) (no SHA) | implicit | 2026-10-21 ("Duyệt A3") | 2026-10-21 (P03 CP2) |
| P03 A4 `QueryChild` | Kernel op 30 (`cell_session.rs:87-91`) | [phase-03-child-status-amendment.md](phase-03-child-status-amendment.md) (no SHA) | implicit | 2026-10-21 | 2026-10-21 |
| P04 S1 boot identity | `/bin/shell` → `/bin/cellbox` launch rows and boot ceiling, identical ceiling | [entry dossier](phase-04-entry-dossier.md) §2–§5 (no SHA) | implicit | 2026-10-21 ("Duyệt S1") | **none found** |
| P04 S2-lite ledger | `ledger.rs` + denial checks (no right change) | [S2 authority proposal](phase-04-s2-authority-proposal.md) | implicit | 2026-10-21 | **none found** |
| P04 S2c-min lifecycle | Raw 502 accepted only for `RAW_SHUTDOWN_CALLERS`; supervisor `SNAPSHOT_CALLER_NAMES` emptied; `cellbox → /bin/shutdown` edge | [S2c proposal](phase-04-s2c-lifecycle-proposal.md) | implicit | 2026-10-21 (Option B) | **none found** |
| P04 S3 initial | `terminal → /bin/cellbox` edge (`profiles.rs:151-156`), TerminalHost. The TerminalHost part was superseded by S5-3c | [S3 proposal](phase-04-s3-gui-terminal-proposal.md) | implicit | 2026-10-21 | No hunk-attributed CP2. `profiles.rs` is hashed whole-file in [S3-output CP2](phase-04-s3-output-cp2-dossier.md):41, but that approval is scoped to S3-output |
| P04 S3-output / GUI proof / tty-merge | `ProcessOutput`, private signing exception, `CELLBOX_STREAMS=merged` | [S3-output CP2](phase-04-s3-output-cp2-dossier.md), [S3-GUI CP2](phase-04-s3-gui-cp2-dossier.md) (9 files) | implicit | 2026-10-22 | 2026-10-22 |
| P04 OOM local sink | Streaming terminal sink | [OOM CP2](phase-04-oom-cp2-dossier.md) `588fff65…` | implicit | 2026-10-22 ("A") | 2026-10-22 |
| P04 S2-K secret namespace | BootFS fallback fail-closed | [S2-K CP2](phase-04-s2k-cp2-dossier.md) `4447cc50…` | implicit | 2026-10-22 | 2026-10-22 |
| P04 S2-N net/DNS/listen | Coarse V1 rights (D-N1) | Annex `d774b9c5…6255`; [CP2](phase-04-s2n-cp2-dossier.md) `5a7fa43d…` | implicit | 2026-10-22 | 2026-10-22 |
| P04 S2-T/W telemetry/mutate | Edge-bounded X/M/W | Annex `57869333…ad6d`; [CP2](phase-04-s2tw-cp2-dossier.md) `1ea82014…` | implicit (cited later as the owner precedent) | 2026-10-22 | 2026-10-22 |
| P04 S4a retirement | `applets` deleted, recipes, `cat` status | [S4a CP2](phase-04-s4a-cp2-dossier.md) `901ae31a…` | implicit | 2026-10-22 | 2026-10-22 |
| P04 C5 desktop parity | Files on VFS, Files ceiling + `VfsMutate`, desktop removals | [C5 CP2](phase-04-c5-cp2-dossier.md) `131ec5a5…` | implicit | 2026-10-22 | 2026-10-22 |
| P05 S5-1 vitui | Ratatui backend `=0.1.0/=0.3.0` | [entry dossier](phase-05-entry-dossier.md) §3 | implicit | 2026-10-22 | 2026-10-23 ([dossier](phase-05-cp2-dossier.md) §16) |
| P05 S5-2 `cellos-vt1` | GUI emulator | [S5-2 proposal](phase-05-s2-vt-screen-proposal.md) | implicit | 2026-10-22 | 2026-10-23 |
| P05 S5-3c frontend | Session selectors 23–28 (`cell_session.rs:101-102`), `FrontendHost` | Annex `41e7f0b1…dea6` | implicit | 2026-10-23 | 2026-10-23 |
| P05 S5-3 / S5-4 | Proof consumer (removed), hostile-input proof | [S5-3](phase-05-s3-proposal.md), [S5-4](phase-05-s4-proposal.md) | implicit | **retroactive** 2026-10-23 | 2026-10-23 |
| D04 telemetry (P06 S6-1) | `TelemetrySnapshot = 430` on allowlist bit 55 (`syscall.rs:614,1140`) | [entry dossier](phase-06-entry-dossier.md) `01abc820…` (post-results hash in [P06 CP2](phase-06-cp2-dossier.md):108) | **H** D60-6 | 2026-10-23 | 2026-10-24 |
| P06 S6-2/S6-3, S6-4, S6-5 | Cell `top`, Frames, Ratatui `top`, load fixture | `da65dde9…`, `85159a2f…`, CP1 `88602bb0…0fce` | **H** D60-6 | 2026-10-23 | 2026-10-24 |
| P06 extra fixes U/Z/V, `$(…)` status | Console prefix, `sleep`, VFS idle, substitution status | [P06 CP2](phase-06-cp2-dossier.md) | implicit | none (fixes outside CP1) | 2026-10-24 |
| P07 S7-1 vigrep | ripgrep 15.2.0 adaptation | [S7-0 admission](phase-07-s0-admission.md) §9 (D72-5) | **H** D70-4 | 2026-10-24 | 2026-10-24 |
| P07 S7-2 VFS read path | `ListAtPage` request 31 → `DirPage` response 11; `FileHandleState` → `FileState` 12 | [entry dossier](phase-07-entry-dossier.md) §6/§14 (D73-1..5) | **H** D70-4 | 2026-10-24 | 2026-10-24 |
| P07 S7-3 / S7-4 | `rg` builtin, 6 MiB arena; fixture | [S7-3](phase-07-s3-proposal.md) D76-1..19; [S7-4](phase-07-s4-proposal.md) D77-1..7 | **H** D70-4 | 2026-10-24 | 2026-10-24 |
| D03-save (P08) | VFS `Observe` 33, `SaveBegin` 34 … `SaveAbort` 39 (`ipc.rs` tests `\x21`,`\x22`,`\x27`); replies `Observed` 13, `SaveId`, `SaveOutcome` 15 | [phase-08-s0-proposal.md](phase-08-s0-proposal.md) D78-1..3 + D03 §3.1 (decision IDs, no SHA) | stated as an "owner-approved delta" (`:40`), no name | 2026-10-24 | S8-1..4 2026-10-24; S8-5 2026-10-25 |
| D03-trust (P09) | VFS `TrustLookup` 40, `TrustChallenge` 41, `TrustCommit` 42 (`ipc.rs` tests `\x28/\x29/\x2a`); replies `Trust` 16, `TrustChallenged` 17; SessionV1 29–31 (`cell_session.rs:114`); control kinds 9/10; `SaveOutcome::CommittedDurable` | [phase-09-s0-proposal.md](phase-09-s0-proposal.md) D79-10..13 (decision IDs, no SHA) | implicit | 2026-10-25 | S9-3, S9-4a/b/c 2026-10-25 |
| P09 launch/entropy/completion | `/bin/cellbox-ssh` NET_CLIENT edge (D79-9 revised); `GetRandom` in the cellbox syscall list; child slot 5 `Completion` (`kernel/src/task/child_context.rs:9`); `cellbox.ssh.completion` v1 80-byte frame | Same proposal D79-9, D79-15 | implicit | 2026-10-25 | S9-1, S9-2, S9-6 2026-10-25 |

### 2.2 Open deltas — no CP1, correctly blocked or deferred

| Delta | Current behavior | State |
|---|---|---|
| D01 lifecycle owner (supervisor `KillTarget`, `ResetSystem`, `SnapshotTarget`, `ReplaceService`) | `kill`/`snapshot` refuse; power-off only via `/bin/shutdown` (name-keyed legacy gate); `hotswap` legacy name check | No CP1; deferred by S2c-min Option B; effects refused, not ungated |
| Q Config per-key grant | `export` ungated (C1-e) | No CP1 and no recorded user disposition → **GAP C2-e** |
| N destination/DNS attenuation beyond coarse V1 | Coarse per-Cell rights | Accepted by the user (D-N1) |
| KMS credential `A` / S9-5 publickey | `--auth publickey` refused | Deferred by the user (D79-14) |
| Production trust bootstrap/issuer | Fixture-seeded `/srv` store only (D79-10) | Open owner decision, recorded |
| Trust durability | First trust persists on `Committed` (synced, not crash-proof), not `CommittedDurable` | Revised by the user (D79-12) |
| D04 clock units for `uptime`/`date` | Still assume 10 MHz | Open defect D60-3 (own CP1 pending) |

### 2.3 Gaps (exact)

| # | Missing item | Smallest closing action |
|---|---|---|
| C2-a | P04 **S1, S2-lite, S2c-min** (and the S3-initial launch edge) have CP1 but no CP2 record. The [S4a CP2](phase-04-s4a-cp2-dossier.md):153 explicitly excludes "S1, … S2-lite, S2c … S3 … hunks". This contradicts [plan.md](plan.md):36 and [phase-04](phase-04-multicall-cutover.md):96 ("every Phase 04 delta CP2-approved"). S1 and S2c-min are authority deltas: boot identity, kernel raw-502 gate, supervisor snapshot names. | One CP2 mini-dossier in the established format. Hash the current `profiles.rs`, `tests.rs`, `boot_ceiling*`, `kernel/src/task/syscall.rs` (`RAW_SHUTDOWN_CALLERS`), supervisor `main.rs` and `ledger.rs`, and cite the existing runner checks (`s2-builtin-shutdown-denied-shell-survives`, `s2-kill-builtin-refused-no-ipc`, `s2-snapshot-builtin-refused-no-ipc`, `s2-blktest-raw-block-denied`, the S1 48/48). Then get user CP2 (no new execution needed), or else correct the plan/phase-04 claims. |
| C2-b | The [contract-deltas.md](contract-deltas.md) approval table (lines 56-62) is stale: CP2 shows "Pending/not eligible" for all scopes, owners show "Missing" for Engine/D03/D04, and the digest manifest (lines 17-23) holds the frozen `spec.md`/`command-catalog.md` digests. | Add a reconciled index to contract-deltas.md that points to §2.1 above, with current digests: spec `cde24178…906a`, catalog `20fda221…2e66` (after C1 revision: the new value), unchanged `abi-session-launch.md` `422df04c…`, `abi-vfs-telemetry.md` `0edda717…`, `command-contracts.md` `4ff1c3be…`. |
| C2-c | P07–P09 CP1/CP2 are bound by decision IDs, file sets and artifact provenance (e.g. `reports/s9-close-evidence/provenance.sha256`: cellbox/kernel/ISO/kernel_fs hashes). There is no SHA-256 of the approved proposal text or of the approved source files. P04–P06 dossiers have both. | Doc-only CP2 addendum per phase that hashes the current proposal files and the named source file sets at their approved state. Use the established S4a "digest before status amendment" convention. |
| C2-d | The owner handoff is explicit only for P02 ("Giao slice Phase 02"), P06 (D60-6) and P07 (D70-4). It is implicit for P03, P04 (except as cited), P05, P08 and P09. | Ask the user for one retroactive line: "assistant was integrating owner for P03–P05, P08, P09 file sets under the recorded CP1s". |
| C2-e | The Q Config delta has neither a CP1 nor a recorded deferral (see C1-e). | Same as C1-e. |
| C2-f (recorded exception, no action) | Edits before CP1 on 2026-10-20 (withdrawn/quarantined, [rollout-containment.md](reports/rollout-containment.md)). Retroactive CP1 for S5-3/S5-4. P06 fixes landed outside CP1 and were CP2-ratified. | None needed beyond noting them in the C2-b index. |

**Assessment of the forward-looking rule.** "Final cutover remains blocked until CP2" holds in practice. No delta in C2-a has reached canonical cutover, because Phase 04 criterion 4 and rollout are blocked (§4). The gap is documentation and approval, not an escaped rollout.

## 3. Criterion C3 — Ratatui, port feasibility, SSH choice, licenses

### 3.1 Evidence

- **Ratatui.** `libs/vitui/Cargo.toml:16-17` pins `ratatui-core = "=0.1.0"` and `ratatui-widgets = "=0.3.0"` with `default-features = false`; the facade crate is excluded by comment. `Cargo.lock:3012,3032` match. P05 entry D5-1 kept the pins, and a target probe was built and then removed ([phase-05](phase-05-ratatui-terminal.md):54). Both packages are in the `app-cellbox` x86_64-unknown-none tree (below).
- **ripgrep.** ripgrep 15.2.0 at `e89fff89ac9af12e8d4ce9d5fd07beb408ca730f` (signed tag). Components: grep-matcher 0.1.9, grep-regex 0.1.14, grep-searcher 0.1.17, globset 0.4.19, ignore 0.4.29, bstr `=1.13.1`. Evidence: [dependency-review §4](dependency-review.md) 2026-10-24 addendum; [S7-0 admission](phase-07-s0-admission.md). Adapted in `libs/vigrep` with `PROVENANCE`, `THIRD-PARTY-NOTICES` and `LICENSE-MIT`. The stock no_std failure and the adapted compile on both x86 profiles are recorded.
- **Kibi.** Reference-only revision `1c33ca09…`. `libs/text-engine/src/editor.rs:19` says "Ideas, not code, follow Kibi … no Kibi source is copied", so no attribution file is needed.
- **Sunset.**
  - `third_party/sunset/PROVENANCE` records upstream `mkj/sunset` commit `62b98f3996c88cf920cac1653e1c9e15ddf0d7d0`, tree `c37df41e…`, sunset 0.6.0 / sshwire-derive 0.3.0, 0BSD.
  - `PATCHES/upstream-files.sha256` and `PATCHES/cellos.diff` hold patches 0001–0005: entropy source, cellbox KEX profile, channel requests/EOF, event pairing, rekey policy.
  - `libs/vissh/Cargo.toml:12` uses features `alloc, openssh-key` only.
  - Russh v0.60.3 was rejected with recorded reasons ([dependency-review §5](dependency-review.md)).
  - The S9-1 CP2 (2026-10-25) covers the vendoring.
- **Cellbox target graph.** Command: `cargo tree --offline --locked -p app-cellbox --target x86_64-unknown-none -e normal,build` (also run with `--all-features`). Result: 161 unique lines; includes `ratatui-core 0.1.0`, `ratatui-widgets 0.3.0`, `bstr 1.13.1`, `sunset 0.6.0`, `ssh-key 0.7.0-rc.11`, `vigrep`, `vissh`, `vitui`. **No `crypto-bigint`, no `ocel*`, no `getrandom`, no `rsa`.**
- **Known workspace failures are outside the cellbox graph.** `cargo tree -i crypto-bigint@0.5.5` shows the chain `elliptic-curve 0.13.8` → `p256` → `attestation`, and `embedded-tls` → `service-net`. `ocel-pdf` (`AGPL-3.0-or-later`, `cells/services/ocel-pdf/Cargo.toml:5`) is a separate crate.
- **Scoped audit, run 2026-10-25.**
  - Command: `cargo deny --offline --locked --exclude-dev --manifest-path cells/tools/cellbox/Cargo.toml check advisories licenses bans sources`.
  - Advisory DB: `~/.cargo/advisory-dbs/advisory-db-3157b0e258782691` at `21d909e4` (2026-10-24).
  - Result: **`advisories ok, bans FAILED, licenses ok, sources ok`** (exit 2).
- **Workspace audit, same flags without `--manifest-path`.** Result: `advisories FAILED, bans FAILED, licenses FAILED, sources ok`. The failures are RUSTSEC-2026-0387 (crypto-bigint 0.5.5), the ocel-pdf AGPL license, and wildcards in app-cellbox, vissh, ocel, cellos-kernel, cellos-boot-config and app-c2c-render.

### 3.2 Gaps (exact)

| # | Missing item | Smallest closing action |
|---|---|---|
| C3-a | **`bans` fails for the cellbox graph.** `deny.toml` has `wildcards = "deny"`, and five Cellbox-added path dependencies have no version: `cells/tools/cellbox/Cargo.toml:29,34,36,38` (`terminal-sdk`, `vitui`, `vigrep`, `vissh`) and `libs/vissh/Cargo.toml:12` (`sunset`). The sibling path dependencies (`ostd`, `api`, `text-engine`) do carry versions. No phase ran the bans check: P05/P07/P09 ran only `licenses advisories`. | Add `version = "0.1.0"` to the four cellbox deps and `version = "0.6.0"` to `sunset` in vissh. Then rerun the scoped deny and expect all four `ok`. This is a source change and needs user admission. The alternative, `allow-wildcard-paths`, is a policy change. |
| C3-b | [dependency-review.md](dependency-review.md) §5 still describes the 2026-10-20 proposal: getrandom `=0.4.3` custom backend, `ssh-key` with `encryption`. As built, getrandom is replaced by an installed entropy source plus `spin` (PATCHES 0001), `ssh-key` runs without `encryption`, publickey is deferred (D79-14), and `bcrypt-pbkdf` is absent. | Append a 2026-10-25 S9-1 as-built addendum to §5, as was done for §4, linking PROVENANCE/PATCHES and the S9-1 CP2. |
| C3-c | [acceptance.md](acceptance.md) CBX-02 is still `pending`. | After C3-a/b, ask the user to mark CBX-02 met for the cellbox graph, with the scoped deny output as evidence. Workspace failures stay out of scope (ocel-pdf, crypto-bigint via service-net/attestation). |

Selection and feasibility themselves are **MET**: Ratatui is still selected and pinned, ripgrep was adapted and proven on target, and Sunset was chosen and vendored with exact provenance, with Russh rejected.

## 4. Criterion C4 — no gate or lock released by a completion label

**MET.** Evidence:

- **Executable guards are present.**
  - `scripts/package-desktop-x86.py:18-24` exits unconditionally with `ROLLOUT BLOCKED` before any signing or image write.
  - `scripts/x86/make-iso-ci.sh:25-33` refuses the default kernel.
  - `scripts/cellos_sign/signing.py:36,106-121,147` refuses `cellbox`/`app-cellbox`/`terminal` unless `CELLBOX_PRIVATE_FIXTURE_SIGNING=1` and a non-dev seed are given. That exception is private-fixture-only, user-approved 2026-10-22. Quarantined hashes are refused unconditionally.
- **Canonical bytes are unchanged.**
  - `kernel/src/embedded-x86_64/kernel_fs.img` SHA-256 is `447d0348…2c`. That equals the quarantined value listed in the [S3-output CP2](phase-04-s3-output-cp2-dossier.md) (2026-10-22) and the 2026-10-21 containment update. The rewrite was a non-Cellbox desktop fixture.
  - `build/x86-iso-root/boot/kernel.elf` is `dbd862d6…` with an mtime before the plan, the same as the [baseline](baseline.md):37.
  - `build/cellos-x86_64.iso` mtime is 2026-10-20 15:36.
- **Locks.** All 38 `executionLock: CELL-CONTRACT-0024` relations are still present (count of `plan.md` files carrying it = 38). The only uncommitted change to `.agents/cell-contract-0024-locks.md` adds the bounded Phase02 slice paragraph and releases nothing.
- **Status labels.**
  - Phase 10 is `blocked`. [rollout-containment.md](reports/rollout-containment.md) release conditions are unchanged.
  - Every phase row in [plan.md](plan.md) says "rollout blocked". Phase 04 criterion 4 (canonical packaging) is open.
  - Phase 09 Tier-3 guest is blocked (D79-2); its criterion 1/2 are partial.
  - No physical Intel, VMX, production or remote claim was found.
- **Wording nit (no gate effect).** `phase-03-typed-pipelines.md` frontmatter says `status: completed`, while its own CP2 line says it "does not lift the rollout quarantine". Optionally normalize it to `completed-private-evidence` to match P02/P05–P08.

## 5. Implementation steps 1–8

| Step | Status | Evidence / remaining item |
|---|---|---|
| 1 Toolchain, identity, user edits | **MET** | `rust-toolchain.toml` is `nightly-2026-05-01`, unchanged. [baseline.md](baseline.md) has the dirty-file manifest. HEAD is `3471028bc`, with later work uncommitted (`git status`); per-phase CP2 dossiers record per-file hashes against HEAD. |
| 2 Enumerate names/callers/images/desktop ops; assign each | **PARTIAL → C1-a..d, C1-f** | Ledger is complete (77) and C5 handles desktop ops. Catalog rows and edges are stale or missing as listed. |
| 3 Freeze catalog, rg/editor/telemetry/SSH semantics, right matrix, capacities | **MET with open items** | [command-contracts.md](command-contracts.md) (digest unchanged `4ff1c3be…`) is implemented by P07/P08/P09 with recorded deviations; [spec.md](spec.md) is amended under CP2. Open: Q right (C1-e); D60-3 clock units; capacities ratified per phase (e.g. D76-3 6 MiB arena, 4096-byte SSH line limit). |
| 4 Typed Value/session/filter/redirect/job contracts | **MET** | spec §1–§3, P03 CP2 (2026-10-21), P06 D63 amendments. Background/mixed forms are rejected before effects (AC-1, P03 lanes). |
| 5 Terminal ownership, endpoints, EOF, cancellation | **MET** | D02 in P02 (CP2 2026-10-21/25); frontend selectors 23–28 in P05. |
| 6 Delta inventory, digests, CP1/CP2 scheduling | **PARTIAL → C2-a..e** | §2.1 table. |
| 7 Pin Ratatui/Kibi/ripgrep/licenses; Sunset vs alternatives; probes removed | **MET; policy closure GAP → C3-a..c** | §3. Probes were removed (P05 probe, P07 `/tmp/p07-s0` isolated). |
| 8 Footprint baseline, fixtures, slice registration | **Baseline MET; comparison deferred to P10; registration GAP** | See below. |

**Step 8 detail.**

- **Baseline** ([baseline.md](baseline.md):240-288): the split subset `/bin/shell` 881,120 B plus `ls/cat/echo/ps/kill` 99,408 B gives **980,528 B in 6 ELFs** (from `kernel_fs.img` `699e1df1…`, 2026-10-20). Signing provenance was unverified.
- **Current**: `target/x86_64-unknown-none/release/cellbox` is **3,077,168 B** (text 2,610,420 / data 159,728 / bss 7,340,296), SHA-256 `7618497c…1fca`, mtime 2026-10-25 19:06. It has **no `__ViCell_sig` section** and no `p09-fault` strings. That makes it **unsigned and unmatched** with the baseline, so it is not a qualified comparison.
- **Text-size trail recorded by phases**: S7-3 2,286,228 → S8 2,354,972 → S9-2 2,535,111 → S9-3 2,583,767. D76-17 says "record, no budget". The bss is dominated by the 6 MiB `rg` arena (D76-3) plus the 1 MiB ostd arena.
- **Retained workers**: the 11 sys-tools and 6 net-tools remain separate ELFs (`RetainedWorker`), so no executable-count saving is claimed.
- **Comparison**: the matched signed before/after footprint is Phase 10's deliverable. It is not a Phase 01 blocker.
- **Fixtures**: [runtime-fixtures.md](runtime-fixtures.md) plus the per-phase private runners and provenance manifests.
- **Registration gap**: the slice is registered in `.agents/cell-contract-0024-locks.md` (bounded Phase02 slice). The `.agents/plan-portfolio.md` Cellbox section (lines 106-136) is stale: it says "SSH/editor/transform command dispatch is incomplete", "Ratatui/ripgrep/Sunset upstream integration is absent" and "No … CP2 was established". **Action:** a doc-only portfolio update to the P02–P09 private-evidence states, with rollout still blocked.

## 6. Closure path (smallest set of actions, in order)

1. **Doc-only, no approval needed to draft:**
   - Catalog revision for C1-a..d, then record its new digest (C1-g).
   - Reconciled index in contract-deltas.md (C2-b).
   - dependency-review §5 addendum (C3-b).
   - Portfolio refresh (step 8).
2. **User decisions:**
   - (a) CP2 for P04 S1/S2-lite/S2c-min/S3-edge via a hash-only mini-dossier (C2-a).
   - (b) Q Config: CP1 proposal or explicit deferral (C1-e/C2-e).
   - (c) Retroactive owner confirmation (C2-d).
   - (d) Optional: hash-bind P07–P09 proposals and file sets (C2-c).
3. **Source change, needs admission:** add versions to the five path deps (C3-a). Then rerun the scoped `cargo deny … check advisories licenses bans sources` and expect four `ok`.
4. **Then:** tick C1–C4 in [phase-01-baseline-contracts.md](phase-01-baseline-contracts.md), set CBX-01/CBX-02 in [acceptance.md](acceptance.md), and set Phase 01 `status` to completed. Rollout, Phase 04 criterion 4 (including C1-f `/bin/cellbox-ssh` packaging) and Phase 10 stay blocked; Phase 01 closure releases none of them.

## 7. Gap status after the 2026-10-25 closure pass

**User decisions recorded 2026-10-25** (relayed by the main agent):

- `export` → draft a CP1 proposal for a per-key Config write right;
- Phase 04's missing CP2 → build a hash-only CP2 mini-dossier for approval;
- the assistant is retroactively confirmed as integrating owner for Phases 03–05, 08 and 09;
- adding `version` fields to the path deps is approved, to fix the cargo-deny bans;
- CBX-02 is to be marked done once the scoped cargo deny passes fully.

Nothing below records an approval the user has not given. The doc-only changes in this pass touched only files under this plan directory; no source was edited.

**Second round of user decisions, 2026-10-25** (relayed by the main agent):

1. **CP2 approved** for P04 S1, S2-lite, S2c-min and the S3 launch edge, per the [mini-dossier](phase-04-s1-s3-cp2-dossier.md) (pre-amendment SHA-256 `8e84ead7…cd1f`), including its deviations 1–7.
2. **Catalog confirmed as frozen** at `command-catalog.md` SHA-256 `f2a82fce61101f9f1dd6d62a75bece735e3eb9129a2295b187c2f49209485339`.
3. **Config CP1 approved:** [phase-01-config-right-cp1.md](phase-01-config-right-cp1.md), option Q1 (a kernel `config_write` right) with every recommendation D-Q2..D-Q7. Agent P01DenyConfig implements it. The `export` denial scenario stays open until its lane evidence and CP2.
4. **Cleanups approved:** the `profiles.rs:148-150` comment (`LEGACY` wording) and the `git rm` of the staged `snapshot_client.rs`. These are recorded here, not performed by this documentation pass.
5. **Config CP2 approved (2026-10-26)** for the Q1 implementation, including deviations 1–2 (see the C1-e/C2-e row below). By that date the item-4 cleanups were in the tree: the `profiles.rs:148-150` comment no longer says "empty child ceiling", and `git status` shows `cells/tools/shell/src/snapshot_client.rs` as a staged deletion (`D`).

### 7.1 New digests (SHA-256, recorded 2026-10-25)

| File | SHA-256 | Note |
|---|---|---|
| [command-catalog.md](command-catalog.md) | `f2a82fce61101f9f1dd6d62a75bece735e3eb9129a2295b187c2f49209485339` | C1-a..d revision; ledger block unchanged (77 rows) |
| [contract-deltas.md](contract-deltas.md) | `b3cf91788c2a9b3200eef1dc7bb7ab740a94ce0ca993df0b1410d344bbfaad3a` | reconciled delta index appended |
| [dependency-review.md](dependency-review.md) | `4ce95ab2b0a20767290ab4a411be85bff2190976293d39df5bddfb05ed08c5c7` | §5 S9-1 as-built addendum |
| [phase-04-s1-s3-cp2-dossier.md](phase-04-s1-s3-cp2-dossier.md) | `8e84ead72f284d5f4b902c090888dd47774f20db3c8056902dfd90c47b5dcd1f` | new; awaiting CP2 |
| [plan.md](plan.md) | `33f960955901b8a1790d5d973e553f9e5e7a0b928363f592ddfd440cdeaf8d9d` | row 04 wording corrected |
| [phase-04-multicall-cutover.md](phase-04-multicall-cutover.md) | `1f84e0a39f8b37c6ebc8c1e961dfa4341bef06c9b687fd5bee0eec35a75c4ca4` | correction entry for `:96` |
| [phase-07-ripgrep-search.md](phase-07-ripgrep-search.md) | `b6089727c31104cc10703dcdfdf2cf246bab07ba1deb8b06d059086ec1b3b7a4` | hash addendum |
| [phase-08-compact-editor.md](phase-08-compact-editor.md) | `afa93892b7a31dd253e48b4b30367cf66656964b286d6c33587ec226a0b53176` | hash addendum |
| [phase-09-ssh-client.md](phase-09-ssh-client.md) | `fce05ad752ad2903141695331ee80e06c46ca09c755714880401a30bdfa7368e` | hash addendum |

The table above lists digests at the end of the first round. The mini-dossier, plan.md, phase-04, contract-deltas and this dossier were amended again in the second round; the digests of the frozen catalog and of the mini-dossier's approved pre-amendment bytes are the ones that bind.

### 7.2 Gap status

| Gap | Status | Evidence / remaining action |
|---|---|---|
| C1-a `rg`/`edit`/`ssh`/`completion` rows | **Closed (doc)** | catalog §2, four new rows after `.` |
| C1-b typed stages / `--records` | **Closed (doc)** | catalog §2 "Typed stages and record modes". A ledger surface for typed stages (a source change) was not proposed |
| C1-c `/bin/cellbox-ssh` edge; init/terminal edges | **Closed (doc)** | catalog §3.1 `ssh` launcher note; §4.2 as-built edge table |
| C1-d stale §5/§6/§2 text | **Closed (doc)** | §5 rows for Terminal, Files, Spotlight and new commands now as built; §6 as built plus a historical baseline; §2 observed columns marked as the historical 2026-10-20 baseline; §3 ceiling wording (`LEGACY`) |
| C1-g catalog digest | **Closed (user, 2026-10-25)** | catalog frozen at `f2a82fce…9339` |
| C1-e / C2-e `export` Q right | **Closed (user CP2, 2026-10-26)** | [phase-01-config-right-cp1.md](phase-01-config-right-cp1.md), option Q1 with D-Q2..D-Q7, deviations 1–2. P04 466/466 at `/tmp/cellbox-phase04-t8uH/evidence/verdict.json` (`90db1ce6…2680`) with `config-export-granted`, `config-export-system-denied`, `config-export-namespace-denied`, `config-denial-on-stderr-only`, `config-probe-set-denied`, `config-probe-delete-denied` and `config-probe-not-on-uart`. Also GUI 290, P03 67, P02 48, kernel lane 7/7 VFS self-tests, P09 103; host kernel 284, cellbox 189, service-config 7, api 153; canonical hashes unchanged. `/bin/config` ships only with feature `ai`; on an image without it `export` reports `config unavailable` (status 1) |
| C1-f `/bin/cellbox-ssh` packaging | **Open, by design** | Phase 04 criterion 4 / Phase 10; recorded in catalog §4.2 and §6 |
| C2-a P04 S1/S2-lite/S2c-min/S3-edge CP2 | **Closed (user CP2, 2026-10-25)** | [mini-dossier](phase-04-s1-s3-cp2-dossier.md), deviations 1–7; recorded in plan.md row 04, phase-04 and contract-deltas |
| C2-b contract-deltas reconciliation | **Closed (doc)** | `contract-deltas.md` "Reconciled delta index (2026-10-25)" |
| C2-c P07–P09 hash binding | **Closed (doc)** | hash addenda "recorded 2026-10-25 at closure, post-approval state" in the phase-07/08/09 deviation logs (proposal docs, key source files, tree digests) |
| C2-d owner confirmation | **Closed (user, 2026-10-25)** | recorded in the contract-deltas index and the mini-dossier §2 |
| C2-f recorded exceptions | **Noted** | contract-deltas index, "Recorded exceptions" |
| C3-a cargo-deny bans | **Closed** | `version` fields present: `cells/tools/cellbox/Cargo.toml:29,34,36,38` and `libs/vissh/Cargo.toml:12`. Scoped run 2026-10-25: `cargo deny --offline --locked --exclude-dev --manifest-path cells/tools/cellbox/Cargo.toml check advisories licenses bans sources` → `advisories ok, bans ok, licenses ok, sources ok` (exit 0) |
| C3-b dependency-review §5 | **Closed (doc)** | §5 2026-10-25 addendum: embedder-installed entropy, `ssh-key` without `encryption`, publickey deferred (D79-14), patches 0001–0006 |
| C3-c CBX-02 | **Closed** | [acceptance.md](acceptance.md):8 reads `met 2026-10-25` under the user's rule (updated outside this pass) |
| Step 8 portfolio registration | **Closed (doc)** | `.agents/plan-portfolio.md` Cellbox section rewritten 2026-10-25 to show the current phase states |
| C4 wording nit (P03 frontmatter) | **Open, optional** | unchanged |

**Phase 01 closed 2026-10-26.** The Q Config CP2 (2026-10-26) removed the last gate. C1–C4 are ticked in [phase-01-baseline-contracts.md](phase-01-baseline-contracts.md) (status `completed-private-evidence`), and CBX-01 is met in [acceptance.md](acceptance.md), with Phase 10 still auditing every delta. Open items that do not block Phase 01: C1-f (`/bin/cellbox-ssh` packaging, Phase 04 criterion 4 / Phase 10), the optional C4 wording nit, and the separately approved cleanups (item 4 above).

Rollout, Phase 04 criterion 4 (including C1-f) and Phase 10 stay blocked.
