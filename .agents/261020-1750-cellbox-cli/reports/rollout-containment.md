# Cellbox acceptance withdrawal and rollout containment

**State: BLOCKED / NOT QUALIFIED.** This supersedes earlier claims that ten phases or all CBX-01..29 passed. Maintainer design consent (`tôi duyệt, làm tiếp đi`) remains recorded; it is not runtime acceptance, missing owner/codec evidence, CP2 or approval of unimplemented behavior. The maintainer subsequently requested correction of acceptance records and rollout blocking before feature work continues.

## Confirmed basis

The review inspected actual source and ran a throwaway Rust program importing the real stream/editor modules. Restricted endpoint duplication to a foreign principal succeeded; descendant read/write remained usable after parent operation cancellation; Backspace after `é` panicked; `\n` roundtripped to empty text; mixed CRLF/LF changed. These are unresolved, not fixed by containment.

`cellbox sh` exits after a banner. GUI retains its private command matcher. Dispatched SSH/editor and typed transform applets return success without the required effects. Cell identity is inferred from task names with generation fixed to 1; `ps` emits fixed rows. No Ratatui/Sunset/ripgrep component dependency was integrated. Raw shutdown bypass remains exposed. Host tests, cross-compilation, F1/F5, signatures and image packaging do not demonstrate these contracts.

All 29 acceptance rows are reopened as pending. All ten phases are blocked/incomplete; observed partial implementation is retained for inspection, not delivered functionality. No CBX pass, signed-image qualification, guest/host SSH proof, safe-save proof or CP2 closure is claimed. Prior observations of 51 text-engine and 17 terminal/TUI host tests are historical narrow evidence, not rerun or promoted to acceptance here.

## Existing artifacts: quarantined in place, not deleted or rewritten

| Path | Bytes | SHA-256 observed before containment verification |
|---|---:|---|
| `target/x86_64-unknown-none/release/cellbox` | 88576 | `64eb28f3be3d08bf2560d6eb0b58bee4a33ff8ea3439bcd5ed6e1ccc42f84a39` |
| `target/x86_64-unknown-none/release/terminal` | 595032 | `636497acc2bdf735471173c5c428e61d16b1d8c870935cfe868e6d0120e52fa2` |
| `kernel/src/embedded-x86_64/kernel_fs.img` | 16777216 | `447d0348735a25e8b396c11c0091ff3d214b0c0b392043bb0515d4104a298e2c` |

These bytes were signed/packaged by the previous run. Signatures are **not revoked** by this correction and do not prove qualification. No unknown pre-existing kernel, ISO, policy or image is called safe or restored from Git: those bytes and user changes are preserved. Do not boot, copy or distribute these quarantined candidates as a release. Already copied artifacts remain outside these local gates; no deployment discovery, remote recall or runtime stop is claimed.

## Executable containment

- `scripts/package-desktop-x86.py` refuses unconditionally at entry with `ROLLOUT BLOCKED`, before signing, reading/replacing the canonical image or invoking other tools. Removing cellbox from the map alone would preserve it in the existing image, so it is not sufficient containment.
- `scripts/cellos_sign/signing.py` preflights the whole signing batch before loading a signer/key or mutating any target. Names `cellbox`, `app-cellbox`, `terminal` (including resolved symlink aliases) are refused, as are renamed copies of the two exact quarantined ELF hashes. This deliberately blocks all terminal candidates until reviewed release, not just the first bytes.
- `scripts/x86/make-iso-ci.sh` refuses the canonical/default kernel input before invoking xorriso, installing staging files or overwriting ISO output. Independent explicit fixture kernels remain available through existing inputs; selecting an alternative is **not** qualification or an override of Cellbox quarantine.
- No new CLI/environment bypass is provided. These are trusted-workflow accident guards, not an anti-malicious-developer boundary. They cannot prevent manual copying, modified/renamed/rebuilt bytes outside the named/hash checks, low-level unchecked signing, arbitrary third-party packagers or execution of an already signed artifact. Existing unsafe signing test escape hatches do not authorize release.

