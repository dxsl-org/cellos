---
phase: 9
title: "Secure outbound SSH and controlled interoperability"
status: pending
priority: P1
tier: thinking
dependencies: [2, 4]
---
# Phase 09: Outbound SSH

## Overview and requirements

Deliver CBX-23..25 as an integrated cellbox client with separate session state. Headless/serial delivery depends on Terminal SDK and multicall, not Ratatui or GUI-emulator completion; P10 adds GUI SSH proof after P05. Do not merge/rewrite inbound sshd, invent a crypto protocol or claim guest connectivity from a compiled client.

## Architecture and related files

Add SSH client/config/trust/auth/session modules under `cells/tools/cellbox`. Consume `libs/terminal`, admitted NetClient/TCP/DNS, entropy and VFS trust/key operations. Use P01's pinned/reviewed SSH library, evaluating Sunset first and requiring justified runtime proof for alternatives. `cells/services/sshd` is read-only risk evidence, not a trusted dependency or test server.

## Implementation steps

1. Bind the selected library to real bounded network I/O, cryptographic entropy, packet/channel limits, timer/rekey and cleanup contracts. Audit protocol/auth/host-verification behavior and current dependency advisories; no fixed secret/cookie, plaintext or insecure verification fallback.
2. Implement user/host/port and authentication selection, key ownership/loading, secret entry with echo disabled and explicit persistent known-host policy. Unknown hosts need a deliberate trust decision or preprovisioned identity; changed keys are rejected, not overwritten automatically.
3. Implement a PTY shell with terminal type/size and resize, correct foreground/control/escape handling and mode restoration. Remote output remains untrusted terminal bytes interpreted by the terminal emulator, not native objects.
4. Implement non-PTY remote execution with exact stdin/stdout/stderr separation, EOF/half-close and remote exit status/signal outcomes. Define the remote command-string quoting boundary explicitly; never claim SSH exec transports a safe structured argv.
5. Handle auth rejection, host mismatch, timeout, packet errors, rekey, disconnect and unknown completion without automatic replay. Limit queued data and close resources on cancellation or consumer death.
6. Prove interoperability against a controlled other-host OpenSSH-compatible server and a real Tier-3 guest server with known route/credentials/identity. Use private fixtures and temporary keys/trust state only with authorization; do not start/expose current Cellos sshd.
7. Exercise PTY resize, remote stderr/nonzero status, changed host key, invalid auth, secret non-disclosure, long-session rekey and network interruption. Record target/server/library versions and exact observed outcomes. Remove temporary secrets/fixtures after evidence capture.

## Success criteria

- [ ] Actual outbound PTY and non-PTY sessions work on both selected endpoint classes, with truthful status/EOF/resize.
- [ ] Host-key/auth failure rejects the connection; real entropy and protected secret/trust handling are demonstrated, no plaintext fallback.
- [ ] Disconnect after dispatch reports uncertainty and never replays remote effects; backpressure/rekey/cancellation remain bounded.
- [ ] VM route/server readiness is actual evidence; no Intel VMX, C2C authority, inbound server or production readiness is inferred.

## Verification

Use library/adapter deterministic boundary regressions and actual SSH interoperability, not mocked success. Packet/rekey/unknown-outcome scenarios require controlled faults. Test server exposure remains loopback/private and separately authorized. An unavailable guest route blocks that scenario explicitly; it cannot be substituted by host localhost or a printed guest label.

## Assumptions

The selected library meets required feature/runtime/crypto contracts, and VFS/entropy/network APIs have admitted safe bindings. Verify exact versions/source and target runtime. Guest server/route availability and host-key persistence are external prerequisites, not implied by ADR approval.

## Security and risk assessment

One ELF/session Cell is not subcommand-specific authorization or stronger isolation. Key/trust mutation and outbound network access retain scoped authority. Rollback restores matched client/library/image but does not undo accepted trust or remote effects; preserve unknown work and never restore the unsafe inbound implementation as fallback.

## Deviation log

None; implementation pending. Log dependency/security/remote-effect deviations with reason/impact/revert; no unapproved feature reduction.
