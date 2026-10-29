# S3-output private runtime evidence

Date: 2026-10-22. **S3-output slice implemented and privately verified; base AC-1..AC-8 proven within the limits below, after Main's final Phase03 completion signal. Its [CP2 baseline was subsequently approved](../phase-04-s3-output-cp2-dossier.md), without lifting containment.** The headless runs recorded here did not exercise GUI pixels; later [separate GUI proof](s3-gui-runtime-evidence.md) now establishes real Terminal rendering on a private TCG desktop fixture only. Full Phase04, canonical packaging/quarantine removal and rollout remain **BLOCKED**; new GUI/signing/drain-order files are outside the earlier CP2 until separately approved.

Authority: [base CP1](../phase-04-s3-output-proposal.md), approved [free-only GetTime amendment](../phase-04-s3-output-deadline-amendment.md), approved [private stock echo edge](../phase-04-s3-output-negative-grant-amendment.md), and the separately approved shell interrupt/exact-status and Phase03 halt-observation closures. These are implementation/private-verification permissions, not release acceptance.

## Final S3 run and durable evidence

- Runner: `scripts/qemu-phase04-terminal-test.sh`; final source run `/tmp/cellbox-phase04-stock-final-ZNkqenVH`.
- Result: **52/52 driver markers and 308/308 independent checks pass**; [full verdict](s3-output-evidence/verdict.json), [serial capture](s3-output-evidence/serial.log), [signed artifact provenance](s3-output-evidence/provenance.json).
- Execution: private signed x86 image, QEMU q35/TCG, qemu64 + pdpe1gb, 256 MiB, desktop/display disabled, `-nic none`; [sanitized argv](s3-output-evidence/qemu-argv.txt). No production GUI image is implied.
- Kernel features: `default+phase02-terminal-profile,phase03-filter-profile,phase04-driver-profile`. `/POLICY.BIN` is absent and the kernel applies its development-permissive posture. The signing tool re-verifies each signed ELF; the signing-required kernel verifies at spawn. This profile is not a production policy qualification.
- Provenance records source HEAD `3471028bc8b4db14275553b4740c17e24fa4f32b`, nightly rustc 1.97.0 (`f53b654a8`, 2026-04-30), Cargo.lock SHA-256 `f73f85ac0fb47825898734b23df186a1a639422a79fc7cc2b8049ffc30bac1e2`. HEAD is provenance context, **not a claim that the exercised worktree equals that commit**.
- Image SHA-256: `92ef44d049902693a0af836dd1a5b04701a0b92de3a0a16a7846776d3b711b25`; kernel SHA-256: `1c13debd0be4a16b3b9753676f12f8df72d8b3d343ec45e4d84f6332ee063265`.

The [copy manifest](s3-output-evidence/copy-manifest.json) records source/copy hashes, lengths and sanitization. Captures and expected binary fixtures are copied byte-for-byte; text source working-directory paths become `<PRIVATE_S3_WORK>`. Only explicit evidence files are retained: **no signing seed/key file, socket, image, executable or build tree is copied**. The public trust-anchor identity is retained only as a provenance hash. The archive includes empty stderr/negative captures, not just successful stdout excerpts.

## Implemented output contract

`ostd::io::ProcessOutput::read_own() -> Result<Self, Status>` reads immutable own context once per command and retains its operation and independently routed stdout/stderr grants. `stdout()`/`stderr()` return private-field `OutputStream` references. There is no process-wide authority cache or public raw-handle constructor.

`OutputStream::write_all(&self, bytes: &[u8], deadline_ns: u64) -> Result<(), Status>` takes a finite, nonzero absolute monotonic deadline. Existing correlated CellSessionV1 WriteStream transport sends at most 512 bytes per request; only positive acknowledged progress advances the slice. Kernel blocking supplies backpressure/wakeup. Zero/invalid progress is an error. A committed prefix is never replayed after failure. Each command emission computes a **30-second** deadline; no infinite timeout or arbitrary yield-count retry replaces it.

All eleven stock sys-tools callers migrate to explicit data stdout and usage/error stderr: echo, ls, cat, date, env, free, uname, ps, kill, shutdown and hotswap. Their existing authority operations remain separate. Actual S3 runtime stock-command proof is for **echo/ls/cat/free only**, plus legacy init/free. Other signed/packaged commands are not silently claimed as exercised; no new route was added merely to launch them. Cat retains its existing path-argument interface, without a new stdin mode.

