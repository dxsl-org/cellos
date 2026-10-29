# Phase 07 S7-0 — ripgrep admission record (Phase 01 step 7, search half of CBX-02)

Status: **S7-0 executed 2026-10-24** under D71-1..4 ([entry dossier](phase-07-entry-dossier.md) §10/§13). **D72-5 approved by the user 2026-10-24: S7-1 CP1 as written in §9.** S7-1 implemented the same day; results in §10. S7-0 itself changed no workspace Cargo, lock or source file. Everything was acquired into `/tmp/p07-s0` with an isolated `CARGO_HOME`. The archived manifest and logs are in [reports/s7-0-evidence/](reports/s7-0-evidence/) (`manifest.json` holds every hash below plus the per-file inventory).

## 1. Acquisition (D71-2, D71-3)
- **Isolation.** Work root `/tmp/p07-s0`. `CARGO_HOME=/tmp/p07-s0/cargo-home` for every cargo call. Separate `GNUPGHOME=/tmp/p07-s0/gnupg`. Target dirs live under `/tmp/p07-s0`. No cargo command ran in the workspace. Afterwards `~/.cargo/registry/cache` holds none of the acquired crates (`bstr-1.13.1`, `grep-matcher`, `globset-0.4.19`: 0 hits), and the workspace `Cargo.lock` still has no `bstr`. (`Cargo.toml`/`Cargo.lock` were already dirty in the tree from other work; S7-0 did not touch them.)
- **Network endpoints used:**
  - `github.com`: `git fetch --depth 1 origin refs/tags/15.2.0` and `BurntSushi.gpg`;
  - `api.github.com`: tag verification;
  - `codeload.github.com`: tag tarball;
  - `index.crates.io` / `static.crates.io`: the five component `.crate` files, `bstr 1.13.1`, rg 15.2.0's locked closure (D71-3), and the `-Z build-std` toolchain dependencies (fetched by cargo into the isolated home).

  The isolated home holds 69 crate archives: names in `isolated-cargo-home-crates.txt`, SHA-256 in `isolated-cargo-home-crates.sha256`. cargo verified each one against the index checksum.
- **Toolchains.**
  - Bare-metal probes: `rustc 1.97.0-nightly (f53b654a8 2026-04-30)` = workspace `nightly-2026-05-01`.
  - Reference rg: `stable` `rustc 1.92.0 (ded5c06cf 2025-12-08)`. Root `rust-version = "1.85"`.
  - `cargo-deny 0.20.2`. git 2.43.0.

## 2. Upstream pin and integrity (D71-1)
| Item | Value |
|---|---|
| Tag | `15.2.0`, annotated tag object `6ec72defacfb042f203ca0b4bf2513a0a5505a7e` |
| Commit | **`e89fff89ac9af12e8d4ce9d5fd07beb408ca730f`** (2026-07-15T12:00:11-04:00, Andrew Gallant, "15.2.0"); tree `c743701524f65f036cf174d6551918be7dfc0d40` |
| Tag signature | `git verify-tag`: **Good signature**, RSA key `2D8111E141151C072EE9AB9DB2E3A4923F8B0D44` "Andrew Gallant (aka burntsushi)". Key file from `https://github.com/BurntSushi.gpg`, SHA-256 `b3c8f72b…1d21c292af`; owner trust not established beyond GitHub (gpg "[unknown]"). GitHub API: `verification.verified = true`, reason `valid`. The commit is also signed (`gpgsig`) |
| Tag tarball | `codeload…/tar.gz/refs/tags/15.2.0`, 606,519 B, SHA-256 **`7605249d3eb0d5f170e3414498e3344e26b1e7a147aec518b57090b80036a562`**. Extracted tree is byte-identical to the git checkout (`diff -r`) |
| Root licences | `COPYING` SHA-256 `01c266bc…d75012390f` (blob `bb9c20a0…`): "dual-licensed under the Unlicense and MIT … either license". `LICENSE-MIT` SHA-256 `0f96a838…0740af850f` (blob **`3b0a5dc0…`**, same blob as the 15.1.0 review): "Copyright (c) 2015 Andrew Gallant". `UNLICENSE` SHA-256 `7e12e5df…cf94e85c` (blob `68a49daa…`). `git archive --format=tar 15.2.0 COPYING LICENSE-MIT UNLICENSE` SHA-256 `b630a8a1…1b79bf72cd` |

