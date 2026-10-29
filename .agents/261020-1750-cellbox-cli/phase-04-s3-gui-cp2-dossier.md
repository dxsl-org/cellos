# S3-gui CP2 dossier — implementation review request

**Status: CP2 APPROVED by the user on 2026-10-22 (“Duyệt CP2 (9 file)”) for the nine inventoried files only (including the changed `executor.rs`). Quarantine, canonical packaging, release signing, GUI resize/focus/multi-window/KVM/physical proofs and rollout remain blocked.**

Prepared 2026-10-22 from source bytes and saved evidence; this documentation update runs no builds/tests. [Runtime report](reports/s3-gui-runtime-evidence.md), [tty evidence archive](reports/s3-gui-tty-evidence/exit-codes-gui.txt).

## Authority and preserved history

- User-approved [GUI-proof CP1](phase-04-s3-gui-proof-proposal.md), including the guarded private Terminal-signing exception.
- Approved [drain-order option 1](phase-04-s3-terminal-drain-order-proposal.md), subsequently **superseded**: run1 originally drew stderr over the prompt; the first run4 passed 103/103, but [later reruns](reports/s3-gui-rerun-evidence/README.md) passed only 1/4 and exposed the mirror race (stderr overtook the typed Enter newline). Original failed/screens/logs remain unchanged.
- User approved the userspace tty-merge proposal in full. No new kernel ABI, syscall ceiling, launch row, capability or compositor change is claimed by tty mode.
- The earlier [S3-output CP2 baseline](phase-04-s3-output-cp2-dossier.md) stays approved **for its recorded hashes only**. This request covers the GUI/signing/tty files below and the changed executor; approval is not inherited from the older baseline.

## Implemented contract

The GUI Terminal launches its shell with `CELLBOX_STREAMS=merged` in the committed environment. `context_session::streams_merged` reads the exact entry; executor's context-mode routing sends diagnostics and forwarded child stderr through stdout, alongside shell echo/newline/prompt, from the shell's single writer. This orders **the shell's writes** without inventing authority from argv or adding an endpoint ABI token. Terminal draining is restored to stdout-then-stderr; the `Streams` scripted-test boundary remains. The GUI driver's pre-launch control is mode-aware for autoload; the ten-iteration stderr oracle remains strict.

Without that entry, non-GUI/fixture hosts keep distinct stdout/stderr and the existing S3 separation behavior. A child's own interleaved stdout/stderr still arrives through two child rings and is **not globally ordered**; full ordering would require an ABI ordering token. No claim is made that merged tty mode solves that separate boundary.

## Evidence now available

Saved raw runner/host/signing logs are under `reports/s3-gui-tty-evidence/`, unlike the initial count-only GUI-era observations. The parent verified results from those saved files; the documentation worker read/hash-inventoried the archive without rerunning the checks.

| Lane | Saved result |
|---|---|
| GUI | **6/6 exit 0**: five consecutive click runs + one autoload, **103/103** checks each, **10/10** strict stderr-before-prompt screens per run; prompt/stock stdout/error/Ctrl-C exact pixels, UART negatives and unchanged canonical hashes |
| Host suites | **459 passed, 0 failed**, 6 ignored: terminal 35, app-cellbox 86, terminal-sdk 49, ostd 59+7, api 128+2, text-engine 74, ostd doctests 19; [raw log](reports/s3-gui-tty-evidence/host-tests.log) |
| Signing guard suite | **49 tests OK**; [raw log](reports/s3-gui-tty-evidence/sign-tests.log) |
| S3 | exit 0, **309 PASS lines including summary**, driver 52 checks; not 309 independent checks; [runner log](reports/s3-gui-tty-evidence/s3.runner.log) |
| Phase03 | exit 0, **60 PASS**, child count 4→4; halt proven with samples **[16,17]**, RIP `0xffffffff80039571`, HLT=1, IF=0, CPL=0; [probe](reports/s3-gui-tty-evidence/p03/final-probe.json), [runner log](reports/s3-gui-tty-evidence/p03.runner.log) |
| Phase02 | exit 0, **48 passes**, orphan ticks 4; [runner log](reports/s3-gui-tty-evidence/p02.runner.log) |
| Supplementary parent observations | app-cellbox `--features applets` **88 pass**; app-cellbox+Terminal bare-metal check clean. No separate raw logs for these supplementary checks are in this archive |
| Ceilings | Saved GUI evidence: cellbox PASS; Terminal only the previously approved SetTimer delta, no new tty-mode ceiling change |

Canonical artifacts remain unchanged: cellbox `64eb28f3be3d08bf2560d6eb0b58bee4a33ff8ea3439bcd5ed6e1ccc42f84a39`, terminal `636497acc2bdf735471173c5c428e61d16b1d8c870935cfe868e6d0120e52fa2`, `kernel_fs.img` `447d0348735a25e8b396c11c0091ff3d214b0c0b392043bb0515d4104a298e2c`. Per-run saved before/after files and signed private provenance remain in the archive. [Saved-evidence hashes](reports/s3-gui-tty-evidence/archive-hashes.json) are an integrity inventory, not an original-to-copy claim against unavailable workdirs.

## Proposed implementation inventory — current disk SHA-256

