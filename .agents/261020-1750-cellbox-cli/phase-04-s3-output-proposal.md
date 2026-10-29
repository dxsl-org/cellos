# Phase 04 S3-output — stock command stdout (CP1 proposal)

Status: **CP1 APPROVED by the user on 2026-10-22 (“duyệt CP1”); S3-output slice implemented and privately verified under the separately approved [free/GetTime](phase-04-s3-output-deadline-amendment.md), [private echo fixture](phase-04-s3-output-negative-grant-amendment.md), and shell interrupt/status closures. AC-1..AC-8 proven within their stated limits: final S3 52/52 driver markers + 308/308 independent checks; Phase02 normal/kernel 48/37; final Phase03 60/60 with independent CPU halt proof.** [Runtime evidence and limits](reports/s3-output-runtime-evidence.md). This is not full Phase04/GUI acceptance; CP2 and rollout remain blocked.
Parent: [S3](phase-04-s3-gui-terminal-proposal.md). This proposal does not approve Phase 04 CP2, canonical packaging, signing quarantine removal or rollout.

## Problem and evidence (pre-implementation observation)

The S3 image proved host output with a stream-aware fixture, not stock command output in the Terminal pane. `libs/ostd/src/io.rs:73-92` sends `print_fmt`, `print` and `println` through `sys_log`. `cells/tools/sys-tools/src/bin/{echo,ls,cat}.rs` uses that interface. Logging must remain logging for existing services; globally redirecting `io::print` would silently change unrelated consumers. `libs/ostd/src/syscall.rs:1249-1262` already exposes `sys_write`, but collapses every negative result to PermissionDenied; it is not an adequate typed backpressure interface by itself.

## Decision proposed

Add explicit fallible stdout/stderr byte-writing helpers to ostd, implemented over the existing CellSessionV1 endpoint operations and committed own-context grants. Migrate the stock sys-tools command output callers to those helpers, not the global console logger. Reuse existing context/codec authority and fd routing; no new syscall, selector, capability, manifest privilege or kernel registry.

For a committed context, stdout/stderr use only their corresponding grants. Missing, revoked, cancelled or malformed grants return errors: **never fall back to UART**. For a genuinely absent context (the legacy Denied outcome of ReadOwnContext), preserve the existing console output path. Other context-read errors fail closed. Explicit diagnostic logging remains `sys_log`.

Writes loop over partial progress with bounded stack chunks and yield/wait through existing permitted mechanisms on backpressure. Zero progress is an error, not an infinite loop. Cancellation/BrokenPipe terminates the write and propagates failure to the command. No whole-output allocation, repeated context parsing per fragment, dependency on terminal-sdk (which already depends on ostd), or unbounded retries. The precise helper interface and lifetime/cache strategy must be reviewed before exported API edits; process-wide caches must not invent session authority.

## Source closure initially proposed (approved amendments recorded below)

- `libs/ostd/src/io.rs`, `libs/ostd/src/session.rs`, `libs/ostd/src/child_context.rs`: explicit stdio helpers and reuse of typed owner calls; adjacent existing tests only.
- `cells/tools/sys-tools/src/bin/*.rs`: command data -> stdout and usage/errors -> stderr; preserve each command's existing arguments, exit behavior and actual data. Census every file before editing; workers/privileged commands keep their existing authority checks and explicit diagnostic logs.
- `cells/tests/phase04-terminal-driver/src/main.rs`, `scripts/qemu-phase04-terminal-test.sh`: replace UART-only stock-output assertions with exact host-stream assertions; keep existing independent session and cancellation checks.
- Existing operational docs/changelog and this plan's evidence records after proof.

Initial boundary: no kernel/API wire changes, launch-profile/ceiling changes, network/VFS rights expansion, cat stdin feature, new applet dispatch, shell fd semantics changes, GUI renderer changes, or canonical image writes. Free-only GetTime, the private echo edge and shell interrupt/status closure were subsequently separately approved; those narrow exceptions do not widen the remaining restrictions.

## Acceptance criteria and evidence (2026-10-22)

