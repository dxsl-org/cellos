# Phase 05 Execution Plan: Migrate Callers and Retire DataPtr

Date: 2026-08-10  
Base: `HEAD c7c3ca31` on `main`  
Scope: production caller migration only. No ABI/syscall/manifest/fast-path/Tier2/async-DMA expansion. Product code remains unchanged by this planning pass.

## Verdict

plan ready: `.agents/260809-0922-sas-lbi-revocable-vfs-access/reports/phase-05-execution-plan.md` -- 6 slices, top risk: sealing a caller before every path-addressed operation it still needs has a handle equivalent.

## Observed Constraints

- `docs/code-standards.md:12` makes `libs/api/` and `libs/types/` Law 1 interfaces. Phase 05 must not change them; Phase 04 already appended `OpenFileAt=23`, `ReadFileHandle=24`, `CloseFile=25`, and `FileHandle=9` in `libs/api/src/services/ipc.rs:155`.
- Spec 17 requires request/reply receives to be masked to the peer tid, not `sys_recv(0)`, at `docs/specs/17-ipc-wire-contract.md:42`; the reusable compliant helper is `ostd::ipc::service_call_typed` at `libs/ostd/src/ipc.rs:66`.
- File-handle reads are post-seal-compatible: `OpenRootDir` is the remaining bootstrap path op, `SealPaths` blocks future path-addressed ops, and `OpenFileAt` / `ReadFileHandle` / `CloseFile` are non-path-addressed in `libs/api/src/services/ipc.rs:190`.
- The file-handle payload cap is 4000 bytes (`IPC_BUF_SIZE - 96`) in `cells/services/vfs/src/dispatch_dirs.rs:31`; legacy `Poll` is capped at 480 bytes in `cells/services/vfs/src/dispatch.rs:196`.
- `GetFile` still serves `DataPtr` on the normal message path at `cells/services/vfs/src/dispatch.rs:55` and on the fast handler at `cells/services/vfs/src/main.rs:127`; both must remain enabled until every production caller below passes.

## Production Caller Matrix

