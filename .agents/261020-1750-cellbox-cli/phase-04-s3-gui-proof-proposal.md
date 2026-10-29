# Phase 04 S3-gui — GUI Terminal proof on a private desktop image (CP1 proposal)

Status: **CP1 APPROVED in full by the user on 2026-10-22 (“Duyệt đầy đủ kể cả ngoại lệ ký terminal”); implemented and privately verified.** GUI-1..GUI-6 are satisfied only within the [runtime report's evidence limits](reports/s3-gui-runtime-evidence.md): run4 **103/103** exact-screen/stream/hash checks, including **10/10** stderr-before-prompt iterations after separately approved [drain-order option 1](phase-04-s3-terminal-drain-order-proposal.md). GUI-5 signing tests and GUI-6 newer regression/host results are parent-observed counts, without archived raw logs. No full Phase04 acceptance or rollout unblocking.

## Goal
Close the one S3 gap the headless runs cannot: show, on a real compositor frame buffer, that the GUI Terminal hosts a real `cellbox sh`, accepts keyboard input and renders stock command output (echo/ls/cat) from the committed stdout/stderr grants.

## Evidence at proposal time (historical; superseded where noted in results)
- The Terminal is only compile/unit tested: no desktop image was built for S3 (`scripts/qemu-phase04-terminal-test.sh` header, §G3f).
- `scripts/package-desktop-x86.py` refuses unconditionally (`ROLLOUT BLOCKED`) and writes the canonical `kernel_fs.img`; it must not be used or edited.
- **`scripts/cellos_sign/signing.py:31-34,104-120` refuses `terminal` unconditionally**, even under `CELLBOX_PRIVATE_FIXTURE_SIGNING=1` (which admits only `cellbox`/`app-cellbox`). A privately signed GUI Terminal therefore cannot exist today without changing a containment guard. This is the main authority question below.
- Prior desktop harness (`tests/integration/src/lib.rs` `boot_x86_bios_with_accel`, QMP screendump + key injection) and the existing S3 private runners are reusable patterns; the canonical desktop ISO and `build/cellos-x86_64-desktop*.qcow2` are not.
- Terminal launch edges: `terminal -> /bin/cellbox` exists (always compiled). Who launches `/bin/terminal` in a desktop image, and which compositor/GPU/input cells a private image needs, is **not yet established**; the scout step below must settle it from source before any edit.

## Proposed change (all private, nothing canonical)
1. **Scout (read-only, no approval beyond this CP1):** enumerate the minimal cell set for compositor + Terminal + input + display on q35, their existing launch rows, and the QEMU devices (virtio-gpu or the existing display path) the kernel supports. Report before any edit.
2. **Private runner** `scripts/qemu-phase04-gui-test.sh`, modelled on the S3/Phase03 runners: throw-away signing key, private cells/kernel target dirs, private embedded dir, owned QEMU with `-nic none`, an owned Unix QMP socket in a private temp dir, evidence saved sanitized, all outputs invalidated first, refuses aliases of the canonical/quarantined artifacts.
3. **Driving and observation:** QMP key injection (`send-key`/input events) into the focused Terminal window; QMP `screendump` of the guest frame buffer; serial markers for process/launch facts. A fixture-only driver is allowed only through existing launch edges plus feature-gated rows exactly like `phase04-driver-profile`; **no hook is added to the production Terminal** to expose its grid.
4. **Pixel proof:** deterministic comparison of screendumps. Preferred: render the expected text with the repository's own font/typography code on the host and match the glyph cells in the Terminal region; fallback: OCR only if a tool is installed (none was found: `tesseract` absent), so the first design is glyph-template matching plus region-diff controls. Exact method is finalized in the scout step and reviewed before use.
5. **Scenarios (each needs before/after screendump and a negative control):** shell prompt appears in the window; typed `echo gui-proof-1` shows stdout in the pane; `ls` of a fixture directory shows entries; `cat` of a fixture text file shows its text; a missing path shows the stderr line; Ctrl-C on a long-running stock output returns to the prompt; window/resize or focus change is exercised only if the compositor already supports it, otherwise reported as not exercised.

## Source closure proposed and approved by the full CP1
- New: `scripts/qemu-phase04-gui-test.sh` and sanitized evidence under `.agents/.../reports/`.
- **Signing guard (needs explicit approval):** allow the name `terminal` to be signed **only** when `CELLBOX_PRIVATE_FIXTURE_SIGNING=1`, an explicit non-default seed is supplied (same rule as cellbox), and the bytes are not the quarantined hash; extend `scripts/test_cellos_sign.py` so refusal of the default path and of the quarantined hash stays tested. No other guard changes. `package-desktop-x86.py` and `make-iso-ci.sh` guards stay untouched.
- **Fixture-only launch rows (needs explicit approval if the scout finds them necessary):** feature-gated rows in `kernel/src/loader/launch_profile/{profiles,tests}.rs` under a new non-default feature, never in ordinary images.
- No change to production Terminal, compositor, input service, ABI, syscall ceilings or capability manifests unless the scout proves a real defect; any such fix returns to CP1 with the exact failing evidence.

## Acceptance criteria (private-fixture scope)
- [x] GUI-1: A privately signed QEMU/TCG desktop boots with the compositor; saved provenance records source HEAD, kernel/image/cell hashes; saved canonical-before/after hashes are unchanged.
- [x] GUI-2: The Terminal launched by a real taskbar click shows the real `Cellos >` shell prompt with exact pixel-region matches and pre-window/wrong-prompt negative controls.
- [x] GUI-3: Virtio keyboard events injected through QMP produce exact typed lines and real stock echo/ls/cat stdout in the pane; distinctive payload UART negatives pass.
- [x] GUI-4: Stock missing-path stderr is visible before the next prompt in all 10 consecutive run4 iterations; flowing external pipeline Ctrl-C returns a stable prompt and exact `cc=130`. Run1's actual ordering failure is preserved.
- [x] GUI-5: Saved canonical cellbox/terminal/kernel_fs.img hashes are unchanged; parent observed **49 signing tests OK** (previously 41), including the guarded Terminal exception/default refusal cases. Raw test output was not archived.
- [x] GUI-6: Parent observed post-signing private S3 exit0 (309 PASS lines including summary), Phase03 exit0/60 PASS with CPU halt proof, Phase02 exit0/48 passes; Terminal/terminal-sdk host suites 33/49 pass and Terminal bare-metal check clean. **Counts only; no archived raw logs.** This row is limited to these runners/suites, not all regressions, boot selftests or full acceptance.

## Out of scope / still blocked
Rollout, canonical packaging, release signing, quarantine removal, multi-window/tab behavior, physical Intel or VMware proof, other CBX rows, Phase04 S2b/d/e and S4. A passing GUI run would be private fixture evidence, not release acceptance.

## Verification and boundary
Throwaway keys/sockets/images live only in an owned `/tmp/cellbox-phase04-gui-*` directory, are deleted after sanitized evidence is saved, and only the owned QEMU is stopped. Failed runs are reported with logs, never rerun to confirm and never accepted by weakening the oracle.

## Rollback
Delete the new runner, revert the signing/test edit and any feature-gated rows together; no persisted state. Canonical artifacts are never touched.

## Approval requested
Select which parts to approve: scout only; scout + private runner without guard change (likely cannot sign `terminal`, so the proof may be unreachable); or the full set including the guarded `terminal` private-signing exception.

## Results and current boundaries (2026-10-22)

New private runner `scripts/qemu-phase04-gui-test.sh`, driver `scripts/phase04_gui_drive.py` and host `scripts/phase04-gui-glyphs/` renderer exercise the actual compositor/Terminal/input path, not a private grid-export hook. Run3 passed 58/58; run4 with the approved Terminal fix passed 103/103, with required text regions at **0 differing pixels**, negative controls, stock output, UART negatives and unchanged saved canonical hashes. The glyph renderer uses ostd TextFonts and masks the cursor cell. Evidence: [run4 verdict](reports/phase-04-gui-evidence/run4-drain-order/verdict.json), [provenance](reports/phase-04-gui-evidence/run4-drain-order/provenance.json), [full report](reports/s3-gui-runtime-evidence.md).

The signing guard now admits names `{cellbox, app-cellbox, terminal}` only with `CELLBOX_PRIVATE_FIXTURE_SIGNING=1` plus an explicit non-dev seed. Exact quarantined ELF hashes remain unconditionally refused; canonical desktop/ISO packaging guards remain closed. This is the explicitly approved private exception, not general release signing.

Run1's stderr-over-next-prompt defect (one occurrence among the first three runs; rate unknown) is preserved; separately approved option 1 drains stderr before stdout and adds two host tests and the strict ten-iteration GUI oracle. Cross-endpoint total ordering is still not guaranteed. A separate unconsumed `top | to jsonl` endless-pipeline OOM remains observed/unisolated, not fixed by this slice. The historical completion-queue selftest failure remains disclosed.

TCG/taskbar-click only; autoload mode, GUI resize/focus/multi-window, KVM, physical Intel and VMware were not exercised. The original private workdirs were deleted; the sanitized GUI archive is the only saved source, with a new integrity hash inventory but no original-to-copy manifest. No keys, sockets, VM disk/ISO images or executables are archived.

The earlier S3-output CP2 baseline remains approved; these new signing/Terminal/runner files are outside it until separately approved. The user's “duyệt rollout” does not satisfy the still-open containment release conditions: nothing was unblocked, full Phase04/other CBX/S2b/d/e/S4 and canonical qualification remain open, and rollout stays **BLOCKED**.
