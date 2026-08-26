# Wave 1 Major fixes — `cellos-sign` admission gate (MJ2–MJ5)

Date: 2026-07-30 · Agent: haily-implementor · Scope: `scripts/cellos_sign/`, `scripts/sign-cell.py`,
`scripts/lib-sign-cells.sh`, `scripts/test-cell-signing.sh`, `gen_disk.ps1`, `docs/security-model.md`
Source review: `.agents/reports/wave1-review-260730.md` · Phase: `phase-11-cellos-sign-f1.md`
(deviations logged live there as rows 9–13)

---

## MJ2 — no signing path without a check

`scripts/sign-cell.py` now carries a module-level `_CHECKED` sentinel (`sign-cell.py:66`), set by
`cellos_sign.signing.sign_and_verify` on the module object it imports (`signing.py:84`) and nowhere
else. `_guard_admission` (`sign-cell.py:251`) runs alongside `guard_prod_key` for every signing-mode
invocation and refuses unless the sentinel is set. A direct `python3 scripts/sign-cell.py` executes a
fresh module whose sentinel is `False`, so the dev-key hole is closed, not documented away.

Legitimate standalone dev signing exists in exactly one place — the ELF round-trip test — so it gets
the explicit `--unchecked-dev-signature` opt-in: dev key only (refused with `--seed-hex` even inside
CI), prints a WARNING, and used only by `scripts/test-cell-signing.sh:50`.

Two direct signing callers were found and rerouted, not just the one the finding named:

- `gen_disk.ps1:165-235` called `sign-cell.py --in/--out` per cell. It is the Windows image lane —
  the same class of caller as `lib-sign-cells.sh`, so it now collects paths and makes one
  `scripts/cellos-sign --sign` call (the F1 scan is per-tree, not per-binary). `Invoke-SignCell`
  became `Add-CellToSign` because it no longer signs.
- `scripts/test-cell-signing.sh` takes the named opt-in, with the reason on the line above it.

Verified from the CLI:

```
$ python3 scripts/sign-cell.py --in /etc/hostname --out /tmp/x.elf
REFUSED: sign-cell.py runs no F1/F5 policy check, so a signature minted here would attest
nothing. Sign with `python3 scripts/cellos-sign --sign ELF...`, which checks first. …
exit=1
$ CELLOS_SIGN_CI=1 python3 scripts/sign-cell.py --unchecked-dev-signature --seed-hex 00*32 …
REFUSED: --unchecked-dev-signature is dev-key only; a production key must never sign
without a passing F1/F5 check.                                                    exit=1
$ python3 scripts/sign-cell.py --emit-pubkey                                      exit=0
```

`--verify` and the `--emit-*` modes stay ungated — they cannot produce a signature.

## MJ3 — literals are stripped in the same pass as comments

New focused module `scripts/cellos_sign/lexer.py` (130 lines) replaces `scan.strip_comments`. It
lexes, in one pass: `//` and nesting `/* */` comments; `"…"` / `b"…"` / `c"…"` with `\` escapes;
`r"…"`, `r#"…"#`, `br##"…"##` with any hash count; `'x'` / `b'x'` char literals including `'\''`,
`'\\'` and `'\u{…}'`. A quote that does not complete a char literal is a lifetime and stays as code.
Every removed span becomes one space plus the newlines it contained, so line structure and line
numbers survive — the space matters, or `un/*x*/safe` would fuse into a keyword rustc never sees.
Prefixes only count at a token boundary, so `r#type` stays a raw identifier.

`FORBID_RE` (`scan.py:29`) is anchored with `^[ \t]*` under `re.MULTILINE`.

All four rows from the finding, plus byte/char literals and the combined bypass, are unit tests in
`scripts/test_cellos_sign.py::ScannerLiteralTests` (11 tests), including an end-to-end
`policy.check` case asserting both layers fire.

**Full-tree re-scan, old stripper vs new, over all 834 tracked `.rs` files:**

| | old | new |
|---|---|---|
| `unsafe` tokens, whole tree | 1831 | 1829 |
| files with a differing count | — | 1 |
| files with a differing `has_forbid` | — | 0 |

The only difference is `kernel/src/main.rs`, 21 → 19: the word "unsafe" inside two log strings
(`main.rs:623,637`). Both are genuine false positives now correctly removed, in a file outside
`CELL_ROOTS`. No count moved in either direction under `cells/`, and no crate root's attribute
verdict changed — the check output is byte-identical before and after.

## MJ4 — docstring corrected, dead state deleted

