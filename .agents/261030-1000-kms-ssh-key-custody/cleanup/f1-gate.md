# KSSH cleanup 1 CP1: make the `cellos-sign` F1 gate pass with no waiver (documents only; nothing applied)

Written 2026-10-30 at HEAD `ca4b872e6` (`git status --short` empty). **Documents only.** No source, manifest, lock, script, allowlist, Cargo or commit action was made. Read-only commands run: `git ls-files/ls-tree/grep/log`, `sha256sum` (including `sha256sum -c` of the K8 freeze files), `cargo metadata --offline --locked --no-deps`, `python3 scripts/cellos-sign --check`, `python3 scripts/test_cellos_sign.py` (71 OK), and the same gate against a **scratch copy outside the repository** (`/tmp/f1sim`, `git archive HEAD` of `cells scripts/cellos_sign scripts/unsafe-allowlist.toml rust-toolchain.toml`, own throw-away `.git`) onto which the proposed patches were applied. No `cargo build/test/check`, no network.

Scope: ticket C1F1 item 1 (F1 gate green without the K8 per-run waiver). Intended consumer: Main, then the accountable owner (this is an F1 allowlist addition: the file is the hole, so each entry needs a named approver).

## 0. Result in one screen

`python3 scripts/cellos-sign --check` at HEAD (run this session; F5 `OK`, rustc `f53b654a8882` = pinned `nightly-2026-05-01`) fails F1 with exactly **7** items, matching `k8-rt1-amendment.md` G-9 and `f1-waiver-unsafe-allowlist.diff`:

| # | Layer | Crate | Path | Real `unsafe`? | Proposed action |
|---|---|---|---|---|---|
| 1 | attribute | `app-phase02-session-child` | `cells/tests/phase02-session-child/src/main.rs` | no | add `#![forbid(unsafe_code)]` |
| 2 | attribute | `app-phase02-session-parent` | `cells/tests/phase02-session-parent/src/main.rs` | no | add `#![forbid(unsafe_code)]` |
| 3 | attribute | `app-phase03-filter` | `cells/tests/phase03-filter/src/main.rs` | no | add `#![forbid(unsafe_code)]` |
| 4 | attribute | `app-phase04-terminal-driver` | `cells/tests/phase04-terminal-driver/src/main.rs` | no | add `#![forbid(unsafe_code)]` |
| 5 | attribute | `app-top-load` | `cells/tests/top-load/src/main.rs` | no | add `#![forbid(unsafe_code)]` |
| 6 | token | `service-input` | `cells/services/input/src/session.rs:1360,1365,1371` | **yes, test-only** (`#[cfg(test)] mod scrub_tests`) | move the one test to a new test-only file and allowlist **that file** (variant B, recommended); or allowlist `session.rs` (variant A) |
| 7 | token | `app-cellbox` | `cells/tools/cellbox/src/rg/tests.rs:1244-1259` (`mod heap` `:1222-1277`) | **yes, test-only** (`unsafe impl GlobalAlloc`) | `[[file]]` allowlist entry, class `test-harness` |

Items 1-5 need no allowlist entry and no code change beyond one inner attribute each. Items 6-7 are genuine `unsafe` in `#[cfg(test)]` code that cannot be reworded away (section 2.6, 2.7).

**Two consequences the K8 text did not list, both required by "no waiver"** (sections 3.1, 3.2): (a) the K8 runner `scripts/qemu-phase09-ssh-test.sh` appends its private waiver to the repo allowlist, so the moment the repo allowlist holds `cells/tools/cellbox/src/rg/tests.rs`, `allowlist.load` raises `duplicate [[file]] path` and the kssh signing step fails; (b) the same runner's RT1 preflight asserts F1 lists **exactly the 7 known items** (`:384-387`), which becomes false when the gate is green. The runner therefore changes in the same commit (delete the waiver block, replace the 7-item assertion by "F1 is OK").

Simulation (scratch copy): HEAD -> 7 violations; after patches 1-5 + item-7 entry -> 1 violation (`session.rs`); after variant B or variant A -> `OK:   F1 — 134 crates and 926 files scanned; unsafe confined to 67 allowlisted files` (variant B) / `... 925 files ...` (variant A), `OK: F5`.

## 1. How the gate decides (evidence)

* `scripts/cellos_sign/policy.py:44` `CELL_ROOTS=["cells"]`; `:83-` `check()` scans the **git-tracked** set only (`scan.tracked_sources`, `scan.py:59-`), so a file becomes visible to the gate when it is `git add`ed (matters for the new file of variant B).
* Attribute layer (`policy.py:108-126`): every crate under `cells/` whose root is tracked must match `FORBID_RE` (`scan.py:33-35`: a **line-start** `#![forbid(... unsafe_code ...)]`; `#![cfg_attr(..., forbid(unsafe_code))]` does **not** match) unless the crate name is in a `[[crate]]` entry.
* Token layer (`policy.py:128-149`): any tracked `.rs` under `cells/` with `\bunsafe\b(?!_)` outside comments and string/char literals (`lexer.py`) fails unless its path is a `[[file]]` entry.
* `allowlist.py:110-139` (duplicates raise at `:129`/`:136`): duplicate `[[file]]` path or `[[crate]]` name is a hard `AllowlistError` (exit usage). Stale entries (`review_by` passed or `max_age_days=90`) are only a `NOTE` (`cli.py:25-36`); `--sign` forces `strict` but strict only means "F5 must not be skipped" (`cli.py:65-72,82-90`).
* Why now: `cells/tests/*` became tracked at `2f0bc0659` (2026-10-29) (`k8-cp2-dossier-part1.md` section 5; reproduced there on `git archive` of `2f0bc0659`, `492774edf`, `070f0d0a1`), so these crates were never gated before; `cells/tools/cellbox/src/rg/tests.rs` came with `5043e2cf5`/`2f0bc0659` (`git log -S'unsafe impl GlobalAlloc for Counting'`).
* Invocation: `scripts/cellos-sign` is mode 100644 in the index (like 249 of the 250 files under `scripts/`); executing it directly gives `Permission denied`. Every caller uses `python3 scripts/cellos-sign` (runner `:362`, `:382`, CI job `f1-admission`, `ci.yml:1724-`). Not part of the fix; see D1-8.

## 2. Findings per item

### 2.1-2.5 The five test crates (attribute layer)

Facts, all read from the tree (`cargo metadata --offline --locked --no-deps` confirms each is a workspace member with exactly one `bin` target `src/main.rs` plus `build.rs`):

