---
phase: 9
title: "Secure outbound SSH and controlled interoperability"
status: closed-partial-private-evidence
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

- [ ] Actual outbound PTY and non-PTY sessions work on both selected endpoint classes, with truthful status/EOF/resize. *Partial at closure: the controlled other-host OpenSSH fixture passes (PTY, resize, exec bytes/EOF/signal/status). The Tier-3 guest class stays blocked.*
- [ ] Host-key/auth failure rejects the connection; real entropy and protected secret/trust handling are demonstrated, no plaintext fallback. *Partial at closure: host-key mismatch, unknown-key confirmation, wrong password, kernel entropy and no secret on serial are proven. Publickey custody (S9-5) is deferred (D79-14).*
- [x] Disconnect after dispatch reports uncertainty and never replays remote effects; backpressure/rekey/cancellation remain bounded. (S9-6: fault cases, no-replay ledger, rekey by bytes/time/peer, bounded peaks, cancel 130.)
- [x] VM route/server readiness is actual evidence; no Intel VMX, C2C authority, inbound server or production readiness is inferred. (QEMU user netdev to a loopback-only fixture; no guest or production claim.)

## Verification

Use library/adapter deterministic boundary regressions and actual SSH interoperability, not mocked success. Packet/rekey/unknown-outcome scenarios require controlled faults. Test server exposure remains loopback/private and separately authorized. An unavailable guest route blocks that scenario explicitly; it cannot be substituted by host localhost or a printed guest label.

## Assumptions

The selected library meets required feature/runtime/crypto contracts, and VFS/entropy/network APIs have admitted safe bindings. Verify exact versions/source and target runtime. Guest server/route availability and host-key persistence are external prerequisites, not implied by ADR approval.

## Security and risk assessment

One ELF/session Cell is not subcommand-specific authorization or stronger isolation. Key/trust mutation and outbound network access retain scoped authority. Rollback restores matched client/library/image but does not undo accepted trust or remote effects; preserve unknown work and never restore the unsafe inbound implementation as fallback.

## Deviation log

2026-10-20 — **Completion/security-policy claims withdrawn; incomplete/blocked.** An SSH argument/dispatch source file exists at `cells/tools/cellbox/src/applets/ssh.rs`; that is not a functioning client or proof of strict host-key verification. Review found a no-op SSH path and no selected Sunset SSH import/integration. No actual protocol, entropy/auth/trust, PTY/non-PTY, rekey, controlled other-host or Tier-3 guest evidence closes CBX-23..25. No remote connection is claimed. User design consent remains recorded; dependency/runtime, owner/allocation and trust-provider admission are incomplete and CP2 pending/not eligible. All criteria remain unchecked and rollout is blocked under [reports/rollout-containment.md](reports/rollout-containment.md).

2026-10-25 — **User decisions before S9-0.**
- **D79-1:** the other-host OpenSSH fixture is a disposable Docker container. It is published only on `127.0.0.1:<random port>`, and the QEMU guest reaches it through user-mode networking at `10.0.2.2`. Keys and host keys are made fresh per run and removed after evidence is captured.
- **D79-2:** the real Tier-3 guest scenario is accepted as explicitly blocked for now and will be unblocked later. Phase 09 proves the other-host class first. It does not substitute host localhost or a labelled container for the guest class.
- **D79-3:** Sunset core at `62b98f3996c88cf920cac1653e1c9e15ddf0d7d0` (0BSD) is vendored into the repository, together with the minimal closure it needs for offline locked builds.
- **D79-4:** `ssh` runs under its own reviewed launch-profile ceiling (`NET_CLIENT` plus `GetRandom`). The shell and the other cellbox commands keep no network authority.
- **D79-5:** host trust uses the candidate VFS trust worker (`abi-vfs-telemetry.md` §trust), not a plain file on `/srv`.
- **D79-6:** the first scope covers password authentication (no echo) and OpenSSH ed25519 private keys.
- **D79-7:** ed25519 private keys stay in KMS custody, as contract §4.1 requires. KMS gains OpenSSH ed25519 import and unlock (unencrypted, or bcrypt plus AES-256-CTR) and session-bound SSH userauth signing. Cellbox uses `--identity KEY_ID` and never reads a key file.
- **D79-8:** slice order: transport first, then trust. The order is S9-1 Sunset, S9-2 authority/entropy/fixture, S9-3 exec and PTY against a preprovisioned host key, S9-4 the trust worker and unknown-host confirmation, S9-5 the KMS key, and S9-6 rekey, timeouts and faults.