| Path | Bytes | SHA-256 |
|---|---:|---|
| `scripts/cellos_sign/signing.py` | 8328 | `e1ec59c35887d6ea14a5631c565d9307121b91d5a4fed329be0045b48063b985` |
| `scripts/test_cellos_sign.py` | 27680 | `bfb3d6dc620c8de817ccce65c82843a5704ddd6d3c8df3f9d145b4588fc46fee` |
| `cells/apps/terminal/src/session.rs` | 15537 | `bb9ee4976b23dab35181f7991f91ac8a1a9cf259c8572b9b65520fe86142e1a2` |
| `scripts/qemu-phase04-gui-test.sh` | 22241 | `795df7ff2880f19ff10d7f27367b36329764030d2cf74a5d3f14eecb07e701e7` |
| `scripts/phase04_gui_drive.py` | 29859 | `8cbd955b46c5b3e693a532ec18303fe88aaa447c5663ef9df564ec41e3bb40d7` |
| `scripts/phase04-gui-glyphs/Cargo.toml.in` | 442 | `ff464ceb96d8740d5a7f87498b9a7851bcd21d4cfa4eac2cade0fd5e98063215` |
| `scripts/phase04-gui-glyphs/src/main.rs` | 3196 | `79967ee3da85c771a0e5b547f3cc2ac20f154c185a691fdcd2dae5a1be7824ae` |
| `cells/tools/cellbox/src/context_session.rs` | 11669 | `c2bc83b87df44c20646978e981617969d520a34b0d280d358f394e4abc915bff` |
| `cells/tools/cellbox/src/executor.rs` | 139053 | `e123054d6d8a9a9ada475042f6a4fba8f8db62dda0692edccec52e125c7c88c1` |

### Relationship to approved S3-output source hashes

`executor.rs` was already in S3-output CP2 and **has changed since that approval**: old `28278b5e2488a5adc144620d2e19407fb8c41ce98a705c4a699f8a5879b1a614` (131742 bytes) → current `e123054d6d8a9a9ada475042f6a4fba8f8db62dda0692edccec52e125c7c88c1` (139053 bytes). The changed bytes require this new decision; older approval is not extended automatically.

`shell_state.rs` was also in that approved baseline but is **unchanged on disk**, not a new scope item: old = current `20b4b89151562593003ebc10d81da82c2e51a6c4d219b64dcf737e11c3104ec4` (8749 bytes). It is therefore intentionally not added to the new implementation inventory. The earlier GUI addendum hashes remain historical snapshots, not current hashes for the changed Terminal/driver files.

## Remaining weaknesses and exclusions

1. A child's internal stdout/stderr interleaving remains unordered; tty mode guarantees the shell writer's echo/newline/diagnostic/prompt order, not arbitrary child-event ordering.
2. Six runs/60 strict stderr iterations are repeat evidence, not exhaustive scheduling or a zero-failure-rate guarantee. TCG only; GUI resize/focus/multi-window/KVM/physical Intel/VMware remain unexercised. Autoload is now exercised once, superseding the former autoload gap.
3. Separate unisolated shell OOM on endless `top | to jsonl` without a consumer and the historical `COMPLETION-QUEUE` selftest failure remain disclosed, not fixed or hidden by passing assigned gates.
4. Private signing relaxes only the reviewed fixture-name guard: `CELLBOX_PRIVATE_FIXTURE_SIGNING=1` + explicit non-dev seed required, quarantined hashes always refused. Canonical packaging/release guards remain untouched.
5. Raw logs now close the earlier **tty rerun** evidence gap for GUI/host/signing/S3/P03/P02. They do not retroactively create logs for the initial deleted runs; supplementary applet/bare-metal observations are separately labeled above.

## What CP2 would authorize

Accept the **current nine-file inventory** as the GUI-proof/tty-mode/signing-fixture baseline, including the changed executor hash. Later Phase04 work may then rely on that exact implementation and scoped evidence. This would **not** lift quarantine, authorize canonical packaging/release signing/rollout, complete Phase04/other CBX rows or accept the remaining limitations as implemented features.

## Options

A. **Approve this CP2 baseline** at the exact current hashes and private-fixture evidence scope above.

C. **Not yet** — retain the implemented evidence and withhold this CP2.

No approval has been recorded for either option. The earlier option-B hold/rerun decision and subsequent failed drain-order runs remain historical evidence, not a current request to repeat already completed checks.

## Rollout and rollback

The user's rollout consent does not satisfy the [containment release conditions](reports/rollout-containment.md#release-conditions): S2b/d/e, S4, other CBX/qualification/dependency/authority prerequisites and affected CP2s remain open. **Nothing was unblocked; rollout stays BLOCKED.**

Rollback must revert the tty-related Terminal/context/executor/driver changes as a matched set; reverting only the drain order reinstates the demonstrated race. Reverting the larger GUI/signing slice must keep its guard/tests in sync. No persisted-state rollback is claimed; already delivered stream/file bytes cannot be recalled.

## Decision recorded
Option A: the nine-file inventory above is the S3-gui baseline. `executor.rs` supersedes its S3-output CP2 hash by this approval. Rollout was not unblocked: the [containment](reports/rollout-containment.md) release conditions are unchanged.
