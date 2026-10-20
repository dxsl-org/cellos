# Cellbox verification protocol

## Current delivery: documents only

This session records accepted decisions and plans future implementation. Do not run target builds, tests, QEMU, downloads, key generation or network services merely to validate Markdown. The documentation smoke check reads the saved artifacts and verifies relative links, D01–D11 coverage, CBX-01..CBX-29 ownership, phase dependencies/acyclicity, pending states and absence of the excluded application example. Review findings/dispositions belong in [review.md](review.md).

The numbering helper failed because its Node runtime was absent; next ADR 0028 was resolved from existing filenames without installing tools. Prior discussion's Cargo metadata/host ripgrep checks are recorded in [source recon](scout-report.md), not upgraded into runtime acceptance.

## Existing commands versus planned consumers

The following targets exist now. Commands are future implementation verification instructions, **not checks run for this docs change**:

```sh
cargo metadata --no-deps --format-version 1
cargo test -p text-engine --target x86_64-unknown-linux-gnu
cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test desktop-x86 -- --nocapture
```

The desktop target accepts `CELLOS_DESKTOP_ISO` and `CELLOS_DESKTOP_ACCEL=tcg|kvm`; inspect current image/helper requirements before use. It presently checks boot/pointer/keyboard paths, not all cellbox acceptance. Do not use a success from this existing test as proof of the new shell bridge, editor or SSH.

After P01 materializes the approved package/features, its cross-build pattern is:

```sh
RUSTFLAGS='-C relocation-model=pic' cargo build --release -p app-cellbox --target x86_64-unknown-none -Z build-std=core,alloc
```

`app-cellbox` is a proposed package name, not an existing target. Additional approved target/runtime flags must follow the actual pinned dependency profile. The kernel has different codegen; do not copy Cell PIC flags into the kernel build. For new `libs/terminal`/`libs/tui-cellos` tests, use their actual materialized package names rather than treating proposed directories as runnable targets.

No new permanent test is required merely for wiring/help strings. Keep meaningful deterministic behavior tests for ownership, type/byte/precision boundaries, lifetimes, cancellation, traversal/errors, metric aggregation and editor data safety. Remove/rewrite incidental fixed-output tests rather than re-pinning them.

## Safe runtime setup after implementation admission

1. Select the exact signed x86 kernel/Cell/policy set, target/features and upstream pins; record hashes. Build failure/missing artifacts must stop packaging. No stale target file fallback.
2. Use disposable copies of disk images/data, unique QMP endpoints and explicit process ownership. Do not kill a VM or overwrite canonical images to free a lock.
3. Avoid unattended `run-desktop.sh`: current source kills image users and opens a host-forward to inbound port 22. Use the integration helper or a dedicated bounded runner with no inbound SSH exposure and reviewed network settings.
4. Use controlled outbound SSH servers, not current Cellos sshd. Pin host identities and authorize any temporary credentials/trust writes. No production endpoints, private user files or unsolicited network service startup.
5. Capture serial output, actual TUI/GUI state and relevant stored/remote results without secrets. Mark TCG/KVM, guest architecture and host/server versions accurately; no physical Intel/VMX/C2C/production promotion from emulator output.

## Required live scenarios

