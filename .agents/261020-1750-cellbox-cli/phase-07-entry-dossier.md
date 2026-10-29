# Phase 07 entry dossier — ripgrep-derived VFS search (`cellbox rg`)

Status: **CP1 inputs decided 2026-10-24 (§13): user approved every §10 recommendation.** Drafted as a read-only investigation of the dirty working tree (HEAD `3471028bc`), 2026-10-24. The only writes at drafting time were this file, the `plan.md` table fix in §12 and throwaway probe crates under `/tmp` (§3.4; deleted after their digests were recorded).

Inputs: [phase-07-ripgrep-search.md](phase-07-ripgrep-search.md), [phase-01-baseline-contracts.md](phase-01-baseline-contracts.md) step 7, [command-contracts.md](command-contracts.md) §1–§2 (the normative `rg` annex), [command-catalog.md](command-catalog.md) `grep` row (`:47`) and new-command row (`:276`), [dependency-review.md](dependency-review.md) §4/§6/§7, [spec.md](spec.md) §1–§3, [abi-vfs-telemetry.md](abi-vfs-telemetry.md) §1 (D03 revisions), [acceptance.md](acceptance.md) CBX-02/19/20, ADR-0028 D08, `docs/cellbox-cli.md` "Ripgrep-based search", `docs/code-standards.md` Law 6.

## 1. Entry gates

| Gate | State | Blocks entry? |
|---|---|---|
| Phase 03 (dependency) | completed, CP2 approved 2026-10-21 | No |
| Phase 04 (dependency) | `in_progress`: criteria 1/2/3/5/6 checked on private fixtures; every Phase 04 delta CP2-approved 2026-10-22; only criterion 4 (canonical packaging) open, held by rollout containment | No. Same state let Phase 05 (D-CL3) and Phase 06 (D60-1) enter. Phase 07 needs no canonical packaging. Entry still needs an explicit **D70-1** |
| Phase 01 ripgrep admission (step 7: pins, per-file/transitive licences, advisories, target probes) | Pins/licence choice reviewed 2026-10-20 for 15.1.0. **Missing:** acquired source, stock and adapted target compile evidence, final graph audit. CBX-02 pending | **Yes, for S7-1** (porting needs the source). §3 below closes everything reachable offline. S7-0 closes the rest once acquisition is approved (D71-2) |
| Phase 05/06 CP2 overlap | Both approved with exact file hashes (Phase 06 CP2 2026-10-24). Phase 07 edits some of the same cellbox/text-engine files (`typed.rs`, `executor.rs`, `native_rows.rs`, `ledger.rs`, `pipeline.rs`, `lib.rs`) | No. The CP2 baselines are frozen; Phase 07 diffs start from them |
| VFS ownership | S7-2 changes the VFS service, `api::ipc` and the `ostd` VFS client (protected service/API files) | Needs an owner handoff (**D70-4**), as C5 had for the `ostd` VfsClient additions |
| Rollout containment | in force (`reports/rollout-containment.md`) | Unchanged. All runtime evidence is private-fixture only |
| WIP rule | One implementation slice at a time; no other Cellbox slice is active | No |

## 2. Starting state (facts)

### 2.1 What exists for `rg` today: nothing that qualifies
- No `rg` command. It is absent from the builtin list (`cells/tools/cellbox/src/typed.rs:106-110`, `executor.rs:788-790`), the native producer registry (`native_rows.rs:40-55`, `ps|ls|top` only) and the ledger.
- `libs/text-engine/src/search.rs` (362 lines, **untracked** `??`, 3 tests, exported by `lib.rs:46`, **no consumer**) is the custom code that Phase 07's deviation log withdrew. Its header calls itself "Upstream-derived" (`:2`); that is false: it imports no ripgrep component. Defects against the contract:
  - "regex" mode is literal `str::find` (`:228-278`); no regex engine is used.
  - It strips a trailing CR from `line_bytes` (`:152-157`); the contract keeps CR as content (§2.2.5).
  - Lines that are not valid UTF-8 can never match (`:190-193`); the contract searches bytes.
  - `-i` computes ranges on a lower-cased copy with `pat.len()` (`:203-215,254-266`), so byte ranges are wrong for case mappings that change length.
  - Submatches from several patterns are appended unordered and may overlap (`:195`).
  - Context fields exist (`:20-21`) but nothing implements them. Binary detection needs the whole file in memory (`:130-133`). Text rendering does no escaping (`:107-112`).
- Recommendation: delete it in S7-3 as the clean cutover (D70-6). It is not a reuse candidate.

### 2.2 Existing `grep` (kept distinct; catalog row `:47`)
- `cells/tools/cellbox/src/text_tools/grep.rs:15-34` → `grep/io.rs`. Every input is loaded whole: stdin/file ≤ 65,536 bytes **aggregate** (`io.rs:35-65`, `io/read.rs:13-15`), UTF-8 required (`io.rs:67-72`), recursion depth 16 (`io.rs:11`).
- `-r` walks with path-addressed `Stat`/`ListDir` (`io/vfs.rs:70-124`). It treats a listing as truncated only when the last byte is not LF (`:102-104`). The backends stop at a whole entry when the 4,000-byte reply is full (§2.3), so **`grep -r` silently skips entries in large directories today**. That is a separate defect (D70-5), not Phase 07 scope.

