---
phase: 4
title: "One cellbox executable and complete caller/image migration"
status: completed-private-evidence
priority: P1
tier: thinking
dependencies: [1, 2, 3]
---
# Phase 04: Multicall cutover

## Overview and requirements

Deliver CBX-09..12 and CBX-14: one essential-suite artifact, real serial/GUI shell consumers, shared-backend behavior and no increased ambient authority. All P01 catalog/launch/identity approvals precede switching startup paths.

## Architecture and related files

Create `cells/tools/cellbox` with registry, shell module (`sh`), shared utility modules and applet entries. Migrate `cells/tools/shell` and selected sources from `cells/tools/sys-tools`/`net-tools`; reuse pure `libs/text-engine` logic. Update `Cargo.toml`, init launches, kernel launch profiles under their owner, scripts/image/sign lists, terminal launcher and every caller found in the catalog. Keep independent servers/workers/apps and related packages if still consumed.

## Implementation steps

1. Move rather than copy selected implementations into shared modules, retaining input/output/status behavior or the explicitly migrated contract. Keep cwd/variables/source/jobs/control flow inside the shell context.
2. Expose canonical `cellbox <command>` and `cellbox sh`; implement selected shell short-name resolution through the catalog. Preserve meaningful grammar aliases without forwarding ELF wrappers, basename assumptions or a second cellutils product.
3. Enforce P01's authenticated caller-edge × backend-right matrix under the common ELF ceiling: every command's file read/mutate, lifecycle/device, network/DNS, telemetry, credentials/key/trust and other rights need a real attenuating service handle or separately authorized worker. Exercise known privileged commands from restricted callers, not just unknown/forged modes. If a right cannot be enforced narrowly, keep its effect behind a separate worker/boundary rather than widening all invocations. An argv command/mode is never authorization.
4. Connect GUI Terminal to actual `cellbox sh`/foreground streams; prove real changing file/process/time output, two-session independence and the removal of fixed responses. Migrate init/foreground identities, argv/cwd/environment, path policy and image/sign lists as one reviewed set. Use admitted disposable candidate images for implementation evidence, then obtain each affected delta's CP2 from its exact diff/caller census/evidence before final caller/image retirement. Pending/blocked CP2 stops rollout; P05 later completes Ratatui/ANSI emulation, not the first real shell connection.
5. Update all affected retained image recipes, including compatibility maintenance of existing non-Intel build references if touched; this is not new platform qualification. Packaging must fail on build failure or missing fresh artifacts rather than embedding stale binaries.
6. Remove obsolete suite Cargo targets, source duplicates, wrapper binaries, dead paths/comments and incidental old wording tests after migrated behavior passes. Retain unrelated tools and standalone server modes with their own real lifecycle.
7. Boot private signed x86 images with desktop disabled and enabled. Exercise shell state/scripts, explicit/short invocation, real VFS/network/system observations, GUI shell sessions, external app launch and denied authority. Demonstrate each cataloged desktop backend operation through its corresponding CLI/shared typed service path; presentation-only window gestures are not new CLI business operations. Capture the artifact list and catalog coverage.

## Success criteria

- [x] Every catalog entry/caller is migrated or explicitly retained outside the suite; no silent command loss. **Met (2026-10-22):** ledger 73 entries with registry/catalog host tests; S4 census shows no remaining `/bin/shell` caller; C5 reconciled the desktop callers. See [closure review](phase-04-closure-review.md).
- [x] Shell state transitions and external byte pipelines behave correctly after the new startup identity/path is used. **Met on private QEMU/TCG fixtures only (2026-10-22):** Phase02 48, Phase03 60/60, Phase04 terminal 354/354, GUI 173/173 ×6.
- [x] Every cataloged ungranted right is denied for known commands under restricted caller edges, including network/DNS, telemetry and secret/key/trust access; final cutover has all affected CP2 approvals. **Met on private QEMU/TCG fixtures only:** S2-lite/S2c-min lifecycle, S2-K secret, S2-N network/DNS/listen/L2, S2-T/W sandbox X/M/W denials; CP2 approved for OOM, S2-K, S2-N and S2-T/W on 2026-10-22. Coarse per-Cell rights; scoped D03/D04 remain in Phases 08/06.
- [x] One suite executable is packaged; no fallback to an obsolete shell/utility artifact; independent apps/services still work. **Met on the promoted dev-posture x86 canonical bytes, QEMU/TCG only (2026-10-29, S10-5 CP2 granted by the user):** `kernel/src/embedded-x86_64` holds the qualified desktop image (`kernel_fs.img` `3848cefb…`, `init` `c0bd1458…`, hash-identical to candidate `4fae8b20…`) with exactly one `/bin/cellbox` plus the byte-equal `/bin/cellbox-ssh`, no `/bin/shell`, `/bin/sshd` or quarantined hash; standalone apps/services launch (L0 desktop `P10G-apps`, L6 desktop and CI boot). Non-Intel recipes are not covered (D10-10); CBX-27 and CBX-23/24/25 are separate. See [S10-5.13](phase-10-s0-proposal.md).
- [x] Headless boot/data commands need no viUI/compositor; every selected desktop backend operation has a demonstrated CLI/shared-service path, not a desktop-only implementation or CLI-table parser. **Met on private QEMU/TCG fixtures only (C5, 2026-10-22):** headless image has no UI task and cellbox has no UI crate; selected desktop operations (Terminal, Files create/copy/move/delete/browse, removed fake power actions and dead entries) go through the same VFS service/request types as cellbox, GUI runner 173/173 single click run. Ocel/rg/edit/ssh/Cell-top not selected.
- [x] GUI Terminal runs the real migrated shell and renders prompt/stock output through the compositor **on private QEMU/TCG fixtures only**, now using `CELLBOX_STREAMS=merged` single-writer tty routing. Five consecutive click runs plus one autoload run pass 103/103 each with ten strict stderr iterations. Non-GUI hosts without the entry retain distinct stdout/stderr. [Evidence](reports/s3-gui-runtime-evidence.md). GUI CP2 remains proposed; final retirement, resize/focus/multi-window/platform/full Phase04 acceptance remain open.