`policy.py:8-38` now states what `forbid` actually buys (rustc fails the *build*, and `forbid`
cannot be overridden by a later `#[allow(unsafe_code)]`) and lists explicitly what it does not
cover: `unsafe` from a macro defined in another crate — the property `ostd::cell_main!` relies on —
and anything in a dependency, since `forbid` is per-crate. The boundary is now written down: *Cells
are forbid-clean; `libs/*` is trusted TCB and out of F1 scope*, with the reason `libs/ostd` is
absent from `CELL_ROOTS`. The stale "false positives inside string literals are accepted" line in
`__init__.py` was corrected too, since literals are no longer read as code.

`Crate.path_deps` and `_dep_dirs` are **deleted** (`scan.py`, −22 lines) rather than wired up: the
attribute layer cannot reach a dependency at all, so collecting dependency directories was state
that looked like a control and was not one.

## MJ5 — the sign path is strict by construction

`cli.run_sign` sets `args.strict = True` before calling `run_check` (`cli.py:87`), so strictness is
not a flag any caller can forget — `lib-sign-cells.sh`, `gen_disk.ps1` and any future lane inherit
it. `--strict`'s help text records that it is always on for `--sign`. Two unit tests
(`SignPathStrictnessTests`) assert a skipped F5 returns `EXIT_POLICY` with `sign_and_verify` never
called, and that a verified F5 still signs.

---

## Files changed

| File | Change |
|---|---|
| `scripts/cellos_sign/lexer.py` | NEW, 130 lines — Rust-source reduction (comments + literals) |
| `scripts/cellos_sign/scan.py` | 213 → 164 — uses the lexer, anchors `FORBID_RE`, drops `path_deps`/`_dep_dirs` |
| `scripts/cellos_sign/policy.py` | 148 → 162 — corrected docstring + stated F1 boundary |
| `scripts/cellos_sign/cli.py` | 134 → 142 — `run_sign` forces strict |
| `scripts/cellos_sign/signing.py` | 118 → 122 — sets the `_CHECKED` sentinel |
| `scripts/cellos_sign/__init__.py` | 39 → 41 — corrected false-positive claim, `libs/*` scope |
| `scripts/sign-cell.py` | 344 → 387 — sentinel, `_guard_admission`, opt-in flag, docstring |
| `scripts/test_cellos_sign.py` | 210 → 353 — 17 new tests |
| `scripts/lib-sign-cells.sh` | +3 comment lines — records that F5 is mandatory and needs no flag |
| `scripts/test-cell-signing.sh` | +4 — named opt-in with its justification |
| `gen_disk.ps1` | signing block routed through `cellos-sign`, batched |
| `docs/security-model.md` | phase-11 bullet updated to the new guarantees |
| `.agents/…/phase-11-cellos-sign-f1.md` | Deviation Log rows 9–13, one Risk row |

