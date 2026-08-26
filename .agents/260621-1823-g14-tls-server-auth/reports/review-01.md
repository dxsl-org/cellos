# Code Review 01 — G14 TLS verify implementation (haily-reviewer, opus)

**Verdict:** GO-WITH-FIXES → **fixes applied + re-verified**. Verification core correct & fail-closed.

## Findings + disposition

| # | Sev | Finding | Disposition |
|---|-----|---------|-------------|
| M1 | major | `handshake()` gates the only `conn.open()` behind `cfg(tls-roots-embedded)`/`cfg(tls-insecure)`. A build with neither (unimplemented `tls-roots-full`, or `--no-default-features`) compiles, skips the handshake, returns a TLS entry with ZERO verification — silent bypass. | **FIXED** — `compile_error!` in socket.rs (tied to the open() path) + `usable_flavor` panic in build.rs. Verified: `--no-default-features` build now fails with a clear message. |
| M2 | major | 3 TLS-connect failure exits (handlers.rs 410/415/449) `table.remove(cap_id)` without `sockets.remove(handle)` → smoltcp socket leak (pool 16) → DoS after ~16 failed connects. | **FIXED** — added `sockets.remove(handle)` at all three sites (incl. the verification-reject arm). Builds clean. |
| m1 | minor | provider.rs comment vs >64-char hostname handling. | Accepted as-is — behavior is fail-closed (`new()` returns `Err`→connect fails); `# Errors` doc is accurate. |
| m2 | minor | embedded-tls `pki.rs:138` panics if a server sends `CertificateVerify` without `Certificate` (DoS, not bypass). Library-rooted. | Accepted — mitigated by the NotifyOnExit supervisor (init auto-restarts service-net). Documented as a residual availability risk. Not forking embedded-tls. |

## POSITIVE (reviewer-confirmed invariants)
- B3: `verifier()` is unconditional `Ok` — verification cannot be silently skipped (the load-bearing guard holds).
- Clock clamp is a true floor — never moves valid time backward, never turns an expired cert valid.
- `ViTlsProvider`/`CertVerifier` constructed per-handshake — no cross-connection state.
- Empty-host + library `tls_hostname_match` fail closed; CA-conflict `compile_error!` matrix complete;
  insecure banner unmissable; reject-vs-timeout log split keeps the gate falsifiable.

## Re-verification after fixes
- default (verifying): builds clean, 459,072 B
- insecure: builds clean
- `--no-default-features` (no flavor): **fails** with `no usable TLS flavor selected` (M1 guard)