### 2.1 Component crate directories (tag) and crates.io identity
`git archive --format=tar 15.2.0 crates/<dir>` is reproducible because it uses the commit mtime; it was run twice with identical results. The content manifest is `sha256sum` over the sorted file list, then hashed.

| Package | Ver | Tag dir | git tree | Dir archive SHA-256 | Content manifest SHA-256 | crates.io `.crate` SHA-256 (= index `cksum`) |
|---|---|---|---|---|---|---|
| grep-matcher | 0.1.9 | `crates/matcher` (9 files) | `bd1982fd…` | `b600916d2bb96f1f3b9889d595e54e43b1979d6d55923ddb4b13c2ef319bc872` | `faf35ab8344a0383066d429f8f138766a7028bac10824eefef67c83a48a24b21` | `f9417543f4870fc8f1c8e1af870afae2431007626d9e703fce6471c468d33847` |
| grep-regex | 0.1.14 | `crates/regex` (13) | `76f2f7c0…` | `2ab1dd668f74ed53ad46cdf9375128b941fbd1bac39604e8eb5d54c2eb12d851` | `278a843f45b4591a7d41886bbffc9504a28390a522d2fe91023bf9d4cfdc41aa` | `0ce0c256c3ad82bcc07b812c15a45ec1d398122e8e15124f96695234db7112ef` |
| grep-searcher | 0.1.17 | `crates/searcher` (15) | `164ee165…` | `3f839ee1099f318da25f7a2349077123ccc302826d374e7f7daee4b0ed489fa4` | `47d6fdf56e43249155c9e3c302b4b6ace4de03ea3e0c9f6bc651a916809bbf93` | `72348823a0eafc4bc2e9051064f28b5b42cc100b571b3a35d67918d711efcbc6` |
| globset | 0.4.19 | `crates/globset` (11) | `c1854ec9…` | `05507e3ce451f097d158f791a2faa6aaa6f9475447e9165a50d4851db2de8800` | `b5ba1612bd7cce2fee3aad95865773e6be3fdf821272a8b32e0056da6de7267f` | `e47d37d2ae4464254884b60ab7071be2b876a9c35b696bd018ddcc76847309cd` |
| ignore | 0.4.29 | `crates/ignore` (18) | `dd67b146…` | `31aa0ab735f11a7274d5033e2db69eda3d0cb5a22b7a9192c8e70cb8c77d16f0` | `0213d2df3fe318cd0188736ebe803afa1dce5b8cbb4280151b5b03e760dbebeb` | `d4ffa3a0547a138e59ddd6fa3b7c672ed47e6ad6a3cd177984ff1116aa5ba742` |

- **Packages match the tag.** Every crates.io package is identical to its tag directory. The only differences are cargo's normalised `Cargo.toml` (its `Cargo.toml.orig` is identical to the tag file) and `.cargo_vcs_info.json`. No crate is yanked, and none declares `rust-version`.
- grep-regex 0.1.14 was published 2025-10-16, before 15.2.0. Its source is unchanged since the 15.1.0 review. The other four were published 2026-07-15.
- **Port source.** The **port source is the tag tree** (D71-1). The `.crate` files serve only as an identity cross-check.

### 2.2 bstr `=1.13.1`
- Integrity:
  - `.crate` SHA-256 **`6bb31b46c14244e20ee9984b11bf5c992b91fb6939fea616e3512c8baecdbe5f`** = crates.io index `cksum` = dossier §3.3 prefix.
  - 356,631 B; published 2026-08-10; not yanked.
  - VCS sha1 `134195be38c7c9a4887980fd0f6f8bae042dd91e`.
- Metadata: `MIT OR Apache-2.0`, edition 2021, `rust-version 1.65`. `src/lib.rs` is unconditionally `#![no_std]`.
- **Selected features: `alloc` only**, which means `memchr/alloc`. `std` and `unicode` (the `regex-automata dfa-search` dependency and the 16 `.dfa` data files) are not compiled.
- Licence files:
  - `LICENSE-MIT` `6b7374c3…7f61715`, "Copyright (c) 2018-2019 Andrew Gallant";
  - `LICENSE-APACHE` `a60eea81…45f759f2`;
  - `COPYING` `68653aaa…c14b` (either licence).