Only `ReadOwnContext = Denied` selects legacy console logging. Any other context-read error, a committed context missing either output grant, or an endpoint error fails closed without logging command payload. Context writes preserve arbitrary bytes; legacy writes require valid UTF-8 and return `UnsupportedInput` for invalid UTF-8 without logging that write. Legacy UTF-8 free is runtime-proven; invalid-UTF-8 rejection is host helper coverage, not a separate legacy binary image run. Global `io::print*` and explicit service logs remain logging APIs.

## Acceptance evidence

| Base AC | Observed proof | Limits |
|---|---|---|
| AC-1 | Stock echo stdout is exactly `p04-ext two\n` (12 bytes), stderr empty; independent no-command-UART check passes | Real signed stock ELF, not the stream-aware filter substitute |
| AC-2 | Stock ls fixture stdout is `.\n..\nalpha-entry\nbeta-entry\n` (28 bytes); missing-path stdout empty, stderr exactly `ls: /p04-output-missing: no such directory\n` (43 bytes), nonzero status | Separate stdout/stderr captures retained |
| AC-3 | Stock cat returns exact 1,600-byte and 98,368-byte arbitrary-byte fixtures with empty stderr; binary canary UART checks 0..192 all pass | Byte fidelity, not lossy UTF-8 rendering or GUI proof |
| AC-4 | Full-ring cat and fragmented ls drain exact bytes; downstream close recovers in **120 ms**; stock full-ring Ctrl-C recovers in **41 ms** with exact `stockcc=130` | Before the 30-second deadline; no separate elapsed-timeout image run claimed. Internal short-write return counts are not externally exposed |
| AC-5 | Missing stdout/stderr/both stock echo children are published and exit 1; invalid-ref admission returns Stale before helper execution; all captures empty, no negative payload on UART or other hosts | Admission rejection is not helper-execution proof; details below |
| AC-6 | Two concurrent hosts receive only their own stock echo data and independent ls stderr; resize, exit and close independence pass | Headless TerminalHost path, not compositor/pixels |
| AC-7 | Existing init/no-context free UART header/data appears once; VFS/input logging remains on UART | Invalid UTF-8 legacy failure covered by host helper tests |
| AC-8 | Final S3 52/52 + 308/308; Phase02 normal 48 observations/40 repeated commands and kernel 37/37; final Phase03 60/60 with child count 4→4 and independently symbolized CPU halt; host 417 tests pass | Assigned gates introduce no new failures; historical completion-queue selftest failure remains disclosed, not a globally clean boot |

Fixture [lengths/hashes](s3-output-evidence/output-fixtures.json) and exact copied bytes:

| Fixture | Bytes | SHA-256 |
|---|---:|---|
| raw cat | 1,600 | `a6cdaa90b8b902935589ad25507a0640efe4c6e021fb8a50f651c9ca144128be` |
| large cat | 98,368 | `b7bdb440172ac681fd2764c5dcc418e0ebf9bdd05ed735be1c42428deb27b5a6` |
| pressure ls | 15,621 | `c6c2121bad7a55c5655550bda9d6649e318a733ff56e0bd64f791c5cca8dfaba` |

The fragmented ls fixture uses 60-byte names and a separate LF across full 4,096-byte rings. Exact retained output is independently checked; the report does not infer particular internal write return counts from that byte equality.

### Negative grants: execution versus admission

The separately approved, non-default `phase04-driver-profile` adds only `phase04-terminal-driver -> /bin/echo` on Path route with an empty child capability ceiling. No Elf route, ordinary-image row, ambient privilege or public helper constructor was added.

| Case | Actual final marker | Meaning |
|---|---|---|
| Missing stdout | `negative-child name=negative-missing-stdout exit=Ok(1) published=true` | Real stock helper executes, fails closed |
| Missing stderr | `negative-child name=negative-missing-stderr exit=Ok(1) published=true` | Real stock helper executes, fails closed even for an otherwise stdout-only command |
| Missing both | `negative-child name=negative-missing-both exit=Ok(1) published=true` | Real stock helper executes, fails closed |
| Invalid stdout reference | `negative-invalid-ref admission=Stale helper-executed=false` | Canonical owner rejects publication; **not** helper-runtime coverage |

The independent checker requires empty stdout/stderr for every case, independently queried nonzero termination for published children, no distinctive negative payload on UART, and no bytes leaking to unrelated hosts. All pass. Stock close/cancel separately exercise valid delegated endpoints being stopped; no separate pre-write revoked-negative echo case is claimed.

### Full-ring interrupt and exact status closure

The earlier amended image observed `stockcc=1`, not 130. That failure is preserved in the [historical blocker report](s3-output-verification-blockers.md); it is **superseded by the approved shell fix and final run**, not a relaxed oracle.