No kernel, `hal/`, or `cells/` source was modified. `cells/demos/hello/src/main.rs` was edited twice
for the break tests and restored from a scratch copy — verified byte-identical afterwards (`diff`
clean; the remaining `M` against HEAD is the parallel phase's `#![forbid]` addition, not mine).
The signature format and `__ViCell_sig` section are untouched; `kernel/src/signing.rs` was not read
or changed.

---

## Verification

```
$ python3 scripts/cellos-sign --check --strict
OK:   F5 — rustc f53b654a8882 is the pinned nightly-2026-05-01
OK:   F1 — 77 crates and 337 files scanned; unsafe confined to 46 allowlisted files
exit=0                                                    (unchanged: 77 crates, 337 files)

$ python3 scripts/test_cellos_sign.py
Ran 35 tests in 0.025s — OK                               (was 18/18; +17)

$ cargo fmt --all --check
exit=0
```

**Break test** — `unsafe {}` injected into `cells/demos/hello/src/main.rs`:

```
FAIL: F1 — …
  [token] app-hello :: cells/demos/hello/src/main.rs — contains the `unsafe` keyword and is
          not in unsafe-allowlist.toml                    exit=1
```

**New bypass test** — the combined construction (`const A: &str = "#![forbid(unsafe_code)]";` +
`const B: &str = "/*";` + real `unsafe`), run twice.

1. In the real tracked tree, replacing the genuine attribute in `cells/demos/hello/src/main.rs`.
   Differential proof on the identical file:

   ```
   OLD scanner: has_forbid=True   unsafe_count=0     ← passed BOTH layers
   NEW scanner: has_forbid=False  unsafe_count=1
   ```

   ```
   FAIL: F1 — …
     [attribute] app-hello :: cells/demos/hello/src/main.rs — crate root lacks #![forbid(…)]
     [token]     app-hello :: cells/demos/hello/src/main.rs — contains the `unsafe` keyword
   exit=1
   ```

   File then restored from the pre-test snapshot; `diff` clean, `--check --strict` back to exit 0.

2. Through the CLI against a scratch tree (`--repo <scratch> --allowlist unsafe-allowlist.toml`)
   containing only `cells/evil/`: the bypass file fails both layers (exit 1); the same tree with a
   real attribute and no `unsafe` passes (exit 0), confirming no blanket-fail artefact.

**Not verified — cannot be, on this box.** No QEMU, no cross-gcc, no cross-objcopy, so no
sign → verify round trip and no image build were run. A host ELF is not a substitute: gcc places the
ELF header inside the first PT_LOAD, so objcopy's `e_shoff` rewrite lands inside the signed payload,
which never happens for a cell built with the kernel's linker script. `gen_disk.ps1` could not be
executed or syntax-checked either (no `pwsh` here); its change is reviewed by eye only. The
signing code paths themselves are unchanged apart from the sentinel assignment.

---

**Status:** DONE_WITH_CONCERNS

**Summary:** All four Major findings are fixed at the code level, not the docstring level —
`sign-cell.py` refuses to sign without a passing check (both direct callers rerouted), the scanner
lexes string/raw/char literals so the combined bypass now trips both layers, `policy.py` states the
real `forbid` boundary and the dead `path_deps` is gone, and `--strict` is imposed by `run_sign`
rather than by a call site. Concerns are environmental (no cross-toolchain to run a real
sign→verify or the Windows lane) plus one scope question below.

**Verification:** `python3 scripts/cellos-sign --check --strict` → 77 crates, 337 files, exit 0
(unchanged) · `python3 scripts/test_cellos_sign.py` → 35/35 (was 18/18) · `cargo fmt --all --check`
→ exit 0 · break test → `[token] app-hello :: cells/demos/hello/src/main.rs`, exit 1, reverted
byte-identical · new bypass test → old scanner `has_forbid=True, unsafe=0`; new scanner
`has_forbid=False, unsafe=1`, both layers fail, exit 1 · differential re-scan of all 834 tracked
`.rs` files → 1831 → 1829 `unsafe` tokens, the only delta being two occurrences of the word inside
log strings at `kernel/src/main.rs:623,637`; zero `has_forbid` changes.

**Concerns / Blockers**

1. **`libs/ostd` outside the scanned set — correct today, but the gap is real and is now one macro
   wide.** Not scanning `libs/*` is right: `ostd` is the supervisor-side runtime whose job *is* the
   `unsafe` a Cell must not write, and token-scanning it would produce a hundreds-of-entry allowlist
   that means nothing. The gap is not that `libs/ostd` is unscanned; it is that **a Cell can execute
   `unsafe` it did not write and F1 will not see it** — a `macro_rules!` exported from `ostd` whose
   expansion contains `unsafe` compiles cleanly inside a forbidding cell (the review verified this on
   the pinned nightly, and `ostd::cell_main!` depends on it). Today that hatch is used exactly once,
   deliberately. Nothing stops the next `ostd` macro from widening it, and no check would fire. That
   is now written into `policy.py` rather than left implicit, but documenting it is not closing it.
   Worth raising as its own item: an `ostd`-side rule that macro bodies expanded into cells carry no
   `unsafe` (a targeted scan of `macro_rules!` bodies under `libs/ostd`, allowlisted like F1) would
   cover it. I did not add it — it is a new control, outside these four findings.
2. **No runtime evidence.** The sign path was exercised only through mocked unit tests and refusal
   paths. First CI run on a lane with a cross objcopy is the real proof; `gen_disk.ps1`'s batched
   invocation in particular has never executed.
3. **`gen_disk.ps1` is now strict about F5.** A Windows box that builds cells but has a `rustc` that
   does not match the pin will now fail the image build instead of silently signing. That is the
   intent of MJ5, but it is a behaviour change someone will hit.
4. **`scripts/sign-cell.py` is 387 lines**, over the 200-line guidance. It was already 344; it is one
   cohesive ELF/Ed25519 producer that must stay byte-compatible with `kernel/src/signing.rs`, and
   splitting it was judged higher-risk than leaving it. Not addressed here.
5. **Shared working tree.** Another agent is editing `hal/arch/arm/` and
   `kernel/src/memory/page_protect.rs`; no overlap with anything I touched. `scripts/cellos_sign/lexer.py`
   is `git add`-ed (130 lines in the index, verified non-empty) to match the staging state of its
   sibling modules — nothing was committed.