### 2.3 VFS read and directory APIs available today
- **Wire.** `api::ipc::VfsRequest` (`libs/api/src/services/ipc.rs:38-203`). Handle-addressed requests: `OpenRootDir` (:115), `OpenDir` (:118-121), `ReadAt` (:123-126), `StatAt` (:134-137), `ListAt` (:143-145), `CloseDir` (:156-158, revokes every derived handle), `SealPaths` (:164), `OpenFileAt` (:167-170), `ReadFileHandle{offset,max}` (:172-176), `CloseFile` (:178-180), `ReadHandleGrant` (:182-187). Variants are append-only, because postcard encodes the variant index (`:97-105`). There are 31 request variants (0–30) and 11 response variants (0–10, `:295-323`). `IPC_BUF_SIZE = 4096` (`:21`).
- **Ranged file reads already exist.** `ReadFileHandle` serves at most `MAX_INLINE_PAYLOAD = IPC_BUF_SIZE − 96 = 4000` bytes per call (`cells/services/vfs/src/dispatch_dirs.rs:34`). Each call re-runs `can_read` and re-stats the file, then calls the backend `read_at` (`dispatch_file_handles.rs:47-119`). All five real backends implement `read_at` (`:100-104`). `OpenFileAt` requires a stated caller identity (`may_own_state`) and `can_read` (`:11-46`). **So streaming a file of any size in 4,000-byte chunks works on the wire today.**
- **Directory listing does not page.** `ListAt` returns one inline reply of `d:NAME\n`/`f:NAME\n` lines (`dispatch_grants.rs:136-153`; contract `backend.rs:34-36`). It has no cursor and no "complete" flag. The FAT backend stops silently when the buffer is full and also stops silently on an iteration error (`backend_fat.rs:184-197`). The manager adds mount children only within the first 480 bytes, again silently (`manager.rs:122-136`). The only kinds are dir and file. A listing of `0` bytes means both "empty" and "missing" (the caller pre-stats).
- **No version or identity.** `Stat`/`StatAt` give only `(size, is_dir)`. There is no inode, file identity, generation, content revision or mtime. For `/srv` paths a file handle holds a shared namespace lease, which blocks unlink and rename (`dispatch_file_handles.rs:31-41`, `namespace.rs:89-111`). Writes take only a shared transient lease (`paths.rs:39-49`), so **content can change under an open read handle undetected**. D03 (Phase 08) designs file revisions (`abi-vfs-telemetry.md:27`), but nothing implements them.
- **Names and paths.** Components are 1–255 bytes, valid UTF-8, with no `/`, `\`, C0/DEL, `.` or `..` (`libs/api/src/services/dir_name.rs:67-89`). Root paths are at most 1,024 bytes (`:30,103-119`). Consequences: VFS paths are always canonical UTF-8, a raw-byte path cannot occur, and a name can never contain LF.
- **Links.** No backend has symbolic links. A search for `symlink` over `cells/services/vfs/src` and `libs/cellos-fs/src` finds nothing. The FsBackend implementors are FAT, CellosFS, BootFsProxy, BinOverlay, RamFS and the GuestDisk stub.
- **Hidden.** No attribute exists; hidden can only mean a dot-prefixed name (which is the contract's definition).
- **Handle budgets.** 32 directory handles per Cell (`dirs.rs:36`) and 32 file handles per caller (`file_handles/table.rs:11`).
- **Client side.** `ostd::clients::VfsClient` (`libs/ostd/src/clients/vfs.rs:27-175`) exposes only a whole-file `read_file` capped at 64 KiB (`:15,42-53`). Internally that runs the handle walk plus a 4,000-byte chunk loop (`read_file/session.rs:43-52,121-165`, `wire.rs:3`). It also exposes path-addressed `list_dir`/`stat`. **No public streaming reader, paged lister or handle API.** Error codes map to IO/OutOfMemory/PermissionDenied/NotFound (`wire.rs:4-17`).
- **Cellbox authority today.** All cellbox file commands are path-addressed through `VfsClient` (`cmd_fs.rs:26-41,809-825`). Cellbox never seals paths, and authority is the coarse per-Cell VFS access table. Phase 04 criterion 3 records "coarse per-Cell rights".
- **Other defect seen (not Phase 07):** `ReadAt` copies `min(len, 4000)` bytes of a whole-file read without reporting truncation (`dispatch_dirs.rs:185-189`). Recorded under D70-5.

### 2.4 Pipeline engine (Phase 03/06 code)
- `Value` (`libs/text-engine/src/pipeline.rs:76-89`) has `Path` and `Bytes`. `RowSource { next_item, close }` (`:1598-1603`) has **no completion-status channel**. A source can only return rows, `Ok(None)` or an error that aborts the stream.
- Ctrl-C is polled by the sink **before each pull** (`cells/tools/cellbox/src/typed.rs:780-785`; terminal sink `executor.rs:1346-1388` via `poll_session_interrupt` `:1315`). A producer that does a long traversal inside one pull, for example a tree with no matches, cannot be interrupted today.
- Standalone typed streams finish with status 0 (`typed.rs:796-799`). The contract's 0/1/2 for `rg` therefore needs a producer status.
- One serialized record line is at most 65,536 bytes (`pipeline.rs:20`). Plain `to json(l)` refuses Path/Bytes without `--typed` (`pipeline.rs:399-403`). So the design-doc example `rg … | select path line_number text | to jsonl` (`docs/cellbox-cli.md:78`) would fail as written. See D74-6.

## 3. Phase 01 ripgrep admission (step 7) — closed as far as reachable offline

### 3.1 Upstream component set and pin
- **New release.** P01 reviewed ripgrep **15.1.0** (`af60c2de…`). Upstream has since released **15.2.0** = commit `e89fff89ac9af12e8d4ce9d5fd07beb408ca730f` (2026-07-15; GitHub API reports a valid PGP signature).
  - Its changelog fixes gitignore matching when searching **multiple directories** (#3320, #3376, #3419). Multiple roots and nested ignore files are inside the supported `rg` surface.
  - It also adds `GIT_CONFIG_*` handling and `.jj` checks, which are outside the port.
- **Component versions at the 15.2.0 tag** (crates.io checksums for reference):
  - grep-matcher **0.1.9** `f9417543…`
  - grep-regex **0.1.14** `0ce0c256…`
  - grep-searcher **0.1.17** `72348823…`
  - ignore **0.4.29**
  - globset **0.4.19**
  - All are `Unlicense OR MIT`, edition 2024, with no component `rust-version`.
  - crates.io already has newer **ignore 0.4.33 / globset 0.4.20** (2026-08-04), published after the tag and not reviewed. Port from one matched tag, not mixed registry heads (D71-1).
- **Selected subset and std census** (read over HTTPS from the tag; non-test lines; nothing stored in the workspace or Cargo cache):

| Component | Kept | std use in kept code | Port work |
|---|---|---|---|
| grep-matcher (1,523) | all | only `impl From<NoError> for std::io::Error` (`lib.rs:511-512`) + `std::{fmt,error,ops}` | `core`/`alloc` rename; gate the io conversion. Trivial |
| grep-regex (2,061) | all | only `std::{error,fmt,cmp,mem}` | rename; bstr `alloc`; regex-automata/syntax without `std`. Trivial |
| grep-searcher (4,502) | `lib`, `line_buffer`, `lines`, `searcher/{core,glue,mod}`, `sink`, `macros` | `io::Read`/`io::Error` throughout (line_buffer, glue, sink); `fs::File`/`Path` in `search_path`/`search_file`; `encoding_rs(_io)` transcoding; `mmap.rs` (the **only `unsafe`** in the whole subset) | Replace `io::Read`/`io::Error` with a core-only `vigrep::io` reader/error. Delete mmap, encoding/BOM transcoding and the path/file entry points. Keep `search_reader` + `search_slice`, binary detection, context, `heap_limit`. Expose line-buffer capacity, which is `pub(crate)` with a 64 KiB default (`line_buffer.rs:6,100,141`). Moderate |
| globset (2,394) | `glob`, `lib`, `pathutil`, `fnv` | `Path` in the public match API; `std::collections::HashMap`; `Arc`; `UnwindSafe` markers | Byte-path `Candidate`. `HashMap` → `BTreeMap` or `hashbrown` (already in the lock via `ostd`). Moderate |
| ignore (5,582) | `gitignore` (703), `overrides` (185), `pathutil` (142), precedence logic of `dir` (1,110), error types from `lib` | `Path`/`PathBuf`/`OsStr`, `fs::File`/`BufReader` (`GitignoreBuilder::add`), `env` (home/global excludes), `Arc<RwLock>` caches, threads | Byte paths. Feed rules through `add_line` from VFS bytes. Drop global/git-exclude/`.jj`/repository detection, `walk.rs` (2,115: walkdir, same-file, crossbeam, threads), `types`/`default_types`. Re-express the `.rgignore > .ignore > .gitignore` and parent-precedence stack for a serial walker. **Largest risk** |

  - About 12k upstream lines are adapted. All 34 upstream `src` files have **no per-file licence header**. The five component `LICENSE-MIT` blobs are identical (`3b0a5dc0…`, same as the 15.1.0 review).
- **Dropped crates:** encoding_rs, encoding_rs_io, memmap2, walkdir, same-file, crossbeam-deque, winapi-util.
- **Kept external dependencies:** memchr, aho-corasick, regex-automata, regex-syntax, log and **bstr**.

### 3.2 Cellos std story (options)
There is **no Rust `std` for Cells**.
- Cells build `x86_64-unknown-none` with `-Z build-std=core,alloc` (`scripts/build-x86_64-cells.ps1:12-21`).
- `target_os = "cellos"` appears only in `ostd/src/startup.rs:15,24`. The custom target JSON's `"std": true` (`targets/x86_64-unknown-cellos.json`) is metadata only; no `library/std` port exists.
- What does exist is **C-level**: `api::services::posix` (C-ABI malloc/stdio/sockets for C ports), `libs/mlibc-shim` (links a prebuilt mlibc `libc.a`) and `libs/port-platform` (pthread/spawn for C apps). None of these gives Rust crates `std::fs`/`std::io`/`std::thread`.

| Option | What | Verdict |
|---|---|---|
| O1 Rust std port (`library/std` sys layer for `os=cellos`: fs/io/path/env/thread over ostd) | New general runtime program; threads for ignore's parallel walker | **Rejected.** The plan assumption says "without a new general runtime program"; Phase 01 explicitly excludes a Linux/std personality |
| O2 Link stock crates on the C POSIX shim/mlibc | Rust std still missing; the shim does not help Rust crates | Rejected |
| O3 **no_std+alloc port of the pure subset + Cellos serial walker** | §3.1 subset; I/O via a core-only reader trait implemented over VFS file handles; traversal over a directory-source trait implemented over VFS | **Recommended** (D72-1 picks the packaging) |
| O4 Vendored fork keeping upstream crate structure, with an io/fs abstraction trait | O3's code, packaged as five forked crates | Packaging variant of O3 (D72-1 P2) |
| O5 Engine outside the Cell: (a) in the VFS service; (b) off-target on the development host | (a) search runs inside VFS's own address space, gets the same no_std problem and adds a large untrusted-input parser to a privileged service; (b) not a Cellos search at all | Rejected. (b) fails CBX-19 "actual target search" |

### 3.3 Licences (selected graph)
| Package | Version | Declared | Choice | In `Cargo.lock` / cache |
|---|---|---|---|---|
| ripgrep components (5) | 15.2.0 tag | Unlicense OR MIT | **MIT**, copyright 2015 Andrew Gallant; keep LICENSE-MIT + provenance | No / no (needs acquisition) |
| memchr | 2.8.3 | Unlicense OR MIT | MIT | yes / yes |
| aho-corasick | 1.1.4 | Unlicense OR MIT | MIT | yes / yes |
| regex-automata | 0.4.16 | MIT OR Apache-2.0 | MIT | yes / yes |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | MIT | yes / yes |
| log | 0.4.33 | MIT OR Apache-2.0 | MIT | yes / yes |
| bstr | 1.13.1 (`6bb31b46…`) | MIT OR Apache-2.0 | MIT; features `alloc` only (`std`/`unicode` off; no_std per its feature table) | **No / no (needs acquisition)** |

`deny.toml`'s allow list (`:28-39`) already covers every choice; no exception is needed. Upstream's existing regex dependencies resolve onto the workspace's pinned `regex-automata 0.4.16`/`regex-syntax 0.8.11` (requirements `^0.4.0`/`^0.8.0`), so no duplicate versions are added.

### 3.4 Executed probes (throwaway, `/tmp`, offline, cached crates only)
Toolchain `rustc 1.97.0-nightly (f53b654a8 2026-04-30)`, `cargo 1.97.0-nightly (eb9b60f1f)`.

- **Probe A** `/tmp/p07-probe-a`: `#![no_std]` **staticlib** with exported functions that:
  - build a `meta::Regex` (Unicode, case-insensitive, `\b`, `\p{Greek}`, LF line terminator, multi-pattern) and run `find_iter`;
  - do memchr line splitting and `memmem`;
  - run a multi-literal Aho-Corasick;
  - parse with regex-syntax and extract literals (the grep-regex inner path).

  It uses a probe-only always-failing allocator and a looping panic handler, to force codegen and archive linking (not a product runtime).
  - Dependencies: memchr 2.8.3 `[alloc]`, aho-corasick 1.1.4 `[perf-literal]`, regex-syntax 0.8.11 `[unicode]`, regex-automata 0.4.16 `[alloc,syntax,meta,nfa,hybrid,dfa-onepass,perf,unicode]`, log 0.4.33, all `default-features=false`.
  - **x86_64-unknown-none:** `RUSTFLAGS="-C relocation-model=pic" cargo +nightly-2026-05-01 build --offline --release --target x86_64-unknown-none -Z build-std=core,alloc` → **Finished, exit 0** (15.6 s). Archive 1,392,072 B; three `probe_*` symbols defined.
  - **Custom target:** `--target targets/x86_64-unknown-cellos.json -Z json-target-spec` (soft-float, no MMX/SSE) → **Finished, exit 0**.
  - Lock `9823a648…`, `src/lib.rs` `98ae872f…`, `Cargo.toml` `d5ab2609…` (after adding `license = "MIT"` for the deny run).