- Contents: 43 `src` files (27 `.rs`, 11,657 lines). 10 files contain `unsafe` (41 occurrences). bstr is a normal dependency; `vigrep`'s `forbid(unsafe_code)` covers only vigrep's own code.

## 3. Host reference `rg` (D71-3)
- **Build.** `CARGO_HOME=/tmp/p07-s0/cargo-home cargo +stable build --release --locked --target-dir /tmp/p07-s0/target-rg`, run in the tag checkout.
  - Finished in 13.6 s.
  - The tag's `Cargo.lock` (SHA-256 recorded in `manifest.json`) was unchanged (`cmp`).
  - Default features: no PCRE2.
- **Binary** `/tmp/p07-s0/target-rg/release/rg`: 27,237,200 B, SHA-256 **`f1ae3ea62707860ddfd1a50c1b28a4525baf84df3d68561684a73723c1f9a918`**. ELF x86-64 PIE; the release profile carries upstream's debuginfo. This binary hash is a record, not a reproducibility claim.
- **`rg --version`** (`reports/s7-0-evidence/rg-version.txt`):
  ```
  ripgrep 15.2.0 (rev e89fff89ac)

  features:-pcre2
  simd(compile):+SSE2,-SSSE3,-AVX2
  simd(runtime):+SSE2,+SSSE3,+AVX2
  ```
- **Reference vs port lock.** The reference uses the tag lock's **bstr 1.13.0** and **regex-automata 0.4.15**. The port uses bstr 1.13.1 and the workspace's regex-automata 0.4.16. memchr 2.8.3, aho-corasick 1.1.4, regex-syntax 0.8.11 and log 0.4.33 are identical. Both are patch releases. A differential mismatch that traces to them is reported as such, not hidden.
- **Correction to dossier §3.4.** Host `/usr/bin/rg --version` reports **14.1.0**, not 15.1.0. This does not matter now that D71-3 supplies the pinned 15.2.0 reference.

## 4. Licences (D71-4)
- **Per-file census.** Two scans found no licence, copyright or SPDX text anywhere in the five crates' 42 `.rs` files (34 `src`, 8 tests/examples/benches): one scan of the header, one over the whole file. The only hits are the `license`/`COPYRIGHT` file-type globs in `ignore/src/default_types.rs`, which is dropped. Every crate directory carries the same `LICENSE-MIT` (blob `3b0a5dc0…`) and `UNLICENSE` (blob `68a49daa…`). globset and ignore also carry `COPYING` (blob `bb9c20a0…`). bstr's `src` has no per-file licence headers either.
- **Choices:** **MIT** for the five components (copyright 2015 Andrew Gallant). **MIT** for bstr (2018-2019 Andrew Gallant), memchr, aho-corasick, regex-automata, regex-syntax and log, as dossier §3.3.
- **What S7-1 must carry:**
  - an upstream-path + tag-commit + "adapted" provenance header and `SPDX-License-Identifier: MIT` on each adapted file;
  - `libs/vigrep/LICENSE-MIT` copied verbatim (SHA-256 `0f96a838…`);
  - a third-party notice naming ripgrep 15.2.0 `e89fff89` and bstr 1.13.1.
- **`cargo deny list` (port subset graph):**
  - aho-corasick 1.1.4 / memchr 2.8.3: Unlicense, MIT;
  - bstr 1.13.1 / log 0.4.33 / regex-automata 0.4.16 / regex-syntax 0.8.11: MIT, Apache-2.0;
  - the throwaway adapted crates: MIT.

  All are covered by `deny.toml` `allow` (`MIT`). No exception is needed.

## 5. Compile evidence (dossier §3.5 items 3–4)
- **Stock failure (item 3).**
  - Setup: throwaway crate `s0-stock` with path dependencies on the five tag crates. The tag lock was reused, so versions equal rg's.
  - Host `x86_64-unknown-linux-gnu`: exit 0.
  - `RUSTFLAGS="-C relocation-model=pic" cargo +nightly-2026-05-01 build --release --target x86_64-unknown-none -Z build-std=core,alloc --keep-going` → **exit 101**.
  - Failures: memchr and regex-syntax `E0463 can't find crate for 'std'` (their `std` features are requested by the components), crossbeam-utils, same-file, memmap2 (24 errors) and encoding_rs_io (61 errors).
  - None of the five components is reached. Log: `stock-x86_64-unknown-none.log` (SHA-256 `062fb71a…0bfc8`).
