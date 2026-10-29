# S3-output CP1 amendment — bounded output deadline for free

Status: **APPROVED by the user on 2026-10-22 (“Duyệt amendment GetTime”); implemented and privately verified.** Final S3 stock/runtime gates pass 52/52 driver markers and 308/308 independent checks, including committed-context free; base AC-8 is proven after final Phase03 60/60 and CPU halt evidence. Parent [approved S3-output](phase-04-s3-output-proposal.md); [evidence](reports/s3-output-runtime-evidence.md). No full Phase04/GUI acceptance, CP2 or rollout approval.

## Review completed

Read-only independent reviews `OutputDesign` and `ToolCensus` inspected the existing owner transport and all eleven sys-tools binaries. Retained helper design:

- `ostd::io::ProcessOutput::read_own() -> Result<Self, Status>` reads immutable own context once per command; no global authority cache. `stdout()`/`stderr()` return private-field `OutputStream` instances.
- `OutputStream::write_all(&self, bytes: &[u8], deadline_ns: u64) -> Result<(), Status>` uses an absolute finite deadline, at most 512 bytes per owner request, and advances only after positive acknowledged progress. A command computes a 30-second deadline for each logical emission; partial-prefix errors terminate, never replay the whole buffer.
- The narrow encoder in `ostd::session` uses existing correlated `KernelOp::WriteStream`, preserving retained-outcome handling and typed errors. Kernel blocking supplies backpressure/wakeup; no timer polling or new selector.
- Only ReadOwnContext=Denied selects legacy logging. Every other read failure, missing output grant or endpoint failure fails closed, without payload logging. Global `io::print*` and service logs remain unchanged.
- All eleven binaries migrate: echo/ls/cat/date/env/free/uname/ps command data; kill/hotswap success output; shutdown informational banner. Usage/errors route to stderr. Existing authority calls and command statuses remain, except new output failures terminate nonzero. Cat preserves arbitrary bytes in context mode; its existing path-argument interface is unchanged.

## Exact blocker and proposed delta

`cells/tools/sys-tools/src/bin/free.rs:6` declares only `Log, MemInfo`. `ostd::session::monotonic_ns()` obtains frequency and current ticks through `GetTime`, whose allowlist bit is 25. Free cannot compute a real future bounded write deadline under its current ceiling.

Approve exactly **adding GetTime** to free's existing `declare_syscalls!` list. This changes one syscall allowlist bit, not capability manifest flags, kernel ABI, owner rights, launch ceilings, selectors, or any other command's syscall list. No SetTimer addition. Free's actual MemInfo authority stays unchanged. All other scope/rollout/quarantine restrictions of the parent remain binding.

Do not substitute infinite writes, fabricated far-future deadlines, fixed yield-count retries, or excluding free: those evade the approved bounded-I/O or full caller migration requirements. If this amendment is declined, retain the approved slice unimplemented rather than silently narrow it.

## Verification required after approval

Record the exact before/after syscall set for every tool: only free gains GetTime; all capability manifest flags stay identical. Run the parent AC-1..AC-8 private-image scenarios and applicable regressions; explicitly exercise free with a committed stdout grant. Preserve private artifacts/provenance and canonical quarantine.

## Baseline captured before source edits

`env CARGO_TARGET_DIR=/tmp/cellos-s3-output-baseline cargo test --offline --locked -p ostd -p terminal-sdk -p api -p text-engine -p app-cellbox --target x86_64-unknown-linux-gnu` passed **403 tests across 11 suites**, 6 ignored, no failures (tool output artifact://1181). This is host baseline evidence only, not runtime acceptance.

## Fixture constraints at amendment approval (historical)

At approval the S3 image packaged ls/cat/echo/ps and inspected their output on UART. The required migration was exact host-stream assertions, binary cat inputs, independent stderr and backpressure/cancellation/missing-grant cases, with package/sign/provenance for every exercised stock binary. Existing launch profiles did not admit every tool; adding routes merely to execute them was not allowed. The later private echo edge was separately approved; final proof is recorded below.

## Implementation evidence (2026-10-22)

Free now declares exactly `Log, MemInfo, GetTime` (previously `Log, MemInfo`). The saved [eleven-tool authority invariants](reports/s3-output-evidence/output-authority-invariants.json) record no other tool ceiling change and no capability manifest change relative to the authorized source baseline. The private negative-grant echo edge was separately approved; it is not part of this free-only syscall amendment. The original S3 Terminal SetTimer delta is preexisting and separately approved.

Final signed free SHA-256 is `018c710d4e0f75bff14069c171fcec2d4d27846b878b5ff6e1c5090202f813b6`. Actual stock free runs both through committed stdout (saved 88-byte output, empty stderr, validated accounting) and through the existing no-context init edge (legacy UART header/data once). Context binary output is byte-preserving; legacy writes require UTF-8, with invalid UTF-8 returning `UnsupportedInput` without logging that write. Global `io::print*` and service logging are unchanged.

Full-ring stock cancellation now returns the prompt and exact `$?`=130 in 41 ms; downstream close recovers in 120 ms, both well before the finite 30-second emission deadline. These are observed early-stop paths, not a claim that this image separately waited out the timeout. Final host suites pass 417 tests/11 suites, 6 ignored (`artifact://1244`), against the recorded 403-test baseline; this is distinct from runtime/GUI acceptance.