2026-10-25 — **CP1 approved (user)** for [phase-09-s0-proposal.md](phase-09-s0-proposal.md). The approval covers S9-1..S9-6, D79-9 (the ssh-only ceiling via a re-launch of `/bin/cellbox`) and D79-10 (the fixture-seeded protected trust store). The phase is now `in_progress`.

2026-10-25 — **D79-9 revised (user), at S9-2 entry.** The kernel picks the launch ceiling from (caller name, route, target **path**) only, never from argv (`kernel/src/loader/launch_profile/mod.rs:1-114`, `syscall.rs:1787-1925`). The syscall allowlist is one section per ELF, and `GetRandom` has no capability gate. So re-launching the same `/bin/cellbox` under an ssh-only ceiling cannot work.
- The user chose a **path alias**: the same cellbox ELF is also packaged at `/bin/cellbox-ssh`.
- Only the cellbox → `/bin/cellbox-ssh` **Path** edge carries `net_resolve` and `net_connect`. The Elf route stays denied, and `/bin/cellbox` keeps no network rights.
- Accepted consequence: `GetRandom` is declared in the one ELF, so the shell can also read entropy. It gains no authority from that.

2026-10-25 — **User decisions at S9-4 entry.**
- **D79-11:** implement the full candidate confirmation design, in which the input owner renders the prompt and captures the answer, and VFS registers and collects the decision.
- **D79-12:** first trust persists only on a new D03 outcome, `CommittedDurable`, backed by crash-lane evidence.

The S9-4a/b/c sub-slice plan is in [phase-09-s0-proposal.md](phase-09-s0-proposal.md) §10 and is waiting for CP1.

2026-10-25 — **D79-12 revised (user), after the S9-4a durability audit.** Default CellosFS is not crash-consistent:
- there is no journal;
- the bitmap is overwritten before the superblock;
- old blocks can be reused before commit;
- there is no mount recovery;
- 4 KiB blocks are written as eight 512 B sectors;
- a timed-out NVMe FLUSH can still complete later.

The user decided: **S9-4a only stores the file**, through the existing D03 `Committed` path ("synced, not crash-proof", as D78-2), and adds the `CommittedDurable` outcome plus a CellosFS durable-commit **function skeleton that reports durability as unavailable**. A complete filesystem with crash safety becomes a **separate plan** ([cellos-fs crash-safety plan](../261025-cellosfs-crash-safety/plan.md)). Until that plan lands, no path returns `CommittedDurable`, and persisted first trust is reported as synced, not crash-proof.

2026-10-25 — **D79-13 (user), at S9-4b entry.** The input owner has no output path: every byte on screen passes through the session controller's relay. The owner-rendered prompt is therefore delivered as a **new control event, `TrustPrompt`**, from input to the session controller.
- The event carries the immutable VFS-registered descriptor: host, port, algorithm and SHA256 fingerprint.
- The controller suspends the child's output relay and renders a fixed prompt built only from that descriptor.
- Input still captures the decision and returns it only to VFS. The ssh child neither draws the prompt nor supplies the decision.

A separate input writer into the output ring was rejected: those bytes would still pass through the controller's relay, so it would add grants without adding assurance.

2026-10-25 — **D79-14 (user), at S9-5 entry: S9-5 deferred.** The survey found:
- KMS has no SSH key class;
- its fixed 128-byte frame needs chunking for a 64 KiB import;
- its protected persistence is deliberately fail-closed (no authenticated root, sealed state or rollback protection);
- `bcrypt-pbkdf` is not in the lock;
- KMS admits only net, net-broker and supervisor callers.

Sunset does support external signing (`SignKey::AgentEd25519` plus `CliEvent::AgentSign`). Until S9-5 resumes:
- `--auth publickey` stays refused;
- pipeline exec and exact non-UTF-8 stdin (S9-3 deviation 3) stay open;
- the publickey part of success criterion 2 stays open.

Work continues with S9-6.