- **Probe A2** (same code, `unicode` features off; the `\p{Greek}` pattern then fails at run time, not at build) → exit 0. LTO archive `llvm-size` totals: **572,934 B with Unicode vs 212,089 B without**. This is only a probe-level signal of the Unicode-table cost (D72-4). It is not a cellbox footprint.
- **Probe C** `/tmp/p07-probe-c`: the **exact feature request of ignore 0.4.29 / globset 0.4.19** (`regex-automata [std,perf,syntax,meta,nfa,hybrid,dfa-onepass]`, `regex-syntax [std]`) → **exit 101**, `error[E0463]: can't find crate for 'std'` in memchr and regex-syntax. This is the compile-time std boundary, and it holds independently of any I/O adapter.
- **Audit:** `cargo deny --offline check advisories licenses` on probe A with the workspace `deny.toml` → **"advisories ok, licenses ok"**. Advisory DB `~/.cargo/advisory-dbs/advisory-db-3157b0e258782691` at commit `640bf5a9…` (2026-10-22).
  - The DB holds advisories only for **excluded** crates: grep-cli RUSTSEC-2021-0071, memmap2 RUSTSEC-2026-0186 (unsound `<0.9.11`), crossbeam-deque RUSTSEC-2021-0093 (patched `≥0.8.1`).
  - None of grep-matcher/grep-regex/grep-searcher/ignore/globset/bstr has an entry. This is not a clean bill for code not yet acquired.
- **Not executed (needs acquisition, D71-2):**
  - a stock compile of the five components (expected to fail as Probe C does);
  - a compile of the adapted subset;
  - bstr;
  - a host reference `rg` 15.2.0.

  Host `/usr/bin/rg` reports **15.1.0** (provenance not recorded). No network download was made; ADR/design text says none is authorized by those documents (`docs/cellbox-cli.md:138`).

### 3.5 What S7-0 still has to produce (closes Phase 01 step 7 for search and the search half of CBX-02)
Each item needs D71-2 acquisition approval:
1. Acquired tag tree and crate archives, with checksums.
2. A per-file licence census of the acquired files.
3. A recorded stock-compile failure.
4. A compile of the adapted subset for both x86 profiles.
5. A final `cargo deny` over the real workspace graph after S7-1.
6. Advisory DB commit and date bound into one report.

## 4. Gaps against Phase 07 steps and criteria

| Phase 07 step / criterion | Gap |
|---|---|
| 1 pinned upstream components, no mmap/PCRE2/parallel walk | Not acquired; std-bound (§3). No `rg` registry entry |
| 2 scoped traversal, bounded chunks, ignore/hidden/binary/symlink, cancellation | Listing has no paging, completeness or mount kind (§2.3). No change detection for the contract's binary preflight. Ctrl-C is not visible inside a producer (§2.4). Only 32 handles per Cell versus depth 64 |
| 3 typed records from callbacks | Schema is fixed by contract §2.3. The record budget conflicts with the line/submatch limits (DV7-8) |
| 4 flags/status, truthful partial errors | No producer status (0/1/2) and no way to continue past per-file errors |
| 5 declared signatures, early close stops work | `take` close works through `RowSource::close`. The `rg -` Bytes→Rows head is not registered |
| 6 >64 KiB, ignore/hidden/binary/UTF-8, inaccessible, symlink cycles, pinned host fixture | No fixture tree. There are **no symlinks to test on target**. The host reference is 15.1.0 unless D71-3 |
| Crit. 1 multi-chunk byte ranges, bounded memory | Wire supports 4,000-byte ranged reads; nothing uses them in streaming form |
| Crit. 2 hidden/ignore/binary/link + denial without bypass | Denial exists (`can_read` per chunk). A `/srv/cellos` access rule exists (`access/rules.rs:40,86`) as a candidate denied subtree |
| Crit. 3 0/1/2, partial errors not silently successful | Engine hook needed (D74-2) |
| Crit. 4 no ANSI or lossy conversion; cancellation stops producer | Text escaping and the in-producer cancel hook are both needed |

## 5. Proposed slices (each gets its own CP1 when reached; one WIP slice)

