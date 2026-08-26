# Test Report — 2026-08-20 — Phase 02 manifest ABI baseline

Mode: host-target baseline | 3 commands on `libs/api`, 1 kernel host lane
Mapped: `libs/api/src/abi/manifest_tests.rs` (co-located); `kernel/src/task/manifest_v2_selftest.rs` (direct host lane)
Ran 3/3: `cargo check` pass, `cargo test ... manifest_tests` pass, `cargo build` pass
Coverage: 38.27% line / 0% branch (threshold 80% line / 70% branch)
Build/typecheck: pass for `libs/api`; kernel host test lane failed before execution

`abi::manifest_tests::hardware_bus_flags_are_distinct_and_queryable` — pass
`abi::manifest_tests::parser_preserves_i2c_and_spi_bits` — pass

[FAIL] `cargo test --offline --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu manifest_v2_selftest -- --nocapture` — `kernel/src/task/user_out.rs:66` unresolved import `SpinLockGuard`; `kernel/src/main.rs:1047` duplicate `panic_impl` via `std`
[GAP] `libs/api/src/abi/manifest.rs` — manifest ABI coverage only 36.28% line; add alias/accessor cases when Phase 2 lands