- [x] AC-1: Context-mode cellbox launches signed stock `/bin/echo`; exact stdout bytes appear once, stderr is empty, and command data is absent from UART.
- [x] AC-2: Signed stock `/bin/ls` returns the fixture entries on stdout; its missing-path diagnostic appears only on stderr with nonzero status. Exact independent captures are retained.
- [x] AC-3: Signed stock `/bin/cat` returns byte-identical 1,600-byte and 98,368-byte fixtures containing NUL/invalid UTF-8, without log prefixes or lossy conversion. Its path-argument semantics remain unchanged.
- [x] AC-4: Full-ring stock cat and fragmented stock ls output drain exactly; downstream close recovers in 120 ms, and full-ring Ctrl-C recovers in 41 ms with `$?` exactly 130, before the 30-second write deadline. Exact retained bytes are externally proven; internal short-write return counts are not independently exposed.
- [x] AC-5: Actual signed stock echo children with missing stdout, missing stderr or both are **published, execute, and exit 1**, with empty owned captures and no payload in UART or unrelated hosts. The invalid stdout reference is instead rejected by canonical admission with **Stale before publication/helper execution**; that case is admission proof, not a helper-runtime claim.
- [x] AC-6: Two hosts receive only their own stock-command data and independent missing-path errors; stdout and stderr remain distinct.
- [x] AC-7: Existing init/no-context stock free prints its UTF-8 console output once; unchanged VFS/input service logs remain on UART. Legacy invalid-UTF-8 output fails with `UnsupportedInput` rather than converting or leaking a prefix (host helper coverage, not a legacy binary runtime claim).
- [x] AC-8: Existing assigned Phase02/03/04 scenarios introduce no new failures; tool syscall ceilings stay unchanged **except the approved free-only GetTime bit**, and capability manifests stay unchanged. Final S3 passes 52/52 + 308/308; Phase02 normal passes 48 observations/40 repeated commands and the kernel lane passes 37/37. Final Phase03 passes 60/60 with child count 4→4 and two stable CPU HLT=1 samples at the DWARF-mapped shutdown loop; absent UART banner/VM exit is not claimed otherwise. Signed private provenance identifies every exercised stock binary. The historical completion-queue boot selftest failure remains disclosed, not a clean-boot claim. Main supplied the final regression completion signal before this row was checked.

## Verification and authorization boundary

Capture applicable host-test baseline before source changes. Run migrated command binaries on a private signed x86 image through the real TerminalHost driver; fixture-generated RAM files may supply exact cat/ls inputs. Stream-aware substitutes are not stock-command proof. Test partial writes, consumer close, cancellation and independent hosts against actual owners, not mocked echoes.

Reuse the private S3 runner's isolated target/embedded/key/image paths and signing admission only; preserve canonical/quarantined artifacts and unrelated work. Save sanitized markers/provenance before deleting owned temporary keys/images and stopping only the owned VM. GUI pixel/compositor proof remains separately pending: headless stream proof is not GUI rendering acceptance.

## Approval requested

Approve only this file-bounded userspace change and its private-fixture build/sign/boot verification. Review the exported helper design/caller census before edits. Any additional protected source or authority requirement returns to CP1. Rollback reverts only this slice's source; delivered IPC or file bytes cannot be recalled.

## Implemented contract and current boundary

`ostd::io::ProcessOutput::read_own() -> Result<Self, Status>` captures immutable own-context output grants once per command; `stdout()`/`stderr()` expose private-field `OutputStream`s, not a global authority cache. `OutputStream::write_all(&self, bytes, deadline_ns)` accepts a finite nonzero absolute deadline, uses existing correlated CellSessionV1 WriteStream requests in chunks of at most 512 bytes, advances only acknowledged prefixes and fails on zero progress. Each command emission computes a 30-second deadline; no infinite retry or whole-output replay follows an error.

All eleven sys-tools callers use explicit stdout for data and stderr for usage/errors. Only `ReadOwnContext = Denied` selects the legacy logger; committed contexts require both grants and never fall back to UART. Context output preserves arbitrary bytes. Legacy output accepts UTF-8 only and returns `UnsupportedInput` for invalid UTF-8 without logging that write. Global `io::print*` and explicit service diagnostics remain logging APIs.

The free-only GetTime declaration and non-default `phase04-driver-profile` Path edge `phase04-terminal-driver -> /bin/echo` (empty child ceiling) are the two separately approved authority exceptions to the initial proposal. No new wire/API selector, capability manifest flag, ordinary-image launch row or production right was added by S3-output. The preexisting Terminal SetTimer delta belongs to the original S3 approval, not this slice.

The approved shell closure polls input while host output is full, outside the session lock, retains only the unsent suffix, and propagates an observed Ctrl-C as `StreamStop::Interrupted` through cancel/reap and settlement. Ordinary output errors remain failures, not fabricated interrupts. Shell `$?` now stores the exact decimal status, including 130 (rather than the former single-digit mapper).

The original S3 “stock output is outside S3” gap was accurate for its earlier image and is **superseded by this approved S3-output extension**, not retroactively erased. This is headless stock/session proof only: GUI compositor/pixels, complete multicall retirement, Phase04 CP2, canonical packaging/quarantine removal and rollout remain unapproved or unproven.

**Subsequent status (2026-10-22):** the S3-output [CP2 baseline](phase-04-s3-output-cp2-dossier.md) was approved separately; later [GUI proof](reports/s3-gui-runtime-evidence.md) now establishes real Terminal shell rendering on a private QEMU/TCG desktop (run4 103/103, ten exact stderr-before-prompt iterations). These later approvals/evidence do not retroactively broaden this headless proof. New GUI/signing/Terminal files remain outside the earlier CP2 pending explicit approval; full Phase04, canonical qualification and rollout stay blocked, with resize/focus/multi-window/KVM/physical Intel/VMware unexercised.
