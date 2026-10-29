# Cellbox CLI and terminal architecture

**Updated:** 2026-10-29  
**Status:** accepted design under [ADR-0028](decisions/0028-cellbox-cli-first-structured-pipelines-and-tui.md). The commands below are implemented and qualified on a **dev-posture Intel x86 QEMU/TCG** image (Phase 10, candidate `4fae8b20…`); its desktop bytes are promoted to the canonical x86 paths (S10-5, CP2 granted by the user 2026-10-29). Partial outcomes are listed under "Delivered state and limitations". This is not a frozen wire/ABI specification and not a production, physical-hardware or remote claim.

## Delivered state and limitations (2026-10-29)

- **What ships:** one signed `/bin/cellbox` suite (entry `sh`, also callable as `/bin/cellbox COMMAND`) plus `/bin/cellbox-ssh` (the same signed file, second copy), the GUI Terminal, `ls cat echo ps kill shutdown` workers, and no `/bin/shell`, no `/bin/sshd` autostart. `/bin/config` is not started (`export` reports `config unavailable`, status 1).
- **Posture:** dev cell/policy/signing keys, `dev-weak-rng` (SSH needs RDSEED/RDRAND), Tier 2 admission disabled. Qualified on Intel x86 under QEMU TCG only (KVM recorded separately); no physical Intel, VMX/EPT, production key or posture, remote C2C, cloud image, non-Intel or crash-durability claim ("synced, not crash-proof").
- **SSH (Phase 09, partial):** password, known-host and confirmation to another host work; `--auth publickey` is refused; the Tier-3 guest endpoint and publickey/KMS custody stay blocked (CBX-23/24/25 partial).
- **Footprint (CBX-27 unmet):** measurement is partial; no RSS, exact or kernel stack, matched historical runtime or full-workload peaks. The canonical image grew from 16,777,216 B to 25,165,824 B; no optimisation claim.
- **Known defects, not fixed:** D60-3 clock units in `top`; the GUI Delete key; the `COMPLETION-QUEUE: FAIL` self-test line; xHCI injected-key decode (pre-existing L6 failure); x86-32 handoff skips and the virtio block-init ignore; the incidental DMA-free/DSTRD gaps disclosed in S10-3; a latent `Surface.status()` check in the S10-4 desktop observation schedule.
- **Evidence and rollback:** [acceptance](../.agents/261020-1750-cellbox-cli/acceptance.md) and [rollout containment](../.agents/261020-1750-cellbox-cli/reports/rollout-containment.md). The pre-promotion bytes are archived, quarantined and not a safe boot set; the quarantined-hash guard is permanent.

## Scope and navigation

Cellbox is one essential-suite executable with modular source. It supplies useful Cellos operations, text/structured automation and selected interactive tools without becoming a Linux command collection. Desktop is an approachable subset of the same typed backend operations. An independently installed CLI application is not required to become a cellbox applet.

- Decision/rationale: [ADR-0028](decisions/0028-cellbox-cli-first-structured-pipelines-and-tui.md).
- Scheduling, phase dependencies and admission: [implementation plan](../.agents/261020-1750-cellbox-cli/plan.md).
- Required outcomes: [acceptance ledger](../.agents/261020-1750-cellbox-cli/acceptance.md).
- Current-source facts and limitations: [source recon](../.agents/261020-1750-cellbox-cli/scout-report.md).
- Authority/runtime constraints: [ADR-0024](decisions/0024-cell-kernel-authority-contract.md), [ADR-0018](decisions/0018-cell-native-portability-and-runtime-profiles.md).

## Accepted boundaries