## Release conditions

Keep rollout blocked until the missing authority/owner/codec prerequisites are settled, actual implementation replaces no-ops and inferred identities, failures above have regression and target evidence, dependency/license/audit closure exists, and each affected CP2 is explicitly approved against exact implementation/caller/image evidence. Then use a reviewed source change to reopen only the qualified routes. There is no auto-unblock from a Markdown status, test count, design consent or signature.

Recovery requires a separately verified matched known-safe source/kernel/init/Cell/policy/image set and cold boot under owner control. No such rollback is executed here; file writes, trust decisions or remote actions (if any occurred elsewhere) cannot be undone by binary rollback.

## Verification

Executed containment checks (no signing or image writes):

- `python3 -B scripts/test_cellos_sign.py`: **41 tests passed**, including six new containment regression methods. They exercise whole-batch refusal without modifying earlier targets, both symlink name orientations, desktop packager refusal preserving an existing image, and default ISO refusal preserving an existing output even when its canonical kernel path is a symlink. Two symlink-orientation findings from containment review were corrected before this run.
- `bash -n scripts/x86/make-iso-ci.sh`: exit 0.
- Actual `python3 -B scripts/package-desktop-x86.py`: exit 1, `ROLLOUT BLOCKED`, before signing or image mutation.
- Actual `bash scripts/x86/make-iso-ci.sh` with default kernel selection: exit 1, `ROLLOUT BLOCKED`, before external ISO tools or staging writes.
- Throwaway renamed copies of both signed quarantined ELFs were refused by `sign_and_verify` on their exact byte hashes; no signer/key/objcopy was invoked.
- SHA-256 comparison before/after those smoke runs matched for all three quarantined artifacts in the table. They remain in place unchanged.

Final in-memory saved-artifact checks: **29/29 CBX rows pending**, **10/10 phase frontmatters blocked**, no checked phase success criteria, plan blocked, and all **five candidate document digests unchanged and matching** their manifest. Across 21 corrected records, 154 relative file links resolved; fragment anchors were not validated. Three modified Python files parsed successfully. Quarantine artifact hashes still match the recorded originals. These checks establish containment/document consistency only, not feature acceptance.

No feature implementation, signing, image rebuild, VM launch, network exposure or deployment was performed in this correction.

Independent containment re-review: no remaining findings after both symlink corrections. Read-only record consistency review found no live false Cellbox completion/acceptance claim; secondary summaries saying “pending or blocked” refer to the authoritative ledger's all-pending state, not passes. No feature defects were fixed or accepted by these reviews.

## Update 2026-10-21
`kernel/src/embedded-x86_64/kernel_fs.img` no longer matches the recorded hash: it was rewritten at 15:18:44 by a desktop fixture build outside the Cellbox work (see [Phase 04 S1 results](../phase-04-entry-dossier.md)). `cellbox` and `terminal` ELFs are unchanged. All containment guards remain; nothing here is qualified.

## Update 2026-10-22 (guard exception; release conditions unchanged)
The user approved a **private-fixture-only** signing exception (see [S3-gui CP1](../phase-04-s3-gui-proof-proposal.md)): `scripts/cellos_sign/signing.py` now admits `terminal` (with `cellbox`/`app-cellbox`) only when `CELLBOX_PRIVATE_FIXTURE_SIGNING=1` and an explicit non-dev seed are supplied; the quarantined byte hashes remain refused unconditionally, and `package-desktop-x86.py` / `make-iso-ci.sh` guards are untouched. The unconditional-`terminal` refusal described above is therefore historical. `scripts/test_cellos_sign.py` now runs 49 tests. This does **not** change the release conditions: rollout remains BLOCKED. Evidence: [GUI runtime evidence](s3-gui-runtime-evidence.md).