| Crate | Root file | Existing inner attributes (line) | `.rs` files / LoC |
|---|---|---|---|
| app-phase02-session-child | `cells/tests/phase02-session-child/src/main.rs` | `#![no_std]` 7, `#![no_main]` 8 | 1 / 356 (+3 build.rs) |
| app-phase02-session-parent | `cells/tests/phase02-session-parent/src/main.rs` | `#![no_std]` 5, `#![no_main]` 6 | 1 / 517 |
| app-phase03-filter | `cells/tests/phase03-filter/src/main.rs` | `#![no_std]` 49, `#![no_main]` 50 | 1 / 389 |
| app-phase04-terminal-driver | `cells/tests/phase04-terminal-driver/src/main.rs` | `cfg_attr(not(test),no_std)` 16, `cfg_attr(not(test),no_main)` 17, `cfg_attr(feature="phase09-ssh", allow(dead_code, unused_imports))` 19 | 9 / 8 561 (incl. `ssh/kssh.rs` 1 826) |
| app-top-load | `cells/tests/top-load/src/main.rs` | `cfg_attr(not(test),no_std)` 30, `cfg_attr(not(test),no_main)` 31 | 2 / 586 |

Does the code contain unsafe, i.e. would `forbid` break the build? Checked with `grep -rn "unsafe\|no_mangle\|export_name\|link_section\|extern \"\|static mut\|asm!\|global_asm\|allow(" cells/tests/<crate>/src build.rs`:

* **No** `unsafe` token, `static mut`, `extern "ABI"`, `asm!` in any of the five crates (the gate itself reports no token violation for them; the only `allow(` hits are `allow(dead_code, unused_imports)` at `phase04-terminal-driver/src/main.rs:19` and two `allow(clippy::declare_interior_mutable_const)` at `top-load/src/main.rs:96,98`; neither touches `unsafe_code`, and `forbid` does not conflict with them). No `#[no_mangle]/#[export_name]/#[link_section]/#[used]` is written in these crates.
* The macros they invoke are `ostd::cell_main!` (5), `api::declare_syscalls!` (5), `api::declare_manifest!` (5) and std/alloc macros; no `macro_rules!` is defined in them. The three non-std macros expand `#[no_mangle]` (`libs/ostd/src/entry.rs:42,48`, under `cfg_attr(not(any(test,unix,windows,target_family="wasm")), no_mangle)`) and `#[used] #[link_section = "__ViCell_manifest"/"__ViCell_syscalls"]` (`libs/api/src/abi/manifest_macro.rs:19-20,28-29`, `abi/syscall.rs:949-952`). Those are attributes the `unsafe_code` lint would flag in the crate's own code, but they come from **external** macros, and the lint is not `report_in_external_macro` (this is the exact boundary `policy.py:20-25` documents). Empirical precedent in the same tree: `cells/tools/kms-ssh-admin/src/main.rs:17` (`#![forbid(unsafe_code)]`) with `cell_main!` (`:142`), `declare_manifest!` (`:24`), `declare_syscalls!` (`:37`), and `cells/tools/init/src/main.rs:5` with `cell_main!(extern "C" ...)` (`:71`), `declare_manifest!` (`:24`), `declare_syscalls!` (`:25`); `kms-ssh-admin` was built and signed into the K8 images after the K7 fix, and 73 crate roots (`git grep -l "forbid(unsafe_code)" -- 'cells/**/main.rs' | wc -l`) carry the attribute.
* No `[workspace.lints]`/`[lints]` table exists (`grep` on root `Cargo.toml`, `.cargo/config.toml`, the five `Cargo.toml`), so nothing re-levels `unsafe_code`.
* `build.rs` is a separate crate target (`cell_build::emit_linker_script()`), not covered by the inner attribute and not a gate root (`scan._root_sources` only takes lib/bin paths).

Decision for all five: **add the attribute**; no allowlist entry (an allowlist entry would hide a future unsafe in a fixture the K8 lane executes in a Cell; the code is unsafe-free so the stricter option is free). Placement: immediately after the `no_main` line (after line 17 / 31 for the two `cfg_attr` crates; `FORBID_RE` needs the attribute at the start of its own line). It is unconditional (not under `cfg_attr`), so `cargo test` host builds of `phase04-terminal-driver` and `top-load` (`#[cfg(test)] mod tests` at `records.rs:357`, `command.rs:148`) are also forbid-clean (no unsafe there either).

### 2.6 `cells/services/input/src/session.rs` (token layer, crate `service-input`)