| Concern | Owner | Must not become |
|---|---|---|
| Parsing, variables, cwd, control flow and jobs | Shell execution context | Stateful operations in an unrelated child |
| Essential tools and canonical command catalog | Cellbox modules | Per-command ELF wrappers or independent duplicate implementations |
| File/network/lifecycle effects | Existing typed SDK and authorized service operations | Privilege inferred from command name, path alias or JSON fields |
| Pipeline processing | Native typed command engine; byte adapters at external boundaries | JSON encode/decode or text capture between every native stage |
| Terminal modes/input/output/resize/session teardown | Reusable Terminal SDK | GUI widget code or a cellbox-only private API |
| First-party TUI widgets/rendering | Ratatui with Cellos backend | Dependency on compositor, viUI windows or font rasterization |
| GUI terminal screen/window | ANSI/VT emulator state rendered through viUI | Fixed-response command interpreter or direct TUI-business-logic host |
| Desktop operations | Simple presentation over shared service contracts | Parsing CLI tables to invoke business logic |
| Standalone CLI applications | Their own executable/lifecycle, using public SDK | Mandatory applets or consumers linking against an executable |

Proposed source ownership is `cells/tools/cellbox`, `libs/terminal` and `libs/vitui`. These paths are design choices, not existing crates. Reuse `libs/text-engine` for already pure logic; introduce a separate data/command-core crate only where an actual second consumer or host-testing boundary warrants it. Keep Ratatui types out of public kernel/service wire contracts.

The proposed single-artifact layout includes a shell module exposed as `cellbox sh`. This is an implementation choice to avoid statically duplicating the utility engine in a separate shell executable, not a new shell language or Unix personality. Init, GUI terminal, launch identities and image paths must migrate together after exact profile approval. Shell builtins stay stateful within this module. Normal independently installed executables remain launchable through approved external-command routes.

## Command catalog and invocation

Canonical artifact/explicit invocation: `/bin/cellbox <command> [arguments...]`. The shell resolves selected short names to registry entries in-process. No reliance on basename dispatch, filesystem symlinks, implicit `argv[0]` or forwarding binaries is required. Startup mode selects behavior but never selects authority by itself.

**As built (Phase 10 S10-0, 2026-10-26).** A launch context whose argv (it never carries the program path) is not `sh` runs one catalog command and exits: `/bin/cellbox ls /tmp`. The closed table of [command-catalog §3.2](../.agents/261020-1750-cellbox-cli/command-catalog.md) decides: file, text and observation commands run with the shell's implementation and status; shell-state commands (`cd`, `export`, `exit`, …), `clear`, the launchers `exec`/`hv` and the typed stage words are refused with status 2 and no effect; any other name, including `/bin/ls` and retained worker names, is 127 with no external fallback, so a one-shot never launches a Cell. Arguments are literal (no expansion, globbing, redirection or pipelines). Output goes only to the context's Stdout/Stderr grants (without both, nothing is written anywhere and the status is 1); typed producers stream typed JSONL unless `--text` is given; stdin-reading commands read the Stdin grant to end of stream, at most 4 MiB. A terminal frontend still runs only `sh`. Inside `cellbox sh`, `cellbox CMD` and `/bin/cellbox CMD` go through the same table in process: `cellbox ps --records | select tid` is `ps --records | select tid`, `cellbox cd /` is refused with 2. No new launch edge exists: on canonical images only the in-shell form is reachable (init has no context and the Terminal passes `sh`).

A single catalog owns command identifiers, arguments, input/output signatures, finite/stream/session mode, status conventions, rights required and help/completion metadata. Semantic aliases such as `[` are catalog entries sharing implementation, not obsolete-executable compatibility aliases. Reject unknown commands and type mismatches truthfully. Preserve literal arguments, empty arguments and whitespace without join/split reconstruction.

| Family | Migration/addition scope | Boundary |
|---|---|---|
| Shell state/control | Existing cwd/variables/aliases/functions/source/read/exit/control flow/jobs | Preserve state transitions in shell; enumerate current syntax before changes |
| File/text utilities | Existing useful ls/cat/cp/mv/rm/mkdir/rmdir/touch/head/tail/wc/sort/uniq/tee and text-tool behavior | One implementation, bounded input, truthful file errors; no automatic GNU compatibility claim |
| System observations | Existing uname/date/uptime/free/ps and upgraded top | Real providers; no fixed output, missing data never fabricated |
| Network/operational clients | Existing useful short-lived clients and admitted Cell/service controls | Client modules only; services/long-running workers remain separate |
| New editor | `edit` | Ratatui TUI over VFS; config/script/text scope |
| New search | `rg` with documented supported surface | Upstream-derived search core, direct match records |
| New outbound SSH | `ssh` | Client/session only; not inbound sshd or C2C authority transport |
| Diagnostics and specialized tools | Inventory every current name, script and package consumer | Retain behavior or record an explicit disposition; do not delete unrelated tools silently |

