# Compiler Integration Strategy Decision

Decision ID: `RUST-STD-COMPILER-STRATEGY-001`
Base: nightly `2026-05-01`, rustc `1.97.0-nightly (f53b654a8)`
Support map: [`pal-hook-support-map.json`](pal-hook-support-map.json), 46-file source-manifest digest `b984d50da89e342974ada8822321edd6b1d091d1da3dcf8ec1819a8986a4b105`
Decision: **SELECTED — B, content-addressed source-overlay patch against a private matching Rust checkout**
Authorization: feasibility decision only; implementation remains blocked.

## Why B

A later child may construct a private sysroot from an exact `f53b654a8` Rust source checkout, apply a no-fuzz content-addressed patch, and add compiler target metadata plus an internal `library/std/src/sys/pal/cellos` selected by `target_os="cellos"`. The repository will retain patch, base/result digests, build recipe, and sysroot manifest, not a vendored Rust tree or published target. This is the maintainable near-term form of a pinned vendored-std PAL patch: the build workspace is ephemeral and hash-guarded.

Required later proof:

1. base checkout commit and every input archive have SHA-256 provenance;
2. patch applies with exact context and refuses any other base;
3. compiler target metadata emits `target_os="cellos"` without impersonating another OS;
4. `library/std/src/sys/pal/mod.rs` selects only the new in-tree Cellos PAL, never `_ => unsupported`;
5. private target specification, sysroot, compiler, `libstd`, patch result, closed kernel security-backing inventory, and production kernel feature tuple are digest-bound as one tuple;
6. x86_64, aarch64, and riscv64 implications are reviewed separately;
7. the production tuple omits `dev-weak-rng`, `GetRandom` is backed by real entropy or returns zero/error, and bounded caller-owned writable validation rejects null/overflow/unmapped/kernel/peer pointers before access;
8. no POSIX/libc ABI, ambient authority, or frozen Cellos ABI change appears.

Owned later source surfaces are `compiler/rustc_target/src/spec/**`, `library/std/src/sys/pal/mod.rs`, a new internal `library/std/src/sys/pal/cellos/**`, and the pinned selectors proven necessary by the 27-module scope manifest. The latter explicitly includes target-sensitive `configure_builtins`, `personality`, `cmath`, `env_consts`, and `platform_version`; a later implementation must resolve their blocking rows without mlibc/POSIX or host-library leakage. Kernel entropy, syscall decode/allowlist/dispatch, `GetRandom`, pointer-validation primitives, VirtIO RNG, and production feature defaults remain a separate closed, digest-bound security-backing inventory. This artifact changes none of them.

## Alternatives

| Alternative | Decision | Evidence / cost / exit |
|---|---|---|
| A. Long-lived Rust source fork | Rejected for near term | Selector-correct but stores a second source tree and increases merge/cherry-pick burden. Exit to B is mechanical patch extraction. |
| B. Exact source overlay | **SELECTED** | Same internal integration as A with base-hash guard, deterministic result, private sysroot manifest, and no repository vendoring. Exit is upstream acceptance or deletion of the private overlay. |
| C. Upstream Cellos target/PAL | Deferred exit path | Long-term preferred after capability semantics and target governance mature. It cannot require publishing a target/triple in this feasibility slice and cannot gate the near-term experiment indefinitely. |
| D. External `libs/std-pal` plug-in | Rejected | Pinned `pal/mod.rs:6,63-66` imports only in-tree modules. An external crate cannot register as `std::sys::pal`. |
| E. Target-OS impersonation | Rejected | Setting Linux, WASI, Hermit, or another OS would select foreign host assumptions and misstate compatibility. |
| F. mlibc/POSIX layer | Rejected | Adds an unapproved authority/ABI surface, hides capability checks, and is outside the selected internal PAL design. |
| G. core+alloc relabeled as std / fake std | Rejected | Does not supply the standard-library contract and would create a false compatibility claim. |
| H. Unsupported-PAL shim | Rejected | Current fallback has success-shaped no-ops and unsupported families; it cannot be promotion evidence. |

## Maintenance Cost and Invalidation

The toolchain owner must rebase and independently review the overlay on every rustc commit change. Minimum recurring work is: all 27 `sys/mod.rs:3-30` declarations and all 36 support-map rows re-inventoried; selector, target-spec, builtins-init, personality, math-symbol, and environment-constant diffs; the exact six-path kernel security-backing inventory and production feature tuple re-hashed; three-architecture sysroot rebuild; fixture-validator schema compatibility check; patch/result/sysroot digest regeneration; and security review of capability/error mappings. Expected burden is one pinned overlay per accepted toolchain, never a floating patch.

The decision is invalidated by any rustc commit or rust-src/source-manifest digest change, any required kernel security-backing path or digest drift, `dev-weak-rng` in a production tuple, promotion of `PAL-019` without real entropy-or-zero/error evidence, promotion of `PAL-031` without bounded caller-owned writable validation and hostile direct-syscall evidence, a declaration missing from the module scope manifest, patch fuzz/offset application, internal PAL API change, compiler target schema change, builtins initializer/personality/cmath/env-constant drift, loader/linker contract drift, frozen ABI drift, thread model introduction, allocator concurrency change, panic strategy change, new ambient authority, or inability to reproduce the private sysroot byte-for-byte. Invalidation restores `NO_GO` until re-review.

## Gates

`PAL-001` is currently Unsupported; `PAL-019` and `PAL-031` are security-blocking Deferred; and other minimal-runtime Deferred rows remain blocking. The selected strategy is technically maintainable and yields only a **CONDITIONAL GO** recommendation after every blocker is implemented and evidenced. It authorizes no PAL work until [`../approvals/compiler-integration.md`](../approvals/compiler-integration.md), runtime/benchmark approvals, the implementation checkpoint, and umbrella Phase 03 production gates are explicitly granted.