The shell now retains the unsent suffix and polls input on full host-output rings **outside the session lock**, before backing off. An actual Ctrl-C becomes `TerminalWriteStop::Interrupted` / `StreamStop::Interrupted`, cancels/reaps the child operation, and settles to 130; ordinary endpoint failures stay output errors. Independently, `shell_state::set_status` now stores exact decimal status strings rather than the former single-digit mapper, so `$?` exposes 130 without truncation. The final marker is `stock-cancel elapsed_ms=41 sent=Ok(()) key=Ok recovered=Ok(()) probe=Ok(["stockcc=130"])`; output stops, shell remains alive and later commands work. This is not a blanket mapping of write failure to user interruption.

## Authority and signed stock provenance

[Saved authority invariants](s3-output-evidence/output-authority-invariants.json) cover all eleven tools. **Only free gains GetTime** (`Log, MemInfo` → `Log, MemInfo, GetTime`); no capability manifest flag changes, wire selector changes, production launch rights or network/VFS expansion are introduced by S3-output. The [protected-source baseline](s3-output-authority-baseline.json) records the authorized source plus the separately approved fixture echo row; the invariant labels are relative to that approved baseline, not pristine HEAD. `mod.rs`, target ceilings and the CellSessionV1 ABI stay unchanged by the echo amendment.

The saved Terminal ceiling diff says `G-S1a FAIL` versus HEAD because of **SetTimer**, already approved in original S3. It is not an additional S3-output syscall change or an unexplained new failure. Cellbox's ceiling remains unchanged. Main reports the protected canonical/quarantined hashes unchanged; this documentation worker did not rewrite or independently recheck canonical artifacts.

| Actually exercised stock ELF | Signed SHA-256 |
|---|---|
| echo | `ccbb3f9589d8d2bf79832ace7e465eca240b2d7bddc23b0b4bcbd3f3af54f72c` |
| ls | `27d28a1130b6608dedb1641933a1e90ac21294487c70fa83e3f26c2b6e8813e9` |
| cat | `d01f5998dbb6d10b4c17e89ccc2d3ad568f6a1b1cc4d350fd00ea476b4c0c068` |
| free | `018c710d4e0f75bff14069c171fcec2d4d27846b878b5ff6e1c5090202f813b6` |

Full provenance also identifies the driver, cellbox, p03-filter, services and every other signed stock ELF. Date/env/uname/kill/shutdown/hotswap are signed and packaged in S3 but **not launched in S3**; no shutdown/service mutation or broadened admission was used to inflate S3 coverage. Final Phase03 exercises its admitted shutdown worker in the separately labeled regression lane below.

## Regression evidence and preserved failures

| Lane | Existing result | Durable evidence |
|---|---|---|
| Host baseline | 403 passed across 11 suites, 6 ignored, no failures (`artifact://1181`) | Recorded before source edits in the deadline amendment |
| Final host | **417 passed across 11 suites, 6 ignored, no failures** (`artifact://1244`) | [Extracted original result headings](s3-output-evidence/host-test-results.txt) |
| Phase02 normal | **48 P02 passes**, 40 repeated external commands, Ctrl-J distinction, top quit, rm-i behavior; runner observations complete | `/tmp/cellbox-phase02-output-normal-xffQOb`; [serial](s3-output-evidence/phase02-normal/serial.log), [provenance](s3-output-evidence/phase02-normal/provenance.json), [runner summary](s3-output-evidence/phase02-normal/runner-summary.txt), original JSON observations |
| Phase02 kernel | **37/37 atomic publication cases pass** | `/tmp/cellbox-phase02-output-kernel-u2GWLi`; [serial](s3-output-evidence/phase02-kernel/serial.log), [provenance](s3-output-evidence/phase02-kernel/provenance.json), [runner summary](s3-output-evidence/phase02-kernel/runner-summary.txt) |
| Final Phase03 | **60/60 assigned gates pass, runner exit 0, child count 4→4; actual shutdown CPU halt proven**, per Main's final completion signal | `/tmp/cellbox-phase03-s3-dwarfhalt-KQ3zxfhH`; [serial](s3-output-evidence/phase03-final/serial.log), [command captures](s3-output-evidence/phase03-final/pipeline-commands.json), [probe](s3-output-evidence/phase03-final/final-probe.json), [CPU samples](s3-output-evidence/phase03-final/cpu-halt-evidence.json), [symbols](s3-output-evidence/phase03-final/shutdown-symbols.json), [provenance](s3-output-evidence/phase03-final/provenance.json), [attributed parent outcome](s3-output-evidence/phase03-final/parent-outcome.json) |

