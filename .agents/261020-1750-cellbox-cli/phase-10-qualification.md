---
phase: 10
title: "Integrated signed-image qualification and measurement"
status: blocked
priority: P1
tier: thinking
dependencies: [1, 2, 3, 4, 5, 6, 7, 8, 9]
---
# Phase 10: Qualification and delivery

## Overview and requirements

Close CBX-26..28 and audit CBX-01..25 plus CBX-29 on a matched signed x86 image/policy. All selected functionality remains required; earlier individual smoke proofs do not replace integrated user paths or lift physical/production gates.

## Architecture and related files

Update the affected image/package/signing recipes, command/app-development docs and existing integration harness. Extend `tests/integration/tests/desktop-x86.rs` or add a focused x86 CLI target only if needed for real behavioral proof; register any new target explicitly. Existing boot/pointer coverage is not a full terminal/search/editor/SSH acceptance run. Preserve unrelated user changes and independent service/app artifacts.

## Implementation steps

1. Audit every concrete delta's CP1 digest, exact implementation diff, consumer migration census, evidence and explicit CP2 approval; pending/blocked CP2 stops final cutover/rollout, not the disposable evidence work needed for review. Rebuild changed artifacts with correct Cell/kernel codegen and pinned notices, assemble a private matched signed image/policy and verify actual bytes; no stale artifact fallback.
2. Run headless and GUI-terminal consumer paths against the same suite: script/state transitions, typed and raw/external pipelines, top record/TUI, multi-chunk search, editor save/reopen and SSH remote command/PTY. GUI SSH proof joins P05's emulator with P09's headless client; P09 does not wait for Ratatui/GUI completion.
3. Observe negative cases on the integrated image: restricted file/lifecycle access, forged mode/alias, cross-session input, canceled streams, unavailable/truncated telemetry, search partial errors, editor interrupted/conflicting saves, SSH host/auth failure and unknown completion. Verify ordinary one-shot/non-terminal commands never unexpectedly become TUI or read keys, and unsupported explicit TUI requests fail truthfully.
4. Measure the same selected command/feature baseline and integrated build: suite executable count/bytes, image footprint, startup, heap/stack, idle redraw and long-stream bounds. Separate linker/artifact measurements from runtime resident-memory claims; report regressions as well as improvements.
5. Audit every command catalog caller, Cargo target, image/sign entry and docs example for a complete clean cutover. No obsolete suite binaries/wrappers, fake GUI interpreter or duplicate authority path may remain; retain unrelated independent consumers.
6. Update each acceptance row only with exact commit/version/profile/image/server provenance and actual evidence. Keep unavailable physical/VMX/remote/production claims outside scope; a missing required guest SSH proof remains blocked rather than marked done.
7. Update current docs, help/command catalog, changelog and portfolio to the exercised state; retain historical evidence at its original ceiling. Remove temporary consumers/fixtures/private secrets and record safe rollback/recovery limitations.

## Success criteria

- [ ] CBX-01..29 have actual evidence with no silent exclusions or placeholders.
- [ ] Real keyboard/screen/state and network/file effects prove selected behavior, not source assertions or fixed mock output.
- [ ] One suite artifact and all retained independent apps/services package correctly; no broad ambient authority or hidden legacy fallback.
- [ ] Measurements compare equivalent feature sets and distinguish host/QEMU/physical ceilings.
- [ ] Public docs, notices, command help, caller routes and portfolio match the delivered implementation; no IDE/build environment is scheduled or implied.

## Verification

Follow [verification.md](verification.md); run affected checks once after implementation edits, then actual consumer smoke and integrated qualification. No broad test reruns to reconfirm a user-reported failure. Keep meaningful behavior regressions, not incidental wording/forwarding tests.

## Assumptions

Exact required endpoint/image fixtures and approvals are available at entry. Verify paths/features and private-image ownership before running. Lack of hardware or external fixture does not authorize fabricated evidence or scope reduction.

## Security and risk assessment

Do not use an unattended runner that kills another VM or opens inbound SSH. Observe artifact/policy provenance and protect captured secret data. Rollback uses a matched safe image/policy and cold boot; file saves, trust changes and remote effects require their own recovery and are not reversed by code rollback.

## Deviation log

2026-10-20 — **Qualification/completion withdrawn; blocked and unqualified; rollout prohibited.** Historical build/sign/package operations did place a signed `/bin/cellbox` candidate (88,576 bytes) and updated signed `/bin/terminal` (595,032 bytes) in `kernel/src/embedded-x86_64/kernel_fs.img` alongside independent cells. Historical F1/F5 observations and host test snapshots across terminal-sdk/tui-cellos/text-engine remain check/artifact evidence only, not integrated acceptance, a complete clean caller cutover or runtime performance measurement. No actual QEMU serial/GUI, SSH interoperability or editor save/reopen proof and no CP2 establish qualification. The full owner/allocation dossier is still incomplete, while actual user candidate-design consent is preserved. All CBX-01..29 remain pending, all success criteria unchecked, and all ten prior phase-completion claims are withdrawn. [reports/rollout-containment.md](reports/rollout-containment.md) is the authoritative current withdrawal/containment record; existing signed images are not authorized for rollout. This correction does not rebuild, sign, mutate or promote an image.

