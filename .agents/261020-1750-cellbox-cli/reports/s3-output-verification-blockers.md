# S3-output observed verification blockers

Status: **Historical failed-run record, superseded by the user-approved fixes and [final evidence](s3-output-runtime-evidence.md).** Final S3 passes 52/52 driver markers and 308/308 independent checks; full-ring Ctrl-C returns130 in41ms. Final Phase03 passes60 gates with DWARF-bound HLT/RIP/IF proof. The historical completion-queue selftest failure remains disclosed, not fixed or newly attributed. CP2/rollout unchanged. Details below preserve the original failures and requested closure, not current unresolved status.

## Verified

Host baseline 403 passed, final helper/caller suite 414 passed (11 suites, 6 ignored); all eleven sys-tools binaries, Terminal, terminal-sdk and cellbox built x86_64 bare-metal. Independent helper/caller review found no issues. Only free gains GetTime under approved amendment.

Latest actual private S3 image: `/tmp/cellbox-phase04-stock-amended-4nXZMcHM/`. 51/52 driver markers passed; 305/308 independent checks passed. Remaining three check failures derive from one real cancellation status failure. Stock echo/ls/cat/free host byte paths, separate stderr, binary cat (1600 and 98368 bytes), fragmented listing, two hosts, downstream close (100ms), legacy free log, VFS/input logs and 193 no-UART binary canaries passed. Signed stock echo missing stdout/stderr/both was published then exited 1 with empty captures; invalid reference admission returned Stale before helper execution. Do not conflate those cases.

## 1. Full-ring Ctrl-C status failure — requires shell closure amendment

Actual diagnostic: `stock-cancel elapsed_ms=32 sent=Ok(()) key=Ok recovered=Ok(()) probe=Ok(["stockcc=1"])`. Required 130 is NOT met. Do not change the oracle to accept 1.

Relevant source boundary: `cells/tools/cellbox/src/executor.rs:153-185` forwards context terminal output while holding SERIAL_SESSION and backs off on a full host ring without polling shell input. External pump at `3069-3085` calls this path; Ctrl-C polling lives separately in ExtRun::poll_interrupt at `2841-2864`. Settlement at `3124-3132` maps StreamStop::Failed to 1 rather than Interrupted to 130. Exact causal transition still needs targeted diagnosis; these source facts identify the required shell owner, not proof of a complete fix.

Proposed extra closure: cellbox executor.rs (and context_session.rs/typed.rs only if diagnosis requires), adjacent behavioral tests and current private driver. No new kernel/ABI/rights. Preserve byte prefixes, actually observe Ctrl-C and cancel/reap the active child operation; do not manufacture status130 from arbitrary output errors.

## 2. Phase03 shutdown observation failed — needs independent halt proof

Regression runner `/tmp/cellbox-phase03-s3-normal-Mv8rO3aI/` passed all other checks but failed final shutdown worker probe: no UART banner, no new prompt, VM still alive. Migrated shutdown enqueues its banner into stdout then invokes unchanged raw shutdown. Existing x86 accepted syscall loops hlt with IF clear; it can stop CPU before parent shell renders queued banner. This is PROBABLE from source plus captured log, not independently observed halt proof. Do not restore context UART logging or add a guessed sleep.

Proposed verification closure: `scripts/qemu-phase03-pipeline-test.sh` only, add owned private QMP/HMP channel and record CPU HLT/RIP/IF; require HLT=1 at the symbolized shutdown loop, not merely query-status=running. Preserve all existing denial/worker admission checks; absence of rendered banner must not alone claim syscall failure. Guaranteed pre-halt banner rendering would require a separate drain acknowledgment contract and is not promised here.

## 3. Historical completion selftest failure — not newly attributed

Normal P02 and S3 serial logs include `COMPLETION-QUEUE: FAIL — an older waiter erased its replacement` (lines107-109). This exact failure already appears in archived pre-slice runs. P02 assigned gates passed48 and kernel AP gates37/37. Report the boot selftest failure separately; do not call the full boot clean or change kernel source under this slice.

## Artifact limits

Private working directories retained for diagnosis; no canonical packaging or rollout authorized. GUI pixel/compositor proof remains absent. Current recorded results are failures/partial proof, not completion or acceptance. Further changes require approval for the named additional closure.
