# Cellbox acceptance ledger

**State:** all outcomes pending. Design approval does not populate runtime evidence. Every row is required for whole-plan completion; a blocked row stays blocked rather than being removed. [ADR](../../docs/decisions/0028-cellbox-cli-first-structured-pipelines-and-tui.md) and [design](../../docs/cellbox-cli.md) own scope; [plan](plan.md) owns scheduling.

| ID | Observable pass/fail outcome | Owning phase | State |
|---|---|---|---|
| CBX-01 | Exact command/caller/image/desktop-operation catalog and caller-edge × backend-right matrix cover all selected commands; the per-delta digest/owner gate ledger requires CP1 before editing and CP2 after implementation evidence before final cutover. Actual closure is enforced by each implementing phase and audited in P10. | 01 | pending |
| CBX-02 | Pinned upstream versions, per-file/transitive licenses, notices, advisories and target runtime feasibility are recorded; no permissive relabeling of GPL code. | 01 | pending |
| CBX-03 | Sessions cannot cross-read input; endpoint inheritance is denied unless explicitly delegated by typed child grant, with generation/lifetime checks; foreground exit/death/cancellation releases actual ownership. | 02 | pending |
| CBX-04 | External child stdin/stdout/stderr, raw non-UTF-8 bytes, empty/space-containing argv, cwd and environment survive spawn/redirection without console-log substitution. | 02 | pending |
| CBX-05 | Duplicate/descendant endpoint holders obey last-writer EOF; operation cancellation/consumer close revokes its delegated subtree, wakes blocked readers/writers and rejects stale generations while unrelated sessions remain intact; buffering stays bounded. | 02 | pending |
| CBX-06 | Native record pipelines select/filter/sort finite data without text or JSON roundtrips; invalid input/output types fail before unintended effects. | 03 | pending |
| CBX-07 | Explicit JSON/JSONL/text conversions preserve values, wide identifiers and byte data; malformed input fails; machine output contains no ANSI/table decoration. | 03 | pending |
| CBX-08 | Generic raw file/external adapters preserve bytes, diagnostics and status; real bounded byte/frame producers emit incrementally, stop on consumer close and do not require an ever-growing list/global infinite sort. Concrete top/SSH consumer proof is owned by CBX-18/23. | 03 | pending |
| CBX-09 | Shell state/scripts and supported mixed pipeline jobs retain defined context, one job/status, foreground, wait/reap/cancellation semantics; unsupported background/stateful forms fail before effects, never silently ignoring `&` or mutating unintended parent state. | 04 | pending |
| CBX-10 | Signed image has one cellbox suite executable; explicit subcommand and short-name dispatch work; all migrated split targets/duplicates/wrappers are removed and standalone services/apps still launch. | 04 | pending |
| CBX-11 | Every caller-edge × backend-right matrix denial holds for known privileged commands and forged modes: file, lifecycle/device, network/DNS, telemetry, credential/key/trust and other cataloged rights cannot widen through the common ELF; launch identities and applicable CP2 approvals hold at final cutover. | 04 | pending |
| CBX-12 | Headless commands work without compositor/viUI; every cataloged desktop backend operation in selected tooling has a demonstrated text/structured CLI path over the same typed service operation, with no desktop-only business logic or CLI-table parser. | 04 | pending |
| CBX-13 | Ratatui TUI draws and handles input/resize through Cellos backend; raw/echo/cursor/alternate-screen state restores on normal exit, cancellation and abnormal app loss. | 05 | pending |
| CBX-14 | GUI Terminal executes the real migrated shell and actual filesystem/process/time changes before final caller/image cutover; the fixed-response interpreter is gone and two sessions remain independent. | 04 | pending |
| CBX-15 | Supported ANSI/VT, Unicode width, fragmented controls, paste, resize and scrollback work in the actual GUI surface and serial terminal; unsupported controls have bounded safe behavior. | 05 | pending |
| CBX-16 | A throwaway standalone CLI consumer uses public Terminal SDK/Ratatui backend without linking the cellbox executable or viUI; no permanent extra application is added. | 05 | pending |
| CBX-17 | Top defaults to real Cell incarnations with task drill-down; ID reuse/restart never merges lifetimes; shared memory is not double-counted. | 06 | pending |
| CBX-18 | Real sampling streams emit bounded frames and stop on consumer close; CPU normalization, unavailable/reset samples, completeness and system-vs-Cell memory scope are truthful; TUI/batch/record views agree for the same sample. | 06 | pending |
| CBX-19 | Upstream-derived search traverses admitted VFS trees with documented ignore/hidden/binary/symlink rules; files over the old 64 KiB bound are processed with bounded memory. | 07 | pending |
| CBX-20 | Search produces direct path/line/byte-range records, preserves non-UTF-8 data, reports partial access errors and returns documented 0/1/2 statuses; early consumer close stops traversal. | 07 | pending |
| CBX-21 | Editor performs open/create/save-as/navigation/selection/copy-cut-paste/undo-redo/search/go-to-line/dirty-exit/selected highlighting and UTF-8 width in real terminal sessions. | 08 | pending |
| CBX-22 | Editor save/reopen preserves content/line endings; original and Save As destination races, existing destinations and denied/short/interrupted/replacement failures preserve stored bytes and dirty edits through confirmed conditional commit; durability claims match VFS evidence. | 08 | pending |
| CBX-23 | SSH authenticates to controlled other-host and Tier-3 guest servers, supports PTY shell/resize and raw non-PTY stdin/stdout/stderr/remote exit status without JSON conversion; producer traffic stops on consumer close. | 09 | pending |
| CBX-24 | SSH rejects changed/untrusted host identities per explicit trust policy and invalid auth; uses real entropy, no plaintext fallback, protects secret input/logs and handles rekey/bounds/EOF/disconnect honestly. | 09 | pending |
| CBX-25 | VM endpoint route/server/credentials are explicitly proven; uncertain dispatched remote work is not replayed; existing unsafe inbound sshd is not started or exposed as a test prerequisite. | 09 | pending |
| CBX-26 | All selected tooling is packaged in a matched signed x86 image/policy, exercised through serial and GUI surfaces; no obsolete utility artifacts or caller routes remain. | 10 | pending |
| CBX-27 | Measured before/after executable/image sizes, startup, memory and bounded long-stream behavior are recorded with feature/image provenance; no unsupported universal optimization claim. | 10 | pending |
| CBX-28 | Source, command docs/help and current scheduling reflect delivered evidence only; independent app/library boundaries, selective TUI and excluded IDE/build-environment scope remain intact. | 10 | pending |
| CBX-29 | Ordinary one-shot commands never enter full-screen mode merely because they have a terminal; piped/non-terminal modes do not read keys or emit screen controls; unsupported explicit TUI requests fail or use only a documented equivalent fallback. | 05 | pending |

## Evidence rules

Record implementation commit, pinned dependencies, target/features, exact signed artifact/image/policy hashes, scenario/command, observed result and evidence path. No row passes from source text, a forwarded mock result, a screenshot with fixed output or a successful compile alone. Deterministic permanent tests target plausible consumer-visible invariants; throwaway consumers/smoke fixtures are removed after use. Existing incidental wording/implementation tests are not re-pinned.

Host tests cover pure behavior; target smoke covers actual Cell integration. QEMU TCG/KVM results are software evidence only. SSH needs actual protocol interoperability with a controlled server; the guest-specific row needs an actual Tier-3 guest path. If unavailable, keep that portion blocked and state the exact prerequisite. No physical Intel, remote C2C or production promotion follows from these rows.

## Documentation-only delivery check

The current change is complete when all D01–D11 decisions map to this ledger/phases, relative links resolve, the phase DAG is acyclic, implementation status remains pending and no excluded application-development work is scheduled. This document check is not any CBX implementation pass.
