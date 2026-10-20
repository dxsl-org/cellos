# ADR-0028 — Cellbox, CLI-first tools, structured pipelines and selective TUI

Date: 2026-10-20

## Status and authority

**Accepted architecture, following the maintainer's explicit approval in the CLI/tooling discussion. Documentation and implementation planning only; no implementation has been performed by this decision.**

The maintainer selected the name **cellbox**, a single executable for the essential tool suite with modular source, structured command data inspired by Nushell, a small editor inspired by Kibi, ripgrep-based search, Cell-aware monitoring, an outbound SSH client and selective Ratatui-based TUI. This is a bounded operational surface for Cellos, not a Linux command inventory or a general application-development program.

[ADR-0022](0022-intel-x86-64-c2c-only-direction.md) continues to own the Intel C2C direction. [ADR-0024](0024-cell-kernel-authority-contract.md), the [lock register](../../.agents/cell-contract-0024-locks.md) and applicable Law-1 checkpoints continue to own authority/ABI admission. This approval does not release legacy locks, select an implementation WIP slice, ratify wire bytes or authorize remote/production exposure. See the [implementation plan](../../.agents/261020-1750-cellbox-cli/plan.md).

## Context and decision drivers

The present tool surface mixes shell-local utility implementations with separate system/network executables. Source inspection found shell-local byte-capture pipelines, external argv transport without a program-name argument, a GUI terminal with its own fixed-response command interpreter, and task-oriented rather than incarnation-aware monitoring. Shared API/service contracts alone do not remove repeated static-linked executable code or make these paths compose.

The goals are one maintainable core tool suite, useful headless operation, scriptable data, reusable terminal facilities, least authority and a simple desktop for non-experts. Smaller images or faster execution are measurement questions, not consequences guaranteed by choosing multicall.

## Decisions

### D01 — CLI-first, shared backend, simple desktop

CLI is the complete operational surface for the selected Cellos tools. Text/structured commands must work without the desktop. Desktop exposes common approachable actions over the same typed SDK/service contracts; it must not duplicate business logic or parse formatted CLI output. CLI completeness never grants blanket administrative authority.

### D02 — One essential-suite executable, modular source

Build the selected essential command suite as **`cellbox`**, with one implementation per command and shared helper modules. Do not combine source into one file. `cellutils` is not a second product or compatibility executable. Multicall is packaging/dispatch, not a requirement for one permanent process or one Cell per pipeline stage.

The final migration removes superseded suite executables, duplicate implementations and image entries; it does not leave forwarding ELF wrappers or deprecated aliases. Short command names remain first-class shell dispatch names, not duplicate binaries. The explicit `cellbox <command> ...` entry is canonical for non-shell callers. Stateful shell operations remain in the shell execution context, never in an unrelated child invocation.

Standalone applications remain allowed and can use the terminal/TUI SDK independently. Services, drivers, hypervisors and long-running workers retain their own lifecycle boundaries; a CLI client can control an admitted service without embedding that service in cellbox. No application installer/store or IDE/build environment is admitted.

### D03 — Curated commands, not Linux breadth

Retain useful existing command behavior through an explicit migration catalog; include only commands with a named Cellos operational consumer. Reuse existing file/text/network logic rather than inventing alternate implementations. The approved additions/upgrades are the compact editor, ripgrep-based search, Cell-aware top and outbound SSH below. Shell state/control flow and optional specialized executables are not an excuse to give the common binary every privilege.

### D04 — Typed data internally; JSON at explicit boundaries

Follow Nushell's separation of values, record streams, byte streams and final display, not a wholesale port of Nushell or its language. Native stages exchange typed data directly when in one execution context. Crossing a Cell/process boundary requires an explicit bounded transport; a common executable alone does not share objects.

Keep values typed, numbers unformatted, units explicit and unknown values distinct from zero. JSON/JSON Lines are import/export encodings, not mandatory serialization between internal stages. Convert records to external text/bytes explicitly; never send a rendered table as implicit stdin. Raw file contents, SSH streams and interactive terminal sessions must not be forced into object/JSON envelopes.

Separate successful data, diagnostics and exit status. Declare accepted input/output types, finite versus streaming behavior and interaction requirements. Bound retained data, implement backpressure/cancellation/EOF, and expose truncation or partial failure rather than silently dropping records. Formatting, colors and table borders belong at the final display sink; machine output never contains terminal decoration.