- **Adapted subset compile (item 4), partial by construction.**
  - Setup: throwaway `s0-matcher` + `s0-regex` with a mechanical adaptation of grep-matcher and grep-regex. Changes: `#![no_std]` + `extern crate alloc`; `std::` → `core::`; an alloc prelude import per file; `impl From<NoError> for io::Error` removed; `Error::generic` bound `core::error::Error` → `core::fmt::Display`, because regex-syntax/automata implement `Error` only with `std`.
  - Diffs: `adapted-matcher.diff` (`9786a7f4…`), `adapted-regex.diff` (`857d93b1…`).
  - These sit under `s0-subset`, which also uses bstr `[alloc]`, memchr `[alloc]`, aho-corasick `[perf-literal]`, regex-automata `[alloc,syntax,meta,nfa,hybrid,dfa-onepass,perf,unicode]`, regex-syntax `[unicode]` and log, all with defaults off at exact pins.
  - `subset/src/lib.rs` builds a `RegexMatcherBuilder` (smart case, LF terminator) and calls `find`.
  - Results:
    - **`x86_64-unknown-none`: exit 0** (clean, 15.5 s, rlib 71,172 B);
    - **`targets/x86_64-unknown-cellos.json -Z json-target-spec`: exit 0** (clean, 20.7 s, rlib 71,956 B);
    - zero warnings.
  - Lock `adapted.Cargo.lock` (SHA-256 `b6898c27…7de5e7`). Its memchr/aho-corasick/regex-automata/regex-syntax/log checksums **equal the workspace `Cargo.lock` entries**. bstr is the only new package; serde_core 1.0.229, bstr's optional dependency that is not built, is already in the workspace lock.
- **What is not executed in S7-0.** The adapted compile of grep-searcher, globset and ignore *is* the S7-1 port: `io` trait, byte paths and the precedence re-expression. Therefore S7-1's acceptance carries item 4 for those modules on both x86 profiles (§9). Item 5, the final `cargo deny` over the real workspace graph, is also S7-1 by definition.

## 6. Advisories (dossier §3.5 item 6)
- **Database.** `~/.cargo/advisory-dbs/advisory-db-3157b0e258782691`, commit **`640bf5a9f06e66d8721e8772766531fe77c8630d`** (2026-10-22T12:49:47+02:00). It was used read-only through a `/tmp` copy of the workspace `deny.toml`; the only change was `db-path` (`reports/s7-0-evidence/deny.toml`). **Offline**, as the dossier specifies; no fresh DB was fetched.
- **Port subset graph** (`s0-subset`; deny graph targets = workspace triples): `cargo deny --offline … check advisories licenses` → **"advisories ok, licenses ok"**, exit 0 (`deny-subset.log`). The only warnings are the unused allow-list entries and ignores of the workspace config.
- **Stock five-component graph** (21 packages incl. encoding_rs/memmap2/walkdir/crossbeam): **ok/ok**, exit 0.
- **Reference rg closure** (`--target x86_64-unknown-linux-gnu`, 34 packages incl. ripgrep, grep-cli, printer, serde_json, termcolor, textwrap, lexopt, anyhow): **ok/ok**, exit 0. Licences: MIT 34, Unlicense 14, Apache-2.0 17, BSD-3-Clause 1 (encoding_rs). Host-only; not shipped.
- **DB entries by crate name.** The DB has entries only for grep-cli (RUSTSEC-2021-0071, patched ≥0.1.6; the reference uses 0.1.12), memmap2 (RUSTSEC-2026-0186, unsound <0.9.11; stock graph 0.9.11; dropped from the port) and crossbeam-deque (RUSTSEC-2021-0093, patched ≥0.8.1; stock 0.8.7; dropped). There are none for grep-matcher/regex/searcher, globset, ignore, bstr, memchr, aho-corasick, regex-automata, regex-syntax or log.
- **Ceiling.** This rests on the DB as of 2026-10-22 and on the code acquired here. S7-1's workspace `cargo deny` re-checks the actual port.