* The three tokens are in one test: `session.rs:1352-1372` `fn staging_and_event_buffers_are_zeroed_but_keep_their_allocation_until_dropped` inside `#[cfg(test)] mod scrub_tests` (`:1239`); `unsafe { core::slice::from_raw_parts(pointer, capacity) }` at `:1360,1365,1371` reads the whole allocation behind a cleared `Vec` after `wipe()`. The `// SAFETY` comment (`:1359`) is right: `wipe` is `ostd::session::zeroize_vec` (`libs/ostd/src/session.rs:349-352`, zeroes `capacity()` bytes through `zeroize_raw`, then `clear()`), so every byte read was written; the allocation is still owned by the `Vec`. `ostd` carries the same pattern in its own test (`libs/ostd/src/session.rs:520-530`), and `libs/*` is outside `CELL_ROOTS`.
* Not reworded: the keyword is real code, not prose. Not removable without weakening: reading bytes behind `len` of a `Vec` has no safe API; dropping the readback would stop checking that `Secret/Event/Paste::wipe` really delegate to `zeroize_vec` (the ostd test only covers `zeroize_vec` itself). The crate already has a `[[crate]]` entry (`unsafe-allowlist.toml:637-642`, class `driver-mmio`) and `virtio_device.rs` is `[[file]]`-listed (`:299-304`); `main.rs:6-7` states "All other submodules are unsafe-free". **Whole-file allowlisting `session.rs` would make the 1 373-line secret-bearing session code, the largest non-driver file of the crate, silently free to gain production `unsafe`.** Hence variant B.
* Variant B (recommended): move that one test verbatim (dedented) to `cells/services/input/src/session_zeroize_tests.rs` (new; same `#[path]` pattern as `session_tests.rs` at `session.rs:19-21`), wire it with a 3-line `#[cfg(test)] #[path = "session_zeroize_tests.rs"] mod zeroize_tests;`, and allowlist only the new file (`test-harness`). The new module redefines the three tiny helpers it needs (`SECRET`, `who()`, `operation()`: `scrub_tests`'s are private to that module) and reaches `Secret/Event/Paste` through `use super::*` exactly as `scrub_tests` does. `rustfmt --check --edition 2021` parses the new file with no error; **compilation is not verified** (needs the cargo test run in section 6).
* Variant A: one `[[file]]` entry for `cells/services/input/src/session.rs`, no source edit, zero compile risk, wider hole. Verified in the scratch copy: gate green.

### 2.7 `cells/tools/cellbox/src/rg/tests.rs` (token layer, crate `app-cellbox`)

* `rg.rs:25-26` declares `#[cfg(test)] mod tests;`, so the file is compiled only for host tests. Tokens at `tests.rs:1244,1245,1246,1253,1254,1258,1259` (`unsafe impl GlobalAlloc for Counting`, three `unsafe fn`, three `unsafe { System.* }`) inside `mod heap` (`:1222-1277`), plus `#[global_allocator] static ALLOCATOR` (`:1267-1268`), measuring per-thread peak heap against `HEAP_TARGET = HEAP_ARENA_BYTES - 1 MiB` (`:1295`). Line `:534` ("unsafe character for {:?}") is a string and is not a token (lexer strips literals).
* A counting allocator cannot be written without `unsafe` (`GlobalAlloc` is an unsafe trait), and the `// SAFETY` comment at `:1243` is accurate (every call is forwarded unchanged to `System`; only a thread-local counter changes). The heap-peak tests are the D76-3 M2 arena evidence (`tests.rs:1295-`), not something to drop.
* `app-cellbox` has a `[[crate]]` entry (`unsafe-allowlist.toml:743-750`, class `temporary-audit`, `review_by = "2026-10-28"`, now overdue; its reason cites "the [[file]] entries of class temporary-audit" and **no such `[[file]]` entry for `cells/tools/cellbox/**` exists**). The crate root `main.rs:1-2` carries no `forbid`, and production code has no unsafe: `git grep` over `cells/tools/cellbox` finds `unsafe` only in `rg/tests.rs` (plus prose in `shell_state.rs:4,8`, `config_client.rs:19`, comments/lexer-stripped). So only the file needs an entry; the crate entry is left as is (an unconditional `forbid` cannot be added while `tests.rs` holds a real `unsafe impl`; a `cfg_attr(not(test), forbid(...))` is invisible to `FORBID_RE`; see D1-6).
* Decision: `[[file]]` entry, class `test-harness` (existing class: `tier2-exploit`, `tier2-grant-*`, `unsafe-allowlist.toml:32-58`), approver a named person (header `:17-23`), dated 2026-10-30, no `review_by` (permanent test-only; the 90-day `max_age_days` NOTE will surface it for re-review, as for the other test-harness rows).

## 3. Consequences beyond the seven items

### 3.1 K8 runner waiver collides with the repo allowlist (blocking if omitted)

`scripts/qemu-phase09-ssh-test.sh:329-360` (kssh mode) builds `$W/unsafe-allowlist.toml = repo allowlist + [[crate]] x5 + [[file]] session.rs + [[file]] rg/tests.rs` and signs with `--allowlist`. After the fix the repo allowlist contains `rg/tests.rs` (and, variant A, `session.rs`) -> `allowlist.load` raises `duplicate [[file]] path` (`allowlist.py:129`) -> exit usage -> signing refused. The five `[[crate]]` waivers would only become "unused" notes. **The waiver block must be deleted in the same commit.** The unified diff is in section 4.2 (verified in scratch: `bash -n` OK; the `test_phase10_candidate_import.py` ordering `import < scripts/cellos-sign < verify < -p cellos-kernel < boot` still holds, positions 17857 < 19912 < 26700 < 27476 < 31693 recomputed on the patched text).

### 3.2 RT1 preflight asserts the broken tree (`:384-387`)

`F1_ITEMS` must equal the seven names, else `pfail "F1 lists items other than the 7 known ones"`. With a green gate the list is empty and the assertion fails. The patch replaces it with `grep -q '^OK:   F1'` (the `--check` run is not `--quiet`, `cli.py:50-52` prints that line; the preceding F5 assertion uses the same idiom).

### 3.3 K8 freeze pins (`frozen-sources*.sha256`)

`reports/k8-evidence/frozen-sources-rt1.sha256` (78 lines; `sha256sum -c` run from the repo root at HEAD: **all OK**) pins `cells/tests/phase04-terminal-driver/src/main.rs` (line 47), `scripts/qemu-phase09-ssh-test.sh` (44), `scripts/kssh_judge.py` (54), `scripts/test_kssh_judge.py` (55) and `scripts/unsafe-allowlist.toml` (62); `frozen-sources.sha256` (T0, historical, not checked in persist mode per `qemu-phase09-ssh-test.sh:58`) pins the same main.rs (47). Editing `main.rs` of the terminal driver, the runner and the allowlist therefore makes `sha256sum -c frozen-sources-rt1.sha256` (runner `:206`) fail: **any future `--kssh --persist` or b0/calibrate run refuses until a new freeze file is cut.** Consistent with the program rules: every K8 set is `RUN_CONSUMED` (`k8-evidence/RUN_CONSUMED-k8-rt0,rt0f,rt1`), "one evidence run per set, no rerun" (`k8-rt1-amendment.md` section on user decisions), the evidence remains valid as recorded at its own SHA-256s; the freeze files are historical evidence and must **not** be rewritten. Any later K8 rerun is a new CP1 with a new freeze. Recorded as a consequence, not blocked by it (D1-7).

### 3.4 Other pins checked and found unaffected

`git grep` over `scripts .github tests docs kernel libs` for the seven paths and the allowlist: no path pin or digest of the five `main.rs`, `session.rs`, `rg/tests.rs` outside the K8 freeze files. `unsafe-allowlist.toml` is bound by hash inside **historical** `build/*/inputs.json` and manifest evidence (`build/p01-*/inputs.json:5766-5772` `91984e5e...`, g1 manifests) which are frozen records of past runs, not live checks. `docs/specs/18c-publisher-provenance-envelope.md:52` lists every Cell crate root, all tracked `cells/**/*.rs` and the allowlist as `tracked_inputs` of the publisher provenance envelope: their digests change by design on any rebuild, no stored expectation exists in the tree [INFERENCE: grep of the tree only; I did not read every provenance consumer]. `scripts/kssh_judge.py:1446` lists `f1-waiver-unsafe-allowlist.diff` in `ALLOW_T1` as an *optional* archive name and `test_kssh_judge.py:2404,2420` creates it in a synthetic archive: both remain valid with no waiver (no file is produced; the name is simply never present) and are **left unchanged**.
Panic line numbers in the five fixtures shift by +1 after the insertion; `git grep -E "(phase02-session-(child|parent)|phase03-filter|phase04-terminal-driver|top-load)[a-z/_.-]*:[0-9]+"` over `scripts kernel libs cells` (non-`.md`) finds no runner expecting a source line.

## 4. Exact fix

One commit, all hunks together (the gate is green only with all of them: the entries without the forbid lines, or the new test file without its allowlist entry, leave it red).

### 4.1 Source and allowlist (variant B). Produced by applying the steps to the scratch copy; `git diff` of that copy:

```diff
diff --git a/cells/services/input/src/session.rs b/cells/services/input/src/session.rs
index a777843..4e73bac 100644
--- a/cells/services/input/src/session.rs
+++ b/cells/services/input/src/session.rs
@@ -19,6 +19,9 @@ use frontend_tests::fake as sys;
 #[cfg(test)]
 #[path = "session_tests.rs"]
 mod frontend_tests;
+#[cfg(test)]
+#[path = "session_zeroize_tests.rs"]
+mod zeroize_tests;
 #[path = "session_trust.rs"]
 mod trust;
 use ostd::session::{zeroize_deque, zeroize_deque_front, zeroize_vec, Scrub};
@@ -1349,25 +1352,4 @@ mod scrub_tests {
         sessions.drop_outcomes(operation());
         assert!(sessions.outcomes.iter().all(|o| o.request.is_empty() && o.result.is_empty() && !o.pending));
     }
-
-    #[test]
-    fn staging_and_event_buffers_are_zeroed_but_keep_their_allocation_until_dropped() {
-        let mut secret = Secret(Vec::with_capacity(64)); secret.extend_from_slice(SECRET);
-        let (pointer, capacity) = (secret.as_ptr(), secret.capacity());
-        secret.wipe();
-        assert!(secret.is_empty());
-        // SAFETY: the allocation is still owned by `secret`; every byte was just overwritten.
-        assert!(unsafe { core::slice::from_raw_parts(pointer, capacity) }.iter().all(|b| *b == 0));
-
-        let mut event = Event { sequence: 1, bytes: SECRET.to_vec(), sent: 0 };
-        let (pointer, capacity) = (event.bytes.as_ptr(), event.bytes.capacity());
-        event.wipe();
-        assert!(unsafe { core::slice::from_raw_parts(pointer, capacity) }.iter().all(|b| *b == 0));
-
-        let mut paste = Paste { reference: operation(), frontend: who(), attachment: operation(), epoch: 1, expires: 0, total: SECRET.len(), bytes: SECRET.to_vec() };
-        let (pointer, capacity) = (paste.bytes.as_ptr(), paste.bytes.capacity());
-        paste.wipe();
-        assert!(paste.bytes.is_empty());
-        assert!(unsafe { core::slice::from_raw_parts(pointer, capacity) }.iter().all(|b| *b == 0));
-    }
 }
diff --git a/cells/services/input/src/session_zeroize_tests.rs b/cells/services/input/src/session_zeroize_tests.rs
new file mode 100644
index 0000000..b6deb22
--- /dev/null
+++ b/cells/services/input/src/session_zeroize_tests.rs
@@ -0,0 +1,35 @@
+//! Host-only read-back of wiped heap buffers: the one test in this crate that needs `unsafe`.
+//!
+//! `Secret`/`Event`/`Paste::wipe` delegate to `ostd::session::zeroize_vec`, which zeroes the
+//! vector's whole allocation (spare capacity included) before truncating it. Observing that
+//! the bytes behind a cleared `Vec` are zero requires reading the allocation through a raw
+//! pointer, so the test lives in its own file: this is the only file of the crate (apart from
+//! the driver `virtio_device.rs`) that `scripts/unsafe-allowlist.toml` lets contain `unsafe`,
+//! which keeps `session.rs` itself under the token rule.
+
+use super::*;
+
+const SECRET: &[u8] = b"hunter2-correct-horse";
+fn who() -> Principal { Principal { cell_id: 7, generation: 1 } }
+fn operation() -> Ref { Ref { id: 9, generation: 1 } }
+
+#[test]
+fn staging_and_event_buffers_are_zeroed_but_keep_their_allocation_until_dropped() {
+    let mut secret = Secret(Vec::with_capacity(64)); secret.extend_from_slice(SECRET);
+    let (pointer, capacity) = (secret.as_ptr(), secret.capacity());
+    secret.wipe();
+    assert!(secret.is_empty());
+    // SAFETY: the allocation is still owned by `secret`; every byte was just overwritten.
+    assert!(unsafe { core::slice::from_raw_parts(pointer, capacity) }.iter().all(|b| *b == 0));
+
+    let mut event = Event { sequence: 1, bytes: SECRET.to_vec(), sent: 0 };
+    let (pointer, capacity) = (event.bytes.as_ptr(), event.bytes.capacity());
+    event.wipe();
+    assert!(unsafe { core::slice::from_raw_parts(pointer, capacity) }.iter().all(|b| *b == 0));
+
+    let mut paste = Paste { reference: operation(), frontend: who(), attachment: operation(), epoch: 1, expires: 0, total: SECRET.len(), bytes: SECRET.to_vec() };
+    let (pointer, capacity) = (paste.bytes.as_ptr(), paste.bytes.capacity());
+    paste.wipe();
+    assert!(paste.bytes.is_empty());
+    assert!(unsafe { core::slice::from_raw_parts(pointer, capacity) }.iter().all(|b| *b == 0));
+}
diff --git a/cells/tests/phase02-session-child/src/main.rs b/cells/tests/phase02-session-child/src/main.rs
index 60ecf3d..e730835 100644
--- a/cells/tests/phase02-session-child/src/main.rs
+++ b/cells/tests/phase02-session-child/src/main.rs
@@ -6,6 +6,7 @@
 
 #![no_std]
 #![no_main]
+#![forbid(unsafe_code)]
 
 extern crate alloc;
 extern crate ostd;
diff --git a/cells/tests/phase02-session-parent/src/main.rs b/cells/tests/phase02-session-parent/src/main.rs
index 3dab469..3155864 100644
--- a/cells/tests/phase02-session-parent/src/main.rs
+++ b/cells/tests/phase02-session-parent/src/main.rs
@@ -4,6 +4,7 @@
 
 #![no_std]
 #![no_main]
+#![forbid(unsafe_code)]
 
 extern crate alloc;
 extern crate ostd;
diff --git a/cells/tests/phase03-filter/src/main.rs b/cells/tests/phase03-filter/src/main.rs
index cde2569..977bc9f 100644
--- a/cells/tests/phase03-filter/src/main.rs
+++ b/cells/tests/phase03-filter/src/main.rs
@@ -48,6 +48,7 @@
 
 #![no_std]
 #![no_main]
+#![forbid(unsafe_code)]
 
 extern crate alloc;
 extern crate ostd;
diff --git a/cells/tests/phase04-terminal-driver/src/main.rs b/cells/tests/phase04-terminal-driver/src/main.rs
index e6730b2..03d842e 100644
--- a/cells/tests/phase04-terminal-driver/src/main.rs
+++ b/cells/tests/phase04-terminal-driver/src/main.rs
@@ -15,6 +15,7 @@
 
 #![cfg_attr(not(test), no_std)]
 #![cfg_attr(not(test), no_main)]
+#![forbid(unsafe_code)]
 // The Phase 09 lane runs only the ssh checks; the Phase04 suite is compiled but unused.
 #![cfg_attr(feature = "phase09-ssh", allow(dead_code, unused_imports))]
 
diff --git a/cells/tests/top-load/src/main.rs b/cells/tests/top-load/src/main.rs
index d768e01..43e0818 100644
--- a/cells/tests/top-load/src/main.rs
+++ b/cells/tests/top-load/src/main.rs
@@ -29,6 +29,7 @@
 
 #![cfg_attr(not(test), no_std)]
 #![cfg_attr(not(test), no_main)]
+#![forbid(unsafe_code)]
 
 extern crate alloc;
 extern crate ostd;
diff --git a/scripts/unsafe-allowlist.toml b/scripts/unsafe-allowlist.toml
index 01fce90..37658e3 100644
--- a/scripts/unsafe-allowlist.toml
+++ b/scripts/unsafe-allowlist.toml
@@ -787,3 +787,17 @@ class = "c-ffi"
 reason = "Owns allowlisted unsafe under cells/tests/c-spawn-child/ — see the [[file]] entry for the justification."
 approver = "dmin"
 date = "2026-09-25"
+
+[[file]]
+path = "cells/tools/cellbox/src/rg/tests.rs"
+class = "test-harness"
+reason = "Host-only (`#[cfg(test)] mod tests;`, rg.rs:25-26) heap-peak measurement for the D76-3 M2 `rg` arena target: `unsafe impl GlobalAlloc for Counting` forwards every call unchanged to `System` and only counts live bytes per thread. `GlobalAlloc` is an unsafe trait, so the counting allocator cannot be written without the keyword; the file holds no other unsafe and is never compiled into a Cell image."
+approver = "dmin"
+date = "2026-10-30"
+
+[[file]]
+path = "cells/services/input/src/session_zeroize_tests.rs"
+class = "test-harness"
+reason = "Host-only (`#[cfg(test)]`, declared in session.rs) read-back of wiped heap buffers: after `Secret`/`Event`/`Paste::wipe` the test reads the whole retained allocation through a raw pointer to prove every byte, spare capacity included, is zero. Observing memory behind a cleared Vec is not possible without `unsafe`. The file holds exactly that one test and is never compiled into a Cell image; it was split out of session.rs so the secret-handling session code stays under the token rule."
+approver = "dmin"
+date = "2026-10-30"
```

Notes: the two `reason` strings are the full text in the allowlist (above wrapped by the 200-column display of this document; they are one TOML line each). The new file ends with a single newline.

### 4.2 K8 runner (`scripts/qemu-phase09-ssh-test.sh`, `git diff` of the scratch copy)

```diff
diff --git a/scripts/qemu-phase09-ssh-test.sh b/scripts/qemu-phase09-ssh-test.sh
index 20d4b0b..da937ce 100755
--- a/scripts/qemu-phase09-ssh-test.sh
+++ b/scripts/qemu-phase09-ssh-test.sh
@@ -326,41 +326,9 @@ if [[ "$KSSH" == 1 ]]; then
 fi
 
 echo "==> Signing with the private key"
-SIGN_EXTRA=()
-if [[ "$KSSH" == 1 ]]; then
-    # F1 is not satisfied by the tracked tree at HEAD (cells/tests/* became tracked in 2f0bc0659; the
-    # tree-wide gate refuses to sign, L5/L5f included). Decision recorded in k8-cp1-approval.md: K8
-    # images are signed under a PRIVATE per-run allowlist = the repo allowlist (verbatim) + explicit
-    # entries for exactly the items below. The repo allowlist and the signing code are untouched, the
-    # diff is saved in the evidence and the dossier states the waiver.
-    ALLOWLIST="$W/unsafe-allowlist.toml"
-    "$PYTHON_BIN" - "$ROOT/scripts/unsafe-allowlist.toml" "$ALLOWLIST" "$EVIDENCE/unsafe-allowlist.diff" <<'PY'
-import difflib, sys
-from pathlib import Path
-src, dst, diff = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
-base = src.read_text()
-CRATES = ["app-phase02-session-child", "app-phase02-session-parent", "app-phase03-filter",
-          "app-phase04-terminal-driver", "app-top-load"]
-FILES = ["cells/services/input/src/session.rs", "cells/tools/cellbox/src/rg/tests.rs"]
-extra = "\n# ---- K8 per-run waiver (private, not in the repository) ----\n"
-for name in CRATES:
-    extra += ('\n[[crate]]\nname = "%s"\nclass = "k8-lane-waiver"\n'
-              'reason = "K8 private per-run waiver: crate tracked since 2f0bc0659 without forbid(unsafe_code) and without an allowlist entry; not in the repository allowlist."\n'
-              'approver = "K8 decision, k8-cp1-approval.md"\ndate = "2026-10-30"\nreview_by = "2026-10-31"\ntracking = "k8-cp1-approval.md"\n' % name)
-for path in FILES:
-    extra += ('\n[[file]]\npath = "%s"\nclass = "k8-lane-waiver"\n'
-              'reason = "K8 private per-run waiver: tracked file contains the unsafe keyword and has no allowlist entry in the repository."\n'
-              'approver = "K8 decision, k8-cp1-approval.md"\ndate = "2026-10-30"\nreview_by = "2026-10-31"\ntracking = "k8-cp1-approval.md"\n' % path)
-dst.write_text(base + extra)
-diff.write_text("".join(difflib.unified_diff(base.splitlines(True), (base + extra).splitlines(True), "repo/unsafe-allowlist.toml", "run/unsafe-allowlist.toml")))
-print("private F1 allowlist: %d crate and %d file waivers" % (len(CRATES), len(FILES)))
-PY
-    SIGN_EXTRA=(--allowlist "$ALLOWLIST")
-    [[ "$PERSIST" != 1 ]] || mv "$EVIDENCE/unsafe-allowlist.diff" "$EVIDENCE/f1-waiver-unsafe-allowlist.diff"
-fi
 SEED_HEX="$(cat "$KEYS/seed.hex")"
 CELLBOX_PRIVATE_FIXTURE_SIGNING=1 CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 "$PYTHON_BIN" scripts/cellos-sign --quiet --objcopy "${OBJCOPY:-objcopy}" \
-    "${SIGN_EXTRA[@]}" --seed-hex "$SEED_HEX" --sign "${CELL_BINARIES[@]}" "$REL/app-init"
+    --seed-hex "$SEED_HEX" --sign "${CELL_BINARIES[@]}" "$REL/app-init"
 unset SEED_HEX
 if [[ "$PERSIST" == 1 ]]; then
     # RT1 preflight items 15-17 (k8-rt1-amendment.md 12.2), on the exact signed artifacts the image receives.
@@ -381,10 +349,8 @@ if [[ "$PERSIST" == 1 ]]; then
     pf "exclusion checker on the signed service-kms: persist 0, custody 1, none 1 (checker.txt)"
     SIGN_CHECK="$("$PYTHON_BIN" scripts/cellos-sign --check 2>&1 || true)"
     printf '%s\n' "$SIGN_CHECK" | grep -q '^OK:   F5' || pfail "cellos-sign --check: F5 is not OK"
-    F1_ITEMS="$(printf '%s\n' "$SIGN_CHECK" | grep -E '^  \[(attribute|token)\] ' | sed -E 's/^  \[(attribute|token)\] ([^ ]+) ::.*/\2/' | sort | tr '\n' ' ')"
-    [[ "$F1_ITEMS" == "app-cellbox app-phase02-session-child app-phase02-session-parent app-phase03-filter app-phase04-terminal-driver app-top-load service-input " ]] \
-        || pfail "F1 lists items other than the 7 known ones: $F1_ITEMS"
-    pf "F1: exactly the 7 known items (private per-run waiver applies), F5 OK"
+    printf '%s\n' "$SIGN_CHECK" | grep -q '^OK:   F1' || pfail "cellos-sign --check: F1 is not OK (no waiver exists)"
+    pf "F1 OK, F5 OK (repository allowlist, no per-run waiver)"
 fi
 
 FAT_ARGS=()
```

### 4.3 Variant A for `session.rs` (instead of the new file and the `session.rs` edit)

Skip the `session.rs` hunk, the new file and its allowlist entry; append instead:

```toml
[[file]]
path = "cells/services/input/src/session.rs"
class = "test-harness"
reason = "Host-only test `staging_and_event_buffers_are_zeroed_but_keep_their_allocation_until_dropped` (`#[cfg(test)] mod scrub_tests`, session.rs:1352-1372) reads the whole retained allocation of a wiped Secret/Event/Paste through `core::slice::from_raw_parts` to prove every byte, spare capacity included, is zero; that cannot be observed without `unsafe`. The `unsafe` is confined to that one test; the rest of the file is unsafe-free and is not compiled into a Cell with it."
approver = "dmin"
date = "2026-10-30"
```

Verified in scratch: `OK:   F1 — 134 crates and 925 files scanned`. Cost: the gate no longer guards the non-test code of `session.rs` (section 2.6).

### 4.4 Documentation (same commit or the record commit)

* `CHANGELOG.md` `[Unreleased]` / `### Fixed` (line 434): one entry in the style of line 1047 (allowlist/F1 repairs): "F1 admission gate green on the tracked tree: `#![forbid(unsafe_code)]` on the five `cells/tests/phase0{2,3,4}*`/`top-load` crate roots (tracked since `2f0bc0659`, no unsafe in them), `[[file]]` `test-harness` entries for the host-only `rg/tests.rs` counting `GlobalAlloc` and the moved `session_zeroize_tests.rs` zeroization readback; the K8 runner's private per-run waiver and its 7-item preflight assertion are removed."
* `.agents/261030-1000-kms-ssh-key-custody/plan.md:84` ("Open outside the program: pre-existing red F1 gate ...") and `.agents/plan-portfolio.md:128` (KSSH implementation state): add that the F1 gate is green on the repository allowlist with no waiver, naming the commit; the K8 dossiers/approvals and `f1-waiver-unsafe-allowlist.diff` stay as historical evidence (do not edit).
* `k8-cp1-approval.md` F1 amendment stays as recorded (it said the repository allowlist and crate roots stay unchanged *for K8*); this cleanup is the separate owner decision that supersedes it going forward. Record it as an amendment/approval file `cleanup/f1-gate-approval.md`.

## 5. Exact file closure (current SHA-256 at HEAD `ca4b872e6`; "after" = the scratch result, for post-edit comparison)

| Path | Action | SHA-256 now | SHA-256 after (scratch) |
|---|---|---|---|
| `cells/tests/phase02-session-child/src/main.rs` | +1 line | `d94485f0c9a63307163b721533d2e59a35db2757d5b409dc6862eeeab574f54b` | `0edb2b963b1654c1abc620fd96ba153b0c599bb375709f18c5f34b59bfbb8d26` |
| `cells/tests/phase02-session-parent/src/main.rs` | +1 line | `319f8d46fe3e9b3af3122830eb8c836811b6cc1726714d6c59bab06a8cebfb9a` | `78cc3a5afc950804620212de19739a619f58c79e7e2c0d5707017b06e1a70054` |
| `cells/tests/phase03-filter/src/main.rs` | +1 line | `2d812d2c7fc32f06d5271e88e57afdf9c89acca3568a3eb8f1eb5db5ffd1b04a` | `1b9036536f52a9fefc89d91d3645be524555a6c4552f875c60485091091a05d7` |
| `cells/tests/phase04-terminal-driver/src/main.rs` | +1 line | `fc24450c779f627af25a878f52e84cb508a89dee94a7cb7d87e2485080c9cfc0` | `b661277daadeadd43904302a31bd9546028b6717bb2bbecd03f03574bdf16ca3` |
| `cells/tests/top-load/src/main.rs` | +1 line | `14731931916787954b11cf63d88b1e4069a574aa4593332707e333c2939123de` | `24d3205b5c1a874f980f8aa32844c199df6d2dad674993b578efff6fe90ff0cf` |
| `cells/services/input/src/session.rs` | variant B: -21/+3 | `00e4314c25440da5bfe5c5f073d94725465d625f005aea32f1c6ef2cde953969` | `2bdd60e54f85648425dac774247d20e9d975c04fd5b9cf609f8d252ae87e632c` |
| `cells/services/input/src/session_zeroize_tests.rs` | variant B: new, 35 lines | absent | `e74a75a6f0630ea8a80b720ce587a7da387357ea056ee3582bb16d0446236496` |
| `scripts/unsafe-allowlist.toml` | +2 `[[file]]` (variant B) / +2 (variant A: rg/tests.rs, session.rs) | `f759ae448be936279980105c52e45e5cea19b23474ef1e1851c9b5a2d744ab16` | `2aa672eafbab5d9a520758689c042e706fa223600b44c293cd0f64cfe75e1c9f` |
| `scripts/qemu-phase09-ssh-test.sh` (mode 100755) | delete waiver block, F1 assertion | `89b7d76062e0fd20741acde18e97e0bc764013b05362a824d060911af9bd0fc7` | `620b735d404037b79c883fde5272e0f1c3cd11c4432ac786003f031ccc071dc2` |
| `CHANGELOG.md` | +1 entry | `5c2022ac045b81cae884e8204994d0ffd4f2b116633d4f1181d89ccb87dceeb1` | n/a |
| `.agents/261030-1000-kms-ssh-key-custody/plan.md` | status line 84 | `e73e232b60784f53647d83e12e09b09f2428bb39b72b738e490863203907796a` | n/a |
| `.agents/plan-portfolio.md` | status line 128 | `baa97c210d3320d3c40ecfeea0316face1d592eb870ccbc0c31a9e90fa801f1b` | n/a |

Read-only bound (must stay byte-equal): `scripts/cellos_sign/{policy,scan,allowlist,cli,lexer}.py` (`policy.py` `5566dec5578d5c964c616c025cb49818b71f49be4859b186d18ea63d10dc1083`, `scan.py` `5b93ac6ee338e12cfad6ddf6ca71d3d54bfb89fe54b12cece8fbdfdee19e5b20`, `cli.py` `ed92c59aff7694458f089e669376acec223099331547c94b2223aa320b126efa`), `scripts/test_cellos_sign.py` `a08c603b969d9a183f097605c902443c8f6fd2f04fbd663b71cb6ef3e5672183`, `scripts/kssh_judge.py` `8f2e06e0eb506040d306cb45c7204178eaeffb287b1994890e8a4d8b2d0dd732`, `scripts/test_kssh_judge.py` `6576a4bf39749f7c0f3979c9f56340b918aea974310e0369eafd1db752396c65`, `scripts/test_phase10_candidate_import.py` `e0daf6a9deee172d04c1561724e88fc2fb2824532dd5b5cd8ad1dc8633347b91`, `cells/tools/cellbox/src/rg/tests.rs` `c1c02941e93a81863ceaf6efdb13176494454fc0612a3764f571fb2bca3bf31e` (not edited), `cells/tools/cellbox/src/rg.rs` `9d0f112be1de8095372a998405c4351eb520914ddfbe7e474ec5bde15a033fec`, `cells/services/input/src/main.rs` `76bd2d3528740cc80a443706ae9dca19121ccd2d6b48237769ace1a2d811ca77`, K8 freeze files `frozen-sources-rt1.sha256` `c56d06b0a55eefe9638b44b5a22113688544a92bdc3ad5ff4993145608b269f1`, `frozen-sources.sha256` `8ca177ef78f78ec320ae76b9d59a482a5454c9632716d28afcb82f86c81a0dcc`. Not touched: `Cargo.toml`, `Cargo.lock`, any `Cargo.toml` of the crates (no feature or dependency change), `libs/**`, `kernel/**`, `build/**`, `target/**`, `scripts/cellos-sign` mode.

## 6. CELL-CONTRACT-0024 lock rows, Law-1, overlapping plans

* **Lock file** (`.agents/cell-contract-0024-locks.md`, SHA-256 `258e920e...1499`, 314 lines): `grep -n -F` for each of the nine source/script paths and for the five crate directories returns **no row**. The file is plan-scoped, not path-scoped, except the bounded-slice paragraphs (`:113-123`: Cellbox Phase02, KSSH K2 CP2, K3, K7, K4b); none of them enumerates a path of this closure (K3's twelve paths and K5/K4b's `cells/services/kms/**`, K7's `cells/tools/kms-ssh-admin/**` are different trees; the Cellbox Phase02 approval `261020-1750-cellbox-cli/phase-02-cp1-approval.md` names none of the five fixtures, `session.rs` or `rg/tests.rs` by path). `261020-1750-cellbox-cli` is not in the lock's `blocks:` list (the lock only carves it out as a new controlling slice, `:113`), and the portfolio's `260731-1653-wx-post-reloc-and-f1-signing` row is "completed -> unchanged history/parked; no new lock" (`locks:186`). Result: no locked-scope exception is needed for this closure. [INFERENCE: not exhaustive. Scope rows are prose, so a reviewer could still read the `input` service as part of a locked unfinished scope; the edit to `session.rs` is test-only relocation with no behaviour or contract change, which should be stated in the approval.]
* **Law-1** (public interface/ABI two-confirmation rule, ADR 0013 `:63,175`; `libs/api`, `libs/types` objects): none of the closure paths is a Law-1 object: no `libs/api`, `libs/types`, syscall, manifest, IPC schema or Cargo.lock change. No confirmation text is needed. The `forbid` attributes and test relocation change no interface.
* **`plan-portfolio.md`** (`baa97c21...`): no line names these paths (grep). Overlapping owners, by subject: (1) `261020-1750-cellbox-cli` Cellbox Phases 02/03/04/05/06/07 own the five fixtures (`phase-02-cp2-dossier.md`, `phase-03-*`, `phase-04-*`, `phase-05-cp2-dossier.md`, `phase-06-cell-top.md`/`phase-06-s5-proposal.md`) and `rg/tests.rs` (`phase-07-ripgrep-search.md`, D76-3); closed phases, private evidence, their file lists are historical; this change touches no behaviour. (2) KSSH `261030-1000-kms-ssh-key-custody` K8 owns `phase04-terminal-driver/src/{main.rs,ssh.rs,ssh/kssh.rs}` and the runner/`unsafe-allowlist.toml` freeze (section 3.3); all K8 sets are `RUN_CONSUMED`, plan state "KSSH complete at DEV_POSTURE". (3) `260727-2101-midori-lessons-cellos` phase 11 (`phase-11-cellos-sign-f1.md`) owns the F1 mechanism and the allowlist's rules (approver naming, `review_by`); this is an ordinary allowlist addition under it. (4) `260927-0739-kernel-architecture-repair`, `260821-1700-tier1-baseline-admission`, `260927-1100-c2c-anywhere-tier-aware` and `.agents/TODO.md` mention `unsafe-allowlist`/`cellos-sign`; none lists these entries. [INFERENCE: grep-based, not exhaustive.]
* Peer cleanups: C3CI/C4CQ items were not visible to me; if either edits `scripts/qemu-phase09-ssh-test.sh`, `scripts/unsafe-allowlist.toml` or `CHANGELOG.md`, sequence the commits (allowlist/CHANGELOG are append-style, the runner hunks here are at `:329-360` and `:384-387`).

## 7. Tests and verification (for the implementing agent, after the edit)

1. `python3 scripts/cellos-sign --check` -> exit 0, prints `OK:   F5` and `OK:   F1 — 134 crates and 926 files scanned; unsafe confined to 67 allowlisted files` (variant B; 925 files for variant A; counts as seen in the scratch copy, adjust if HEAD moves). The new file must be `git add`ed first (the gate scans tracked files only). The `NOTE:` list of overdue entries (including `app-cellbox`, `review_by 2026-10-28`) remains and is non-fatal.
2. `python3 scripts/test_cellos_sign.py` (71 tests, run at HEAD: OK) and `python3 scripts/test_phase10_candidate_import.py` (runner wiring tests), `python3 scripts/test_kssh_judge.py`, `bash -n scripts/qemu-phase09-ssh-test.sh`.
3. Compile/test of every crate whose root or tests changed (host tests need the repo's usual host target; the five fixtures are `no_std`/`no_main` Cells, so their real check is the cross build used by the lanes): `cargo test -p service-input -p app-cellbox --target x86_64-unknown-linux-gnu` (the CI line `ci.yml:1691`; must show `staging_and_event_buffers_are_zeroed_but_keep_their_allocation_until_dropped` still runs, now under `session::zeroize_tests::`, and the `rg` heap-peak tests still pass); `cargo build --release --target x86_64-unknown-none -p app-phase02-session-child -p app-phase02-session-parent -p app-phase03-filter -p app-phase04-terminal-driver -p app-top-load` plus `-p app-phase04-terminal-driver --features kssh` and `--features phase09-ssh` with the toolchain environment the lane uses (`qemu-phase09-ssh-test.sh:111-114`); `cargo test -p app-phase04-terminal-driver -p app-top-load` for the `cfg(test)` modules. These confirm `forbid` plus the external-macro expansions compile; they are not run by me.
4. A dry gate demonstration of the negative case: `python3 scripts/test_cellos_sign.py` already covers allowlist/forbid behaviour; no new test is needed (`policy.py`, `scan.py` are unchanged).
5. Optional, K8 lane (not required, no evidence is regenerated): a `--kssh` run is **not** part of this change (section 3.3).
6. Hygiene: `git status --short` shows only the closure paths; `sha256sum` equals section 5 "after" for the nine code/script paths (assuming the patches apply verbatim).

## 8. Risks

| Risk | Level | Mitigation |
|---|---|---|
| `forbid(unsafe_code)` does not compile in a fixture because of an expansion I judged external | low | External-macro precedent in the same tree (`kms-ssh-admin`, `init` and the 73 `main.rs` roots that carry the attribute); compile step in section 7.3. If one fails, fall back to an allowlist `[[crate]]` entry for that crate only (not the waiver). |
| Variant B `session_zeroize_tests.rs` does not compile (`Principal`/`Ref` imports, private field access) | low-medium | Same `use super::*` pattern as `scrub_tests`; only syntax was checked (`rustfmt --check`). Variant A is the zero-compile-risk fallback. |
| Allowlist entries widen the F1 hole | low | Both are `#[cfg(test)]`-only; variant B keeps `session.rs` token-guarded; the 90-day staleness NOTE re-surfaces them. |
| K8 runner left unedited -> kssh signing breaks (duplicate `[[file]]`) and RT1 preflight fails | certain if omitted | Included in the closure (4.2). |
| Edits invalidate K8 freeze pins; a future `--persist`/b0 run refuses | accepted | Evidence sets are `RUN_CONSUMED`; freeze files stay historical; any rerun is a new CP1 with a new freeze (D1-7). |
| New file untracked at gate time | trivial | `git add` before running the gate; single commit. |
| Overdue `app-cellbox` crate entry and ~50 other stale entries keep printing `NOTE` | cosmetic | Not part of this change (D1-5). |
| Edits shift panic line numbers / rebuild fixtures, so any image built before the edit differs from one built after | low | Fixtures are private (`never part of an ordinary image` in their doc comments, e.g. `phase02-session-child/src/main.rs:4-5`); no digest of them is pinned in the tree (section 3.4). |

## 9. Decision table

| ID | Decision | Options | Recommended | Why |
|---|---|---|---|---|
| D1-1 | Five test crates | **A** add `#![forbid(unsafe_code)]`; **B** `[[crate]]` allowlist entries | **A** | no unsafe in them; B would hide future unsafe in executed Cells |
| D1-2 | `session.rs` unsafe test | **A** allowlist `session.rs`; **B** extract the one test to `session_zeroize_tests.rs`, allowlist that; **C** drop the readback (weaker test, no entry) | **B** | keeps the secret-bearing production file under the token rule; real unsafe cannot be reworded; C loses the wrapper-level zeroization check |
| D1-3 | `rg/tests.rs` | **A** `[[file]]` `test-harness`; **B** rewrite without a counting allocator (drops D76-3 M2 heap-peak evidence) | **A** | `GlobalAlloc` is unsafe by definition; test-only; file is purely tests |
| D1-4 | Named approver and date of the two entries | `dmin` / 2026-10-30 as drafted; or another person; add `review_by`+`tracking`? | `dmin`, no `review_by` | header `unsafe-allowlist.toml:17-23` requires a person; owner must confirm the name; permanent test-only |
| D1-5 | Overdue `[[crate]] app-cellbox` (review_by 2026-10-28, dangling reason) and other stale rows | leave / refresh in this commit / separate cleanup | **leave** (separate cleanup) | only a NOTE; out of the "7 findings" |
| D1-6 | Tighten `app-cellbox` to a real `#![forbid(unsafe_code)]` later | needs `FORBID_RE` to accept `cfg_attr(not(test), forbid(...))` or tests.rs moved to a non-forbid crate | later, separate | would change `scripts/cellos_sign/scan.py` (security tooling, own CP1) |
| D1-7 | K8 freeze/provenance after the edit | leave historical (recommended) / cut a new freeze file | **leave** | evidence sets consumed; no rerun authorized |
| D1-8 | `scripts/cellos-sign` is mode 100644 (direct execution fails) | leave; or `git update-index --chmod=+x` | **leave** | not an F1 finding; all callers use `python3`; mode change is a separate script decision |
| D1-9 | Runner edit | **A** delete waiver + new F1 assertion (as drafted); **B** keep the waiver and make it tolerant of existing entries | **A** | the waiver is obsolete by construction; B keeps dead code |
| D1-10 | Commit shape | one commit (source + allowlist + runner + CHANGELOG), gate green at every commit; records in a following docs commit | one commit | the gate is red unless all hunks land together |

## 10. Not done / not verified

No cargo build or test run (compile of the forbidden crates and of the moved test is predicted, not observed); `--sign` itself not executed (it runs the same `run_check` with `strict` and F5 verified, `cli.py:82-90`); the `--kssh` runner was not executed; peer cleanup documents (C3CI, C4CQ) not read; lock/portfolio overlap is grep-based [INFERENCE]; the scratch tree at `/tmp/f1sim` is throw-away and not part of the repository.