### D05 — Text/structured baseline, selective TUI

Use ordinary commands for one-shot operations and automation; TUI only where interaction materially helps. The editor is TUI-first; top offers interactive and batch/structured views; search returns records/text; SSH provides terminal sessions or raw remote-command streams. No universal TUI launcher/menu framework is required.

Do not open a full-screen interface merely because output is a terminal. Piped/non-terminal operation must not unexpectedly read keys or emit screen-control sequences. Explicit unsupported TUI requests fail truthfully; text fallback is allowed only where it preserves the requested operation.

### D06 — Public terminal SDK; Ratatui separate from viUI

**Select Ratatui** for new first-party TUI work, with a Cellos backend and only needed features. Keep the terminal SDK and TUI integration reusable libraries outside cellbox and outside viUI's pixel/widget core. Disable unsuitable default terminal backends; `no_std` support is not proof that a dependency builds on Cellos.

Terminal session ownership, input, output, resize and mode restoration are common platform facilities. Ratatui widgets/render buffers consume them. The GUI terminal is an ANSI/VT terminal emulator rendered through viUI, not a second implementation of the tool suite. Serial and remote terminals need no compositor or font rasterizer.

Other CLI applications may link the SDK/Ratatui integration normally, or retain another toolkit using the same terminal contract. They do not link to the cellbox executable. Static-link duplication across independently installed apps is acceptable until measurement justifies another design; no shared-object ABI or universal rendering service is introduced merely to deduplicate it.

### D07 — Small editor inspired by Kibi

Provide an approachable nano-like terminal editor for config, scripts and text: open/create/save, dirty-exit confirmation, navigation, selection/copy/cut/paste, bounded undo/redo, search, go-to-line and correct UTF-8/display-width handling. Add lightweight highlighting only for selected relevant formats. Keep terminal/VFS adaptation separate from edit logic. Preserve existing content on failed/partial saves and concurrent modification; report actual backend durability, not an unimplemented atomic-save promise.

Kibi is a code/design reference, not a line-count target or a requirement to reuse Unix `libc` terminal calls. No plugin/LSP/IDE, embedded terminal or general development environment is selected.

### D08 — Ripgrep-based search

Reuse appropriate upstream matcher/searcher/ignore components and adapt traversal/I/O to Cellos. Produce match records directly from search callbacks rather than reparsing colorized text. Stream bounded chunks, respect access boundaries and document hidden/binary/ignore behavior, UTF-8 versus byte data and exit status. No mmap, PCRE2, parallelism or full GNU/rg compatibility is presumed necessary.

Publish the supported compatibility surface before calling the command `rg`; do not rebrand the current bounded ERE-lite grep as ripgrep. If required upstream code cannot run under the admitted runtime, that is a prerequisite to resolve, not permission for a fake substitute or an unrequested full runtime rewrite.

### D09 — Cell-aware monitoring, not summed task rows

Reuse existing top sampling/delta logic where correct. Default monitoring should identify a Cell incarnation and expose a task/thread drill-down. Use authoritative identity, cumulative counters and explicit sample metadata. Deduplicate Cell-scoped memory, define multi-core CPU normalization and display unknown/stale/truncated telemetry truthfully. Keep system allocator metrics distinct from Cell-owned or resident-memory claims. TUI and structured output consume the same records; lifecycle actions remain separately authorized.

### D10 — SSH client in cellbox, not a server merge

Include the outbound client in the common executable, with separate modules and per-session state. A fresh invocation may run in a separate Cell; this is not by itself stronger isolation or subcommand-specific authority.

Required client behavior is verified host identity/known-hosts handling, public-key authentication and safe password input when selected, shell/PTY/resize, non-PTY remote commands with separate stdout/stderr and exit status, timeout/EOF/close/rekey and modern cryptography backed by real entropy. No plaintext/deterministic-key fallback is acceptable. Remote command quoting must not pretend SSH transmits a structured argv.

Evaluate Sunset first for its embedded core; Russh is an alternative only with a justified, demonstrated runtime. No particular SSH library/version is ratified here. Forwarding, jump hosts and SFTP are not required by this decision. VM access requires an actual guest SSH server, route, credentials and host identity; it does not authorize VM auto-provisioning or claim Intel VMX/network readiness.