2026-10-26 — **Slice-plan CP1 approved by the user** (relayed by Main) for the [S0 record](phase-10-s0-proposal.md). Order: S10-0 → S10-1 → S10-2a → S10-3 → S10-4 → S10-5, with S10-2b inside S10-5. Every recommendation in D10-1..D10-10 and D10-12..D10-14 was taken:
- dev-posture image, signed with the dev key through a qualified route;
- the GUI Terminal ships;
- `/bin/config` stays `ai`-only;
- `/bin/cellbox-ssh` is a second FAT copy;
- the current worker set;
- `/bin/shell` and `/bin/sshd` are dropped;
- ssh ships with CBX-23/24/25 marked partial;
- the ISO guard reopens for the qualified kernel only;
- non-Intel recipes stay refused;
- `dev-weak-rng` is kept and documented;
- the S10-3 qualification rule;
- the quarantined image is archived.

D10-11: the user chose to **add slice S10-0**, which implements `/bin/cellbox <command>` for catalog commands before the cutover. Its CP1 is pending, on DS0-1..DS0-6. Each slice keeps its own CP1 and CP2. No slice is implemented; status stays blocked and rollout stays prohibited.

2026-10-26 — **S10-0 CP1 and CP2 approved by the user** (relayed by Main). CP1 took DS0-1..DS0-6 as recommended. CP2 covers deviations 1–7 in the [S0 record](phase-10-s0-proposal.md) S10-0.11 and the catalog revision `2a4c3e6652c90b5dad840c5a403276a3a8fa4236b8bab12ca1b319bc237cbd70`.
- Evidence (`reports/s10-0-evidence/`): P04 494/494, GUI 290, P02 48, kernel lane pass, P03 67, P09 103 (`/tmp/cellbox-phase09-KSz1`, archived in `p09/`), host 205. Private QEMU/TCG images only.
- CBX-10 is met on private evidence; the canonical image is pending S10-3.
- S10-1 (recipe cutover, no canonical bytes) is drafted and awaits its CP1.

Status stays blocked and rollout stays prohibited.

2026-10-26 — **S10-1 CP1 and CP2 approved by the user** (relayed by Main). CP1 took D11-1..D11-6 as recommended. CP2 covers deviations 1–11 in the [S0 record](phase-10-s0-proposal.md) S10-1.7. Evidence is in `reports/s10-1-evidence/`:
- P04 494/494;
- the scratch headless and desktop images (private seed, `/tmp`) boot on a fixture kernel;
- the canonical artifact hashes are unchanged.

The S10-1 CP2 was given on its own rather than jointly with S10-2a. S10-2a (the dev-key candidate signing route) is drafted and awaits its CP1. Status stays blocked and rollout stays prohibited.

2026-10-26 — **S10-2a CP1 and CP2 approved by the user** (relayed by Main). CP1 took D2a-1..D2a-4 as recommended. CP2 covers deviations 1–10 in the [S0 record](phase-10-s0-proposal.md) S10-2a.8. Evidence is in `reports/s10-2a-evidence/`:
- signing tests 66, assembler tests 38;
- both dev-key candidate routes verified in `/tmp`;
- a boot smoke on a matching default-feature kernel;
- the canonical artifact hashes are unchanged.

Deviation 9 (a `sed` correction of a scratch evidence summary after a correct rerun) was disclosed and accepted. G1, G3 and the quarantined-hash set are unchanged. S10-3 (candidate image and lane battery) is drafted and awaits its CP1. Status stays blocked and rollout stays prohibited.

2026-10-26 — **S10-3 CP1 approved by the user** (relayed by Main): D3-1..D3-7 as recommended. D3-5 (`[selftest] COMPLETION-QUEUE: FAIL`) is a known, non-blocking defect.

S10-3 then **stopped at the L6 regression rule** (S10-3.10). Two failures appeared:
- the UART `cellbox sh` does not run lines ended by a lone LF (`ahci`/`nvme` FAT write consumers);
- the default-feature candidate kernel reports `Tier 2 admission: ENABLED`, where `qemu-x86_64-test.sh` requires DISABLED.

No CBX row changed; evidence is in `reports/s10-3-evidence/`.