| Surface | Exercise | Observe |
|---|---|---|
| Headless suite | Boot without compositor; explicit/short commands, shell script/state, independent app launch | Real effects/status and no viUI dependency |
| Desktop/CLI parity | Enumerate selected desktop backend operations and exercise each through its CLI path and shared typed service | No desktop-only business operation or parsed CLI table |
| Typed pipeline | Select/filter/sort finite numeric records, JSONL conversion, bad types/encodings, wide IDs/non-UTF-8 | Correct unformatted values, explicit errors, no ANSI/implicit table conversion |
| Streams | Actual child/descendant with duplicate and retained endpoints; parent/last-writer close, slow/closed consumer, cancellation, holder death and stale reuse | Exact bytes, last-writer EOF, operation-scoped revocation, awakened holders and no unrelated-session effects |
| Sessions | Two concurrent sessions, foreground child, resize, interrupt, child death | No cross-read/leak, correct foreground owner and recovered prompt/modes |
| Pipeline jobs/state | Supported mixed native/external background jobs with jobs/fg/bg, or unsupported syntax rejected before effects | Defined job/status/wait/reap/cancel, no unintended parent-state mutation or stranded foreground |
| GUI terminal/TUI | Real changing file/time/process data, cursor/erase/alternate screen, fragmented ANSI, Unicode/paste/resize | Real shell output and visual state; no fixed private interpreter |
| Selective TUI | One-shot commands on terminals and through pipes/non-terminal output; explicit TUI request on unsupported session | No unexpected full-screen/key reads/screen controls; truthful failure or documented equivalent fallback |
| SDK reuse | Throwaway standalone TUI linked to public SDK/backend, not cellbox or viUI | Real target key/resize/output proof; remove consumer afterward |
| Top | Multi-thread Cell, restart/reuse, unavailable/reset sample, more than 64 tasks if admitted, denied query | Incarnation/task distinction, no shared-heap double count, correct CPU units, completeness |
| Search | Multi-chunk >64 KiB file, ignore/hidden/binary/link/access errors, bad regex, early consumer stop | Correct byte-range records/status 0/1/2, bounded I/O and no lossy conversion |
| Editor | Edit/search/select/undo/resize/dirty exit; save-as/reopen, original conflicts and existing/concurrently created/changed destinations, short writes/replace failure | Preserved original/destination bytes and dirty buffer on refusal/failure; no stale-confirmation overwrite or false durability |
| SSH | Controlled other-host and real Tier-3 guest: auth, host identity, PTY/resize, exec stderr/status, EOF/rekey/disconnect | Actual negotiated secure sessions, denied mismatch/auth, no secret logs/plaintext/replay |
| Authority | Every caller-edge/right matrix denial for known commands plus forged modes/records: file, lifecycle/device, network/DNS, telemetry, key/credential/trust and session rights | Real enforcing service/worker denial; no privilege or disclosure union from common ELF |
| Image cutover | Inspect/run final image and command registry callers | One suite executable, no wrappers/obsolete targets; retained services/apps functional |
| ABI checkpoints | Inspect each delta's digest/CP1, exact diff/caller census/runtime evidence and subsequent CP2 | No ABI edit before CP1 and no final caller/image retirement before CP2; no impossible CP2-before-code dependency |

A static screenshot, mock echo, source assertion, non-empty result or successful compilation cannot satisfy these scenarios. For whole-plan completion every [acceptance](acceptance.md) row requires evidence; unavailable guest/hardware prerequisites remain explicit blockers rather than reduced scope.

## Measurements and evidence ledger

Use the same selected command/features/data and target profile before and after. Record executable count/bytes, image footprint, cold/warm startup separately, heap/stack, idle redraw and streaming bounds. Distinguish file size, mapped code and actual resident/committed memory; do not infer loader behavior from ELF size.

Per result record: phase/CBX IDs, implementation commit, dependency pins, target/features, build/runner command, signed image/policy hashes, server/guest identity where relevant, scenario/input, observed output/state, evidence path and limits. Mark pass/blocked/fail individually. No production-ready, physical or universal performance claims without their separate evidence.

## Rollback and cleanup

Restore a matched known-safe code/image/policy set and cold boot before retrying. Code rollback cannot undo saved files, changed trust or executed remote commands. Use disposable fixtures/backups, preserve unknown outcomes and never replay uncertain dispatched work automatically. Remove throwaway consumers, data and temporary secrets after preserving sanitized evidence; do not restore insecure inbound SSH or permissive gates as compatibility fallback.

## Documentation check result — 2026-10-20

The post-review saved-artifact check completed with zero errors:

- 28 Markdown files checked: 18 new decision/design/planning/log artifacts and 10 updated documentation/index/changelog files.
- 68 new/affected local links and anchors resolved. Unrelated historical links were not audited.
- All 10 phase-table rows match frontmatter dependencies; the graph is acyclic, every phase is pending and each phase file is under 150 lines.
- CBX-01..CBX-29 are unique/sequential, have valid phase owners and remain pending. All D01–D11 decisions have a plan coverage entry.
- No occurrence of the excluded application example in new artifacts. Scope remains essential tooling, not an IDE/build environment.
- Ten automated-review findings were integrated; `review.md` records parent dispositions without claiming a later reviewer approval.

This result validates documentation structure/coverage, not Cellos runtime behavior. No implementation build/test/QEMU, dependency import, key generation or network exposure was performed for this documentation delivery.
