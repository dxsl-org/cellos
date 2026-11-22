# Scout report: Monty thin port (P0)

Recon 2026-11-22. Monty upstream HEAD inspected: `5f4a3dfc493538bb9dc319974393391ba9e7cf14` (2026-11-20), `1.7.0-beta.1`.

## Monty facts (upstream)

- Workspace MSRV `rust-version = "1.98"` (bumped 2026-11-06; 1.96 on 2026-09-21; 1.95 on 2026-04-28).
- Workspace dep `ruff_python_parser = { version = "0.0.21", features = ["stacker"] }` (`Cargo.toml:84`), with an upstream TODO (`Cargo.toml:81-83`) to drop stacker in favour of recursion limits. The parser gates growth on `#[cfg(feature = "stacker")]` (`ruff_python_parser-0.0.21/src/parser/mod.rs:950-984`).
- `crates/monty/src/source_nesting.rs`: the nesting pre-scan runs only for sources longer than `source_scan_threshold` (default 4 KiB), because stacker absorbs shorter ones. Without stacker, the host must set the threshold to 0.
- `monty-types` `cpu-clock` feature pulls `nix` only under `cfg(target_os = "linux")` (`crates/monty-types/Cargo.toml:29-31`), so it is inert on cellos.
- Entropy: `getrandom` 0.4 (direct, `boundary_uuid.rs:18` uses `.expect`, `types/random.rs:445` maps the error) and 0.3 (via `ahash` `runtime-rng`). Both `custom` backends call the same symbol `__getrandom_v03_custom(dest: *mut u8, len: usize) -> Result<(), Error>`.
- `max_memory` is enforced only when `monty-alloc` feeds `monty_types::resource::LIVE_MEMORY`; otherwise it is "silently not enforced" (`resource.rs:386-388`). P1 scope.
- Dependency licenses outside cellos `deny.toml` allow list: `BlueOak-1.0.0` (`minicbor`, `minicbor-serde`), `Unicode-DFS-2016` (`unicode_names2`, `ruff_unicode_names2`).

## Probe result (2026-11-22, throwaway)

With `cargo +nightly-2026-05-01 check -Z build-std=core,alloc,std,panic_abort -Z json-target-spec --target targets/x86_64-unknown-cellos.json`, the blockers were: `psm` build script (riscv64 `cc -mabi`), `stacker` (`libc::mmap`/`mprotect`), `getrandom` (no backend), and 7× `result_option_map_or_default` (1.98 API). With all four worked around the check finished. Link and run were not attempted.

## Cellos contracts touched

| Contract | Location |
|---|---|
| Toolchain pin | `rust-toolchain.toml`; `.github/workflows/{ci,docs,perf,release,security}.yml`; `scripts/ci-runner/setup-runner.sh:26`; `scripts/build-cellos-sysroot.sh` (`EXPECTED_COMMIT`); `scripts/run-std-smoke-qemu.sh`; `scripts/run-std-parity-benchmark.sh` |
| rust-std PAL | `patches/rust-std-cellos.patch` (random hook `sys/pal/cellos/random.rs` panics on zero entropy, PAL-019); `targets/*-unknown-cellos.json` |
| Pin-coupled tests | `tests/rust-std-promotion/rust_src.py:15`, fixtures; `scripts/rust_std_promotion/validator.py:26` (compiler-builtins identity + digest); `scripts/test_cellos_sign.py:1268`; `tests/app-tier-acceptance/fixtures.py:90` |
| Ratified contract | Spec 23 §3.1 tuple table (`docs/specs/23-native-sdk-contract.md:107-119`); `scripts/app_tier_acceptance/source.py:97` denominator. The matrix digest covers §5 only (`source.py:44-73`), so a §3.1 edit is a prose-only `source_amendment` event (`ledger.py:260`) |
| PAL approvals | `.agents/260821-1800-tier1-rust-std-pal-feasibility/approvals/*.md`: digest re-bind under `PAL-IMPLEMENTATION-CHECKPOINT` condition 6, owner-approved (precedent 2026-11-16) |
| Tier 2 class | `abi::declare_manifest!(…, tier = abi::manifest::PROTECTION_CLASS_UNTRUSTED)` (`cells/runtimes/lua/src/main.rs:15-20`) |
| Unsafe allowlist | `scripts/unsafe-allowlist.toml`; an exported symbol called from foreign code is class `c-ffi` (precedent `cells/runtimes/lua/src/bindings_io.rs`) |
| Entropy syscall | `ostd::syscall::sys_get_random` (`libs/ostd/src/syscall.rs:2506`): ≤64 B per call, 0 = no entropy |
| Allocator | `ostd` owns `#[global_allocator]` (`libs/ostd/src/heap.rs:105`) |
| x86 Tier 2 runtime lane precedent | `scripts/build-x86_64-cpp-hosted-ci.sh` (test-hooks kernel, Tier-2 admission open) + `scripts/x86/qemu-cpp-hosted-smoke.sh` |
| std cell precedent | `cells/demos/std-smoke` (own `[workspace]`, excluded in root `Cargo.toml`) + `scripts/run-std-smoke-qemu.sh`; CI job `rust-std-lane` (`ci.yml:515`) |
| Third-party fetch precedent | `scripts/build-mlibc.sh` (pin, fetch to gitignored `-src`, apply `third_party/mlibc/patches/*.patch`) |
| Supply chain | `deny.toml` (license allow list; `cargo deny --all-features check` at `ci.yml:2826`; separate-manifest step precedent `ci.yml:2828-2831`) |

## Corrections from red-team (2026-11-22)

- **`v1.6.0` cannot be the pin.** It pins `ruff_python_parser = "0.0.14"`, where `stacker` is a mandatory dependency, uses getrandom 0.3 only, and uses `unicode_names2` 1. The pin needs a commit with ruff 0.0.21 (phase 1 chooses it).
- **A rustc change is a new PAL compiler-strategy decision, not a condition-6 re-bind** (`artifacts/compiler-strategy-decision.md:41-43`). This is now phase 3.
- **The PAL ignores `thread::Builder::stack_size`.** There is no in-cell stack fallback.
- **The std PAL heap is fixed at 4 MiB.**
- **The test-hooks kernel uses `dev-weak-rng`.**
- **`DOMAIN-FRAME-RELEASE: PASS` names no cell.** Runners must count occurrences.
- **The cpp-hosted lane has `tier="manual"` with unknown status.** Model the witness lane on `build-x86_64-domain-test-ci.sh`, which is pr tier.