The user's decisions (2026-10-26):
- **(a)** fix the product in a new slice **S10-3a**: a lone-LF line submitted over the UART must run;
- **(b)** the canonical kernel uses the **fleet profile, with Tier 2 DISABLED** (the baseline's default features plus `policy-required`);
- **(c)** UART input going to the focused GUI Terminal on desktop is accepted and documented: serial is qualified on headless, desktop through the GUI;
- **(d)** the S10-3.2 marker forms are amended (`take --frame N`, no `return`).

S10-3a (root cause, fix, fleet kernel, tests and reruns) is drafted in the [S0 record](phase-10-s0-proposal.md) and awaits its CP1. Status stays blocked and rollout stays prohibited.

2026-10-26 — **S10-3a CP1 approved by the user** (D3a-1..D3a-4) and implemented ([S0 record](phase-10-s0-proposal.md) S10-3a.8):
- Control+J (a lone UART LF) submits in cellbox's four line readers;
- the `spec.md` sentence is added (new digest `f9c550f7…`);
- `CANONICAL_KERNEL_FEATURES = policy-required` in the assembler, used by the CI kernel and PC steps; the route pin is re-pinned.

Evidence (`reports/s10-3a-evidence/`): cellbox host 208, api 153, signing 66, assembler 39, P04 494/494. Deviation 1: the change reverses the user-approved Phase 02 A2 item 1 (Ctrl-J not Enter), which the CP1 did not cite. The P02 lane's `ctrl_j_not_enter` check now fails; it was left unchanged pending a decision. The S10-3a CP2 is pending.

The **S10-3 battery was then resumed** on the fleet-profile candidate `a9f384a6…` (S10-3.11; `reports/s10-3-evidence/rerun-fleet/`):
- L6: 0 regressions; the boot check passes with Tier 2 DISABLED;
- L3 494; L4 6/6 at 290; L2 67; P09 34 (no-faults) and 103 (faults); P02 kernel lane PASS;
- L0 headless 15/16 and desktop 8/9.

It stopped on three conditions:
- **SC1**: builtin `sleep` and a blocking `cat` sink ignore Ctrl-C (pre-existing);
- **SC2**: builtin `cat` prints a false "No such file" for non-UTF-8 or large VFS files;
- **SC3**: the Phase 02 A2 conflict.

No fleet-profile rights breakage was seen, so no policy rows are needed (D3a-2). No CBX row flips; decisions (e)–(g) are pending. Canonical hashes are unchanged. Status stays blocked and rollout stays prohibited.

2026-10-26 — **User decisions (e)–(g)** (relayed by Main):
- **(e)** S10-3a **supersedes Phase 02 amendment A2 item 1**. This is recorded in the Phase 02 log, `contract-deltas.md`, the S0 record and `plan.md`. The P02 check `ctrl_j_not_enter` is replaced by `ctrl_j_submits`.
  - A first one-burst version failed: typed-ahead input while a command runs is discarded (finding F-TA).
  - The check now sends each LF-terminated command on its own. The P02 import-mode rerun on `a9f384a6…` **PASSes** (48 plus `ctrl_j_submits`).
- **(f)/(g)**: SC1 (`sleep` and the blocked Collected sink ignore Ctrl-C) and SC2 (`cat` false ENOENT for non-UTF-8 or oversized VFS files) go to new slice **S10-3b**.

S10-3b's root causes, fix, D3b-1..D3b-3, tests and reruns are drafted in the [S0 record](phase-10-s0-proposal.md) S10-3b, awaiting its CP1. Status stays blocked and rollout stays prohibited.

2026-10-26 — **S10-3b implemented; the S10-3 rerun stopped at SC4** (the [S0 record](phase-10-s0-proposal.md) S10-3b.8 and S10-3.12).
- **S10-3b.** The CP1 was approved with every recommendation (D3b-1 B, D3b-2 A, D3b-3 A).
  - `sleep`, `top -b` and the `top --records` wait between frames return 130 on Ctrl-C.
  - Collected typed runs poll Ctrl-C before every pull.
  - Builtin `cat` of a VFS path no longer falls through to a false ENOENT: it prints lossy text, `Is a directory`, or `File too large` (oversized boot-image files still stream).
  - cellbox host tests 213; P04 regression run 494/494. Its CP2 is pending, with deviations 1–4.
- **S10-3 rerun on the rebuilt candidate `9c4d16eb…`.**
  - P02 48 + `ctrl_j_submits`, kernel lane, P03 67, P04 494, GUI 6/6 at 290, P09 34/103, L6 0 regressions with the CI boot check, and the audit all PASS.
  - Every S10-3b sub-check passes on L0, including `P10G-vt` hostile 162/162.
  - **L0 6/6 was not reached** after 12 runs per profile (headless 7 pass, desktop 6 pass). The failures are classified by code; none is in an S10-3b path: Docker sshd fixture start ×7, NVMe poll-budget timeouts ×3, driver idle detection ×1.
  - **SC4**: under S10-3.5 the desktop image is withheld until D3-8 is decided. A (harness hardening, then rerun) is recommended.
  - CBX-14 and CBX-26 are not met; no CBX row flips.
  - Canonical hashes are unchanged. Status stays blocked and rollout stays prohibited.

2026-10-26 — **The user chose D3-8 C, starting at once** (relayed by Main). The harness is fixed (a prebuilt ssh fixture image, a run that never booted does not count, prompt detection that ignores background log lines), and the NVMe driver's timeouts become time-based. This is the new slice **S10-3c**, drafted in the [S0 record](phase-10-s0-proposal.md) S10-3c and **awaiting its CP1** (D3c-1..D3c-5). No product source was changed.
- Every NVMe wait uses one budget of 50,000,000 polls (`controller.rs:58`): reset, enable, admin and I/O, including flush.
- Init waits only 400 yields for the block driver before the VFS mounts (`init/src/boot.rs:140-155`).
- Proposed budgets: RDY by `CAP.TO` (at least 1 s), admin 60 s, I/O and flush 30 s, timed with `sys_get_time_ms`.
- Then a full S10-3 rerun on a rebuilt candidate. Status stays blocked and rollout stays prohibited.

2026-10-28 — **S10-3c CP1 approved 2026-10-27; implemented; CP2 pending.** D3c-1 A / D3c-2 B / D3c-3 A / D3c-4 A / D3c-5 as proposed. Existing partial implementation was reused. NVMe reset/enable uses CAP.TO × 500 ms (minimum 1 s), admin 60 s, I/O/Flush 30 s on `sys_get_time_ms`, retaining one-log poll fallback and fail-closed saves. Init waits on registration/NVMe exit/60 s, not AHCI lifetime. Host entry/CI tests and H1–H3 are implemented.
- Actual host results: NVMe **6**, init **13**, cellbox **213**, VFS **109** passed; harness initially 25, finally **27** passed. Existing cellbox/VFS warnings and an unclosed mocked-stderr ResourceWarning are disclosed.
- Default regressions: P04 terminal **494/494, 202 markers** (`/tmp/cellbox-phase04-jyc28jns`); P02 kernel **PASS**, VFS self-tests **7/7** (`/tmp/cellbox-phase02-y9quex7v`).
- C0–C6 rebuilt outside the repository. Read-only candidate `/tmp/cellbox-s10-3c-resume/cand/candidate.json`, SHA-256 **`0718a96542e7a3f4d223b02d5ad0511a2e23d83b8cbdb690450aea7662a62863`**; fleet `policy-required`, plus `board-x86-pc` for PC; exact image/init embedding true, no recorded build failures.
- L0 headless attempt 1: **14/16**; rejected genuine prompts appended directly to Ratatui's alternate-screen/mode restore, not product exit failures. Diagnosis derives five boundaries from retained UART (`logs/h3-tui-diagnosis.json`). A narrow H3 framing correction and two new tests fix this; candidate bytes were not rebuilt, and the original verdict remains counted/unchanged.
- L0 headless attempt 2: **15/16**; top/editor and all other main-boot markers pass. **New SC5**: the SSH boot logs `[nvme] admin timeout opc=6 after 60000 ms`, controller init IO/exit, failed VFS mounts and unavailable SSH trust store. Evidence `/tmp/cellbox-phase10-w5t1anar/evidence/serial-ssh.log:187–215,362`. This is a full-budget storage/product finding, not a flake; the slice stopped immediately.
- Headless 0 consecutive passes after 2 counted attempts; desktop **0 runs**. Imported P02+kernel, P03, P04 terminal, GUI ×6, P09 L5/L5f, L6 12 consumers/CI baseline, fresh CBX-01/02 audit and remaining default-mode lanes **not run** after SC5; no old result counts for the rebuilt candidate.
- **No CBX row flips. CBX-14/26 remain unqualified; 23–25 remain partial/blocked; 27/28 await S10-4/5.** Other passing main/P04 observations retain only their stated private ceiling; full per-row disposition and CP2 deviations are in [S10-3.13](phase-10-s0-proposal.md).
- **469 canonical file hashes unchanged**, with user's `kernel/src/main.rs` hash `a9636fee…08c64` unchanged. Known COMPLETION-QUEUE failure remains accepted nonblocking. No containment guard was lifted.
- Archived generated evidence: `reports/s10-3c-evidence/`, rerun in `rerun/`; binaries remain read-only outside the repository. **D3c-6 decision required:** separately authorize Identify/completion investigation/fix then full rebuild/rerun (recommended), or retain the blocked candidate pending that work. No speculative timeout increase or failure waiver.

2026-10-28 — **D3c-6 user decision: investigate now; product fix requires a new CP1. Investigation complete; S10-3d CP1 proposed.**
- Confirmed mechanism: x86 kernel fallback already exposes NVMe when Platform starts its destructive BAR-size scan. Platform can temporarily disable MemorySpace/move BAR0 while NVMe resets/programs the controller. QEMU then never receives the intended queue programming; a later doorbell uses firmware-old queues and intended CQ/Identify memory remains zero. Controlled `/tmp` reproduction: `probe-forced` admin opcode 6 timeout **60111 ms**. **Confidence: confirmed mechanism; probable original SC5 attribution**, since the original lacked device trace.
- Eight unchanged-candidate device-trace control boots reached the shell; none is qualifying L0 evidence. A widened existing decode-off interval reproduces lost I/O and exposes a second confirmed bug: stale-CID handling repeatedly acknowledges one cached CQ slot, bypassing its own deadline/yield. Source and device snapshots agree.
- Intermediate diagnostic failures are disclosed: one QMP-connect readiness race (no guest evidence), one disposable Platform GetTime allowlist omission/panic (excluded from causal proof). Final controlled probe has diagnostic-only scheduling changes, not a product fix, retry or enlarged NVMe budget.
- Proposed S10-3d: x86 init waits for bootstrap Platform lifetime to end **before any driver/VFS spawn** (uncertain/live-over-budget state stops boot); both NVMe loops refresh CQ head/phase and retain deadline/yield through unexpected completions; cold raw-CAP/admin-state diagnostics. Exact files, host/controlled regression criteria, risks/alternatives and full C0–C6/S10-3 rerun are in [S10-3d](phase-10-s0-proposal.md).
- Incidental pinned Identify-free leak and large-DSTRD MMIO-bounds gap are recorded, **not** silently patched or claimed causal to SC5. Existing `is_platform_registered` marks the first BAR, so is not a valid scan-completion barrier.
- Generated `reports/s10-3d-investigation/summary.json`: **469 canonical hashes unchanged**, candidate still `0718a965…`, product source hashes unchanged, user's kernel main hash still `a9636fee…08c64`. Raw probes remain `/tmp/cellbox-s10-3d-investigation/`; no new product source edits or qualification claims. CP1 approval is the next gate; S10-3c CP2/CBX/containment status unchanged.

2026-10-28 — **S10-3d CP1 approved exactly as proposed (D3d-1 A, D3d-2/3/4); implemented; full new-candidate S10-3 battery has passing evidence, CP2 pending.**
- Product: x86 init lifetime barrier before every driver/VFS spawn; fail-closed process-table/clock/deadline uncertainty; unchanged non-x86 flow. Shared production CQ cursor refreshes head/phase, consumes each late completion once and preserves original deadline/yield. Raw CAP/admin timeout state diagnostics; no retry or budget increase. Incidental DMA-free/DSTRD findings remain out of scope.
- Host **NVMe11/init20/platform9/cellbox213/VFS109**, harness **27** passed. Default P04 **494/494,202 markers**; P02 kernel PASS, VFS **7/7**. Controlled delayed-Platform proof PASS; retained raw trace/DMA analysis shows scan complete before CAP/first DMA and intended Identify CID1/CNS1/PRP/CQE success. No product sleeps.
- New read-only fleet candidate `/tmp/cellbox-s10-3d-resume/cand/candidate.json`, SHA-256 **`4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821`**, C0–C6 no failures, exact image/init embeddings true.
- **L0 headless6/6 consecutive at16 markers and desktop6/6 at9**, no retries/status3, no NVMe timeout/slow-completion events; provenance/no-secret/ISO hashes pass. **L4 GUI6/6 at290**, five click+one autoload, strict stderr10/10 each.
- Imported P02 **48 + ctrl_j_submits**, kernel **39/39 + VFS7/7**, P03 **67**, P04 **494**, P09 L5 **34** and lane-only L5f **103** pass. All default-mode runners pass. Payload-equality evidence retained with fixture-profile ceilings.
- **L6 12 consumers + CI boot before/after: zero regressions**, 39 pass→pass, existing xHCI key-decode fail→fail, four x86-32 handoff skips, one virtio ignored. Candidate NVMe3/3, AHCI5/5, bus1-NVMe2/2, remapper3/3; CI boot bothPASS.
- **SC6 audit correction, explicit disposition:** old harness had pre-S10-3a spec pin `cde24178…`, current approved spec `f9c550f7…`. Reported/stopped; Main directed the exact prior S10-3b script already pinning the recorded S10-3a/supersession(e) revision. Only document audit reran with existing dependency logs: **PASS** all five pins/no missing CP rows; original FAIL/script/log retained. No spec/criteria edits, new hash approval, runtime rerun or rebuild. Scoped cargo-denyPASS; workspace exit7 same advisory/license/wildcard categories as prior evidence.
- **469 canonical file hashes unchanged**; user's kernel main SHA-256 `a9636fee…08c64` unchanged; old candidate retained. Archive `reports/s10-3d-evidence/`, rerun under `rerun/`; raw/binary outputs outside repo. Known COMPLETION-QUEUE/xHCI, warnings/ResourceWarning, D60-3/Delete, synced-not-crash-proof and SSH guest/publickey limitations remain disclosed.
- **No CBX row flips:** 14/26 now have required passing candidate evidence but await approval; 23–25 remain partial, 27/28 await S10-4/5, 16 retains old audited evidence. Full per-row disposition and CP2 deviations: [S10-3d.7/S10-3.14](phase-10-s0-proposal.md).
- **Requested user CP2: S10-3a, S10-3b, S10-3c, S10-3d and S10-3.** None is inferred from tests or Main's audit-harness disposition. Containment, Phase04 criterion4, production/physical/VMX and S10-4/S10-5 gates remain.

2026-10-28 — **User CP2 APPROVED S10-3a, S10-3b, S10-3c, S10-3d and S10-3, including all recorded deviations/disclosures** (relayed by Main).
- Ratifies S10-3a's Control+j line acceptance/supersession (e), S10-3b's interrupt/file behavior, S10-3c's elapsed deadlines/H1–H3, S10-3d's scan lifetime/current-CQ progression and the full generated battery above. Historical stops, initial SC6 FAIL and document-only corrected PASS remain intact; no fresh contract/hash was approved to make an audit pass.
- Qualified scope is retained read-only dev-posture Intel x86 QEMU/TCG candidate **`4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821`**. [Acceptance](acceptance.md) now records only the exercised S10-3.14 ceilings, including **CBX-14/26 met on this candidate**; CBX-23/24/25 remain partial and CBX-27/28 remain pending. Original withdrawal/history is retained, not retroactively converted to success.
- Existing **469-file canonical comparison and user's VMX source proof remain the qualification evidence**. This approval-recording pass edits documentation only: no product/artifact/guard write, rebuild, runtime rerun, new footprint measurement or canonical promotion. Phase04 criterion4, Phase10 final closure, production/physical/VMX and all containment/authority gates remain open.
- **S10-4 CP1 approved by user later on 2026-10-28** (relayed by Main) for the original [S0](phase-10-s0-proposal.md) S10-4 method: recorded980,528B vs isolated historical HEAD rebuild, qualified candidate unchanged, matched toolchain/flags/dev signing, separate unsigned/signed/file/FAT/kernel/ISO/physical alias bytes, startup, heap/stack, idle redraw and long-stream reporting with feature/provenance ceilings. The proposed detailed replacement had **not** landed at approval; no unstated measurement protocol or unavailable runtime observer is inferred approved. Execution now authorized within that original method; findings/deviations and user CP2 remain separate. No baseline build/measurement had run at this approval record; S10-5 remains blocked.

2026-10-28 — **S10-4 initial execution STOP: missing historical B2 workspace member; no result CP2 or acceptance closure.**
- Exact approved detached HEAD `3471028bc8b4db14275553b4740c17e24fa4f32b` was checked out under `/tmp/cellbox-s10-4/baseline-worktree`, own target directory, matching nightly/PIC/release/build-std/C environment. Version queries succeed; the one selected `app-shell`/five-worker build exits **101 before compilation** because historical Cargo workspace lists absent `cells/apps/vm-runner/Cargo.toml`. Full member census finds only this gap; historical `git ls-tree` is empty for that member.
- Reported to Main immediately; **no manifest repair, alternate revision, stub, retry or signing bypass authorized/attempted**. Retain worktree for explicit disposition then remove after the B2 stage. Reflink unsupported/plain checkout and initial 60s hash-process deadline are disclosed infrastructure events; successful generated before/after proof follows (no QEMU counted).
- Generated `reports/s10-4-evidence/summary.json`, command logs, stop verdict, historical manifests and canonical comparison: **546/546 paths unchanged, including469 canonical**, user's VMX main and every qualified-candidate file. No B2 ELF/signing results, new static-footprint report, new QEMU/runtime run, startup/redraw/heap/stack measurements. No product/canonical artifact changes.
- Requested disposition: minimal isolated exclusion of the one nonexistent unrelated member (non-pristine comparator deviation) vs missing-source block; and explicit runtime reservation-only ceiling vs separately approved candidate-preserving memory observer. Existing `.bss`/arena/host bounds/L0-L3 behavior are not resident/peak/stack/startup measurements. **CBX-27 and Phase10 criterion4 remain unmet; CBX-28/S10-5 and containment remain blocked.**

2026-10-28 — **User decision1 authorizes minimal disposable B2 vm-runner exclusion; static report executed; B2 stopped again on locked resolution.**
- Deleted only the nonexistent `cells/apps/vm-runner` workspace member from `/tmp` root `Cargo.toml`; original source and patch retained. B2 explicitly non-pristine; recursive local-path closure covers8 manifests with no vm-runner compile dependency in shell/five-worker graph. Historical shell still names vm-runner as a runtime user-app fallback, so no universal behavior-independence claim.
- Repaired build preserves all approved compiler/target/features/signing constraints but exits101: **Cargo.lock needs updating and --locked refuses**. No lock edit/offline/unlocked retry authorized or attempted; reported immediately. Further B2 completion requires explicit offline lock-reconciliation disposition/audit; original and repaired failures separate.
- Actual generated static evidence (`reports/s10-4-evidence/static/measurement.json`): B1 six980,528B; headless six3,245,416B (**+2,264,888**), desktop six3,245,480B (**+2,264,952**). Charging SSH alias twice gives6,339,368/6,339,496B (**+5,358,840/+5,358,968**). Suite-only3,093,952/3,094,016B vs881,120B: regressions, not savings. All measured B1/selected candidate dev signatures verify.
- Full per-ELF signed/section-removed/available original-unsigned/section tables and FAT payload/container/kernel/ISO totals in S10-4.2 and generated JSON. Retained unsigned artifacts prove matching desktop signing payload; not headless. Section-removed files are expressly derivatives, not original unsigned files. Profiles/features differ; no equivalent-image or universal optimization claim.
- Cellbox `.bss`7,340,360B,6MiB custom/1MiB ostd arenas are reservations only. Resident/allocator/stack peaks unmeasured; decision2 still awaits user. No new runtime, startup or idle-cost measurements after stop; L0/L3 behavior is not substituted for them. **CBX27/criterion4 unmet, no resultCP2/promotion/containment change**. Qualified candidate unchanged/read-only; worktree retained for lock disposition then removal.

2026-10-28 — **Explicit lock fallback authorized; B2 build/unsigned measurement complete; checked-signing F1 STOP.**
- Preferred vm-runner lock-entry deletion impossible (no such entry). Authorized offline metadata reconciliation adds only local app-chrome/files/terminal0.1.0, none in selected resolved graph; **407 original package blocks byte-identical, no registry/source/version changes**. Original/full lock diffs and graph audit retained, B2 explicitly non-pristine. Only disposable root Cargo.toml/Cargo.lock changed.
- Matched `--locked --offline` build exits0 (release34.53s), original unsigned six **994,064B** = shell880,992 + workers113,072. Per-ELF text/data/bss/file/hash in generated `b2-measurement.json`; desktop matching unsigned six3,261,680B (**+2,267,616B**, non-equivalent features). B1signed980,528B vs B2unsigned is not a matched-signing-state comparison/reproduction.
- **New stop reported immediately:** historical checked dev signer F5PASS, F1FAIL at unrelated unallowlisted unsafe token in `ocel-quickjs/native_layout.rs`; refuses exit1. No allowlist edit, unchecked signature/route manipulation or retry. Staging copies remain unsigned; B2 signed results null. Need explicit audited signing disposition vs retain incomplete signed comparator.
- Archive `reports/s10-4-evidence/` preserves initial/lock-stop summaries, all attempts, authorized deltas, build/sign logs, unsigned static table, pin graph, retained L0/L3 behavior and latest **546/546 unchanged proof including469 canonical**, user VMX/candidate. No new runtime/peak/startup/idle-cost run, no S10-4 CP2 or CBX27/criterion4 closure; S10-5/containment unchanged. Worktree retained only pending signing disposition then removal.

2026-10-28 — **User selects “design runtime protocol first”; consolidated observer/signing CP1 proposed, not executed.**
- Static capability evidence only: B1/B2 shell and candidate preserve symbol tables but no DWARF; actual HEAP used-word offset0 and add/sub successors identified from unchanged ELFs. Source/assembly semantics are aligned live allocator bytes, no persistent peak counter. Candidate stack watermark/test-hooks absent. QEMU8.2.2 pinned manual supports TCG hardware break/watchpoints; configured watchpoint VA is not exact access depth. GDB/Node lookup helper absent; exact-version primary QEMU docs fetched directly.
- Proposed [S0 S10-4.4](phase-10-s0-proposal.md): separate untraced fresh-VM monotonic startup/idle/stream measurements (3boots/profile) and one observed boot/profile; host-only private-socket read-only RSP client, hardware-only observation, complete heap-event coverage guard,4KiB CPU-stack-write-attempt depth bounds, deduplicated mapped-backing witnesses. Explicitly not RSS, dirty-page heap/stack peaks, exact RSP/kernel-stack peak, physical/VMX or unperturbed trace timings. Original900s/command limits/new-stop reporting retained.
- B1/B2 have no approved matched runtime carrier under unchanged candidate policy/launch contracts. Proposed candidate characterization retains missing before-runtime comparison and **CBX27/criterion4 unmet**. Retained L0 first-substring times3.1/2.8/2.5/2.5/2.5/2.5s are coarse historical `time.time()` observations, not the proposed monotonic/exact-idle metric; complete-fixture seconds remain separate.
- Existing CLI supports `--repo` release-tarball source fallback, **not** a package-scoped flag; named candidate routes still run whole-root F1. Proposed explicit new source-input approval: unchanged CLI against source-exact complete shell/sys-tools mirror (38Rust files/12roots;8total local crates), original app-shell exception/allowlist unchanged, strict F1/F5 and same dev section/key, no bypass/product edit. Whole-workspace F1 refusal remains. Alternative unsigned-only or separately reviewed unrelated F1 audit stated.
- **Await user D4-Obs1/Obs2/Obs3/D4-Sign1** (recommendA each). Evidence `reports/s10-4-evidence/protocol/` generated from static inspection/census/source hashes only: **zero new VMs, zero observer implementation, zero new scoped check/signature**. Worktree/candidate/evidence retained; all promotion/containment gates unchanged.

2026-10-28 — **User explicitly APPROVES S10-4.4 CP1: D4-Obs1/2/3 A and D4-Sign1 A** (relayed by Main).
- Authorizes exact proposed host-only candidate-preserving observation plus source-exact scoped mirror signing, not a candidate rebuild, global F1 waiver/allowlist change, baseline carrier, exact missing memory metrics or promotion.
- Execute3 untraced timing boots +1 observed boot/profile and specified startup/idle/long-stream phases; preserve original refusals/provenance, sign only six checked original B2 copies, remove disposable worktree after evidence secure. New genuine blockers stop/report, no invented substitute.
- ResultCP2 remains separate; approved absence of matched baseline runtime/exact RSP/kernel-stack/RSS observations keeps **CBX27/criterion4 unmet**. Containment/canonical/user-VMX invariants unchanged.

2026-10-28 — **Approved scoped B2 signing complete; host observer43/43; runtime schedule executing.**
- Current unchanged CLI against exact254-file mirror:2 Cell crates/38 Rust files/12 roots, original app-shell exception/allowlist, strict F1/F5 PASS;6 existing-verifier/canonical-payload signatures valid, one64B section each. B2 signed980,528B (same total as B1; onlyshell byte-identical), original unsigned994,064B. All candidate signed-file regressions and feature/runtime ceilings unchanged.
- Generated `signing/verified/summary.json` + `scope-comparison.json`: complete source mirror before/after SHA65af0e93…23c0,546protected paths/originals/tools/refusal unchanged. Earlier helper single-link/census-order/compiler-prefix/layout overconstraints retained/corrected, no policy waiver; objcopy .init_array/BSS-offset metadata normalization disclosed, no PHDR identity claim or resign. Misplaced signing archive moved to phase reports only, unrelated root reports retained.
- After evidence secure, restored only owned disposable2-manifest changes; plain `git worktree remove` exit0, mirror/build cache removed (`cleanup.json`). Signed/unsigned originals retained outside repo. No canonical/product/user-VMX write.
- Host-only parser/protocol/coverage tests **43/43** once; raw tool result retained for automated archive extraction. Started exact3 untraced boots +1 observed/profile schedule; no result is inferred before generated runtime verdicts. Original absent baseline/exact memory metrics and CBX27/criterion4 ceiling remain.

2026-10-28 — **Two observer assumption bugs preserved/corrected, not product defects; Main directs contract-correct runtime continuation.**
- Startup prefix: genuine prompt plus coalesced Init exit message made cumulative-buffer polling miss earlier exact idle state,900s host stop. Existing `Shell.idle` now judges each received prompt prefix with receive timestamp;3new host cases pass, raw failure retained, no grammar/ignore-policy/product change.
- Stream premise:120s no-live-output stop came from wrongly expecting streaming out of `…|cat`, which approved S10-3b explicitly **buffers** and cancels130/no bytes. Exact current executor/typed paths and qualified L0 notes (“0rows beforeCtrlC”, cat130/0.3sPASS) establish failed premise, not a demonstrated hang. Separate no-cat terminal sink is the live-output path.
- Main explicitly directs finite2frame/status0 control;10s bufferedcat zeroJSON/no prematureprompt→CtrlC/newprompt130/quiescence; separate no-cat≥3live rows10s→CtrlC130/quiescence. Preserve both failed observations asCP2deviations; fresh original3+1/profile schedule continues within approved contracts, no product change/deadline waiver/acceptance weakening. Actual partial startup2,925.614379ms/idle100samples/finite2frames0 retained separately, not a completed run.

2026-10-28 — **Observed-mode native hardware step amendment explicitly user-approved; exact echo host correction, no timing rerun.**
- All3 headless+3 desktop untraced runtime runs complete on unchanged candidate; separate startup component timestamps/idle samples/top intervals and contract-correct headless streams retained.
- QEMU target XML omits xi namespace expected by Python parser: captured-packet parser correction,2testsPASS, original pre-resume failure retained.
- Original hardware execute breakpoint+continue control repeatedly stops at Platform/Init entry, so900s expires before Cellbox. User authorizes **only current enrolled breakpoint removal→one native instruction→restoration before continue**, keeping all otherbreak/writewatch guards and no guestmemory/registerwrites/injection/Z0.7new host testsPASS. Raw4,282,115,315B traceSHA46f873d6… retained outside repo,431,927,301B gzip also/tmp; small generated diagnostic/excerpt archived. No silent protocol fallback/deadline change.
- First amended observed run captures initial/idle mapping/live allocator values but existing Shell.run parser rejects exact command echo interrupted by already-recognized `[net-bridge]` log; guest actually completes two frames/newprompt. /tmp wrapper now requires exact contiguous command fragments across **same SERVICE_LINE/KERNEL_LINE only**, retains background lines and unchanged newprompt/deadline;4new testsPASS. Partial metrics are not full-workload peaks; original failed attempt retained. Two fresh observed lanes continue; no completed timing/product tests/builds repeated.

2026-10-28 — **S10-4 actual partial delivery/limited CP2 requested; observer prerequisite remains blocked.**
- Static/scoped signing complete; original-unsigned B2six994,064B/signed980,528B, strict mirrorF1F5/sixvalidsignatures/407priorpins, richer candidate6=3,245,416/3,245,480B and alias7=6,339,368/6,339,496B are regressions with explicit feature/carrier non-equivalence. Original refusals retained; disposable checkout/mirror/cache removed.
- **Headless3/3+desktop3/3 fresh untraced complete**. Headless exactprompt median2,646.040699ms; desktopready4,146.777226ms/click→exactprompt4,209.187360ms/boot→prompt11,436.829514ms includes retained3s automationwait. Full per-run/min/median/max inS0S10-4.8. OrdinaryGUI idle0/0/0 content-change frames;top10/10/10, actual99/97 etc samples/elapsed/rawscreens kept; notdrawcall/shellCPU claims. Headless bufferedcat/no-cat stream and pinnedrg cancellation3/3pass/status130/quiescence.
- Final observed headless **STOP** at sameRIP after exactlyone native step of knownjmp; currentZ1 restored, allguards kept. Partial209postevents/peak43,056B onlycapturedinterval; initial29,216→idle33,312B live, mappedPTLOAD+stack10,199,040B, stackwritebound32,769–36,864B. Earlier distinctpartialattempt59,416B notmerged. Fullworkloadpeak absent; desktopobservedUNRUN.
- Main-authorized pinnedQEMU source/isolatedbootsector investigation: possiblependingDEBUG canprecede instruction and bareT05 isn'tretirementproof; actualhiddenstate notobservable/proven.2isolatedvariants+1000repeated steps advance normally, no samePC reproducer. No candidateextra-step/retry/softwareBP/write/dropguard/deadlineincrease; unavailable independentlydemonstrated step-completion/disambiguation prerequisite remains. SourceURLs/SHA/packets/argv/results archived; notproductdefect.
- Distincthosttests59PASS, failedprefix/livecat/XML/native-resume/echo assumptions and useramendment/finalstop disclosed. Generated`reports/s10-4-evidence/summary.json`/SHAmanifest; latest**546/546protected incl469canonical+VMX+candidate unchanged**. LimitedCP2 requestedforactualpartialonly; **CBX27/criterion4 unmet**, no fullruntimebaseline/RSS/exactstack/kernelstack/physicalVMX/production/canonicalpromotion/containmentlift/S10-5closure.

2026-10-28 — **User approved limited-result CP2 for S10-4**, covering [S10-4.8](phase-10-s0-proposal.md)'s actual partial results and recorded deviations only: static/scoped signing, six untraced startup/idle runs, headless controlled streams and honestly partial headless memory. This approval does not convert the observer stop into success, approve extra debugger control, or supply missing full-workload peaks, desktop observation or matched historical runtime.
- **CBX-27 and criterion 4 remain unmet. S10-5, canonical promotion and containment remain blocked.** No product/artifact/guard change, rebuild or runtime rerun is authorized or performed by this documentation-only approval record. Existing generated 546/546-path preservation proof remains the measurement evidence.