| Slice | Delivers | Main files | Proof |
|---|---|---|---|
| **S7-0 Admission** (Phase 01 step 7, search) | Isolated acquisition of the 15.2.0 component tree + bstr 1.13.1 (D71-2). Checksums, per-file licences, stock-compile failure, adapted-subset compile on both x86 profiles, offline `cargo deny`. Report appended to `dependency-review.md` §4 | plan docs only; `/tmp` | Recorded commands/digests |
| **S7-1 `vigrep` port** | New `libs/vigrep`, `#![no_std]`+`alloc`, `#![forbid(unsafe_code)]`. Modules `vigrep::{matcher, regex, searcher, glob, ignore, io}` adapted per §3.1, plus `vigrep::walk`: a serial depth-first walker over a public `ViSearchTree` trait (`open_dir`/`next_page`/`open_file`/`read_chunk`/`file_state`/`close`). The walker applies root dedup, byte-sorted children, hidden, ignore precedence and command globs, binary preflight, bounds and a cancellation callback. Each adapted file carries upstream path + tag commit + "adapted" note + SPDX `MIT`; `LICENSE-MIT` and a third-party notice. Upstream's non-filesystem unit tests are ported | `libs/vigrep/**`, root `Cargo.toml` (member), `Cargo.lock` (+bstr), notices | Host tests; bare-metal builds |
| **S7-2 VFS read-path delta** | Paged directory listing with entry kinds/complete/changed flags. Handle-scoped modification state. Public streaming `ostd` VfsClient read API. No syscall, allowlist bit or new right (§6) | `libs/api/src/services/ipc.rs`, `cells/services/vfs/src/{dispatch.rs,dispatch_dirs.rs,dispatch_file_handles.rs,dispatch_grants.rs,dispatch_paths.rs,paths.rs,grant_write.rs,manager.rs,backend.rs,backend_{fat,ramfs,cellosfs,bootfs,bin_overlay}.rs,access/stub.rs,dirs.rs,file_handles/table.rs}`, `libs/ostd/src/clients/vfs{.rs,/…}` | VFS/api/ostd host tests; VFS boot selftests |
| **S7-3 Engine hooks + `cellbox rg`** | Engine: producer completion status and a producer-visible interrupt latch (D74-1/2), a Phase 03 engine delta. Cellbox: `rg` argv per contract §2.1. `ViSearchTree` over the S7-2 client. `RgSource` RowSource emitting `cellbox.search.line`. Streaming text renderer with contract escaping. Bytes→Rows head for `-`. Classification, registry, ledger, help and catalog rows. Delete `text_engine::search` + `pub mod` (clean cutover). Docs: `docs/cellbox-cli.md` example, CHANGELOG | `libs/text-engine/src/{pipeline.rs,lib.rs,search.rs(delete)}`, `cells/tools/cellbox/src/{rg.rs,rg/*,typed.rs,executor.rs,native_rows.rs,ledger.rs}`, `cells/tools/cellbox/Cargo.toml`, `Cargo.lock`, `docs/cellbox-cli.md`, `CHANGELOG.md`, `command-catalog.md` | Host tests; runners |
| **S7-4 Fixtures and runtime proof** | Fixture generator staging a search tree into the **private** FAT image. Host reference expectations from the pinned rg. New terminal-driver/P03/P02 scenarios (§9) | `scripts/phase07_rg_fixture.py`, `scripts/qemu-phase0{2,3,4}-*.sh`, `cells/tests/phase04-terminal-driver` | Private QEMU/TCG |

Recommended order (D70-3): **S7-0 → S7-1 → S7-2 → S7-3 → S7-4.** S7-2 does not depend on S7-1 and may run first if acquisition approval is delayed. S7-3 needs both.

### 5.1 Options per slice
- **Packaging (D72-1):**
  - **P1 — one crate `libs/vigrep` with module paths (recommended).** Law 6 requires `vi` + one word + module paths for parts (`vitui`, `vimio` precedent). One provenance/notice point. The upstream→module mapping is recorded in the crate docs.
  - P2 — vendored fork `third_party/ripgrep-15.2.0/crates/*` under upstream package names with a patch series (`third_party/redoxfs` precedent). Easier to diff upstream, but five crates, the upstream names collide with crates.io names, and these are new library crates that Law 6 governs.
  - P3 — crates.io dependencies + `[patch]`. Rejected: the patches are structural (std removal), not small.
- **Walker authority and handles (D74-5):**
  - **H1 — per-directory reopen (recommended).** Path-addressed `OpenRootDir(path)` → page through `ListAtPage` → process file children with `OpenFileAt` on that handle → `CloseDir` before descending → reopen when continuing. At most 2 directory + 1 file handles at any time, so depth 64 fits the 32-handle budget. Same path authority as `ls`/`cat`/`grep` (DV7-2).
  - H2 — derived handle chain (`OpenDir`). Depth is capped by the 32-handle budget minus other holders (≈28), which contradicts contract depth 64.
- **Cancellation (D74-1):**
  - **C1 — shared interrupt latch (recommended).** The executor owns one latch fed by `poll_session_interrupt`. Sinks and producers both read it. `RgSource` polls once per entry and per 4,000-byte chunk; on interrupt it releases handles and returns `Ok(None)`. Drivers re-check the latch after end-of-stream, so the result is 130, not "complete".
  - C2 — a `Pending` item threaded through every stage. Larger engine change.
- **Unicode tables (D72-4):**
  - **U1 — full `unicode` features (recommended).** This is the contract's "Unicode classes from the pinned adaptation"; size is recorded per probe A2 (≈ +360 KB probe-level).
  - U2 — `unicode-case`+`unicode-perl` only, `\p{…}` rejected. This would be a contract deviation.

## 6. S7-2 CP1 request (concrete, pending D73-1..4)
- **Allocation.** Append only (`ipc.rs:97-105`). Request **31** `ListAtPage { dir: ViDirHandle, cursor: u32 }` and **32** `FileHandleState { file: ViVfsFileHandle }`. Response **11** `DirPage { next: u32, flags: u8, entries: &[u8] }` and **12** `FileState { size: u64, modified: bool }`.
  - Both requests are handle-addressed (`is_path_addressed = false`), read-only (`requires_mutation_authority = false`) and added to the directory routing group (`dispatch.rs:197-202`).
  - No syscall number, allowlist bit, CapSet or launch-profile change. cellbox's declared syscall list is unchanged.
- **`ListAtPage`.**
  - `entries` is packed `kind:u8, len:u8, name[len]`. Kind is 0 File, 1 Dir, 2 Mount, 3 Other. `name` passes `validate_dir_component`.
  - A page fits `MAX_INLINE_PAYLOAD` and holds only whole entries. `cursor` 0 starts; `next` is opaque.
  - `flags` bit0 `COMPLETE` (final page); bit1 `CHANGED` (a child of this directory was created, removed or renamed since the handle was opened, so the listing may be inconsistent).
  - Mount children follow backend entries as kind 2, with no 480-byte cap.
  - Errors: `ERR_HANDLE`; `ERR_DENIED` (`can_read`); `ERR_IO` for not-a-directory, a backend iteration error (never a silent stop) or a stale cursor.
  - New `FsBackend::list_page(&self, path, start, out) -> Option<ListPage{written, next}>`, implemented for all six implementors (GuestDisk stub returns `None` → `ERR_IO`). The existing `list`/`ListDir`/`ListAt` behaviour is unchanged (D70-5).
- **`FileHandleState`.**
  - `modified` becomes true when a content or binding mutation reaches the handle's path after `OpenFileAt`: Write, WriteAt, Append, WriteGrant, WriteHandleGrant (through any handle), Unlink/UnlinkAt, Rename (old or new), RmdirRecursive (prefix).
  - The same hooks mark the parent directory's open dir handles `CHANGED`.
  - Cost: one scan of the bounded handle tables per mutation; no per-path history is retained.