Phase 01 freezes the exact migration catalog after source/caller census. The table is a family boundary, not permission to drop a command because it is omitted above. New commands require a named use case; no Linux command-count goal applies. Server programs such as httpd and long-lived workers retain explicit package/image entries where still consumed.

## Pipeline data and execution

### Four categories, not everything-as-JSON

1. **Value:** a scalar or typed record; preserve booleans, integer precision, path/byte data, timestamps, duration and explicit absence.
2. **Record stream:** incremental results such as search matches or telemetry frames; retain schema/sample identity and bounded buffering.
3. **Byte stream:** exact bytes for file contents, external stdout and non-PTY SSH commands; no mandatory UTF-8 decode or newline normalization.
4. **Terminal session:** duplex interactive terminal I/O plus resize/control/lifecycle, not a JSON record stream.

Diagnostics and command completion/status are separate channels/control outcomes, not successful records. A record containing an integer resembling a capability is never authority. Across process/Cell boundaries, only approved handle delegation and concrete framed transports have authority/effect semantics.

### Conversion and display

- Native stage to native stage: move/borrow typed data in the same context; do not stringify, serialize or deep-copy unnecessarily.
- Record to external byte input: explicit `to json`, `to jsonl` or `to text`. Do not implicitly render table borders.
- External bytes to record input: explicit parser such as `from json`/`from jsonl`; malformed/incomplete input is an error, not an empty result.
- Final interactive record output: one display renderer may add table layout, colors and human units. Text output remains available.
- Redirected structured results: explicit serializer or the command's documented machine encoding; no TUI, prompts or ANSI. Define a deterministic non-terminal encoding per command in Phase 01; never choose it from content heuristics.
- File content/SSH byte output: preserve bytes; do not reinterpret it as a table or automatically detect JSON.
- Explicit full-screen operation on an unsupported session: fail clearly. Top may select its documented batch behavior; an editor cannot silently become a successful no-op.

The bounded filter vocabulary needed for the selected consumers is field selection, typed comparisons, sorting, taking a finite prefix and explicit encoding/decoding. This is not approval for Nushell's whole grammar, closures, plugin model or evaluation runtime. Freeze exact syntax and script migration in Phase 01; examples below illustrate semantics only.

```text
cells | where state == "running" | sort-by cpu_percent | select name cpu_percent heap_bytes
rg "timeout" /data/logs | select path line_number text | to jsonl --typed
ssh host --command 'remote-tool --json' | from json | select status
```

These are target examples (the `rg` line runs since Phase 07 S7-3; `cells` and `ssh` do not exist yet), not approval of an arbitrary remote-tool package. `rg` paths are `Path` values and `text` may be Null, so the lossless `--typed` encoding is the one that round-trips. Record schemas must define byte offsets versus character/display columns, units, nullability and versioned external encoding. JSON encoding must preserve wide identifiers without silent precision loss and non-UTF-8 bytes without lossy replacement.

### Streaming and state

Finite filters pull bounded records; downstream cancellation/closed pipes stop upstream work and release resources. Declare and bound materializing stages such as global sort. Group continuous top data by snapshot; never sort an infinite history before emitting a first result. Carry EOF, timeout, cancellation, partial result and terminal error separately. No automatic retry after uncertain side effects.