2026-10-25 — **D79-15 (user), at S9-6 entry:**
- (a) The completion channel is kernel kind-1 child-context slot 5 `Completion`, write-only. The child writes `CompletionV1` before each dispatch step (write-ahead). The shell reads the last record and synthesizes `cancelled`/`unknown` itself. A `completion` builtin prints it.
- (b) Loss of the remote transport exits 255. The outcome is `unknown` once the request is dispatched and `local_error` before that. Exit 125 is only for loss of the local terminal session.
- (c) A lane-only `p09-faults` cargo feature adds fault hooks and lowered rekey thresholds (20 s, 2 MiB, 4096 packets). It prints `ssh-test-config rekey=lane`. Production stays at 1 h, 1 GiB and 2^30 packets, with each KEX limited to 30 s.
- (d) Sunset patch 0006 raises the receive window to 65,536 B and the maximum packet to 16,384 B. `--timeout` no longer pauses during trust confirmation, which reverses S9-4c deviation 4.

2026-10-25 — **S9-6 CP2 approved by the user**, including deviations 1–10 (proposal §11). Phase 09 stays in progress: only S9-5 remains, and it is deferred (D79-14).

2026-10-25 — **128-byte line fix, then Phase 09 closed at its current level (user).**
- **Fix.** S9-6 deviation 10 is fixed. The shell line editor accepts up to `SHELL_LINE_MAX` = 4096 bytes. A longer line is refused whole at Enter with `shell: line longer than 4096 bytes; discarded`; it is never run truncated.
- **Tests.** Host test `command_line_overflow_is_refused_whole_never_truncated`. Lane marker `shell-long-line`: a 300-byte line runs exactly, a 4117-byte line does not run, and the next line runs.
- **Lane results.**
  - Phase 09 lane: 103/103 (36 markers), twice. Between the two passes, one start failed in the host Docker fixture (`fixture sshd did not start`) before the guest booted.
  - P04: 459. P02: 48.
  - Evidence: `reports/s9-close-evidence/`.
- **Closed as `closed-partial-private-evidence`.** Criteria 3 and 4 are met. Criteria 1 and 2 stay open:
  - the Tier-3 guest endpoint is blocked;
  - S9-5 (KMS-held key) is deferred to the roadmap entry in `.agents/plan-portfolio.md`, which covers KMS protected persistence.
- **Rollout** stays blocked.

2026-10-25 — **Hash addendum: recorded 2026-10-25 at closure, post-approval state** (Phase 01 closure dossier gap C2-c). Doc-only. This addendum adds no scope or approval. The CP1 (2026-10-25), D79-1..15 and the S9-1..S9-4c and S9-6 CP2s in [phase-09-s0-proposal.md](phase-09-s0-proposal.md) remain the binding records. These SHA-256 values bind the current bytes, after the 4096-byte line fix and the user's partial closure. Shared files (`main.rs`, `executor.rs`, `profiles.rs`, `targets.rs`, `ipc.rs`, `cell_session.rs`) also carry other phases' approved hunks. Tree digest command: `git ls-files --cached --others --exclude-standard DIR | LC_ALL=C sort | xargs sha256sum | sha256sum`.

| Approved proposal / evidence manifest | SHA-256 |
|---|---|
| [phase-09-s0-proposal.md](phase-09-s0-proposal.md) | `64a4801a83815f7ac1f625cbd4104c16f9ae43e555863420ec3282a961b33cd4` |
| `reports/s9-close-evidence/provenance.sha256` | `5bab6bccd916fa4d6528ab93221c3262beb2d02714b2f8fa9fabf10cbd2ebc6a` |

