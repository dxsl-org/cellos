# Preparation saved-artifact verification

**Read-only preparation smoke: PASS after citation corrections. No files modified.**

**Executed checker**
- Entry point: `functions.eval(language="py", title="Rechecking final saved preparation documents")`.
- In-memory Python using `pathlib`, `re`, `json`, and `hashlib`; no permanent test or checker files.

**Final saved-document results**
- All four artifacts exist and are nonempty:
  - `command-catalog.md`: 10,955 bytes
  - `contract-deltas.md`: 16,973 bytes
  - `dependency-review.md`: 14,248 bytes
  - `baseline.md`: 7,084 bytes
- No searched truncation/aborted-agent markers.
- **168 citation checks:** no missing targets, out-of-bounds ranges, or abbreviated paths. Root `Cargo.toml` resolves by repository-root precedence.
- **99 relative Markdown links across nine documents:** none broken. This includes final plan, Phase 01, acceptance, portfolio, and current-focus records. The four artifacts themselves contain no Markdown links.
- Retained DAG: **10 phases, 26 edges, acyclic, exact expected dependencies**.
- Phase 01: `blocked: preparation incomplete`; phases 02–10 remain `pending`.
- Acceptance: **29 unique sequential CBX-01…CBX-29 rows, all pending**.

**Baseline evidence**
- Supplied `artifact://5` records **38 unit tests passed, zero failed; zero doctests**, matching corrected baseline wording.
- Source contains **38 `#[test]` declarations**. `libs/text-engine/src/lib.rs:1-23` and its manifest support the documented pure host-test boundary.
- Supplied `artifact://1` matches **0 staged, 29 unstaged tracked, 65 untracked**, plus recorded HEAD/toolchain identity.
- All **29 baseline file-size/SHA-256 rows** matched current bytes.

Existing baseline command, inspected from saved evidence—not rerun:
```sh
cargo test --offline --locked -p text-engine --target x86_64-unknown-linux-gnu
```

**Limits:** citation checking establishes target existence and range bounds, not exhaustive semantic correctness of every cited claim. Markdown fragment anchors and external URLs were not validated. No new Rust tests, builds, dependency acquisition, signing, packaging, QEMU, or runtime qualification occurred. This smoke passes documentation structure only; no CBX outcome or CP1/CP2 approval is implied.