- **ostd.** `VfsClient` gains public `open_root_dir`, `list_page`, `open_file_at`, `read_chunk(file, offset, &mut [u8]) -> usize` (≤ 4,000), `file_state`, `close_file`, `close_dir`. The existing `read_file` 64 KiB path is untouched.
- **Tests.**
  - api: encode-byte pins of the new variant indices.
  - VFS host: paging over RamFS/FAT fixtures larger than one page; exact entry set; mount kind; iteration-error → `ERR_IO`; `COMPLETE` only on the last page; `CHANGED` after a child create/unlink/rename; `modified` after write/append/write_at/unlink/rename and not after read or stat; denial; handle revocation by `CloseDir`.
  - ostd: client wire round trips (existing `read_file/tests` seam).

## 7. Record schema and pipeline shape (contract §2.3, spec §1–§3)
- **Category.**
  - `rg PATTERN [PATH…]` is a finite **Rows** producer. It is not Frames: there is no sampling, and no Begin/End metadata exists for Rows.
  - `rg … -` is a **Bytes→Rows** head fed by the previous byte stage, and requires `-a` (no rewind).
  - Native record inputs to `rg` are rejected before effects.
- **Row** (`Value::Record`, field order fixed): `schema` String `cellbox.search.line`, `schema_version` UInt 1, `kind` String `match|context`, `path` Path (canonical VFS bytes) or Null for stdin, `line_number` UInt (1-based), `absolute_offset` UInt (first content byte, counting original bytes incl. CR/BOM), `line_bytes` Bytes (no LF), `text` String or Null (exact valid UTF-8), `terminator` Bytes (`0a` or empty), `submatches` List of Record `{start: UInt, end: UInt}`. Ranges are half-open, relative to `line_bytes`, ordered and non-overlapping; context rows have none.
- **Typed JSONL** (default at non-terminal sinks), e.g. a match of `foo` in line 3 `xfoo` of `/data/a.txt`:
  `{"cellbox":1,"value":["record",[["schema",["string","cellbox.search.line"]],["schema_version",["uint","1"]],["kind",["string","match"]],["path",["path","2f646174612f612e747874"]],["line_number",["uint","3"]],["absolute_offset",["uint","14"]],["line_bytes",["bytes","78666f6f"]],["text",["string","xfoo"]],["terminator",["bytes","0a"]],["submatches",["list",[["record",[["start",["uint","1"]],["end",["uint","4"]]]]]]]]]}`.
  Plain `to jsonl` refuses the Path/Bytes fields by spec. The user writes `to jsonl --typed`, or projects to scalar fields first (D74-6).
- **Text** (terminal, or `--text`): `ESCAPED_PATH:LINE:ESCAPED_LINE` for matches and `…-LINE-…` for context, streamed one row at a time (no table buffering; D74-3).
- **Status/diagnostics.** Status is 0 (≥1 selected line and complete), 1 (complete with none) or 2 (any error, including partial traversal, even after matches).
  - Each per-entry error is one stderr line `rg: PATH: reason`, and the walk continues.
  - One end summary line on stderr when errors or skipped mounts are non-zero (D74-4).
  - As a sole/final stage the status is rg's. Upstream of another stage it is retained in the member status and reported on stderr (spec §3).
  - `take` close → `close()` releases every handle, with no error.
- **Bounds.**
  - Contract §2.3 limits apply.
  - Read scratch 16 KiB (searcher capacity exposed in S7-1); searcher `heap_limit` = 16 KiB + 65,536-byte line.
  - Pending sibling lists for the sorted depth-first walk: ≤ 4,096 entries per directory and ≤ 1 MiB names retained across the stack (DV7-6).
  - A row whose `payload_bytes` would exceed 65,536 is error 2 for that file (DV7-8).

## 8. Deviations from the contract needing ratification

| # | Contract (`command-contracts.md` §2 / P01) | Proposal | Why |
|---|---|---|---|
| DV7-1 | ripgrep 15.1.0 `af60c2de`, matcher 0.1.8, searcher 0.1.16, ignore 0.4.24, globset 0.4.18 | 15.2.0 `e89fff89`, matcher 0.1.9, regex 0.1.14, searcher 0.1.17, ignore 0.4.29, globset 0.4.19 (or keep 15.1.0, D71-1) | Upstream gitignore multi-directory fixes; one matched tag |
| DV7-2 | "Paths are resolved from explicit directory grants, never ambient cwd authority" | Same path-addressed VFS route and per-Cell access table as `ls`/`cat`/`grep`; R is checked per request (`can_read`) | No cellbox command has scoped directory grants today; adding them is an engine-wide delta, not rg-only |
| DV7-3 | Root dedup by "VFS file object identity+generation" | Dedup by canonical VFS path: a root equal to, or below, an earlier root is skipped | VFS exposes no identity. Paths are canonical: no `.`/`..`, no links (§2.3) |
| DV7-4 | "Stable version/read view or identity+content revision" | Handle-scoped `modified` flag (§6), checked after preflight and after the search pass; any mutation (incl. same-bytes/ABA) fails the file with error 2 | Exact, bounded, no revision store. D03 revisions (Phase 08) may later supersede it |
| DV7-5 | Symlinks never followed; `links_skipped`; explicit link → UnsupportedSymlink | No Cellos backend has links. Kind `Other` entries are an error 2 per entry. The symlink-cycle case is host-only (fake tree) | Nothing to follow; no fabricated target case |
| DV7-6 | Depth 64, 65,536 visited entries | Plus ≤ 4,096 entries per directory, ≤ 1 MiB pending sibling names, and paths ≤ 1,024 bytes (`MAX_DIR_PATH_LEN`); beyond → error 2 | Byte-sorted DFS must hold sibling lists; the VFS root-path limit is real |
| DV7-7 | "Mount transitions … are not followed" | Mount-kind children are skipped (not an error) and counted in the stderr summary; explicit roots on another mount are fine | Defines the unspecified case |
| DV7-8 | Line ≤ 65,536 B, ≤ 4,096 submatches | Limits kept; additionally, a row whose payload exceeds the 65,536-byte record bound is error 2 (never truncated) | `line_bytes`+`text` double the payload; spec §1 record bound |
| DV7-9 | `--text` semantics only | Terminal default = the same streaming text renderer, not a buffered table | Table rendering buffers whole Rows streams |

## 9. Verification plan
Behavioral tests only. There are no source-text or wording tests. The 3 tests in `search.rs` are removed with it.

- **Host:**
  - `cargo test -p vigrep --target x86_64-unknown-linux-gnu`:
    - ported upstream unit tests for glob, gitignore, overrides, matcher, regex and searcher (context, binary, heap limit, line terminator);
    - walker tests over an in-memory `ViSearchTree`: byte order, hidden, `.rgignore > .ignore > .gitignore`, deeper-overrides-ancestor, last-match-wins, negation cannot revive a pruned dir, command globs, explicit hidden file, mount skip, `Other` kind, overlapping roots, depth/path/entry/name bounds, late NUL preflight, `modified` → error, cancellation stops reads, `close` releases every handle (counted by the fake), symlink-cycle fake;
    - **differential:** a committed fixture corpus with expected records generated once from the pinned reference `rg --json` with equivalent flags (D71-3); the generator command and digest are retained. Intentional differences (`--text`, quiet full scan, subtree-only ignore, no-follow, preflight) are asserted separately.
  - `cargo test -p text-engine --target x86_64-unknown-linux-gnu`: producer status and interrupt latch; record payload bound.
  - `cargo test -p api` (variant pins); VFS host tests (§6); ostd client tests.
  - `cargo test -p app-cellbox --target x86_64-unknown-linux-gnu` where the existing harness covers `rg` classification and argv errors before effects.
  - Bare-metal builds: `vigrep`, `app-cellbox`, `service-vfs` for `x86_64-unknown-none` (`-Z build-std=core,alloc`, PIC). RISC-V/aarch64 builds for the touched shared crates (`api`, `ostd`, VFS).
  - `cargo deny check licenses advisories`. The pre-existing `ocel-pdf` AGPL finding is out of scope, as in Phase 05.