## 7. Per-crate port inventory (D72-3 boundary)
- **Line counts.** Non-test lines are the lines before the file's `#[cfg(test)] mod tests`. Per-file SHA-256 values are in `manifest.json` → `files`.
- **`unsafe`.** Only `searcher/src/searcher/mmap.rs` (dropped) contains `unsafe` (2 occurrences). Every kept file has 0, so `#![forbid(unsafe_code)]` is attainable.

| Crate | File | Lines | Non-test | Upstream tests | Disposition |
|---|---|---:|---:|---:|---|
| matcher | `src/lib.rs` | 1,379 | 1,379 | 0 | adapt → `vigrep::matcher` (gate the io conversion) |
| matcher | `src/interpolate.rs` | 331 | 144 | 3 | adapt |
| matcher | `tests/{tests,test_matcher,util}.rs` | 3/232/96 | — | 10 | port tests (dev-dep `regex =1.13.1`, already in the workspace lock) |
| regex | `src/{lib,ast,ban,config,error,literal,matcher,non_matching,strip}.rs` | 18/216/83/356/94/1,016/670/159/170 | 18/145/54/356/94/640/552/82/120 | 26 | adapt → `vigrep::regex` (2,061 non-test lines) |
| searcher | `src/lib.rs`, `src/macros.rs` | 106 / 25 | 106 / 25 | 0 | adapt → `vigrep::searcher` |
| searcher | `src/line_buffer.rs` | 965 | 553 | 21 | adapt (`io::Read` → `vigrep::io`; expose capacity) |
| searcher | `src/lines.rs` | 466 | 198 | 10 | adapt |
| searcher | `src/searcher/core.rs` | 713 | 713 | 0 | adapt |
| searcher | `src/searcher/glue.rs` | 1,549 | 352 | 37 | adapt (drop path/file/mmap glue) |
| searcher | `src/searcher/mod.rs` | 1,088 | 1,045 | 3 | adapt (drop encoding/BOM transcoding, `MmapChoice`, `search_path`/`search_file`) |
| searcher | `src/sink.rs` | 663 | 663 | 0 | adapt (`io::Error` → `vigrep::io::Error`) |
| searcher | `src/testutil.rs` | 797 | 725 | 6 | port as a test-only module |
| searcher | `src/searcher/mmap.rs`, `examples/search-stdin.rs` | 124 / 34 | — | — | drop |
| globset | `src/glob.rs` | 1,686 | 1,077 | 6 | adapt → `vigrep::glob` |
| globset | `src/lib.rs` | 1,307 | 1,133 | 13 | adapt (byte-path `Candidate`; `HashMap` → `BTreeMap`/`hashbrown`) |
| globset | `src/pathutil.rs`, `src/fnv.rs` | 141 / 30 | 78 / 30 | 3 / 0 | adapt |
| globset | `src/serde_impl.rs`, `benches/bench.rs` | 128 / 113 | — | — | drop |
| ignore | `src/gitignore.rs` | 883 | 703 | 9 | adapt → `vigrep::ignore` (rules via `add_line` from VFS bytes) |
| ignore | `src/overrides.rs` | 293 | 185 | 9 | adapt |
| ignore | `src/pathutil.rs` | 142 | 142 | 0 | adapt (byte paths) |
| ignore | `src/dir.rs` | 1,608 | 1,110 | 24 | adapt: precedence logic only, re-expressed for the serial walker; its filesystem tests are re-expressed over the in-memory `ViSearchTree` |
| ignore | `src/lib.rs` | 547 | 486 | 0 | adapt: error types only |
| ignore | `tests/gitignore_matched_path_or_any_parents_tests.{rs,gitignore}`, `tests/gitignore_skip_bom.{rs,gitignore}` | 318+216 / 17+2 | — | 6 | port tests + fixtures |
| ignore | `src/walk.rs`, `src/types.rs`, `src/default_types.rs`, `examples/walk.rs` | 2,607 / 584 / 378 / 64 | 2,115 / 494 / 347 / — | — | drop |

**Totals (`.rs`):**
- adapt: 28 files, 16,704 lines, **12,183 non-test**;
- test sources ported: 6 files, 1,463 lines;
- dropped: 8 files, 4,032 lines.

Plus per crate: `Cargo.toml`, `README.md`, `LICENSE-MIT`, `UNLICENSE` and (globset/ignore) `COPYING` as metadata/licence. Upstream `#[test]` functions in kept files: 186 in total (matcher 13, regex 26, searcher 77, globset 22, ignore 48 = `dir.rs` 24 + `gitignore.rs` 9 + `overrides.rs` 9 + integration 6).

