---
phase: 03
title: E2E + negative tests (the gate) + docs + threat model
status: Planned
tier: medium
depends_on: [02]
owns:
  - cells/apps/https-demo/src/main.rs          # extend with POS + NEG scenarios
  - docs/specs/07-networking.md                # verification contract + residual threats
  - cells/services/net/src/handlers.rs         # log distinct TlsError (timeout vs reject) at 0x30 path
---

## Context Links
- Plan: [plan.md](plan.md) · Red-team M4/T1: [reports/red-team-01.md](reports/red-team-01.md).
- `handlers.rs:432` collapses every handshake `Err` to cap 0 — fine on the wire, but logs must distinguish.

## Overview
Prove the security property: good certs connect, bad certs are rejected. **Negatives are the deliverable**
— a no-op verifier passes a positive-only suite. Add timeout-vs-reject logging so the gate is falsifiable,
and document the verification contract + residual threats.

## Key Insights
- A green POS with red (rejecting) NEGs is the only acceptable outcome. A NEG that connects = silent
  bypass = ship-blocker (directly tests B3).
- `handlers.rs:432` returns cap 0 for both timeout and rejection → without a log line that prints the
  actual `TlsError`, "MITM blocked" and "handshake timed out" are indistinguishable (M4). Add it.
- Negatives should be **deterministic** → drive them from a local self-signed TLS server, not a flaky
  public host. Use a stable public host only for POS, and gate POS behind an internet check.

## Requirements
**Functional**
- `handlers.rs` 0x30 path: on `handshake()` `Err`, log the concrete `TlsError` variant (e.g.
  `InvalidCertificate` vs `IoError`/timeout) before replying cap 0. Wire reply unchanged.
- `https-demo` scenarios (compile-cfg or arg-selected):
  - **POS:** connect to the target broker / a stable host whose cert chains to `roots/ca.der`, matching
    SNI, valid dates → cap != 0, write/read OK.
  - **NEG-untrusted:** self-signed / out-of-anchor CA → cap 0.
  - **NEG-host:** valid cert, wrong SNI → cap 0.
  - **NEG-expired:** local server with an expired cert (vs clamped clock) → cap 0.
  - **NEG-tampered:** flipped signature/cert byte → cap 0.
  - **NEG-empty-host:** empty SNI in verifying build → cap 0.
  - **INSECURE regression:** `tls-insecure` build → NEG-untrusted now cap != 0 (escape hatch works + banner shown).
- `docs/specs/07-networking.md`: TLS server-auth section — what is verified (chain-to-single-anchor,
  validity w/ min-date clamp, RFC-6125 SAN hostname), build flavors, and **residual threats (T1)**:
  RTC manipulation defeats expiry on untrusted clocks; pinned-key + no-revocation = permanent trust if
  leaked; DER parser is attack surface on hostile certs; no CT/name-constraints; `pki.rs` 3-SAN/64-char
  caps; **TLS 1.3-only ⇒ no protocol downgrade (positive)**; revocation out of scope + mitigations
  (short-lived certs, pin, OTA).

**Non-functional**
- Negatives reproducible offline; document the POS host + that it may rotate.

## Implementation Steps
1. Add the distinct-`TlsError` log line in `handlers.rs` 0x30 path.
2. Stand up a host-side self-signed TLS server (script) for deterministic NEG cases.
3. `https-demo`: implement the 6 NEG + 1 POS + INSECURE scenarios.
4. Run default flavor: **POS pass, every NEG → cap 0** (the gate). Confirm logs show
   `InvalidCertificate` (reject), not timeout, for NEGs.
5. Run `tls-insecure`: NEG-untrusted connects + banner printed.
6. Write `07-networking.md` section + residual threats; update `roots/README.md` CA-expiry note.
7. `haily-reviewer` on provider/clock/transport; `haily-tester` on scenarios.

## Todo
- [ ] distinct-`TlsError` logging at 0x30 path (M4 falsifiability)
- [ ] local self-signed server helper
- [ ] https-demo: POS + 6 NEG + INSECURE scenarios
- [ ] **gate:** default flavor POS pass, all NEG cap 0, NEG logs = reject (not timeout)
- [ ] insecure flavor: NEG-untrusted connects + banner
- [ ] 07-networking.md verification contract + residual threats (T1)
- [ ] reviewer + tester pass

## Success Criteria
- **Gate:** default embedded build — POS succeeds; every NEG returns cap 0 with a reject (not timeout) log.
- Insecure build demonstrably bypasses (proving the others genuinely enforce) + prints banner.
- Docs enumerate residual threats + revocation-out.

## Risks
- **Network flakiness / POS cert rotation** → false failures. Mitigate: negatives fully local; POS gated on internet + documented host.
- **NEG fails via timeout not reject** → masks whether verification ran. Mitigate: the distinct log line + tuned `TLS_IO_TIMEOUT` from P00/P02.

## Security Considerations
- This phase is the proof of the whole plan. Any NEG that connects, or any NEG that "fails" only by
  timeout, means verification may not be running — treat as ship-blocker, not a flaky test.

## Next Steps
- G1 done after this. Full multi-root server/PC build = [phase-04](phase-04-full-build-webpki.md) (deferred).
- Future: operator roots via `/POLICY.BIN` (VIFS1); OCSP/CRL for G2; per-connection pin. Roadmap §G.2.
