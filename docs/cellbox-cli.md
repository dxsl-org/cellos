# Cellbox CLI and terminal architecture

**Updated:** 2026-10-20  
**Status:** accepted design under [ADR-0028](decisions/0028-cellbox-cli-first-structured-pipelines-and-tui.md); implementation pending. This is not a frozen wire/ABI specification or a claim that the commands below already exist.

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

Proposed source ownership is `cells/tools/cellbox`, `libs/terminal` and `libs/tui-cellos`. These paths are design choices, not existing crates. Reuse `libs/text-engine` for already pure logic; introduce a separate data/command-core crate only where an actual second consumer or host-testing boundary warrants it. Keep Ratatui types out of public kernel/service wire contracts.

The proposed single-artifact layout includes a shell module exposed as `cellbox sh`. This is an implementation choice to avoid statically duplicating the utility engine in a separate shell executable, not a new shell language or Unix personality. Init, GUI terminal, launch identities and image paths must migrate together after exact profile approval. Shell builtins stay stateful within this module. Normal independently installed executables remain launchable through approved external-command routes.

## Command catalog and invocation

Canonical artifact/explicit invocation: `/bin/cellbox <command> [arguments...]`. The shell resolves selected short names to registry entries in-process. No reliance on basename dispatch, filesystem symlinks, implicit `argv[0]` or forwarding binaries is required. Startup mode selects behavior but never selects authority by itself.

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
rg "timeout" /data/logs | select path line_number text | to jsonl
ssh host --command 'remote-tool --json' | from json | select status
```

These are target examples, not currently runnable commands or approval of an arbitrary remote-tool package. Record schemas must define byte offsets versus character/display columns, units, nullability and versioned external encoding. JSON encoding must preserve wide identifiers without silent precision loss and non-UTF-8 bytes without lossy replacement.

### Streaming and state

Finite filters pull bounded records; downstream cancellation/closed pipes stop upstream work and release resources. Declare and bound materializing stages such as global sort. Group continuous top data by snapshot; never sort an infinite history before emitting a first result. Carry EOF, timeout, cancellation, partial result and terminal error separately. No automatic retry after uncertain side effects.

Preserve shell cwd/environment, quoting, status, job tracking and source execution through cutover. Freeze standalone versus pipeline stateful-builtin context, whole-pipeline job/status, foreground transfer, wait/reap and cancellation. Preserve supported forms; reject unsupported background/mixed/stateful syntax before any stage has effects, never silently ignore `&` or mutate unintended parent state. Define redirects/script migration before implementation; replace incidental wording tests with behavioral coverage.

## Terminal SDK and Ratatui

The public terminal SDK provides session attachment, one effective input reader, foreground ownership, bytes/events, size/resize, supported capabilities, echo/raw mode, cancellation and teardown. EOF/peer death releases ownership; a dying child must not steal or strand the next shell prompt. Control effects cannot cross sessions. Use actual session/kernel enforcement where required; a local `has_reader_lease` boolean is not an ownership protocol.

Stream/control endpoints are non-inheritable unless explicitly delegated as typed child grants. Define duplicated/descendant holder ownership, generation/reuse protection and EOF after the last authorized writer closes or is revoked. Operation cancellation/consumer close revokes that operation's delegated subtree and wakes blocked holders, without revoking unrelated sessions. Test retained writers and descendant death; a parent retaining an endpoint cannot strand cancellation.

Define a minimum ANSI/VT profile for cursor movement, erase, color, wrapping, scrolling and alternate screen, plus supported keys, paste and resize. Bound parser sequences, dimensions and scrollback; discard/escape unsupported control operations, including unapproved clipboard/device side effects. Do not advertise TERM features not implemented. Secret entry disables echo and never reaches logs/scrollback/history.

The Cellos Ratatui backend translates changed cells/cursor operations to the session output contract; input/session lifecycle stays explicit rather than assumed to be handled by the widget library. Pin supported Ratatui features/dependencies and Unicode-width behavior. TUI code is unavailable in plain data paths only through command mode, not through a mandatory GUI dependency. Share theme semantics only where useful; do not port pixel layouts or viUI widget trees.

GUI Terminal attaches to the real shell/foreground session, interprets output and forwards input/resize. Remove its independent command matcher and fabricated uname/pwd/ls/ps/date data after the real bridge is proven. Preserve applicable ADR-0027 native session/tab rules; neither TUI buffer nor terminal viewport is a separate Cell Tab. Serial/SSH terminals remain functional without desktop startup.

New independent apps consume the SDK/backend as libraries. Their static executables may contain required TUI code; no dynamic linking, stable Rust ABI or universal render-service dependency is implied. Accepting Ratatui does not force an existing third-party app to rewrite its own TUI.

## Selected tools

### Small editor

Deliver open/create/save/save-as, navigation, selection/copy/cut/paste, bounded undo/redo, search, go-to-line, dirty-exit handling, UTF-8 width and selected highlighting. Preserve content/line endings/final-newline state unless explicitly changed. Original and Save As destination identity/version must be checked at commit, including pre-existing or concurrently created/changed destinations: explicit overwrite confirmation plus enforcing conditional create/replace behavior is required. Failed/short/interrupted replacement retains stored bytes and dirty edit state. Verify actual VFS guarantees before claiming atomic/durable save. Kibi's line limit/Unix terminal calls and any plugin/LSP/IDE/build system are not requirements.

### Ripgrep-based search

Use upstream library boundaries and VFS stream traversal; do not substitute ERE-lite grep. Document selected flags, ignore precedence, default hidden/binary handling, symlink policy, error/status semantics, encoding and cancellation. An unchanged file can exceed the old 64 KiB aggregate input bound without loading it all. Report inaccessible entries/partial traversal truthfully. Direct records carry path, line number, absolute byte offset, line bytes/text and submatch byte ranges; no ANSI or colon-delimited reparse. Validate 0/1/2 match/no-match/error conventions for the chosen compatibility surface. PCRE2/mmap/full parallelism/indexing are outside required scope.

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