## 8. Snapshot storage and retention
The dossier names `/tmp` (D71-2: "workspace Cargo files are untouched until S7-1 CP1"), so no vendored copy enters the repository before the S7-1 CP1.
- **Retained in `/tmp/p07-s0`:**
  - `src/`: git checkout at tag `15.2.0`, which is the port source;
  - `dl/`: tag tarball, per-crate dir archives, root-licence archive, `.crate` files, index lines, signing key;
  - `cargo-home/`: isolated registry;
  - `gnupg/`;
  - `target-rg/release/rg`: the reference binary.

  The throwaway probe crates and their target dirs were deleted after their logs, manifests, locks and diffs were archived.
- **Archived in the repository:** `reports/s7-0-evidence/`, i.e. `manifest.json` plus the logs, locks, diffs, deny config/outputs, `rg-version.txt` and the isolated-cache list.
- **Rebuilding after `/tmp` loss.** `/tmp` is volatile. Re-acquire with the §1–§3 commands, then check the result against `manifest.json`: tag object, commit, tarball SHA-256, dir-archive SHA-256, `.crate` SHA-256. A rebuilt reference rg must print the same `--version` line; its binary hash may differ.

## 9. S7-1 CP1 request (D72-5) — for user approval
Scope is as approved in D72-1..4 and dossier §5 (S7-1 row). Concretely:
- **New crate `libs/vigrep`** (Law 6 name):
  - `edition = "2024"`, `license = "MIT"`, `publish = false`;
  - `#![cfg_attr(not(test), no_std)]` + `extern crate alloc` + `#![forbid(unsafe_code)]` (the `text-engine`/`vitui` pattern; host tests with `--target x86_64-unknown-linux-gnu`).
- **Modules:**
  - `vigrep::io`: core-only `Read`-like trait and error;
  - `vigrep::matcher`, `vigrep::regex`, `vigrep::searcher`, `vigrep::glob`, `vigrep::ignore`: §7 dispositions, line-for-line adaptations where possible;
  - `vigrep::walk`: the serial depth-first walker over the public `ViSearchTree` trait (`open_dir`/`next_page`/`open_file`/`read_chunk`/`file_state`/`close`). It handles root dedup by canonical path (DV7-3), byte-sorted children, hidden, `.rgignore > .ignore > .gitignore` and parent precedence, command globs, binary preflight, the DV7-6 bounds, mount skip (DV7-7), `Other` → per-entry error (DV7-5), the `modified` → error hook (DV7-4) and a cancellation callback.
  - The searcher's line-buffer capacity is exposed. The crate docs carry the upstream → module mapping.
- **Provenance:**
  - Each adapted file starts with an upstream path, `ripgrep 15.2.0 e89fff89ac9af12e8d4ce9d5fd07beb408ca730f`, "adapted", `SPDX-License-Identifier: MIT` and "Copyright (c) 2015 Andrew Gallant".
  - `libs/vigrep/LICENSE-MIT` is the upstream file, verbatim (`0f96a838…`).
  - `libs/vigrep/THIRD-PARTY-NOTICES` names ripgrep 15.2.0 (MIT chosen from `Unlicense OR MIT`) and the MIT choices for bstr 1.13.1, memchr, aho-corasick, regex-automata, regex-syntax and log.
  - The port starts from the `/tmp/p07-s0/src` tag tree; per-file SHA-256 values in `manifest.json` are its baseline.
- **Dependencies** (exact pins, defaults off; identical to the compiled §5 subset):
  - `memchr =2.8.3 [alloc]`;
  - `aho-corasick =1.1.4 [perf-literal]`;
  - `regex-automata =0.4.16 [alloc,syntax,meta,nfa,hybrid,dfa-onepass,perf,unicode]` (U1);
  - `regex-syntax =0.8.11 [unicode]`;
  - `log =0.4.33`;
  - `bstr =1.13.1 [alloc]`;
  - dev-only: `regex =1.13.1` for the ported matcher/searcher tests (already in the lock; workspace `resolver = "2"` keeps its `std` features out of bare-metal builds).
  - [INFERENCE] When vigrep and text-engine are in the same graph, cargo feature unification gives text-engine's `regex-automata` the Unicode features too. Size is recorded, not judged (dossier §11).
