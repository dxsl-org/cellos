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

## Re-decision 2026-11-22: nightly `2026-11-21`, rustc `1.102.0-nightly (c7d5ba59d)`. State: SIGNED 2026-11-22 (dmin, both COMPILER-INTEGRATION roles)

The repository moved its pin from `nightly-2026-05-01` (`f53b654a8`) so upstream Monty (MSRV 1.98) builds
unmodified (plan `.agents/261122-1036-monty-thin-port-p0`, phases 1 and 3). Under "Maintenance Cost and
Invalidation", this invalidates the decision above. The strategy is unchanged: option B, an exact source overlay.
This section records the recurring work for the new base. Both
[`../approvals/compiler-integration.md`](../approvals/compiler-integration.md) roles signed it on 2026-11-22.

| Recurring item | Result |
|---|---|
| Overlay rebase | `patches/rust-std-cellos.patch` applies to pristine rust-src with no fuzz or offset (29 files). It round-trips byte-identically against the staged tree. Changes are forced by upstream internal PAL API moves: (a) per-platform free-function allocator (`sys/alloc/cellos.rs`; `realloc` = `realloc_fallback`; `alloc_zeroed` fallback arm); (b) `sys::sync::futex` re-exports `pal::cellos::futex`; (c) `abort` → `abort_immediate`; (d) the personality arm keeps cellos, while upstream dropped `motor`; (e) cellos joins the `io::error` generic arm. The TLS guard keeps cellos in the hermit/xous arm (std runs destructors), as before the rebase. |
| Independent review | Reviewer agent `PalRedecisionReview` (2026-11-22, not the rebase author): no blocking or major findings. `pal/cellos/random.rs` is byte-identical, so PAL-019 still fails closed and PAL-031 still validates before the syscall. Allocator alignment and locking, futex and error mappings are unchanged. Three minor findings, all fixed: the TLS guard arm was restored; 15 hook anchors were re-pointed, including PAL-007's symbol, which now names `System: Allocator` via the `GlobalAllocator` blanket impl; PAL-023/026 text now records that the `no_threads` implementations `compile_error!` under the new `target_has_threads` cfg. |
| 27 `sys/mod.rs` declarations | Re-read at `sys/mod.rs:3-30`. The set is unchanged. Order and visibility moved: `alloc` is now `pub mod alloc` at line 8. The module manifest is regenerated in declaration order. |
| 36 support-map rows | 26 of 46 pinned sources changed and 22 hooks cite one; all 22 were re-inventoried and every classification still holds (36 total: 8 Supported, 10 Unsupported, 18 Deferred). `sys/io/io_slice/unsupported.rs` moved to `core/src/io/io_slice/repr_generic.rs`, the same fallback. |
| Selector / target spec | `sys/pal/mod.rs` is identical. The resolved spec JSON is identical for all three targets under both rustc. Derived cfgs gained `target_has_threads`, `target_has_reliable_f16b` (aarch64) and `zmmul` (riscv), and `atomic_equal_alignment` was renamed to `atomic_primitive_alignment`. |
| Builtins init / personality / math / env constants | `configure_builtins.rs` is identical. The aarch64 std-smoke ELF has no `__aarch64_*` outline-atomic symbols, so the PAL-032 gate is inactive. Cellos keeps the aborting personality stub. `cmath.rs` is identical and compiler-builtins only adds `fmaf16`. `nm -u` of std-smoke is empty on all three targets. The cellos env-constant row is identical. |
| Kernel security backing | Required paths and roles are unchanged. Their digests are re-hashed into the approval-input manifest. |
| Sysroot / runtime | `build-cellos-sysroot.sh` passes on all three targets. The staging stamp is now keyed on the rustc commit. `run-std-smoke-qemu.sh` passes on riscv64, aarch64 and x86_64. |
| Digests | Support map `source_digest` `1aad1e22…`. Approval-input manifest regenerated by `scripts/rust_std_promotion/regen_pal_inventory.py`. |

The gates are unchanged: `PAL-001` Unsupported; `PAL-019`/`PAL-031` security-blocking Deferred. This re-decision does
not promote, publish or qualify anything.

After both signatures:
1. Run `scripts/rust_std_promotion/regen_pal_inventory.py --bind-records`.
2. Update the source identity and digests in `implementation-decision-package.md`.
3. Run `python3 -m unittest discover -s tests/rust-std-promotion -p 'test_*.py'`; it must be green.
