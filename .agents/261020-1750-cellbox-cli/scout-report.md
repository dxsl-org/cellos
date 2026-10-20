# Cellbox source recon

**Observed:** 2026-10-20; read-only analysis and Cargo metadata from this discussion. No Cellos build/boot, new upstream port or ABI approval was performed. Recheck actual source and approvals at phase entry; historical plan-complete labels are not consumer-path evidence.

## Project and patterns

Rust workspace, predominantly `no_std` + `alloc` native Cells, `ostd` SDK, explicit ELF manifests/syscall allowlists, signed images and userspace service clients. Architecture is mixed: Cell/service-oriented backend with shell-local procedural dispatch and separate utility executables; viUI is a pixel-oriented GUI toolkit. Current x86 Cell codegen differs from kernel codegen.

## Observed implementation facts

| Evidence | Finding and planning impact |
|---|---|
| `cells/tools/sys-tools/Cargo.toml:13-55` | 11 binaries: ls, cat, echo, ps, kill, env, uname, date, free, shutdown, hotswap. Shared package does not mean shared executable. |
| `cells/tools/net-tools/Cargo.toml:12-34` | 6 binaries: ping, curl, nc, wget, httpd, mqtt. Inventory short-lived client versus long-lived service roles before migration. |
| `cells/tools/shell/src/executor.rs:51-56,598-742` | Completion inventory and builtin dispatch are separate; shell-local utilities overlap executable names. Catalog/caller census required. |
| `cells/tools/shell/src/executor.rs:354-388` | Pipelines sequentially capture `Vec<u8>`; not typed or incremental external pipelines. |
| `cells/tools/shell/src/executor.rs:879-953` | External spawn publishes argv, selects path and waits/focus-handoffs. No inherited shell capture/stdin buffer contract is wired here. |
| `libs/ostd/src/args.rs:16-65` | Structured spawn arguments have a 512-byte payload bound and no automatic program-name argument. Do not assume symlink/basename multicall dispatch. |
| `libs/ostd/src/io.rs:84-93,262-286` | Print uses sys_log; Stdout accepts UTF-8 and logs. Not an exact-byte redirectable stdout abstraction. |
| `libs/ostd/src/syscall.rs:364-450`; `libs/api/src/abi/syscall.rs:190-219` | Existing create/read/write/share/close pipe wrappers/primitives. Reuse where their owner/lifetime contract fits; do not claim all pipes are missing. |
| `kernel/src/loader/launch_profile/mod.rs:1-5,54-94`; `profiles.rs:56-69`; `targets.rs:7-107` | Launch authority depends on caller, route and reviewed target. A new common path/name is an authority migration, not just a Cargo rename. |
| `cells/tools/net-tools/src/bin/curl.rs:25`; `wget.rs:30-38` | Allowlist difference: wget includes VfsMutate. One artifact cannot pretend to retain per-command ELF restrictions. |
| `libs/text-engine/src/lib.rs:14-31`; `grep.rs:21-63` | Reusable pure cores exist, but current grep preloads bounded text and returns formatted output; it is not ripgrep. |
| `cells/tools/shell/src/top.rs:28-54`; `libs/text-engine/src/top.rs:93-118` | Top exists, samples two snapshots, calculates task CPU deltas and renders up to 64 rows. Reuse, do not rebuild as a fake implementation. |
| `libs/api/src/abi/syscall.rs:1439-1462` | ProcessInfoV2 lacks Cell identity/generation; MemInfo reports committed physical frames, not RSS. Cell-aware rows require a reviewed authoritative provider. |
| `cells/apps/terminal/src/session.rs:76-140`; `grid.rs:67-115` | Independent command matcher includes fixed uname/pwd/ls/ps/date output; grid handles basic control characters, not a full ANSI/VT emulator. A claimed terminal phase completion is not proof of a real shell bridge. |
| `libs/viui/Cargo.toml:6-22`; `libs/viui/src/lib.rs:1-30`; `cells/apps/terminal/Cargo.toml:11-15` | viUI is pixel rendering; GUI terminal depends on it; shell does not. Preserve headless dependency direction. |
| `cells/services/sshd/src/session.rs:199-208`; `kex.rs:29-38,71-74,98-106` | Inbound server authenticates without validation, uses fixed private key material and advertises plaintext. Not a safe outbound core or interoperability fixture. |
| `scripts/build-x86_64-cells.ps1:12-21,67-71` | x86 Cells use PIC while kernel config differs; current builder warns on some failed builds, so new packaging must not consume stale artifacts. |
| `run-desktop.sh:18-24,67-81` | Interactive script may kill image users and forwards inbound TCP port 22; do not run it as an unattended qualification default. Use private image/QMP runner with no inbound server exposure. |
| `tests/integration/Cargo.toml:214-216`; `tests/integration/tests/desktop-x86.rs:33-68` | Existing desktop-x86 target and q35 BIOS helper can be extended; currently a pointer/boot scenario, not proof of typed CLI, editor or SSH. |

## Contracts and admission

- ADR-0028 owns new tooling decisions; `docs/cellbox-cli.md` owns design boundaries.
- ADR-0018 owns runtime adaptation; ADR-0024 and the lock register own authority/ABI cutover; ADR-0027 owns retained desktop session presentation.
- Current-focus/portfolio contain both freeze wording and later completion records. Reconcile exact per-surface evidence/handoff in Phase 01 without rewriting historical completion claims or inferring a blanket unlock.
- `docs/specs/11-shell.md` is explicitly historical. It must point to the new design, not become a second normative contract.

## Observed checks

- `cargo metadata --no-deps --format-version 1` succeeded during analysis; confirmed the 11/6 executable targets and viUI/shell/terminal dependency separation.
- Host ripgrep 15.1.0 smoke: JSON begin/match/end/summary; no ANSI; non-UTF-8 bytes encoded; status 0/1/2 for match/no match/invalid pattern. This is host upstream behavior, not a Cellos port.
- ADR numbering helper could not execute because its Node runtime was absent. Existing decision filenames were enumerated; next free sequence was 0028. No Node installation or unrelated tooling change was made.

## Unresolved implementation prerequisites

Pin and prove Ratatui feature/backend support, upstream search/runtime route, editor save semantics and SSH library/entropy/key-store integration. Resolve Cell telemetry identity and memory scope, stream attachment/ownership, exact launch identities, script compatibility and machine schemas under existing approval gates. No currently absent path/package/test name is represented as an available implementation.