## Verification

Run behavior-focused existing affected tests plus the new path smoke once after edits. Compare signed artifact hashes with packaged bytes; compilation alone is not cutover proof. Status and actual mutations must be observed, not just command-help output.

## Assumptions

The approved suite can operate through safe scoped service contracts under one artifact ceiling. Verify the exact backend before merging a privileged command. If not, stop for the specific boundary decision; do not silently omit it or widen rights.

## Security and risk assessment

Path/caller identity is load-bearing in current policy. A separate invocation alone does not supply hardware isolation. Before rollout rollback is a matched previous image/policy plus cold boot; after file or external effects, preserve state and audit outcomes rather than replay. Do not introduce permanent shims to ease rollback.

## Deviation log

2026-10-20 — **Completion/cutover acceptance withdrawn; incomplete/blocked.** `app-cellbox` source and caller/build/image changes occurred. Historical F1/F5 observations reported zero unallowlisted unsafe code, and development signing reported `__ViCell_sig` on an approximately 88 KiB cellbox binary (88,576 bytes in the package record). These are source/check/signing/artifact observations, not proof of shell behavior, command coverage, authority attenuation or an accepted caller/image retirement. Review found the shell exits and GUI private command matcher remains; no serial/GUI integration proof or CP2 supports cutover. User design consent is preserved; owner/allocation admission remains incomplete. CBX-09..12/14 and all criteria remain pending/unchecked. Existing signed/package outputs are unqualified and rollout is blocked under [reports/rollout-containment.md](reports/rollout-containment.md).

2026-10-21 — Slice S1 admitted (CP1) and implemented: shell moved into `cellbox` with an identical ceiling and boot identity migrated; private-image evidence 48/48, 47/47, kernel AP 36/36. Remaining: S2 authority, S3 GUI, S4 retirement. See [entry dossier](phase-04-entry-dossier.md). Criteria unchecked.

2026-10-21 — S2-lite admitted and implemented (ledger + denial proof). Criterion 3 remains open: builtin `shutdown`/`kill`(cooperative)/`snapshot` are ungated (S2c). See [S2 proposal results](phase-04-s2-authority-proposal.md).

2026-10-21 — S2c-min (Option B) implemented: raw shutdown gated by reviewed caller name, builtins kill/snapshot/shutdown refuse, supervisor snapshot names removed. Criterion 3 still open for net/telemetry/VFS-scoped rights. See [S2c results](phase-04-s2c-lifecycle-proposal.md).

2026-10-21 — S3 implemented (userspace topology, no new ABI): GUI Terminal runs a real `cellbox sh` through `TerminalHost`; private-image evidence 53 checks incl. two independent sessions and Ctrl-C recovery. GUI render proof absent; stock-tool output does not reach the Terminal (ostd print routing). See [S3 results](phase-04-s3-gui-terminal-proposal.md).