- **Private QEMU/TCG** (signed fixture images, existing guards, no canonical image, no inbound network). A fixture tree is staged under `/data/rg/` in the private FAT image by `scripts/phase07_rg_fixture.py`:
  - **one file larger than cellbox's Cell quota** (target ~24 MiB versus the 16 MiB default [INFERENCE: cellbox runs at the default quota; verify in S7-4]), so whole-file buffering cannot succeed;
  - a directory with more entries than one 4,000-byte page;
  - nested competing ignore files, hidden files/dirs, a late-NUL binary, CRLF/BOM/final-no-LF and invalid-UTF-8 lines.

  `qemu-phase04-terminal-test.sh` + driver:
  - exact matches and offsets in the large file;
  - status 0/1/2;
  - `-q` with a later error → 2;
  - partial denial on a denied subtree (`/srv/cellos` if present in the headless image [INFERENCE: verify], else a fixture access row);
  - binary exclusion and `-a`;
  - unsupported flags → 2 before reads;
  - escaped control bytes in text output;
  - elapsed time recorded.

  `qemu-phase03-pipeline-test.sh`:
  - `rg … | take 1` stops traversal (handle count back to baseline via a follow-up probe of the VFS handle table, or a second `rg` succeeding at the handle limit [design detail for the S7-4 CP1]);
  - `rg … | where …`;
  - `rg … > f` then `from jsonl --typed < f` round trip;
  - `cat bytes | rg -a x -`.

  `qemu-phase02-session-test.sh`: Ctrl-C during a long no-match traversal → 130 and the prompt returns.

  Regression: terminal runner, GUI ×6, P03, P02, kernel AP lane, VFS boot selftests.
- **Evidence ceiling:** TCG, 1 vCPU, FAT `/data` + RamFS. No symlink backend. Concurrent-modification evidence is VFS host tests only unless D75-2 adds a mutator fixture. No speed claim beyond the recorded TCG time.

## 10. Decisions requested (recommendations first)

**Entry and process**
- **D70-1 Enter Phase 07** with Phase 04 = in_progress (criterion 4 only) (recommended; same as D-CL3/D60-1), or hold.
- **D70-2 Phase 01 ripgrep admission:** close it inside Phase 07 as the named slice S7-0 (plan entry gate item 3) (recommended), or reopen Phase 01 as a whole.
- **D70-3 Slice order:** *S7-0 → S7-1 → S7-2 → S7-3 → S7-4, with S7-2 allowed before S7-1 if acquisition is delayed* (recommended), or strict order.
- **D70-4 Owner handoff:** the assistant as integrating owner for `libs/vigrep`, the text-engine hooks, cellbox `rg`, and the VFS service/`api::ipc`/`ostd` read-path files listed in §5 (recommended, as for C5/S6-1), or name another owner.
- **D70-5 Separate defects:** record `grep -r` silent listing truncation (§2.2) and `ReadAt` silent 4,000-byte truncation (§2.3) as their own defects with their own CP1 (recommended), or fold them into S7-2.
- **D70-6** Delete `libs/text-engine/src/search.rs` in S7-3 as the clean cutover (recommended), or keep it (not recommended: false provenance and wrong semantics).