- **Workspace edits:**
  - root `Cargo.toml`: add `"libs/vigrep"` to `members`, one line;
  - `Cargo.lock`: adds `vigrep` and **`bstr 1.13.1` (checksum `6bb31b46…`) only**;
  - no other manifest or consumer changes. `cellbox` takes the dependency in S7-3.
- **Not in S7-1:** VFS/`api`/`ostd` (S7-2, approved separately), the engine hooks, `cellbox rg`, and the `text_engine::search` deletion (S7-3).
- **Acceptance:**
  1. `cargo test -p vigrep --target x86_64-unknown-linux-gnu`. It runs the ported upstream tests (§7; filesystem-bound `dir.rs` tests re-expressed in memory) and the walker tests over an in-memory `ViSearchTree` (dossier §9 list). It also runs the differential corpus: expected records are generated once by the §3 reference `rg --json` (binary SHA-256 and command recorded, generator digest retained), and intentional differences are asserted separately.
  2. Bare-metal `vigrep` builds for `x86_64-unknown-none` and `targets/x86_64-unknown-cellos.json` (`-Z build-std=core,alloc`, PIC). This closes §3.5 item 4 for all five modules.
  3. `cargo deny check licenses advisories` over the real workspace graph. This closes item 5. The pre-existing `ocel-pdf` AGPL finding stays out of scope.
  4. No source-text or wording tests.

**Requested decision — D72-5:** approve S7-1 as above, or name changes.

**Decision recorded:** D72-5 **approved** by the user on 2026-10-24, S7-1 CP1 exactly as written above.