Preserve shell cwd/environment, quoting, status, job tracking and source execution through cutover. Freeze standalone versus pipeline stateful-builtin context, whole-pipeline job/status, foreground transfer, wait/reap and cancellation. Preserve supported forms; reject unsupported background/mixed/stateful syntax before any stage has effects, never silently ignore `&` or mutate unintended parent state. Define redirects/script migration before implementation; replace incidental wording tests with behavioral coverage.

## Terminal SDK and Ratatui

The public terminal SDK provides session attachment, one effective input reader, foreground ownership, bytes/events, size/resize, supported capabilities, echo/raw mode, cancellation and teardown. EOF/peer death releases ownership; a dying child must not steal or strand the next shell prompt. Control effects cannot cross sessions. Use actual session/kernel enforcement where required; a local `has_reader_lease` boolean is not an ownership protocol.

Stream/control endpoints are non-inheritable unless explicitly delegated as typed child grants. Define duplicated/descendant holder ownership, generation/reuse protection and EOF after the last authorized writer closes or is revoked. Operation cancellation/consumer close revokes that operation's delegated subtree and wakes blocked holders, without revoking unrelated sessions. Test retained writers and descendant death; a parent retaining an endpoint cannot strand cancellation.

Define a minimum ANSI/VT profile for cursor movement, erase, color, wrapping, scrolling and alternate screen, plus supported keys, paste and resize. Bound parser sequences, dimensions and scrollback; discard/escape unsupported control operations, including unapproved clipboard/device side effects. Do not advertise TERM features not implemented. Secret entry disables echo and never reaches logs/scrollback/history.

The Cellos Ratatui backend translates changed cells/cursor operations to the session output contract; input/session lifecycle stays explicit rather than assumed to be handled by the widget library. Pin supported Ratatui features/dependencies and Unicode-width behavior. TUI code is unavailable in plain data paths only through command mode, not through a mandatory GUI dependency. Share theme semantics only where useful; do not port pixel layouts or viUI widget trees.

GUI Terminal attaches to the real shell/foreground session, interprets output and forwards input/resize. Remove its independent command matcher and fabricated uname/pwd/ls/ps/date data after the real bridge is proven. Preserve applicable ADR-0027 native session/tab rules; neither TUI buffer nor terminal viewport is a separate Cell Tab. Serial/SSH terminals remain functional without desktop startup.

**Current S3-output operation (2026-10-22):** stock sys-tools initialize `ostd::io::ProcessOutput::read_own()` once, route command bytes to `stdout()` and usage/errors to `stderr()`, and use fallible `OutputStream::write_all(bytes, deadline_ns)` with finite absolute deadlines (30 seconds per emission). Committed contexts require both grants and never fall back to UART on grant/endpoint failure; only absent-context `ReadOwnContext = Denied` selects legacy UTF-8 logging. Context output preserves arbitrary bytes; invalid legacy UTF-8 returns `UnsupportedInput` without logging that write. Global `io::print*`/service logging stays unchanged. Only free adds GetTime; the negative-grant echo edge is private-feature-only with an empty child ceiling. Headless stock echo/ls/cat/free proof passes 52/52 driver markers + 308/308 independent checks, including byte fidelity, separate stderr and host isolation. Full-ring output polls Ctrl-C outside the lock, retains unsent bytes, cancels/reaps and preserves exact `$?`=130 (41 ms; close 120 ms); endpoint failure alone is not an interrupt. Missing-grant echo children execute/exit 1; invalid references are rejected with Stale before helper execution. Base AC1–8 and the earlier S3-output CP2 baseline are approved/proven only as recorded in [the output report](../.agents/261020-1750-cellbox-cli/reports/s3-output-runtime-evidence.md); historical completion-queue failure remains disclosed.