| Key source file | Bytes | SHA-256 |
|---|---:|---|
| `third_party/sunset/` tree (38 files) | — | `c49aec8d2c7cb7cbf3540ff9bbe62191e9b3fdb0e316993ef7a30a6a2b94ef48` |
| `third_party/sunset/Cargo.toml` | 2017 | `1a3316bbe68589c87e45137159d072a7dcabda3bd73bad96316e72f8bfea55b8` |
| `third_party/sunset/PROVENANCE` (patches 0001–0006) | 3737 | `1f7f12d8af67fb4729ff49c467a57b71edaa3cabcadd2beb2efdca7e48717307` |
| `third_party/sunset/LICENSE` | 2963 | `5c7f1cdb02b48c7d2b778da52e12902cd56369220e97f433fce27d7de1229c29` |
| `third_party/sunset/PATCHES/cellos.diff` | 21756 | `71a77afe8eb8692946c1cdc98acc4e862c5fe34e36a9ce8b37c031936b058ab2` |
| `third_party/sunset/PATCHES/upstream-files.sha256` | 2763 | `2ddc94a97be9c7c6e6eabca97008b6d94b983e01996277a9b39bd4f30d380faf` |
| `libs/vissh/` tree (10 files) | — | `1f31ae74a3c06cd11d573f207a48d84f5dafdd1e486bbbb52d8c72a737b8b9f1` |
| `libs/vissh/Cargo.toml` | 626 | `0e57ce657d564d63bedeb72d03350f65940410237596b045dcb7fb50d2a33274` |
| `libs/vissh/src/args.rs` | 7351 | `755c547c2b6623c70a025009795803d55d275c1dd0451ce6073dd9e8c8cb247b` |
| `libs/vissh/src/client.rs` | 4252 | `0b0f3f5d33af2896a93e7b22383070ec8ff5abfe68dee670dc95fdf0c5ea5ad5` |
| `libs/vissh/src/lib.rs` | 855 | `e5634265c5b02e3f97c6c73fcd765520984dbe147c51dd38fbf5a3a2e938609c` |
| `libs/vissh/src/session.rs` | 21806 | `9bd88a8110121e20878182c2739981b48204c29b0ef1ae56657b46717b0b7e20` |
| `cells/tools/cellbox/src/ssh.rs` | 40650 | `6dfe62e946cc1a088da2afa6c3bfa69d2dd0200bfe22e71e3bfa221d9885142a` |
| `cells/tools/cellbox/src/ssh/tests.rs` | 6799 | `aaa7d86c4486a3d28358eba23de00f792aa706c29125e4853a87a59c158cb748` |
| `cells/tools/cellbox/src/main.rs` (shared) | 8576 | `f1590c9cfcd1edb9f2b2f109511414c776d0a9e98dae9d3c209feb48af371716` |
| `cells/tools/cellbox/src/executor.rs` (shared: `completion`, `SHELL_LINE_MAX`) | 172884 | `bcc454f6c5203a7f67ab30205da593e13fdfb5b3841bcc760ee7ba6eeeb9ce55` |
| `cells/services/vfs/src/trust.rs` | 7388 | `7d80a80d97e63cac5a9266450c854f92f7aebf4a5cc3d1a8d654da559653f240` |
| `cells/services/vfs/src/trust/first.rs` | 10383 | `79aeb05c5877f0692f6c8509fb1baacce4e7690469fe0752fad3a95f9a9710fc` |
| `cells/services/vfs/src/trust/tests.rs` | 18631 | `b1324224551e27af8f802c6977825060fa4ca2cd9364ebae1b41fb922e39455e` |
| `kernel/src/loader/launch_profile/profiles.rs` (shared: `/bin/cellbox-ssh` edge) | 14356 | `ded1ea57b6e042f150ff9efc11e42908ceb2edd110e4710e0d8b9ef87ca608bd` |
| `kernel/src/loader/launch_profile/targets.rs` (shared: `NET_CLIENT`) | 5480 | `45834caee5979f219a8df2027958ff6969a5b9d6068da07dea7ce7fb5306a48f` |
| `libs/api/src/abi/cell_session.rs` (shared: SessionV1 29–31) | 21667 | `590fe5ccaa74ae25c751dedb111d76ff6ec102edd52c088198daa28666441595` |
| `libs/api/src/services/ipc.rs` (shared: trust requests 40–42, replies 16/17, `CommittedDurable`) | 48085 | `ca8916570450f794c427d8519efb1e275b686a9e3c90d498a831fc5d936b0072` |
| `kernel/src/task/child_context.rs` (slot 5 `Completion`) | 13028 | `04433254374f65fd1e7a630c9541d8e0fdebd0f8e04e6f276e4ead9410fadf27` |
| `libs/ostd/src/child_context.rs` | 15984 | `3e6847006d2d02cc615d3155087142f340b46041e95842d6ac9802eb878d5433` |
| `scripts/qemu-phase09-ssh-test.sh` | 26732 | `499362c596a90e2d960c97a916cb701b24fff3dd6719a4a478972a9e705c781f` |
| `scripts/phase09_ssh_fixture.sh` | 5924 | `9507f4ed003a5020c672f7e754d100acac6f6d4bb5aefe5fc0b877acb017c1e9` |
| `scripts/phase09_trust_disk.py` | 3683 | `10901b916c948efd62b632e2144e1a3ab519ce8487d324f87e42c60390e6e5e1` |

A later private run, `/tmp/cellbox-phase09-Y12o/evidence/verdict.json` (mtime 2026-10-25 21:29), reports 103/103 (`07a95574164dbc128f278ecdfbcb3069fac2cea60f59816eb8f64aa772e0dd88`). It is not archived under `reports/`.