## 10. S7-1 results (2026-10-24)
- **Files.** New `libs/vigrep/**` (`Cargo.toml`, `LICENSE-MIT` verbatim `0f96a838…`, `THIRD-PARTY-NOTICES`, `PROVENANCE` with every upstream path + SHA-256 and disposition, upstream `rustfmt.toml` verbatim `4b4f1d82…` so adapted files keep upstream formatting under `cargo fmt --all`). Root `Cargo.toml`: one member line. `Cargo.lock`: `vigrep` and `bstr 1.13.1` (checksum `6bb31b46…`) only. `CHANGELOG.md` entry. No other manifest, consumer or S7-2 file touched.
- **Modules.** `matcher`, `regex`, `searcher`, `glob`, `ignore` (headers: upstream path, commit, "adapted", SPDX MIT, copyright); `io` and `walk` are Cellos code. All upstream per-file hashes match `manifest.json`. `glob/fnv.rs` dropped (fnv `HashMap` → `BTreeMap`; `hashbrown` is not in the §9 dependency list). Crate root allows 25 clippy style lints that the verbatim upstream code trips (documented in `lib.rs`); `io`/`walk` are clean without them.
- **`ViSearchTree`** (`open_dir`/`next_page`/`open_file`/`read_chunk`/`file_state`/`close(Handle::{Dir,File})`), path-addressed so S7-3 can implement H1 over the S7-2 client; `open_dir` on a file returns `io::ErrorKind::NotADirectory`. The walker is pull-based: `Walker::next` yields `File{path, explicit}`/`MountSkipped`/`Error`/`Cancelled`, and `Walker::search_file` runs preflight + search + `file_state` checks with the caller's `Searcher`/`Matcher`/`Sink`. Each directory is listed completely and closed before its children are visited, so at most one directory or one file handle is open at any time and none between calls.
- **Tests** (`cargo test -p vigrep --target x86_64-unknown-linux-gnu`): unit 547 passed + 1 ignored (upstream's own `#[ignore]`), integration 21 (matcher 10, gitignore fixtures 5 + 1, differential 5), doctests 7; 0 failures. Upstream `#[test]` census 186: **171 ported, 15 dropped** (gitignore `parse_excludes_file1-5`; dir `explicit_ignore`, `git_exclude`, `exclude_lowest`, `git_info_exclude_in_linked_worktree`, `gitignore_with_jj`, `absolute_parent`, `absolute_parent_anchored`, `absolute_parent_matchers_are_cached_across_roots`, `multi_root_gitignore_order_independent`, `multi_root_custom_ignore_order_independent` — features outside the port; the multi-root ones are re-expressed as walker test `roots_use_only_their_own_subtree_rules`). Adapted expectations: searcher `binary3`/`binary4` reader byte count 262142 (upstream 262146 is an artefact of encoding_rs_io's 3-byte BOM-peek read; confirmed by reproducing 262146 with a 3-byte first read), `uft8_bom_sniffing` (BOM kept), dir `gitignore_no_git`/`stops_at_git_dir` (no repository detection). New: 41 tests (walk 29 over a handle-counting fake tree covering every dossier §9 walker item, io 2, pathutil 2, dir 2, gitignore 1, differential 5).
- **Differential.** `tests/differential/expected.txt`: 172 match/context records over 20 cases (plain, `-i`, `-w`, `-x`, `-F`, multi-pattern, `-C`/`-A`/`-B`, `--hidden`, `--no-ignore`, globs, glob over ignore, `\p{Greek}`, empty pattern, zero-width `\b`, non-UTF-8 byte regex, `-a`, `.`/`$`, 300 KB multi-buffer file, ordered roots), generated once by `rg --json --no-config --sort path --no-ignore-parent --no-ignore-global --no-ignore-exclude --no-require-git --encoding none` (binary `f1ae3ea6…`, version line checked), corpus digest `998026b8…` checked on every run. vigrep reproduces every record (path, line, absolute offset, line bytes, submatches) with the Cellos searcher bounds (16 KiB buffer, 16 KiB + 65,536 heap limit). The live re-run against the binary passes; without the binary it writes an explicit `SKIPPED:` line to the real stderr. Intentional differences asserted separately (and against live rg defaults): whole-file NUL preflight vs rg's per-buffer quit, subtree-only ignore discovery, `.gitignore` without a repository, no BOM sniffing, path dedup of overlapping roots. `--text` and quiet full scan are S7-3 renderer/status concerns, not vigrep.
- **Intentional semantic differences from upstream code:** no BOM sniffing/transcoding; read granularity no longer shaped by encoding_rs_io; ignore files are supplied as bytes (`GitignoreBuilder::add_bytes`, `BufRead::lines` splitting checked against std); every ignore-load error is reported (upstream drops I/O errors, so a non-UTF-8 line silently ended loading); a directory whose ignore file cannot be read, is over a §2.3 limit or changes while read is skipped with an error; no ancestor/global/exclude/`.jj`/`.git` logic (`require_git(false)` semantics); category precedence kept as upstream (`.rgignore` > `.ignore` > `.gitignore`, deepest match per category); `/`-only paths (upstream `cfg(unix)` variants kept unconditionally); `ignore::Error` loses `WithDepth`/`Loop`/types variants; dedup also skips an entry equal to an earlier root (first occurrence wins, extends DV7-3 to later-root-contains-earlier).
- **Bare metal** (`-Z build-std=core,alloc`, `-C relocation-model=pic`, release): `x86_64-unknown-none` exit 0, `targets/x86_64-unknown-cellos.json -Z json-target-spec` exit 0, no warnings; `cargo clippy -p vigrep --target riscv64gc-unknown-none-elf -- -D warnings` clean. Closes §3.5 item 4 for all five modules.
- **Sizes** (record only, dossier §11): `libvigrep.rlib` 3,545,138 B (none) / 3,540,058 B (cellos JSON); dependency rlibs regex-syntax 8,255,506, regex-automata 7,074,728, aho-corasick 2,356,492, memchr 1,464,648, bstr 982,770, log 250,944. Throwaway LTO staticlib probe (`opt-level="s"`, walker + searcher + `RegexMatcher`, x86_64-unknown-none, deleted after measuring): text 451,000 B, data 114,352 B, total 565,352 B.
- **Audit** (`cargo deny check licenses advisories`, workspace, cargo-deny 0.20.2, advisory DB `84d943a6` 2026-10-23): **advisories ok**; licenses FAILED **only** on the pre-existing `ocel-pdf` `AGPL-3.0-or-later` (out of scope); `vigrep` MIT, `bstr 1.13.1` MIT/Apache-2.0 accepted. Closes §3.5 item 5.
- **Not done here (outside the approved file set):** a CI line for `cargo test -p vigrep --target x86_64-unknown-linux-gnu` next to text-engine in `.github/workflows/ci.yml`; the workspace clippy/fmt jobs already cover vigrep.
- **CP2 (user, 2026-10-24): approved.** The CI line above is still not added; it stays outside this slice's file set.