**Private GUI tty operation:** Terminal launches the shell with committed environment entry `CELLBOX_STREAMS=merged`; only a context-mode shell with that exact entry routes diagnostics/forwarded child stderr to its stdout writer, ordered with shell echo/newline/prompt. Non-GUI hosts without the entry retain separate stdout/stderr. Terminal drains stdout then stderr; the former stderr-first fix was superseded after failed mirror-race reruns. Five consecutive taskbar-click runs plus one autoload run pass **103/103 each**, including ten strict stderr-before-prompt exact screens, stock output, Ctrl-C `cc=130` and UART negatives. Raw GUI/host459/signing49/S3/P03/P02 evidence is now archived; old missing-log observations remain historical. A child's own stdout/stderr interleaving is still unordered and would require an ABI ordering token. TCG only; resize/focus/multi-window/KVM/physical Intel/VMware unexercised; unconsumed-pipeline OOM and historical completion-queue failure remain. Private signing still requires its explicit non-dev-seed/env guard, quarantined hashes always fail and canonical packaging guards stay closed. [Current GUI CP2](../.agents/261020-1750-cellbox-cli/phase-04-s3-gui-cp2-dossier.md) is **PROPOSED**, not approved; new tty/GUI/signing files and changed executor are not implicitly accepted by the earlier S3-output CP2. **Nothing was unblocked; full Phase04, canonical qualification and rollout remain BLOCKED.** [Evidence and limits](../.agents/261020-1750-cellbox-cli/reports/s3-gui-runtime-evidence.md).

**Serial line input (Phase 10 S10-3a).** On the UART, Enter is CR. A lone LF is decoded as Control+J, and every shell line reader (command line, `read -s`, `rm -i` and other confirmation prompts, the ssh password) treats Control+J as accept-line, so clients that end lines with LF alone work. CRLF submits the line and then an empty line, as the old `/bin/shell` did. Text inside a bracketed paste never submits. TUIs (`top`, `edit`) keep Control+J as an ordinary key.

**Ctrl-C and builtin `cat` of disk files (Phase 10 S10-3b).**
- **Ctrl-C:**
  - Builtin `sleep`, `top -b` and the wait between `top --records` frames end on Ctrl-C with `^C` and status 130.
  - A Collected typed run checks for Ctrl-C before each record it pulls; this covers a redirect, `$(…)` or a byte consumer such as `| cat`. On Ctrl-C it ends with 130 and writes nothing.
  - Like every other Ctrl-C poller in the shell, these waits throw away other keys typed while they run, so type-ahead during a foreground command is lost.
  - A one-shot `/bin/cellbox sleep` has no session; only the kernel's Operation cancel stops it.
- **Builtin `cat` of a path the VFS has** (for example `/tmp`, `/data` or a disk mount):
  - Prints the text with one U+FFFD per invalid UTF-8 sequence, the same as `cat` of a boot-image file.
  - A directory is `cat: PATH: Is a directory`, status 1.
  - A file over 1 MiB is `cat: PATH: File too large`, status 1, unless it is a boot-image file, which still streams at any size.
  - Before S10-3b, a non-UTF-8 or over-1-MiB disk file was reported as "No such file or directory".

**UART input on the desktop image.** When the desktop profile is running and a GUI Terminal window has focus, bytes typed on the UART go to that focused Terminal, not to the serial `cellbox sh`. The user accepted this on 2026-10-26. The serial shell is qualified on the headless image; the desktop image is qualified through the GUI.

New independent apps consume the SDK/backend as libraries. Their static executables may contain required TUI code; no dynamic linking, stable Rust ABI or universal render-service dependency is implied. Accepting Ratatui does not force an existing third-party app to rewrite its own TUI.

## Selected tools

### Small editor

Deliver open/create/save/save-as, navigation, selection/copy/cut/paste, bounded undo/redo, search, go-to-line, dirty-exit handling, UTF-8 width and selected highlighting. Preserve content/line endings/final-newline state unless explicitly changed. Original and Save As destination identity/version must be checked at commit, including pre-existing or concurrently created/changed destinations: explicit overwrite confirmation plus enforcing conditional create/replace behavior is required. Failed/short/interrupted replacement retains stored bytes and dirty edit state. Verify actual VFS guarantees before claiming atomic/durable save. Kibi's line limit/Unix terminal calls and any plugin/LSP/IDE/build system are not requirements.

### Ripgrep-based search

