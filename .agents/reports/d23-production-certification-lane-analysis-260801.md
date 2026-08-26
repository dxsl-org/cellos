# D23 — Development target versus safety-certification lane

**Status:** approved/applied 2026-08-01. No code or ABI changed.

## Finding

The PDR conflates a development/reference target with a product certification lane.
RV64 QEMU can remain the primary bring-up/CI target without being the safety-certified
release target.

Current Ferrocene documentation lists `aarch64-unknown-none` as a qualified bare-metal
target when cross-compiled from x86-64 Linux. It does not list bare-metal RV64 as
qualified; only RV64GC Linux appears as a non-safety “Supported” target. The same manual
also limits library claims: only a subset of `core` is certified, while end-user use of
`alloc` and uncertified library portions remains the integrator's responsibility.

Sources:

- https://public-docs.ferrocene.dev/main/user-manual/targets/index.html
- https://public-docs.ferrocene.dev/main/qualification/report/rustc/aarch64-unknown-none.html

Spec 16 also contains stale/overbroad wording: x86-64 bare metal is not in the current
qualified-target table, the RV64 ETA is speculative, and “drop-in replacement” does not
by itself certify Cellos, its custom target, libraries, kernel, or hardware integration.

## Recommended ruling [FINAL]

**Approve recommendation A: split the lanes.**

1. RV64 remains the primary reference build/QEMU development lane.
2. ARM64 `aarch64-unknown-none` becomes the first safety-qualification candidate lane for
   a named supported board/profile and an x86-64 Linux build host.
3. RV64 production may ship without safety claims; an RV64 safety SKU is blocked until a
   matching qualified toolchain/target exists or a funded qualification engagement lands.
4. Amend the PDR and Spec 16 to distinguish compiler qualification from product
   certification and to avoid unsupported ASIL/target claims.

The named hardware profile still needs confirmation: generic Armv8-A qualification does
not automatically prove every RK3588 compiler flag, HAL path, library, or board artifact.