| Caller | Startup order | Current read | Reply safety | Bounds/close | Phase 05 decision |
|---|---|---|---|---|---|
| `VfsClient` facade | Lazy service registry resolve in `libs/ostd/src/service.rs:75` | `GetFile` at `libs/ostd/src/clients/vfs.rs:48` | Wildcard `sys_recv(0)` at `libs/ostd/src/service.rs:122` | Rejects `DataPtr`, no handle close path | Slice 1: add masked handle reader and close-on-all-paths helper; then migrate facade consumers. |
| Hypha tool-fs | Lazy per-request client in `cells/apps/hypha/tools/fs/src/main.rs:60` | Facade `read_file` | Inherits facade wildcard risk | Truncates tool result to 2048 chars at `cells/apps/hypha/tools/fs/src/main.rs:63`; no file handle | Slice 2b: use facade handle reader for read only; leave write/list unsealed unless migrated too. |
| net-broker identity | Init-time `identity.load_config()` at `cells/services/net-broker/src/main.rs:107` | Facade `read_file` for `/etc/cellos/cluster.cfg` at `cells/services/net-broker/src/identity.rs:60` | Inherits facade wildcard risk | Config parse tolerates missing file; no handle close | Slice 2a: fixed-root read via facade; no grant/syscall widening. |
| net-broker K1 | Scaffolding only; `main.rs` TODO at `cells/services/net-broker/src/main.rs:117` | `VfsFileKeySource::load()` uses facade at `cells/services/net-broker/src/transport.rs:65` | Inherits facade wildcard risk | Requires >=32 bytes at `cells/services/net-broker/src/transport.rs:67` | Slice 2a tests exact 32-byte/short-file behavior even if not startup-wired. |
| shell | Init spawns VFS first and shell last at `cells/tools/init/src/main.rs:86` and `cells/tools/init/src/main.rs:94` | Phase 02 `Stat` + `ReadFileGrant` in `cells/tools/shell/src/cmd_fs.rs:365` and `cells/tools/shell/src/cmd_fs.rs:408` | Already masked through `service_call_typed` at `cells/tools/shell/src/cmd_fs.rs:29` | Grant freed by `Drop` at `cells/tools/shell/src/cmd_fs.rs:338`; no file handle | Slice 3: migrate VFS reads to the common handle walker or explicitly keep shell grant as rollback until the all-caller gate. |
| Lua | VFS tid hardcoded at `cells/runtimes/lua/src/bindings_vfs.rs:16`; comment assumes VFS before Lua at `cells/runtimes/lua/src/main.rs:48` | `GetFile` / `DataPtr` at `cells/runtimes/lua/src/bindings_vfs.rs:57` and `cells/runtimes/lua/src/bindings_vfs.rs:68` | Wildcard replies at `cells/runtimes/lua/src/bindings_vfs.rs:65` | Read cap 64 KiB at `cells/runtimes/lua/src/bindings_vfs.rs:46`; writes chunk 400 at `cells/runtimes/lua/src/bindings_vfs.rs:128`; no `CloseFile` | Slice 4: resolve VFS, use masked helper, then handle-read Lua script loading/read APIs. Do not `SealPaths` until stat/list/write are handle-safe too. |
| WASM loader | VFS tid hardcoded at `cells/tools/wasm/src/main.rs:16` | `GetFile` / `DataPtr` at `cells/tools/wasm/src/main.rs:100` and `cells/tools/wasm/src/main.rs:113` | Wildcard, only `sender > 0`, at `cells/tools/wasm/src/main.rs:110` | Trusts `len as usize` into `to_vec()` at `cells/tools/wasm/src/main.rs:118`; no close | Slice 4: resolve VFS and load by bounded handle chunks; reject oversize modules with typed error. |
| service HTTPD | Waits for NET and VFS at `cells/services/httpd/src/main.rs:34` | `ReadAsync` + one `Poll` at `cells/services/httpd/src/net_ipc.rs:131` and `cells/services/httpd/src/net_ipc.rs:144` | Wildcard net and VFS replies at `cells/services/httpd/src/net_ipc.rs:16` and `cells/services/httpd/src/net_ipc.rs:136` | Single-poll truncates; TCP send chunked 480 at `cells/services/httpd/src/net_ipc.rs:40`; no file close | Slice 5a: mask service replies, replace VFS read with bounded handle loop and close. |
| net-tools HTTPD | Looks up NET/VFS once at `cells/tools/net-tools/src/bin/httpd.rs:236` and `cells/tools/net-tools/src/bin/httpd.rs:243` | `ReadAsync` + one `Poll` at `cells/tools/net-tools/src/bin/httpd.rs:77` and `cells/tools/net-tools/src/bin/httpd.rs:90` | Wildcard replies at `cells/tools/net-tools/src/bin/httpd.rs:82` and `cells/tools/net-tools/src/bin/httpd.rs:129` | Single-poll truncates; no file close | Slice 5b: migrate or mark out of Phase 05. Do not leave it as the last production `ReadAsync` file server accidentally. |

## Data Flow Target

For each migrated read:

1. Resolve VFS tid through the service registry, never fixed `3`, except for tests that deliberately validate legacy boot lore.
2. Before sealing, obtain the required directory scope with `OpenRootDir`; fixed-path cells can use a narrow parent such as `/etc/cellos`, while arbitrary path tools need `/` plus `OpenDir` traversal.
3. If and only if all VFS operations that caller needs have handle equivalents, send `SealPaths`. Do not seal Hypha/Lua while their `write_file`, `list_dir`, `stat`, or startup install paths still use path-addressed requests.
4. For each file read: `OpenFileAt` -> repeated `ReadFileHandle { offset, max <= 4000 }` until short/EOF -> `CloseFile`.
5. On encode/send/recv/decode/VFS error, close any live file handle before returning the typed failure. No retry through `GetFile`, `DataPtr`, fast IPC, `ReadAsync`, or `ReadFileGrant`.

## Dependency Graph and File Ownership

1. Slice 1 -- shared facade, serial blocker.
   - Owns: `libs/ostd/src/service.rs`, `libs/ostd/src/clients/vfs.rs`, likely a new focused submodule under `libs/ostd/src/clients/vfs/`.
   - Must finish before Hypha/net-broker and should finish before Lua/WASM unless they intentionally stay direct.
2. Slice 2a -- net-broker consumers, can run after Slice 1 and independent of Slice 2b.
   - Owns: `cells/services/net-broker/src/identity.rs`, `cells/services/net-broker/src/transport.rs`, net-broker tests.
3. Slice 2b -- Hypha tool-fs read path, can run after Slice 1 and independent of Slice 2a.
   - Owns: `cells/apps/hypha/tools/fs/src/main.rs`, Hypha boot/tool tests.
