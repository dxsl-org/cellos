# D30 — Reconcile entropy behavior by build profile

**Status:** approved/applied 2026-08-01. No code changed.

## Finding

Both conflicting descriptions are conditionally true. The kernel's default feature set
includes `dev-weak-rng`, so default development/QEMU builds use a predictable xorshift
fallback with a warning. When that feature is absent, `GetRandom` fails closed instead of
returning weak bytes. Calling the default build fail-closed is therefore inaccurate.

## Recommended ruling [FINAL]

**Approve recommendation A: define two explicit entropy profiles.**

1. Development/QEMU profile: `dev-weak-rng` may supply deterministic weak bytes, with a
   prominent warning; it is never acceptable for credentials, signatures, or Noise keys.
2. Fleet/production profile: `dev-weak-rng` is forbidden and lack of trusted entropy
   fails closed.
3. Add a release/CI gate proving production artifacts do not enable `dev-weak-rng`.
4. Align architecture, roadmap, and security language with this profile distinction;
   do not change the syscall ABI in this ruling.