Use upstream library boundaries and VFS stream traversal; do not substitute ERE-lite grep. Document selected flags, ignore precedence, default hidden/binary handling, symlink policy, error/status semantics, encoding and cancellation. An unchanged file can exceed the old 64 KiB aggregate input bound without loading it all. Report inaccessible entries/partial traversal truthfully. Direct records carry path, line number, absolute byte offset, line bytes/text and submatch byte ranges; no ANSI or colon-delimited reparse. Validate 0/1/2 match/no-match/error conventions for the chosen compatibility surface. PCRE2/mmap/full parallelism/indexing are outside required scope.

**Current `rg` (Phase 07 S7-3).** `rg [OPTIONS] PATTERN [PATH…]` and `rg [OPTIONS] -e PATTERN [-e …] [PATH…]`, exact surface in `command-contracts.md` §2 (`rg -h` prints it). Every argv error, unsupported upstream flag, backreference or look-around is status 2 before any file is opened.

- **Data.** `rg` is always a typed row producer: one `cellbox.search.line` record per match or context line (`path` Path or Null for stdin, 1-based `line_number`, `absolute_offset`, `line_bytes` without the LF, `text` Null when the line is not UTF-8, `terminator`, `submatches` as half-open byte ranges into `line_bytes`). Rows stream: a search segment runs only when the consumer pulls, at most 8 rows / 32 KiB are queued, and `take` stops traversal and releases every VFS handle.
- **Display.** At the terminal, and with `--text` at redirects, `$(…)`, byte consumers and external commands, rows render as `PATH:LINE:TEXT` (context `PATH-LINE-TEXT`, stdin `<stdin>`) as long as only `where`, `take` or `sort-by` follow; otherwise the table / typed JSONL defaults apply and an explicit `to` always wins. Every displayed byte is escaped: `\\ \n \r \t \0`, other controls, the bidi/format controls and invalid UTF-8 as `\xHH`, and `:`/`-` in paths. Paths are the absolute canonical VFS paths, not operand-relative as upstream prints them. **`--text` selects this renderer; it is not upstream's alias for `-a`.**
- **Input.** No path means the shell cwd (an unreadable cwd is status 2, never `/`). `rg -a PATTERN -` searches the bytes of `cat FILE |`, a built-in, `< FILE` or external commands; `-` requires `-a` and must be the only path.
- **Traversal.** Depth first, children in byte order; `.rgignore` > `.ignore` > `.gitignore`; hidden entries skipped unless `--hidden`; `--no-ignore`; a file with a NUL byte is skipped after a whole-file preflight unless `-a`; mounts below a root are skipped and counted; nothing is followed. Authority is the caller's VFS table only (no BootFS fallback).
- **Errors and status.** Each failed entry, file or ignore file prints `rg: PATH: reason` and the walk continues; a run with errors or skipped mounts ends with one `rg: summary: …` line. Status 0 = a matching line and no error, 1 = complete without a match, 2 = any error (even after matches), 130 = Ctrl-C. As the last data stage (optionally before `to`) `rg`'s status is the pipeline status; otherwise the final stage's, with a nonzero `rg` status reported as `rg exited with status N`. A Collected `rg` output (redirect, `$(…)`, byte consumer) over 256 KiB is status 2 and writes nothing.
- **Bounds (DV7-10).** Regex 128 KiB, lazy-DFA cache 64 KiB, 16 KiB read buffer with lines up to 65,536 bytes, ignore files 16 KiB / 256 rules, 32 KiB retained ignore rules, 96 KiB / 8,192 pending directory entries, typed-JSONL row bound 65,536 bytes. Compiling ignore-file globs peaks at about 20 KiB per non-literal rule, so cellbox runs on a 6 MiB arena (D76-3 M2) and a search stays within 5 MiB of it, leaving 1 MiB for the shell (host-measured).

### Top and Cell observations