4. Slice 3 -- shell parity and migration checkpoint, serial before final disable.
   - Owns: `cells/tools/shell/src/cmd_fs.rs`, `cells/tools/shell/src/shell_test.rs`, shell integration cases.
5. Slice 4 -- Lua and WASM runtime readers, can be split only if the shared helper is already frozen.
   - Owns: `cells/runtimes/lua/src/bindings_vfs.rs`, `cells/runtimes/lua/src/main.rs`, `cells/tools/wasm/src/main.rs`, runtime tests.
6. Slice 5 -- HTTPD readers, split service HTTPD and net-tools HTTPD only because files do not overlap.
   - Owns: `cells/services/httpd/src/net_ipc.rs`, `cells/services/httpd/src/handlers.rs`, `cells/services/httpd/src/main.rs`; separately `cells/tools/net-tools/src/bin/httpd.rs`.
7. Slice 6 -- retirement gate and Law 1 checkpoint B, serial after every production caller passes.
   - Owns: VFS `GetFile` serving/fast arm disablement and grep gates; no public variant removal or renumbering.

No two parallel slices may touch `libs/ostd/src/service.rs`, `libs/ostd/src/clients/vfs.rs`, or shared integration test harness files at the same time.

## Slice Plans, Failure Modes, and Rollback

### Slice 1: ostd masked handle reader

- Steps: harden request/reply helper masking; add a path splitter/walker using `OpenRootDir`, `OpenDir`, `OpenFileAt`, `ReadFileHandle`, `CloseFile`; make close RAII-like; keep old path methods until migrated.
- Failure modes: slash/name validation mismatch; leaking file/dir handles on partial traversal; breaking non-VFS `ServiceRef` users by changing reply masking.
- Mitigation: unit tests for path decomposition, wrong sender, EOF/zero/max, error-close; keep generic `ServiceRef::call` behavior change small and test service-id callers if changed globally.
- Rollback: revert ostd helper files only; old callers continue using current paths. Irreversible: none.

### Slice 2: facade consumers

- Steps: migrate net-broker fixed config/key reads first, then Hypha read tool; preserve Hypha `write_file` and `list_dir` path APIs until a separate handle-write/list pass or migrate them in the same slice before any seal.
- Failure modes: net-broker loses optional config behavior; Hypha binary/UTF-8 result behavior changes; premature `SealPaths` breaks writes/listing.
- Mitigation: tests for missing config, exact 32-byte key, short key, multi-chunk Hypha read, binary text fallback.
- Rollback: restore the two callsites in `identity.rs`/`transport.rs` and one Hypha dispatch callsite. Irreversible: none.

### Slice 3: shell checkpoint

- Steps: decide whether shell remains the `ReadFileGrant` rollback sentinel or moves to the common handle walker; if moved, open `/` before seal and traverse components so future arbitrary paths remain readable.
- Failure modes: sealing shell before it has root traversal breaks arbitrary `vcat`; replacing grant path removes current RV64 shell proof too early.
- Mitigation: retain grant helper until handle path proves `vcat`, command substitution, missing-file, directory, and `>480B` reads; no runtime fallback after a handle-read failure.
- Rollback: restore Phase 02 shell helper; Law 1 B cannot proceed while shell is rolled back to a path/grant dependency. Irreversible: none.

### Slice 4: Lua/WASM

- Steps: replace fixed `VFS_ENDPOINT=3` with registry lookup; add masked VFS helper; migrate Lua script/read APIs and WASM loader to bounded handle chunks; introduce explicit oversize behavior.
- Failure modes: Lua startup bundled-script install still path-addressed; Lua API currently returns empty/nil on errors; WASM now fails where it formerly returned an empty Vec.
- Mitigation: do not seal Lua until stat/list/write/install are converted or excluded; add tests for script load, `vfs.read_file` >480B, missing file, and WASM missing/oversize/valid module.
- Rollback: revert Lua/WASM files; Law 1 B remains blocked. Irreversible: none.

### Slice 5: HTTPD readers

- Steps: first mask NET/VFS replies or use `ostd::ipc::service_call_typed`; then replace VFS `ReadAsync`/`Poll` with a bounded handle loop that returns 404 on typed not-found, 500 on transport/internal errors, and never equates truncation with success.
- Failure modes: changing NET reply masking in the same file may expose unrelated net helper bugs; static files larger than memory budget; legacy net-tools HTTPD divergence.
- Mitigation: split service HTTPD and net-tools HTTPD; add >480B static file, missing file, dynamic reread, wrong-sender noise, and close-after-error tests.
- Rollback: per-HTTPD file revert; final disable remains blocked. Irreversible: none.