[Regression copy manifest](s3-output-evidence/regression-copy-manifest.json) and the [final Phase03 copy manifest](s3-output-evidence/phase03-final/copy-manifest.json) record archived source/copy hashes and sanitization. These are copies/extracts of existing runs, not new builds/tests by the documentation worker.

The Phase02 and final S3 serial logs still contain the historical `COMPLETION-QUEUE: FAIL — an older waiter erased its replacement` boot selftest (S3 lines 107–109). This exact failure predates the slice; it is **not newly attributed to S3-output**, but it is not hidden behind passing assigned gates. Do not call these boots globally clean.

### Final Phase03 shutdown halt proof

The final probe preserves **banner absent, VM still alive, no subsequent shell prompt**, while recording `worker_load_seen=true` and `halt_proven=true`. QMP `query-status=running` alone is not the proof. An owned private read-only QMP/HMP register channel records a baseline HLT=0, then samples 14 and 15 **301.340651 ms apart**, both at RIP `0xffffffff80039571`, **HLT=1, CPL=0, RFLAGS=0x46, IF=0**.

Decoded DWARF maps the corresponding `hlt` at `0xffffffff80039570` to `kernel/src/task/syscall.rs:7887` in `cellos_kernel::task::syscall::handle_syscall_inner`; the stopped RIP is its `jmp` backedge at `0xffffffff80039571`. The symbol map, CPU evidence and signed-image provenance all identify kernel SHA-256 `4772ebab4d0fc2430e16566c59039d81800cdbefb113a07ca02519c6d8110f73`. This directly proves the accepted shutdown halt loop, not a guessed effect inferred from missing output or a live VM process.

The earlier Phase03 failure was an inadequate banner/VM-exit observation, now **resolved by the approved independent halt oracle**, not by restoring context UART logging, adding a sleep, changing shutdown semantics or erasing the original observation. Guaranteed pre-halt banner rendering would require a separate drain-acknowledgment contract and remains unpromised. Together with the final exact130 full-ring result, this supersedes blocker-report items 1 and 2; its historical completion-queue item remains disclosed.

## Headless-run limits and historical supersession

The original S3 “stock tools still print to UART / outside S3” statement accurately described its earlier fixture. The approved S3-output extension and final stock evidence **supersede that product gap now**; they do not rewrite the earlier observation into a pass. The earlier cancellation-status failure is similarly retained as history with its actual failed oracle.

Headless TerminalHost/stock byte proof alone does not establish GUI rendering, compositor input/resize, physical-terminal behavior or complete Phase04 cutover. The subsequent GUI slice described below supplies only its separately exercised private-fixture proof. S2 authority coverage, multicall/command retirement, other Phase04 criteria and their CP2 gates are not closed by this report. The subsequently approved S3-output CP2 baseline does not authorize canonical writes, release/signing-quarantine removal, rollout or any broader acceptance.

## Subsequent private GUI proof (2026-10-22)

The [GUI runtime report](s3-gui-runtime-evidence.md) and [approved GUI CP1](../phase-04-s3-gui-proof-proposal.md) record a real taskbar-click Terminal launch, injected virtio keyboard input, exact compositor framebuffer-region matches for shell prompt and stock echo/ls/cat, separate stderr and Ctrl-C `cc=130`. Run3 passes **58/58**; run4 after separately approved stderr-first draining passes **103/103**, including **10/10** consecutive exact stderr-before-prompt screens. Run1's real prompt-ordering defect remains preserved. This supersedes “GUI not yet proven” only for this private QEMU/TCG fixture, not resize/focus/multi-window/KVM/physical Intel/VMware or full Phase04.

Private signing admits `{cellbox, app-cellbox, terminal}` only with `CELLBOX_PRIVATE_FIXTURE_SIGNING=1` plus an explicit non-dev seed; quarantined hashes still fail unconditionally. Parent observed 49 signing tests OK, Terminal/terminal-sdk 33/49 pass, clean bare-metal check and later private S3/P03/P02 runners passing. **These newer regression/host results have no archived raw logs and are reported as parent-observed counts only**, distinct from the older fully archived evidence above. The unconsumed endless-pipeline OOM and historical completion-queue selftest failure are not claimed fixed.

The earlier CP2 inventory stays approved as recorded; new GUI/signing/Terminal/runner files appear in its [dossier addendum](../phase-04-s3-output-cp2-dossier.md) and are not covered without new approval. User rollout consent did not satisfy the containment release conditions: no release guard was reopened, nothing was unblocked, and rollout remains **BLOCKED**.