Expose one-shot Cell/task records and sampled frames; top consumes these for interactive/batch views. An incarnation requires authoritative identity/generation, not a process-name group. Sum CPU deltas only for matching lifetimes; distinguish unavailable first samples/counter resets from real idle. State whether CPU uses core-equivalents or machine-normalized capacity. Count shared Cell heap once, separate per-task stack/segment gauges and system committed frames, and do not call them RSS without evidence. Add pagination/completeness metadata rather than a silent 64-row ceiling. Read-only telemetry does not grant kill/restart rights.

### Outbound SSH

Keep protocol/core library choice gated; Sunset is the first evaluated candidate, not a finished port. Required interoperability covers key/password authentication, persistent known-host identity and changed-key rejection, PTY shell/resize, raw non-PTY stdout/stderr/status, EOF/half-close, timeout, rekey and bounded packet/channel flow. Protect credentials, disable secret echo, use real entropy and modern negotiated algorithms. No insecure cipher or host-verification bypass is an acceptable fallback.

Use a controlled guest/other-host server for proof, not Cellos's currently unsafe inbound sshd. Resolve VM route/server/credentials independently; a working outbound command does not qualify Intel VMX, C2C authority or a remote exposure policy. Do not silently retry a dispatched remote command; unknown completion remains unknown. Forwarding, jump hosts and file transfer are not required. Trust-file writes and key ownership need explicit storage/authority policy.

## Authority, dependencies and cutover gates

A common ELF has common manifest/allowlist declarations. Do not use an argv command, alias or PATH mapping to grant elevated rights. Review the artifact's syscall ceiling separately from scoped per-resource service authority; the same signature is not proof every invocation may mutate every file. Keep per-command requirements in the catalog for admission/audit, not as an unenforced security claim.

Maintain an authenticated caller-edge × backend-right matrix for all selected commands, covering file read/mutate, lifecycle/device, network egress/DNS, cross-Cell telemetry, credential/key/trust access and every other cataloged right. Test known privileged commands from restricted callers, not only unknown modes. Unattenuable effects stay behind a separately authorized worker/boundary; no administrative, network or disclosure union is granted for packaging convenience.

Before cutover, inventory init launches, shell identity-based profiles, GUI foreground routes, VFS attachments, external argv, all suite binaries, scripts, signing/image lists and tests. A new path/identity needs exact policy review. Privileged effects stay behind reviewed backend authority; retain a specialized worker where a genuine boundary requires it. If one artifact cannot meet a selected command's safety contract, stop for that exact design decision rather than widening all invocations' authority.

[Law 1](code-standards.md#law-1-interface-is-sacred) is sequenced per delta: CP1 binds an exact specification digest before ABI edits; implementation then supplies the exact diff, consumer census and evidence for CP2 before its final caller/image retirement or rollout. Pending/blocked CP1 stops dependent editing; pending/blocked CP2 stops cutover. Disposable candidate evidence requires its own admitted verification profile and is not production permission.

Import upstream code only after release pinning, per-file/transitive license and advisory review. BusyBox GPL source is not permissive code; uutils/Kibi/ripgrep references do not waive attribution. Ratatui is selected, but feature/runtime feasibility still needs target evidence. No network download, system installation, private-key generation or service exposure is authorized by this document.

After a successful migrated consumer set, remove its duplicate code, obsolete executable targets/image copies and associated dead wording tests. Preserve unrelated user edits and independently consumed service/diagnostic artifacts. Rebuild and sign a matched image/policy set; do not restore a vulnerable server or permissive launch gate as rollback.

## Evidence and completion

All acceptance rows remain pending until exercised under the exact implementation profile. Prove real serial/headless commands, object filtering/encoding, raw bytes, external pipes, GUI terminal sessions, TUI interaction, search traversal/errors, editor save/reopen and SSH against a controlled server. A dependency build, a static-output screenshot or an API mock is not that proof.

Report artifact/image footprint, startup latency, heap/stack and bounded streaming behavior against the existing split-tool baseline; do not promise universal size/RAM/speed gains. Host and QEMU evidence are labeled separately from physical Intel, actual LAN/guest and production qualification. This plan does not activate general IDE/application-ecosystem or new hardware work.
