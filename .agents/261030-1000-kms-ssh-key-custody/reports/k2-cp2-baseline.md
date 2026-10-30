# K2 CP2 baseline (2026-10-30, before any source edit; HEAD bfade27f5; `git status --short` empty)

- `cargo test --offline --locked -p types --target x86_64-unknown-linux-gnu`: **54 passed, 0 failed** (equals the Phase 06 recorded baseline of 54).
- `cargo test --offline --locked -p service-kms --target x86_64-unknown-linux-gnu`: **59 passed, 0 failed** (equals the Phase 1 recorded 59/59).
- Start-of-slice overlap re-check of the four unexamined plans: `260825-sdk-delivery` phase 02 (lists `cells/services/kms/src/dispatch.rs` in its file list; plan P02 client blocked), `260712-1902-dice-attestation-identity` phase 03 (historic KMS creation text, blocked by the lock), `260819-1409-cell-to-cell-anywhere-core` phase 02 (`status: blocked`, lists `dispatch.rs`), controlling `migration-ledger.md` E12 (evidence row for `main.rs`/`node_identity.rs`, not an edit claim). None is active; no concurrent edit is in flight. Stop rule stays: an owner claiming a closure path stops the slice.