2026-10-22 — **S3-output extension implemented and privately verified under approved CP1/amendments; full Phase04 remains incomplete and CP2/rollout BLOCKED.** Explicit `ostd::io::ProcessOutput`/`OutputStream` routes all eleven stock sys-tools callers' command data to stdout and usage/errors to stderr, using finite 30-second deadlines and existing CellSessionV1 transport. Only free gains GetTime; capability manifests stay unchanged. The separately approved non-default driver Path edge to stock echo has an empty child ceiling. Legacy no-context UTF-8 output and service logging remain on UART; legacy invalid UTF-8 fails without logging that write. Final private S3 stock echo/ls/cat/free proof passes **52/52 driver markers + 308/308 independent checks**, including byte-exact binary cat, fragmented/full-ring drain, independent hosts, missing-grant stock exits 1 and invalid-ref admission Stale **before helper execution**. Approved shell changes poll Ctrl-C while the host ring is full and retain unsent bytes; exact status mapping now preserves `$?`=130 (41 ms full-ring cancellation; close 120 ms). The preceding S3 stock-output gap was a historical observation and is **superseded**, not retroactively rewritten. Final Phase02 normal/kernel regressions pass 48/37; final Phase03 passes **60/60**, child count 4→4, with stable HLT=1 CPU samples at the DWARF-mapped shutdown loop despite absent banner/live VM. S3-output AC-8 is now checked following Main's final regression signal. The historical completion-queue boot selftest FAIL is retained. No GUI pixels/compositor proof, complete command migration/retirement, CP2 or rollout is established. [Runtime evidence and sanitized archive](reports/s3-output-runtime-evidence.md).

2026-10-22 — **GUI rendering criterion proven on the separately approved private desktop fixture; full Phase04/rollout still BLOCKED.** Full GUI CP1 included the guarded private Terminal-signing exception; separately approved drain-order option 1 changes the Terminal pump to stderr-first without merging endpoints or adding total-order metadata. Real taskbar-click launch and injected virtio keys render the actual shell prompt and stock echo/ls/cat, errors and exact Ctrl-C `cc=130`; run3 58/58 and fixed run4 **103/103**, including **10/10** exact stderr-before-prompt screens, UART negatives and unchanged saved canonical hashes. Run1's actual ordering defect is retained, as are the separate unisolated unconsumed-pipeline OOM and historical completion-queue selftest failure. GUI-5/6 newer guard/host/private regression results are parent-observed counts only, without archived raw logs; see [the report](reports/s3-gui-runtime-evidence.md). TCG/taskbar launch only, no autoload/resize/focus/multi-window/KVM/physical Intel/VMware proof. Earlier S3-output CP2 stays approved; new signing/Terminal/GUI-runner files need separate CP2. User rollout consent did not satisfy containment release conditions: no canonical packaging guard reopened, nothing unblocked, S2b/d/e, S4, other CBX rows and full qualification remain open.

2026-10-22 — **Fixed drain-order approach superseded by approved single-stream tty mode.** Later option-1 reruns passed only 1/4 (mirror race: stderr overtook the Enter newline); those failures/screens remain. Terminal now launches the shell with `CELLBOX_STREAMS=merged` and again drains stdout then stderr. Context-mode cellbox routes shell diagnostics/forwarded child stderr through its single stdout writer, ordered with echo/newline/prompt; non-GUI hosts retain separate streams. A child's own interleaved stdout/stderr remains unordered. New archived evidence: **6/6 full GUI runs**, five consecutive click plus one autoload, each **103/103** and **10/10** strict stderr iterations; host459, signing49, S3 exit0/309 PASS lines, P03 exit0/60 with halt samples16/17, P02 exit0/48, canonical hashes unchanged. Raw logs are now saved for those runs; older count-only observations remain historical. [Current GUI CP2](phase-04-s3-gui-cp2-dossier.md) is **PROPOSED**, no longer on hold and not approved; executor changed from its S3-output CP2 hash, shell_state did not. TCG only, no resize/focus/multi-window/KVM/physical Intel/VMware qualification; unconsumed-pipeline OOM and historical completion-queue failure remain. Full Phase04/canonical qualification/rollout stay **BLOCKED**, nothing unblocked.

2026-10-22 — OOM local-sink option A (approved “A”) verified: host 89/89, private terminal runner 314/314 incl. endless local-sink Ctrl-C 130/no OOM, P03 60/60, P02 exit0; GUI runner not rerun. See [OOM proposal results](phase-04-oom-local-sink-proposal.md). Remaining Phase04: S2 net/telemetry/VFS rights, S4 retirement, GUI CP2 rollout gates.

2026-10-22 — S2 rights CP1 drafted (VFS mutate/net-DNS/telemetry/secret-trust): [phase-04-s2-rights-proposal.md](phase-04-s2-rights-proposal.md). Awaiting selection; no source change.

2026-10-22 — S2-K implemented (approved “S2-K”, step 1 re-scoped to fail-closed): BootFS fallback no longer bypasses VFS refusals; reproduced leak before fix, terminal runner 327/327 after; P03 60/60, P02 exit0. See [S2 rights results](phase-04-s2-rights-proposal.md). Criterion 3 still open for N/T/W.

2026-10-22 — S2-N annex drafted: [phase-04-s2n-net-annex.md](phase-04-s2n-net-annex.md) (text review pending; D-N1..D-N4 open). No source change.