Do not reuse the current inbound `sshd` as a trusted client core: source inspection found unconditional authentication success, fixed key material and a plaintext cipher offer. Keep it off new test/exposure paths; do not silently expand this plan into an inbound-server rewrite.

### D11 — Authority and clean cutover

Executable consolidation must not increase ambient authority. One ELF manifest/allowlist is not per-subcommand protection; an argv string or alias is not an authorization credential. Service-side scoped authorization, approved launch edges, object handles and execution tiers remain load-bearing. If a command needs an unavailable safe authority path, record and resolve that exact prerequisite; do not grant the union of administrative/device privileges.

Changing shell image path/name, startup mode, argv/cwd/environment, stream attachment, telemetry layout or rights requires a concrete caller/ABI/approval census. Implement the final cutover atomically per selected consumer set; no hidden compatibility shim or fallback to obsolete binaries.

## Alternatives rejected

1. **Every command permanently gets its own ELF, with only shared API calls.** Maintains repeated entrypoints/static-linked helpers and many packaging/signing targets without a demonstrated boundary need for ordinary utilities.
2. **One source file, one privileged daemon or every installed application in cellbox.** Confuses packaging with execution/authority; grows fault, update and review scope unnecessarily.
3. **Force all input/output through JSON or formatted text.** Adds copies/parsing, loses byte-stream/session semantics and mixes human presentation with machine data.
4. **Put TUI into viUI, or require every application to use Ratatui.** Couples headless tools to pixel-oriented dependencies and makes unrelated application ports diverge needlessly.
5. **Copy all BusyBox/uutils/Nushell functionality.** Solves Linux breadth rather than named Cellos needs and introduces runtime, maintenance and licensing scope without a consumer.
6. **Make every command a TUI or build an IDE now.** Obstructs automation; an IDE has no established demand/value without a usable build environment.

## Consequences and verification

Benefits are coherent command semantics, reusable terminal infrastructure and fewer essential-suite artifacts. Costs include stream/session integration, a real GUI-terminal bridge, command migration, dependency porting and Cell-scoped telemetry prerequisites. No footprint, speed, stronger isolation, physical qualification or compatibility claim follows from the design alone.

Measure the signed before/after artifact set, image footprint, startup, memory and streaming behavior with exact profiles. Demonstrate real headless and GUI sessions, structured pipelines, safe editor persistence, denied authority, search errors and SSH interoperability/host-key rejection. Historical completion labels do not substitute for those consumer paths.

Rollback uses a matched known-safe code/image/policy set before admission. File writes and accepted trust-state updates are not undone by binary rollback; preserve data and never restore insecure authentication to recover compatibility.

## References

- [Design and migration contract](../cellbox-cli.md); [implementation plan](../../.agents/261020-1750-cellbox-cli/plan.md).
- [ADR-0018: runtime adaptation](0018-cell-native-portability-and-runtime-profiles.md); [ADR-0027: desktop](0027-haiku-tabs-and-spotlight-hud-desktop.md).
- [BusyBox architecture](https://busybox.net/downloads/BusyBox.txt), [GPLv2 obligations](https://busybox.net/license.html).
- [uutils structure](https://github.com/uutils/coreutils), [MIT license](https://github.com/uutils/coreutils/blob/main/LICENSE).
- [Kibi](https://github.com/ilai-deutel/kibi), MIT OR Apache-2.0; [ripgrep](https://github.com/BurntSushi/ripgrep), MIT OR Unlicense.
- [Nushell command contracts](https://www.nushell.sh/contributor-book/commands.html), [PipelineData](https://docs.rs/nu-protocol/latest/nu_protocol/enum.PipelineData.html).
- [Ratatui no_std release](https://github.com/ratatui/ratatui/releases/tag/ratatui-v0.30.0), [backend boundary](https://ratatui.rs/concepts/backends/).
- [Sunset](https://github.com/mkj/sunset), [Russh](https://docs.rs/russh/latest/russh/).

Upstream links inform the decision; pin releases/commits, file licenses, transitive dependencies and current advisories before importing code. Reading source does not waive copyright obligations. Prefer permissive uutils code where appropriate; copying GPL BusyBox code requires deliberate compatible licensing/distribution review, not relabeling it as permissive Cellos code.