**S7-0 admission**
- **D71-1 Pin:** *ripgrep 15.2.0 `e89fff89` + component versions in §3.1* (recommended), or keep 15.1.0 `af60c2de`.
- **D71-2 Approve isolated acquisition:** the 15.2.0 tag source (five crate dirs + root `COPYING`/`LICENSE-MIT`/`UNLICENSE`) and `bstr =1.13.1`, into `/tmp` with an isolated `CARGO_HOME`. Workspace Cargo files are untouched until S7-1 CP1. **This needs network access**; no probe so far has used it.
- **D71-3 Reference fixture binary:** *build host rg 15.2.0 from the same tag in `/tmp`* (recommended if D71-1 = 15.2.0; widens the acquisition to rg's std closure: printer/cli/lexopt/serde_json/termcolor/textwrap/anyhow), or use system `/usr/bin/rg` 15.1.0 with recorded divergences.
- **D71-4 Licences:** MIT for every dual-licensed package in §3.3 (recommended).

**S7-1 port**
- **D72-1 Packaging:** *P1 `libs/vigrep`* (recommended) / P2 vendored fork / P3 `[patch]`.
- **D72-2 Edition 2024** for `vigrep`, matching upstream to minimise the diff (recommended), or 2021.
- **D72-3 Port boundary as in §3.1:** drop mmap/encoding/path entry points/walk/types/env/global git; add `vigrep::io` and byte paths; expose searcher capacity (recommended).
- **D72-4 Unicode:** *U1 full tables* (recommended) / U2 subset with `\p` rejected.
- **D72-5 Approve the S7-1 CP1** (after S7-0 evidence).

**S7-2 VFS**
- **D73-1 Listing:** *L1 `ListAtPage` with kinds/COMPLETE/CHANGED* (recommended) / L2 grant-backed large `ListAt` / L3 refuse directories that fill one reply.
- **D73-2 Change detection:** *V1 handle-scoped `modified`* (recommended) / V2 per-file content revision (D03 subset, more state, pre-empts Phase 08) / V3 none + contract amendment.
- **D73-3 Allocation:** request 31/32, response 11/12, no syscall/right (recommended).
- **D73-4 ostd streaming API** as in §6 (recommended).
- **D73-5 Approve the S7-2 CP1** as written in §6.

**S7-3 engine and consumer**
- **D74-1 Cancellation:** *C1 shared interrupt latch* (recommended) / C2 `Pending` items.
- **D74-2 Producer completion status** on `RowSource` (default none), used when the producer is the sole/final stage (recommended).
- **D74-3 Terminal rendering:** *streaming text renderer* (recommended) / table.
- **D74-4 Diagnostics:** per-entry stderr line plus one end summary when errors/skips > 0 (recommended) / per-entry lines only.
- **D74-5 Walker:** *H1 per-directory reopen* (recommended) / H2 derived chain (depth ≈ 28).
- **D74-6 Plain JSON:** keep `path`/`line_bytes` as Path/Bytes per contract and fix the `docs/cellbox-cli.md:78` example to `to jsonl --typed` (recommended), or amend the contract so `path` is String (VFS paths are always UTF-8).
- **D74-7 Ratify DV7-1..DV7-9** (§8), or name the ones to keep as written.

**S7-4 proof**
- **D75-1 Fixture generator** kept as a runner fixture (recommended, re-runnable like `s2tw-probe`/`top-load`), or a throwaway.
- **D75-2 Concurrent-modification evidence:** *VFS host tests only, recorded as the ceiling* (recommended), or add a feature-gated mutator fixture Cell with a private write-capable launch row.
- **D75-3 Bounded-memory proof:** *a file larger than cellbox's quota* (recommended) / a TelemetrySnapshot quota reading during the search.

## 11. Risks and rollback
- **Port drift.** ignore's precedence logic is re-expressed for a serial walker. Mitigation: upstream tests plus the differential corpus. A semantic difference is either fixed or listed as an intentional difference; it is never silently accepted.
- **VFS hot path.** Mutation hooks scan the bounded handle tables on every write. Measure write latency before and after on the private image. If the cost is material, stop and report rather than drop the flag.
- **Listing consistency.** `CHANGED` turns a concurrently mutated directory into error 2 instead of a skewed listing. This is truthful, but it can surface under heavy writers; that is reported, not retried.
- **TCG time.** A ~24 MiB read is about 6,300 IPC round trips of 4,000 bytes. If the runner budget is exceeded, shrink the file to just above the quota. The 64 KiB `ReadHandleGrant` path is an optimisation outside this scope.
- **Footprint.** cellbox gains regex/Unicode tables (§3.4 probe signal). Size is recorded, not judged; Phase 10 owns it.
- **Rollback.** Revert each slice's files together and cold-boot a matched private image. The VFS variants are appended, so old clients are unaffected; never reuse variant indices 31/32/11/12 for other meanings. No persisted state. Rollout, CP2 and quarantine stay as they are.

## 12. Plan housekeeping done with this dossier
- `plan.md` Phase 04 row was stale ("S1 done; S2-S4 pending; no accepted cutover"). It now reads: criteria 1/2/3/5/6 checked (private QEMU/TCG fixtures); every Phase 04 delta CP2-approved 2026-10-22; only criterion 4 (canonical one-artifact packaging) open, held by rollout containment. This matches `phase-04-multicall-cutover.md:31-36,96,98` and `phase-04-closure-review.md` §4.
- `plan.md` Phase 07 row links this dossier (state unchanged: incomplete/blocked, CP1 pending).
- Probe crates `/tmp/p07-probe-{a,a2,c}` were deleted after recording; nothing was added to the workspace or the Cargo cache.

## 13. Decisions recorded (user, 2026-10-24)
The user approved the CP1 inputs in §10 **per every recommendation**:
- **Entry/process:** D70-1 enter Phase 07 with Phase 04 `in_progress` (criterion 4 only); D70-2 close the Phase 01 ripgrep admission inside Phase 07 as S7-0; D70-3 order S7-0 → S7-1 → S7-2 → S7-3 → S7-4, S7-2 may precede S7-1 if acquisition is delayed; D70-4 the assistant is integrating owner for `libs/vigrep`, the text-engine hooks, cellbox `rg` and the VFS/`api::ipc`/`ostd` read-path files in §5; D70-5 `grep -r` listing truncation and `ReadAt` truncation are separate defects with their own CP1; D70-6 delete `libs/text-engine/src/search.rs` in S7-3.
- **S7-0:** D71-1 pin ripgrep 15.2.0 `e89fff89` with the §3.1 component versions; D71-2 isolated network acquisition (tag source, root licence files, `bstr =1.13.1`) into `/tmp` with an isolated `CARGO_HOME`, workspace Cargo files untouched until S7-1 CP1; D71-3 build host reference `rg` 15.2.0 from the same tag in `/tmp`; D71-4 MIT for every dual-licensed package in §3.3.
- **S7-1:** D72-1 P1 `libs/vigrep`; D72-2 edition 2024; D72-3 port boundary per §3.1; D72-4 U1 full Unicode tables. **D72-5 (S7-1 CP1) is approved only after the S7-0 evidence** is presented ([phase-07-s0-admission.md](phase-07-s0-admission.md) §9).
- **S7-2:** D73-1 L1 `ListAtPage`; D73-2 V1 handle-scoped `modified`; D73-3 request 31/32, response 11/12, no syscall/right; D73-4 ostd streaming API per §6; **D73-5 S7-2 CP1 approved as written in §6.**
- **S7-3:** D74-1 C1 shared interrupt latch; D74-2 producer completion status; D74-3 streaming text renderer; D74-4 per-entry stderr plus end summary; D74-5 H1 per-directory reopen; D74-6 keep Path/Bytes and fix the docs example to `to jsonl --typed`; D74-7 DV7-1..DV7-9 ratified.
- **S7-4:** D75-1 fixture generator kept as a runner fixture; D75-2 VFS host tests only as the concurrent-modification ceiling; D75-3 a file larger than cellbox's quota.

S7-0 was executed on 2026-10-24: [phase-07-s0-admission.md](phase-07-s0-admission.md). Correction to §3.4: host `/usr/bin/rg` reports **14.1.0**, not 15.1.0.

## 14. S7-2 results (2026-10-24; CP2 approved by the user 2026-10-24)
S7-2 was implemented as approved in §6 (D73-1..5), before S7-1 (D70-3). It adds no syscall, allowlist bit, right, CapSet or launch-profile change. No runner pins a changed file: the terminal runner's `authority` dict pins only `launch_profile/*` and `abi/cell_session.rs`, and the two `check-*-law1-digests.sh` scripts pin AI-SDK and `ostd/src/syscall.rs`. Nothing was re-pinned.

### 14.1 What changed
- **Wire (`libs/api/src/services/ipc.rs`).**
  - Appended request 31 `ListAtPage { dir, cursor: u32 }` and request 32 `FileHandleState { file }`. Both are handle-addressed and read-only.
  - Appended response 11 `DirPage { next: u32, flags: u8, entries: &[u8] }` and response 12 `FileState { size: u64, modified: bool }`.
  - New `ipc::vfs_dir_page`: the `kind:u8, len:u8, name` entry format, `EntryKind` (0 File, 1 Dir, 2 Mount, 3 Other), `COMPLETE` (bit 0), `CHANGED` (bit 1), `push_entry`, and a validating `entries()` decoder that yields one error and then stops. The service encodes with these helpers and the client decodes with them.
- **VFS service.**
  - `FsBackend::list_page(path, start, out) -> Option<ListPage { written, next }>` is a required method. The shared `backend::fill_page` pages entries and fails on an iteration error, an invalid name, a cursor past the end, or ≥2³⁰ entries. Implementors:
    - RamFS: BTreeMap order.
    - FAT: on-disk order through `backend_fat::page_dir`.
    - CellosFS: `list_dir` order. Read errors poison the backend as before.
    - BootFS proxy: ReadDir order. A ReadDir error is an error, not the end of the listing.
    - `/bin` overlay: VIFS1 entries, then cell-store entries that VIFS1 does not already list. Cursor bit 30 marks the cell-store half.
    - GuestDisk stub: `None` → `ERR_IO`.
  - `VfsManager::list_page` (`manager/listing.rs`) appends the mount-table entries after the backend's own entries. Cursor bit 31 marks that phase. The appended entries are:
    - mount points, as kind `Mount`;
    - directories that only lead to a mount, as kind `Dir`;
    - `/etc` configuration names.

    A backend entry that is a mount point (RamFS `/tmp`) is re-kinded `Mount`. There is no 480-byte cap.
  - `dispatch_grants::list_at_page` re-checks four things on every page: handle ownership (`ERR_HANDLE`), `can_read` (`ERR_DENIED`), health and directory kind (`ERR_IO`). Health is checked again after the listing.
  - `dispatch_file_handles::file_state` re-checks owner and `can_read`, then stats. A path that is no longer a regular file is `ERR_IO`.
  - Both requests are routed in the directory group.
  - Mutation hooks (`manager/change_marks.rs`) are called by the `VfsManager` mutating delegates, which every write/delete request ends in:
    - `write`/`append` mark the path; if the write created an entry, they also mark the parent. The service stats only when a handle names the parent.
    - `write_at` (WriteHandleGrant) marks the path.
    - `mkdir` marks the parent of the outermost directory it creates.
    - `unlink` and `rename_no_replace` (old and new) mark the path and its parent.
    - `rmdir` and `rmdir_recursive` mark everything at or below the path, plus the parent.
  - Marks set `FileEntry::modified` and `DirEntry::changed` (both sticky).
- **ostd.**
  - `VfsClient` gains `open_root_dir`, `list_page(dir, cursor, out) -> VfsDirPage { next, complete, changed, entries() }`, `open_file_at`, `read_chunk(file, offset, buf) -> usize` (≤ `VFS_READ_CHUNK_MAX` = 4,000; 0 means end of file), `file_state -> VfsFileState { size, modified }`, `close_file` and `close_dir`.
  - The implementation is in `clients/vfs/stream.rs` over the existing `VfsReadOps` seam, whose visibility was widened inside `clients::vfs`.
  - `read_file` is unchanged.

### 14.2 Decisions taken inside §6 (for CP2 ratification)
1. On the `COMPLETE` page, `next` is 0 and has no meaning. §6 only said "opaque".
2. A backend entry whose name fails `validate_dir_component` makes the whole page `ERR_IO`. It is not skipped: a skipped entry would be a silent omission, and a handle could not name it anyway.
3. Marks are set when the mutation reaches the backend, even if the backend reports failure. FAT can fail after a partial write, and CellosFS failures may have been accepted, so "failed" does not mean "unchanged". The cost is a false `modified`/`CHANGED` after a refused write, never a missed one.
4. Paths are compared ASCII-case-insensitively (`paths::may_alias`), because fatfs (built without `unicode`) matches names that way. On case-sensitive backends this can only add marks.
5. `Mkdir` and `Rmdir` also set `CHANGED`, because they create or remove a child. A content-only write sets `modified` and leaves `CHANGED` clear.
6. A write to `/mnt/sd/etc/cellos/{services,autoload}.toml` also marks handles on the `/etc/cellos/...` path it supplies (`config_source`).
7. Rename on `/srv` cannot move an open file: the namespace lease of a `/srv` file handle refuses it (`ERR_DENIED`). So for an open file, rename shows up only as `CHANGED` on the parent directories. This was observed in a test; it is not a new rule.

### 14.3 Verification (host and builds; this agent ran no QEMU)
- `cargo test -p service-vfs --target x86_64-unknown-linux-gnu`: **94 passed** (baseline 79). The new tests are in `manager/read_path_tests.rs`, and all of them go through the encoded `dispatch::handle_request` round trip:
  - Paging: 200 entries × 42 bytes = 8,400 bytes over ≥3 pages; every page holds only whole entries and is ≤4,000 bytes; `COMPLETE` only on the last page; exact set; a second walk is identical. An empty directory is one complete page.
  - Kinds: `/` lists tmp/data/bin/srv as `Mount`, mnt/etc as `Dir` and readme.txt as `File`, with no duplicates. `/mnt` lists `sd` as `Mount`.
  - Change during paging: a create after page 1 sets `CHANGED` on every later page, including the complete one. The flag is sticky for the handle; a fresh handle starts clean.
  - `CHANGED` is set after create, unlink, append-create, mkdir and rmdir. It is not set after an overwrite, an append, ReadAt or StatAt. RmdirRecursive marks the outer handle, and the inner handle then answers `ERR_IO`.
  - `modified` is set after Write, Append and WriteAt (same bytes too), for this reader and another reader. It is not set after ReadFileHandle, StatAt, ListAtPage or ReadAt. After unlink the state query is `ERR_IO`; after re-creation with identical bytes (ABA) it reports `modified: true`. RmdirRecursive marks a handle below the removed tree and leaves a handle beside it untouched.
  - Permission rechecks: an access-table swap between pages gives `ERR_DENIED` on the next page and on the state query; both work again after the swap back. Another cell gets `ERR_HANDLE`. A handle naming `/srv/cellos` gets `ERR_DENIED`. A sealed cell without VfsMutate keeps both requests.
  - Revocation: `CloseDir` revokes paging and the state of derived files; `CloseFile` revokes the state.
  - Handle caps: paging holds no handles. At 32 file handles the 33rd `OpenFileAt` is `ERR_QUOTA`, while paging and state queries still work; closing one handle frees a slot. At 32 directory handles `OpenRootDir` is `ERR_QUOTA` and the held handle still pages.
  - Stale cursors (past the end, either phase) and a directory replaced by a file are `ERR_IO`. A cursor exactly at the end gives an empty complete page. A backend that cannot page answers `ERR_IO`.
  - FAT, through `page_dir` on an in-memory fatfs image: 150 long-name files plus one subdirectory over ≥2 pages, exact set and kinds. An overlay skip filter takes no index. A device read failure part-way through gives `None`, not a short page. Stale and at-end cursors behave as above.
  - RamFS and CellosFS paged through 10-byte buffers (≥4 pages each): exact sets; a file or a missing path is not listable. A CellosFS directory holds at most 7 entries (428-byte inode payload / 56-byte records), so real CellosFS listings fit one page.
  - Rename on a test CellosFS at `/srv` marks both parent handles `CHANGED`. With a reader's file open, rename is `ERR_DENIED`. `write_at` marks the reader's handle and the writer's own handle.
  - `may_alias`/`may_alias_within`: FAT case folding and component boundaries.
- `cargo test -p api --target x86_64-unknown-linux-gnu`: 147 passed. New tests:
  - byte pins `1f 05 ac 02` / `20 07` / `0b 03 01 03 00 01 61` / `0c a1 1f 01`; both requests are not path-addressed and need no mutation authority; `PreparedChildV1` keeps index 10;
  - entry round trip; invalid names, unknown kinds and truncation rejected;
  - a 4,000-byte page plus `u32::MAX` cursor fits one 4,096-byte frame.
- `cargo test -p ostd --target x86_64-unknown-linux-gnu`: lib 67 passed. Four new tests in `read_file/tests/stream.rs`:
  - cursor chain with both flags;
  - a malformed page, an unknown kind or a too-small buffer is an error;
  - wire error mapping;
  - `read_chunk` is bounded by `min(buf, 4000)`, and an over-long reply is `IO`;
  - `file_state` and the handle lifecycle requests.
- Bare-metal builds:
  - `service-vfs` with and without `test-hooks` for `x86_64-unknown-none` (`RUSTFLAGS=-C relocation-model=pic`, `-Z build-std=core,alloc`, as `build-x86_64-cells.ps1` does): Finished with no warnings.
  - `service-vfs --features test-hooks` for `riscv64gc-unknown-none-elf`: Finished.
  - `cargo check` of `service-vfs` for `aarch64-unknown-none`: Finished.
  - `cargo check` of `api` and `ostd` for aarch64 and riscv64: Finished.
  - A plain build without the PIC flag fails to link (R_X86_64_32S). That is the known cell-build requirement, not this change.
- Kernel unaffected: `cargo check -p cellos-kernel --target x86_64-unknown-none` with the runner's kernel RUSTFLAGS and a separate target dir finished. The only warnings are pre-existing ones in `task/cell_session.rs` and `task/child_context.rs`. The kernel references `VfsRequest` only in a test stub signature.

### 14.4 Runtime proof
§6 names "VFS boot selftests" as the S7-2 runtime proof. `manager/read_path_selftest.rs` (`test-hooks` only) runs at VFS boot against a private RamFS `/tmp`, through the handle dispatcher and with no kernel attestation, and prints:
- `[vfs-dir-page] multi-page-listing-complete-only-on-last PASS`
- `[vfs-dir-page] child-create-sets-changed PASS`
- `[vfs-dir-page] write-sets-file-modified PASS`

`tests/integration/tests/vfs-quota.rs` (`riscv64_vfs_quota_all_pass`, after `scripts/build-test-hooks-ci.sh`) now expects all three markers. **That QEMU lane has not been run yet** (the parent runs QEMU).

That canonical lane cannot run while the rollout containment holds: `scripts/build-test-hooks-ci.sh` signs the canonical `target/riscv64gc-unknown-none-elf/release/cellbox` and `cellos-sign` refuses it (`ROLLOUT BLOCKED: unqualified Cellbox/terminal artifact`). The refusal stays in force. The private substitute is the existing x86 kernel-regression lane of `scripts/qemu-phase02-session-test.sh`: with `PHASE02_KERNEL_TESTS=1` it now builds `service-vfs` with `test-hooks` in its private work dir. It refuses an image whose `service-vfs` lacks the three marker strings. After the `ATOMIC_PUBLICATION_*` suite it waits for the last dir-page marker. It fails unless all three markers are present, with no `[vfs-*] ... FAIL` line and no service-vfs panic. The default P02 session lane keeps the production `service-vfs`.

Proof of the cellbox consumer path lands in S7-4: a FAT fixture larger than one page, BootFS/overlay listings of `/bin`, and grant-write marking on target. BootFS and the `/bin` overlay are syscall-backed and have no host fixture, so their `list_page` is covered by construction and by compiling only.

### 14.5 Open items for CP2
- §11 asks for write latency to be measured before and after on the private image, to judge the mark cost. This is not done (it needs QEMU). Per mutation the cost is one scan of the directory and file handle tables, one stat when a directory handle names the parent of a write, and stats up the missing ancestors for `Mkdir`.
- Unchanged, as D70-5 requires: `ListDir`/`ListAt` still truncate silently (FAT/RamFS caps, the 480-byte mount cap), and `ReadAt` still truncates at 4,000 bytes.

### 14.6 CP2 (user, 2026-10-24)
S7-2 CP2 is approved, covering the §14.2 decisions 1–7. Two items remain open: the write-latency measurement from §11 is not done, and the D70-5 truncation defects keep their own CP1.