2026-10-22 — S2-N annex text approved (recommended D-N1..D-N4); E1–E6 resolved and folded in; frozen for CP1.

2026-10-22 — S2-N implemented under CP1 `d774b9c5…`: private runner 342/342 (7 net denials + curl admitted), P03 60/60, P02 exit0. See [annex §12](phase-04-s2n-net-annex.md).

2026-10-22 — S2-T/S2-W annex drafted (edge-bounded X/M/W; full D03/D04 deferred to Phases 08/06): [phase-04-s2tw-annex.md](phase-04-s2tw-annex.md). Text review pending.

2026-10-22 — S2-T/S2-W implemented under CP1 `57869333…`: runner 353/353 (sandbox X/M/W denied, /bin control allowed), P03 60/60, P02 exit0. See [annex §11](phase-04-s2tw-annex.md).

2026-10-22 — S4 retirement CP1 drafted: [phase-04-s4-retirement-proposal.md](phase-04-s4-retirement-proposal.md). Awaiting selection.

2026-10-22 — S4a done (applets deleted, recipes/docs fixed, cat exit 1; runner 353/353, P03 60/60, P02 exit0). S4b: four CP2 dossiers proposed (OOM, S2-K, S2-N, S2-TW). Criterion 5 still open; containment unchanged.

2026-10-22 — Four CP2s approved (OOM, S2-K, S2-N, S2-T/W). Criterion 3 checked within private-fixture limits. Open: criterion 5 (desktop backend ops ↔ CLI coverage), criteria 1/2/4 formal closure, KVM/GUI reruns; containment unchanged. S4a hunks are covered by the S4 CP1 only (no separate CP2).

2026-10-22 — Criterion-5 slice C5 drafted: [phase-04-c5-desktop-parity-proposal.md](phase-04-c5-desktop-parity-proposal.md). Awaiting selection.

2026-10-22 — C5 implemented (approved D-C5-1..3): GUI 173/173, headless 354/354, P03 60/60, P02 exit0. Criterion 5 checked within private-fixture limits. Delete-key input bug recorded.

2026-10-22 — User “tiếp tục” on the closure review taken as D-CL1 (check criteria 1, 2) and D-CL2 (prepare S4a/C5 CP2 dossiers). D-CL3 (Phase 05 entry) still needs an explicit decision.

2026-10-22 — CP2 approved for S4a and C5. Every Phase 04 delta now has CP2. Criteria 1/2/3/5/6 checked (private-fixture limits); criterion 4 open pending rollout containment release conditions. D-CL3 (Phase 05 entry) pending.

2026-10-22 — D-CL3 accepted by the user (“bắt đầu phase 5”): Phase 05 entry allowed with Phase 04 in_progress (criterion 4 only).

2026-10-25 — **Correction to the 2026-10-22 entry above.** "Every Phase 04 delta now has CP2" was wrong. The [S4a CP2](phase-04-s4a-cp2-dossier.md):153 excludes the S1, S2-lite, S2c-min and S3 hunks, and no CP2 record exists for them. A hash-only [CP2 mini-dossier](phase-04-s1-s3-cp2-dossier.md) now requests it; status: awaiting CP2. The criteria checkboxes are unchanged. Criterion 3's text ("final cutover has all affected CP2 approvals") is not satisfied for those four slices until that CP2 is recorded.

2026-10-25 — **CP2 approved by the user** for S1, S2-lite, S2c-min and the S3 launch edge, per the [mini-dossier](phase-04-s1-s3-cp2-dossier.md) (pre-amendment SHA-256 `8e84ead72f284d5f4b902c090888dd47774f20db3c8056902dfd90c47b5dcd1f`), including its deviations 1–7. Every Phase 04 delta now has a CP2 record, and criterion 3's "all affected CP2 approvals" clause is satisfied on the private-fixture evidence. The user also confirmed the catalog as frozen (`command-catalog.md` `f2a82fce61101f9f1dd6d62a75bece735e3eb9129a2295b187c2f49209485339`) and approved two cleanups: the `profiles.rs:148-150` comment and the `git rm` of the staged `snapshot_client.rs`. Criterion 4 and rollout remain blocked.

2026-10-29 — **Criterion 4 checked and Phase 04 `completed-private-evidence`; S10-5 CP2 granted by the user (2026-10-29).** The user approved S10-5 CP1 and the qualified candidate's desktop bytes were promoted to the canonical paths (hash-verified copy; V1–V7 PASS in [S10-5.13](phase-10-s0-proposal.md)); CP2 accepted deviations 1–8. The rollout guards G1/G3 and the canonical assembler route were reopened by S10-2b on content rules; the quarantined-hash guard stays. This is a packaging criterion at the x86 dev-posture QEMU/TCG ceiling: no physical, VMX, production or remote claim, no CBX-27 measurement claim, and non-Intel recipes stay refused.