### Slice 6: Law 1 checkpoint B and retirement

- Timing: only after the inventory gate reports no production `VfsRequest::GetFile`, `VfsResponse::DataPtr`, direct `get_file_ptr`, or production `ReadAsync`/single-`Poll` file read outside VFS/tests/bench.
- Checkpoint B content: disable serving behavior for `GetFile/DataPtr` and fast VFS `GetFile`; keep public enum variants/discriminants reserved. Physical removal/renumbering is not in Phase 05.
- Failure modes: hidden production caller in docs-generated or embedded source; disabling fast arm changes benchmark/test-only paths; VFS service still needs `get_file_ptr` internally for backends.
- Mitigation: grep gate excludes only VFS implementation, tests, and bench with an explicit allowlist; keep backend `get_file_ptr` until Phase 06 if removing it would be broader than serving disablement.
- Rollback: restore the serving arms if smoke tests fail before release. Irreversible: if disabled behavior is released, re-enabling raw pointers requires a new explicit decision and must not be silent fallback.

## Test Matrix

- Unit/API: existing `cargo test -p types -p api --target x86_64-unknown-linux-gnu` stays required to prove discriminants remain reserved/stable.
- ostd unit: add facade/path-walker tests for path split, masked wrong-sender, chunk loop, EOF, close-on-error, and `DataPtr` rejection.
- VFS service: retain Phase 04 file-handle selftests and `vfs-quota` markers for post-seal `ReadFileHandle`.
- Shell: keep `cells/tools/shell/src/shell_test.rs` bounded grant/read tests and add handle parity if shell migrates.
- Runtime: add Lua read/script load tests and WASM loader success/missing/oversize tests.
- HTTP: extend `tests/integration/tests/boot.rs` beyond current minimal HTTPD tests at `tests/integration/tests/boot.rs:637` and `tests/integration/tests/boot.rs:693` to include >480B static file content and dynamic reread.
- Gate: CI grep must fail on production `VfsRequest::GetFile`, `VfsResponse::DataPtr`, fast VFS `GetFile`, and direct production single-poll file reads.
- Runtime smoke: RV64 QEMU for shell, Lua, WASM, Hypha, net-broker config, service HTTPD, and net-tools HTTPD before Slice 6; AArch64/x86_64 remain compile-only unless their host gates are cleared.

## Backwards Compatibility

- Keep `VfsRequest::GetFile` and `VfsResponse::DataPtr` discriminants present as reserved wire slots through Phase 05.
- Keep the Phase 02 shell grant path available as rollback until all handle-read callers pass; do not use it as an automatic fallback after migration.
- Preserve public app behavior where practical: missing net-broker config still means no peers, Hypha read errors remain explicit tool errors, HTTPD still returns 404 for absent/empty only where current contract intentionally does so.

## Success Criteria

- [ ] Every production caller in the matrix has a masked VFS reply path.
- [ ] Every migrated read uses bounded handle chunks and closes `CloseFile` on success and failure.
- [ ] Every caller that sends `SealPaths` has bootstrapped sufficient dir handles first, and no remaining path-addressed operation is needed afterward.
- [ ] No production code outside VFS/tests/bench decodes or requests `GetFile/DataPtr`.
- [ ] Both HTTPD implementations serve a file larger than 480 bytes without truncation.
- [ ] Lua and WASM no longer hardcode VFS tid `3`.
- [ ] Law 1 checkpoint B is recorded before disabling serving behavior; public variants remain reserved.

## Unresolved Questions

- Should shell move to handle reads in Phase 05, or remain the `ReadFileGrant` rollback sentinel until Slice 6? The safer execution default is: prove handle parity, keep grant code until final checkpoint, then delete/disable only after all callers pass.
- Should Hypha/Lua be sealed in Phase 05? Only if their write/list/stat/startup install paths are migrated too; otherwise handle-read migration alone must not send `SealPaths`.
- Is legacy `cells/tools/net-tools/src/bin/httpd.rs` still considered production? If yes, it is a required Slice 5b gate; if no, document the exclusion before Law 1 checkpoint B.
