# Phase 09 — outbound SSH (`cellbox ssh`): CP1 proposal

Status: **CP1 approved by the user on 2026-10-25.** The approval covers slices S9-1..S9-6 with the file sets in §3, D79-9 (re-launch `/bin/cellbox` under an ssh-only ceiling) and D79-10 (a fixture-seeded protected trust store for S9-3). The user decisions D79-1..8 are recorded in [phase-09-ssh-client.md](phase-09-ssh-client.md) under the deviation log, dated 2026-10-25.

## 1. Entry facts (survey 2026-10-25)

| Area | State | Evidence |
|---|---|---|
| SSH protocol crate | Missing. No Sunset or other SSH crate in `Cargo.lock`, `vendor/` or the registry cache. Upstream `main` is still the reviewed pin `62b98f3`. | `dependency-review.md:9,72,154,168`; `git ls-remote` |
| Crypto crates | Already locked: `x25519-dalek 2.0.1`, `chacha20 0.9.1`, `poly1305 0.8.0`, `sha2 0.10.9`, `ed25519-dalek 2.2.0`, `curve25519-dalek 4.1.3` | `Cargo.lock` |
| TCP/DNS | `ostd::clients::NetClient::{tcp_connect,tcp_send,tcp_recv,tcp_close,dns_lookup}` goes through `service-net`. `Connect` and `Resolve` rights are checked from the caller identity, and `host` resolves to `10.0.2.2`. | `libs/ostd/src/clients/net.rs:49-140`, `cells/services/net/src/rights.rs:51-72`, `dns.rs` |
| Guest→host TCP | Shown by an existing integration test (`nc 10.0.2.2`). | `tests/integration/tests/boot.rs:245-281` |
| Entropy | `GetRandom` (RDSEED, then RDRAND, at most 64 B per call). The weak fallback exists only under `dev-weak-rng`. The fail-closed consumer pattern to copy is `service-net` `tls/rng.rs`. | `kernel/src/task/syscall.rs:8762-8805` |
| Authority | Cellbox has `network=false` and no `GetRandom`. `NET_CLIENT` is `net_resolve + net_connect`. There is no `/bin/ssh` target. | `cells/tools/cellbox/src/main.rs:9-64`, `kernel/src/loader/launch_profile/targets.rs:9-15,73-84` |
| Trust store | Candidate only (`abi-vfs-telemetry.md` §4); nothing in code. | — |
| KMS | `cells/services/kms` exists and refuses whenever a precondition is missing. Its only signing operation is P-256 TLS (`SignTls13ClientCertificateVerify`); it has no ed25519 or SSH. | `cells/services/kms/src/dispatch.rs:177` |
| CLI | The old `applets/ssh.rs` was deleted in Phase 04. The shell has no `ssh` builtin. | `phase-04-s4a-cp2-dossier.md:66-67` |
| PTY bridge | Some primitives exist: `RawInput` (`libs/terminal/src/raw_input.rs:27-100`), foreground spawn/restore (`session.rs:214-272`) and `executor::with_tui`. There is no remote relay. | — |
| Runners | P02 has `-nic none`. P04 uses `-netdev user,restrict=on` and asserts there is no `hostfwd`/`guestfwd`. | `scripts/qemu-phase02-session-test.sh:252-255`, `qemu-phase04-terminal-test.sh:591-600` |
| Inbound `service-sshd` | Unsafe: deterministic keys, a `none` cipher and unconditional auth success. It is not used as a dependency or fixture. | `cells/services/sshd/src/kex.rs:30-106`, `session.rs:195-208` |

## 2. Scope and contract

- The command surface is exactly `command-contracts.md` §4.1–§4.3:
  - `--user`, `--auth password|publickey`, `--identity KEY_ID`, `-p/--port`, `--connect-timeout`, `--timeout` and `--command`;
  - every other OpenSSH flag is refused before any effect;
  - completion is reported on the control channel as `cellbox.ssh.completion` v1;
  - shell status is 0..255, 255, 124, 125 or 130.
- **Wire profile** (§4.2): `curve25519-sha256`, `ssh-ed25519`, `chacha20-poly1305@openssh.com` and compression `none`, with strict KEX. There is no fallback. KEX does not start without real entropy.
- **Out of scope:** forwarding, agent, scp/sftp, jump hosts, X11, config files, reconnect or replay, and the inbound sshd.
- **Tier-3 guest (D79-2):** explicitly blocked. Criteria 1 and 4 stay open for the guest class.

## 3. Slices (D79-8)

Each slice has its own CP1 detail and CP2 evidence. Anything outside a slice's file set is reported as a deviation.

### S9-1 Vendor Sunset and build it for the target (D79-3)
- **Files.**
  - `vendor/sunset/`: core (and derive, if needed) at `62b98f3`, with `LICENSE` (0BSD) and a `PROVENANCE` file holding the commit, the tree hash and the list of removed paths (demos, async, std, server, sftp, ...).
  - Workspace `Cargo.toml` `[patch]` or path dependency, plus `Cargo.lock`.
  - Any missing transitive crates are vendored the same way, each with its license and source hash.
- **Configuration.** `default-features=false`; features `alloc` and `openssh-key` only. The profile is narrowed (§4.2) in the adapter, not by editing Sunset, unless the upstream API cannot express it; that case is listed as a deviation with a patch file.
- **`getrandom`.** A custom backend (`__getrandom_v03_custom`) over `sys_get_random` that loops until the buffer is full and returns an error on zero. It has no fallback.
- **Acceptance.**
  - `cargo build --offline --locked` for the x86 cell target links a throwaway `ssh-probe` that runs the Sunset client state machine against an in-memory transcript.
  - On the host, `cargo deny`/advisory status for the new closure is recorded.
  - Host tests:
    1. a deterministic KEX transcript against a recorded OpenSSH 9.x server exchange is accepted;
    2. a server that offers only refused algorithms gives a negotiation error;
    3. entropy failure aborts before KEXINIT is sent.

### S9-2 Authority, entropy and the Docker fixture (D79-1, D79-4)
- **Launch profile.** A new reviewed target for the `ssh` command, with ceiling `NET_CLIENT` plus `GetRandom`. Shell and other cellbox commands stay without network.
  - The exact mechanism is one of two. Either `cellbox ssh` re-launches `/bin/cellbox` with an `ssh` entry argv under the ssh ceiling, or a separate `/bin/ssh` ELF is built from the cellbox crate.
  - The proposal for CP1 is the **re-launch of the same ELF with an ssh-only ceiling**. It keeps D02's single artifact, and the kernel boot ceiling self-test is extended to show that the shell still has no network.
  - Files: `kernel/src/loader/launch_profile/{targets,profiles}.rs`, `boot_ceiling.rs`, its self-test, and the cellbox manifest/entry.
- **Fixture.** New `scripts/phase09_ssh_fixture.sh` uses `alpine:3.21` pinned by digest with `openssh-server`.
  - Per run it creates an ed25519 host key, a test user with a password, and an authorized ed25519 public key.
  - `sshd_config` restricts the algorithms to the §4.2 profile, so any fallback fails visibly.
  - The container is published as `-p 127.0.0.1:0:22`, and the port is read back.
  - Teardown removes the container and the key material, and records `sshd -ddd` logs as evidence.
- **Runner.** New `scripts/qemu-phase09-ssh-test.sh`, with its work dir under `/tmp/cellbox-phase09-XXXX`.
  - It packages `service-net` and the NIC driver and uses `-netdev user,id=net0` **without** `restrict`. The script asserts that there is no `hostfwd`/`guestfwd`.
  - It reuses the P04 frontend driver pattern (`cells/tests/phase04-terminal-driver`) or a new `phase09` driver.
- **Acceptance.**
  - On the target, `ssh` reaches TCP connect to `10.0.2.2:<port>` and reads the server identification line.
  - The shell's own `curl 10.0.2.2` is still denied.
  - The entropy path shows nonzero bytes.

### S9-3 Exec and PTY against a preprovisioned host key
- **Trust store v1.**
  - A codec in `libs/api` (version, revision, sorted entries `(host, port, alg, key blob, provenance)`) with the §4 bounds: 1,024 entries, 4 KiB per key, 1 MiB in total.
  - A VFS trust-worker **read-only** `TrustLookup` that returns `KnownExact`, `Unknown`, `Mismatch`, `Denied` or `Unavailable`.
  - The store is preprovisioned by `tools/cellos-fs-provision --seed`. The protected placement on `/srv` is denied to common callers.
  - `Unknown` is refused in this slice.
- **Client.** `cells/tools/cellbox/src/ssh/{args,transport,trust,session,exec,pty,completion}.rs` and the shell builtin, help, ledger and catalog rows.
  - The transport adapter carries Sunset over `NetClient`, with bounded scratch: 35,000 B per direction, a 64 KiB window and 64 KiB pending (§4.2).
  - Password authentication prompts once under the D02 no-echo lease, after trust passes.
  - **Exec**:
    - stdin goes over as exact bytes; stdout and extended type 1 stay separate;
    - EOF sends one half-close;
    - the remote status is exact, or the remote signal is reported;
    - `dispatch` moves through `not_sent`, `may_have_run` and `accepted`.
  - **PTY**:
    - TERM is `cellos-vt1` with the SDK size;
    - resize sends `window-change`;
    - `~.` and `~~` escapes are honoured; Ctrl-C is forwarded as a byte;
    - the terminal mode is restored.
- **Acceptance on target against Docker.**
  - Exec: `printf` exact bytes including NUL and invalid UTF-8; stderr kept separate; `exit 7` gives 7; a remote kill gives a signal and 255; a stdin round trip; an EOF half-close.
  - PTY: echo, resize observed remotely with `stty size`, `~.` disconnect, and restore.
  - Refusals: a host-key mismatch (fixture regenerated) is refused before a password prompt; a wrong password gives auth failure without retry; the shell keeps no secret in history or the transcript.

### S9-4 Trust worker: unknown host and durable first trust (D79-5)
- The challenge is bound as §4 describes and expires in 60 s, under the challenge caps.
- D02 service-input gains `RegisterTrustChallenge`, `BeginTrustConfirmation` and `CollectTrustDecision`. Service-input runs the fixed prompt: fresh, non-paste `yes` plus Enter.
- On accept the worker commits through D03 conditional replace of the whole store revision. Only `CommittedDurable` counts.
- **Acceptance (target):**
  - Unknown host, then `yes`, then connect; a reconnect is `KnownExact`.
  - `no`, an expired challenge, a pasted `yes`, a headless session and a forged or replayed receipt are each refused.
  - Two concurrent first-trust attempts never overwrite each other.
  - A changed key never prompts.

### S9-5 KMS ed25519 identity (D79-7)
- **KMS** gains the operations `ImportSshEd25519` and `SignSshUserauth`.
  - Import takes an OpenSSH v1 key of at most 64 KiB, either unencrypted or encrypted with bcrypt (1..64 rounds, 16..64-byte salt) plus AES-256-CTR. The passphrase is read under a no-echo lease. KMS keeps the key in its own storage, sealed as existing KMS records are, and returns a `KEY_ID`.
  - Signing covers exactly the session-bound userauth blob for that session id. KMS never exports the key.
  - Import is done by a separate admin path, such as a `cellbox ssh-key import` builtin or a provisioning tool. The **exact import surface is a sub-decision at S9-5 CP1**.
- **Acceptance (target):**
  - Login with `--auth publickey --identity K` works.
  - An unknown `K`, a wrong passphrase, an unsupported format and a signature for another session are each refused.
  - No key bytes appear in logs or output.

### S9-6 Rekey, timeouts, interruption and faults
- **Rekey.** Thresholds are 1 h, 1 GiB or 2^30 packets. They are tested with a test-configuration build that lowers them (labelled separately), and with a server-initiated rekey (`RekeyLimit` in `sshd_config`).
- **Timeouts.** The connect, auth, request and rekey deadlines and `--timeout` all give 124.
- **Interruption.**
  - Cancellation gives 130 and session loss gives 125.
  - The fixture can stop or pause the container before the request, after `may_have_run`, after `accepted`, and after the exit status but before output drains.
  - Each case reports the right `dispatch` value and `unknown` outcome, and never replays.
- **Backpressure.** A slow local consumer keeps queues bounded.

## 4. Success-criteria mapping

| Criterion | Slices | Note |
|---|---|---|
| 1 PTY/non-PTY on both endpoint classes | S9-3 (other-host) | The guest class stays blocked (D79-2), so the criterion stays open until then. |
| 2 Host-key/auth rejection, entropy, protected secret/trust | S9-1, S9-2, S9-3, S9-4, S9-5 | |
| 3 Uncertainty without replay; bounded backpressure, rekey and cancel | S9-6 | |
| 4 Route/server readiness as actual evidence | S9-2, S9-3 | Other-host only. No VMX, C2C or production claim. |

CBX-23..25 stay pending until Phase 10.

## 5. Risks

- **Sunset API fit.** If the strict-KEX/profile narrowing or the session-bound external signer (needed for KMS signing) cannot be expressed without patching Sunset, the patch is kept as a reviewed diff in `vendor/sunset/PATCHES` and listed at S9-1 CP2.
- **Kernel launch-profile edits.** These touch authority code. The boot-ceiling self-test must still show that the shell, `edit`, `rg` and the other commands have no network authority.
- **D02 trust challenge.** This is the largest new ABI surface. S9-4 may need to split into worker-side and input-side sub-slices.
- **TCG speed.** KEX and bcrypt are slow under TCG. bcrypt rounds in tests stay small (within 1..64), and timeouts are judged with test-configuration values.
- **Docker.** The daemon must be reachable. The fixture never publishes on a non-loopback address.

## 6. Requested decisions for CP1

1. Approve slices S9-1..S9-6 and their file sets as above.
2. **D79-9 (proposed):** `ssh` authority is a re-launch of `/bin/cellbox` under an ssh-only reviewed ceiling. The alternative is a separate `/bin/ssh` ELF built from the same crate.
3. **D79-10 (proposed):** for S9-3, the preprovisioned trust store is seeded into a protected `/srv` placement by the fixture provisioner. Production bootstrap and issuer policy remain an open owner decision and are not claimed.

## 7. S9-1 results (2026-10-25) — CP2 approved by the user on 2026-10-25, including deviations 1–5
- **Vendored source** in `third_party/sunset/`: Sunset 0.6.0 core and `sshwire-derive` 0.3.0 at `62b98f3` (tree `c37df41e`, 0BSD).
  - `PROVENANCE` lists what was kept, what was removed and how the manifest was rewritten.
  - `PATCHES/upstream-files.sha256` holds the SHA-256 of all 34 kept upstream files.
  - `PATCHES/cellos.diff` (142 lines) is the complete difference from upstream. Only `src/kex.rs`, `src/lib.rs`, `src/random.rs` and the derive manifest differ.
- **Patch 0001 (entropy).** `getrandom` is removed. `sunset::set_entropy_source(fn(&mut [u8]) -> bool)` installs one source; the first installation wins and is guarded by `spin::Once`.
  - With no source installed, or after a failed draw, `fill_random` errors and key generation panics (as upstream `UnwrapErr` does).
  - `#![forbid(unsafe_code)]` stays in place.
- **Patch 0002 (profile).** KEX `curve25519-sha256` only (the `@libssh.org` alias is dropped) and cipher `chacha20-poly1305@openssh.com` only (`aes256-ctr` is dropped). A peer that does not offer `kex-strict-s-v00@openssh.com` is refused.
- **New crate `libs/vissh`.** It re-exports Sunset and provides `install_entropy`; S9-3 adds the transport and session there.
  - Host tests (8/8) with `--target x86_64-unknown-linux-gnu`:
    - the client's first KEXINIT offers exactly `curve25519-sha256` plus the marker names, `ssh-ed25519`, chacha20-poly1305 in both directions, and compression `none`;
    - against a Sunset server, the right password authenticates and the password is asked only after the host-key check;
    - a wrong password never authenticates;
    - a rejected host key ends the session before any password is asked;
    - a scripted OpenSSH KEXINIT is refused when it offers only `diffie-hellman-group14-sha1`, the libssh alias and AES-CTR, and when it lacks strict KEX; the same KEXINIT with the profile is accepted;
    - no source and a failing source each send no KEXINIT, and the failing source cannot be replaced afterwards (separate test binary).
- **Target build.**
  - `cargo build --offline --locked --release --target x86_64-unknown-none -p vissh` passes.
  - A throwaway cell `ssh-probe` (`GetRandom` entropy in 64-byte chunks, client runner up to KEXINIT) linked as a 163,912-byte PIE with `-C relocation-model=pic -Z build-std=core,alloc`. The ELF is in `reports/s9-1-evidence/`. The probe crate was then removed from the workspace.
- **Live interop smoke (throwaway, not kept as a test).** A host test client reached `authenticated` against a disposable `alpine:3.21` container running OpenSSH_9.9p2 with `KexAlgorithms curve25519-sha256`, `Ciphers chacha20-poly1305@openssh.com` and `HostKeyAlgorithms ssh-ed25519`. sshd logged `Accepted password for tester`. Container and test file were removed.
- **Dependency closure.** 38 registry crates were added to `Cargo.lock`. No existing entry was removed and no `getrandom` 0.4 remains.
  - The additions are the RustCrypto 0.11/0.5 generation (`digest`, `cipher`, `sha2`, `hmac`, `aes`, `ctr`, `poly1305`, `chacha20 0.10`, `hybrid-array`, ...), `curve25519-dalek 5.0.0`, `ed25519-dalek`/`x25519-dalek 3.0.0`, `ssh-key 0.7.0-rc.11` with `ssh-cipher`/`ssh-encoding`, `snafu 0.9.2`, `heapless 0.9`, `ascii`, `virtue 0.0.17` and `fiat-crypto 0.3.0`.
  - `cargo deny check advisories licenses` reports only the two known pre-existing findings (ocel-pdf AGPL; RUSTSEC-2026-0387 in `crypto-bigint 0.5.5` via p256). There is nothing for the new closure.
- **Deviations (for CP2).**
  1. D79-3 said "together with the minimal closure". Only Sunset and its derive crate are copied into the repository. The 38 registry crates are pinned by `Cargo.lock` checksums and the local cargo cache, as every other workspace dependency is.
     - `cargo deny --offline` needs `fiat-crypto 0.3.0` downloaded once (it is a target-conditional dependency).
  2. §3 S9-1 planned a `getrandom` custom backend. Patch 0001 replaces it because the backend is selected by a global `--cfg`, which would have to go into the gitignored `.cargo/config.toml` and the CI flags block.
  3. The profile is narrowed inside Sunset (patch 0002), not in an adapter, because Sunset has no API for algorithm selection (`kex.rs` "TODO this will be configurable").
     - `hmac-sha2-256` stays in the MAC name-list: Sunset negotiates the list, but with the only cipher being an AEAD it is never used.
  4. The planned "recorded OpenSSH transcript" host test is replaced by the in-memory Sunset server and scripted OpenSSH KEXINIT tests above, plus the live OpenSSH smoke. A recorded transcript cannot be replayed, because each side's ephemeral key and cookie are random.
  5. `ssh-key` is a release candidate (`0.7.0-rc.11`), as pinned upstream. It matters only for the OpenSSH key format path (S9-5).

## 8. S9-2 results (2026-10-25) — CP2 approved by the user on 2026-10-25, including deviations 1–5
- **Authority (D79-9 as revised by the user at S9-2 entry: path alias).**
  - `cellbox_profile` gives `LaunchRoute::Path` to `/bin/cellbox-ssh` exactly `NET_CLIENT` (`net_resolve` + `net_connect` + the legacy rights). The ELF route stays unknown, so it is denied.
  - `CapSet::with_path_net_rights` requests the client pair for `/bin/cellbox-ssh`, and `scripts/sign-policy.py` gains the matching row (no direct NIC right).
  - `targets::NET_CLIENT` is now `pub(super)`.
  - New kernel host test `cellbox_ssh_edge_is_shell_only_path_only_client`:
    - the exact ceiling;
    - request ∩ ceiling keeps the pair;
    - `/bin/cellbox` requests none;
    - the ELF route is denied;
    - init, desktop, cellbox-ssh and terminal callers are denied on both routes.
  - The boot-ceiling self-test asserts the Path edge and the denied ELF route on the target.
  - Results: kernel host tests 277/277; the self-test reports PASS in the lane.
- **Cellbox.**
  - The manifest is unchanged (`network = false`); `GetRandom` is added to the one syscall list.
  - `main` reads its launch context first and takes the ssh entry only when argv[0] is `ssh`.
  - The `ssh` builtin validates argv with `vissh::args` (exit 2 before any launch) and runs `/bin/cellbox-ssh ssh ...` in the foreground of the shell's session (`spawn_serial_foreground`, now `pub(crate)`).
  - The child installs kernel entropy (whole 64-byte `GetRandom` chunks or failure), resolves the host (IPv4 literal, or a name through `NetClient::dns_lookup`; IPv6 is refused), connects, and runs `vissh::client::probe_host_key`.
    - KEX runs up to the host-key check, then the key is refused, because no trust store exists until S9-3.
    - The result is exit 255 with `host key for HOST:PORT not verified (no trust store): ssh-ed25519 SHA256:...`, or 124 on the connect deadline.
  - Wiring: `typed.rs` makes `ssh` with valid argv a Session (bad argv stays Builtin); BUILTINS, help, ledger (59) and catalog are updated.
  - Host tests 178/178. Cellbox text 2,535,111 B (+180,139).
- **`libs/vissh`.**
  - `args`: the full §4.1 argv contract.
  - `client`: `Transport`, `pump`, `HostKey::{blob, fingerprint}` and `probe_host_key`.
  - Host tests 14/14:
    - the argv forms and 23 refusals;
    - IPv6 canonical form;
    - the fingerprint equals `ssh-keygen -l` for a fresh key;
    - against a Sunset server, `probe_host_key` returns the exact key and no authentication follows;
    - a silent peer times out;
    - plus the S9-1 tests.
- **`ostd` NetClient.** New `tcp_send_some` reports the exact accepted byte count, where `tcp_send` returned `WouldBlock` with the count lost. New `tcp_state` returns the SocketState byte.
- **Fixture and lane.**
  - `scripts/phase09_ssh_fixture.sh` starts `alpine@sha256:ce64758a…` with OpenSSH_9.9p2. It makes a fresh ed25519 host key and password per run, uses only the cellbox algorithms, publishes on `127.0.0.1:<random>` only, and removes keys and container at `stop`.
  - `scripts/qemu-phase09-ssh-test.sh` builds a private signed image with `/bin/cellbox` and the same signed file at `/bin/cellbox-ssh`, plus `/p09/ssh.conf` (`PORT FP`). QEMU gets one `-netdev user,id=net0` with no `restrict` and no forwards, on `qemu64,+rdrand,+rdseed`.
  - The driver is the Phase04 terminal driver built with the new `phase09-ssh` feature (`src/ssh.rs`); it waits for the DHCP lease through `ifconfig` first.
  - Lane result: **20/20 checks, 8 markers, PASS twice in a row** (logs in `reports/s9-2-evidence/`):
    - argv refusals with status 2 and no host key;
    - the fixture's exact fingerprint over `10.0.2.2` and over the name `host`, with exit 255, about 2.2 s each on TCG;
    - closed port 1 gives 255/124 with no key;
    - the shell is still alive;
    - the boot-ceiling self-test passes;
    - sshd logged guest connections and **no** authentication attempt;
    - no `WEAK xorshift32` entropy warning, no key material, no panic.
- **Regression lanes (private TCG):** Phase04 terminal 459/459 (174 markers, with the updated authority digests), P02 48, P03 67.
- **Failed attempts on the way:**
  1. `tcp_recv` is refused while the socket cannot receive yet (SynSent). The client first read that as "closed". It now reads the socket state after a refusal and treats SynSent, SynReceived and Established as "nothing yet": a handshake that completes between the two calls reads Established.
  2. The first attempt ran while DHCP was re-acquiring the lease after an early NIC-driver reply timeout, and timed out (124). The lane now waits for the lease before connecting.
- **Deviations (for CP2).**
  1. D79-9 is implemented as the user's revised path alias. `GetRandom` is in the one cellbox syscall list, so the shell can also read entropy. `scripts/cellbox-ceiling-diff.py` allows `GetRandom` by default, as it already allowed `Write`.
  2. Files outside the S9-2 set in §3:
     - `libs/ostd/src/clients/net.rs` (`tcp_send_some`, `tcp_state`);
     - `scripts/sign-policy.py` (policy row);
     - `scripts/cellbox-ceiling-diff.py` (allowed addition);
     - `scripts/qemu-phase04-terminal-test.sh` (the pinned authority digests of `profiles.rs`, `targets.rs` and `tests.rs` are updated for the new edge);
     - the Phase04 terminal driver (the `phase09-ssh` feature).
  3. The lane reuses the Phase04 driver behind a feature instead of a new driver crate.
  4. The S9-2 acceptance "the shell's own `curl 10.0.2.2` is still denied" is shown as the kernel host test plus the target boot-ceiling self-test (`/bin/cellbox` requests and holds no net right). `curl` is a separate reviewed `NET_CLIENT` target, so it does not show the shell's own rights.
  5. The `ssh` builtin's argv is validated twice: by the shell, so that no launch happens on bad argv, and by the child.

## 9. S9-3 results (2026-10-25) — CP2 approved by the user on 2026-10-25, including deviations 1–6
- **Trust store, read-only (D79-10).**
  - New `VfsRequest::TrustLookup { host, port, algorithm, key }` (wire 40) with reply `VfsResponse::Trust(u8)` (wire 16); the codes are `api::ipc::ssh_trust`: KnownExact, Unknown, Mismatch, Denied, Unavailable. The request is classified as neither path-addressed nor mutating.
  - `cells/services/vfs/src/trust.rs` reads `/srv/.cellos-trust/ssh_known_hosts` (store format v1, documented in the module) through `VfsManager` and answers only the code.
    - The caller must hold the attested `CALLER_FLAG_NET_CONNECT`; otherwise the answer is Denied.
    - The bounds are 1,024 entries, 4 KiB per blob and 1 MiB in total; the host must be canonical, with one line per host:port.
    - Any malformed store makes the whole store Unavailable. An absent store means Unknown.
  - New access rules deny every caller list, read and write on `/srv/.cellos-trust` (exact and prefix).
  - `ostd::VfsClient::trust_lookup` is the client call.
  - Host tests: VFS 103 (3 new: exact/unknown/mismatch/denied plus no cross-port or cross-name authorization; the store cannot be read, listed or unlinked by any caller; 17 malformed forms plus the entry bound), api wire test (40/16).
- **Session (`libs/vissh::session`).** `run(transport, local, user, mode)` drives the whole connection:
  - the host key goes to `Local::trust` before any credential, and only KnownExact continues;
  - the password is asked once, with no retry; the server's "no more methods" is reported as Auth;
  - a PTY shell (`cellos-vt1`, SDK size, `window-change`) or one exec string;
  - stdout and extended data are kept apart in exec mode and merged into one terminal stream in PTY mode;
  - stdin, one EOF, `~.`;
  - `Dispatch` goes NotSent, then MayHaveRun, then Accepted on the request reply; the outcome records exit status or signal.
  - `probe_host_key` (S9-2) is removed; the silent-peer timeout test moved to `tests/session.rs`.
  - vissh host tests 16/16. The four new tests use an in-memory Sunset server: every non-exact trust outcome ends before any password; a wrong password is tried once and nothing runs; exec carries the exact command string, `a\0\xff` stdin and its EOF, with stdout/stderr apart and dispatch Accepted; a silent peer times out.
- **Sunset patches** (`PATCHES/cellos.diff`, now 298 lines; PROVENANCE updated):
  - 0003: shell/exec/subsystem requests ask for a reply, kept as `Runner::channel_request_reply`; new `Runner::send_channel_eof`.
  - 0004: an upstream bug in `Runner::progress` completed "the previous event's payload" even for events without one (`Authenticated`, extra events). That silently discarded the next packet; it showed up as a server that never saw the client's ChannelOpen. Fixed by pairing events with payloads.
- **Cellbox child (`/bin/cellbox-ssh`).**
  - It requires a terminal session (`TerminalSession::from_own_context`); `--auth publickey` is refused until S9-5.
  - Trust comes from `VfsClient::trust_lookup` with the configured host spelling.
  - The password prompt `user@host's password: ` runs under the echo-off lease. Typed text only (no paste), Escape cancels, at most 1,024 bytes; the secret is wiped after use.
  - PTY mode takes the SDK `RawInput` lease and forwards raw bytes, applying the `~.` / `~~` escape at line start. A `WouldBlock` from stale typed input drops that input and retries.
  - Exec from the terminal reads canonical events: Text, Enter becomes LF, and Ctrl-D (a key, not an Eof event) becomes channel EOF.
  - Deadlines: connect, then 120 s for auth once the host key is verified, then none, always bounded by `--timeout`.
  - Status and message come from `ssh::report`:
    - the remote status 0..=255 is the local status; anything else is 255;
    - a signal gives 255 with `signal NAME`;
    - `HOST KEY CHANGED ... refused`, `is not trusted`, `authentication failed` and `disconnected` give 255;
    - a timeout gives 124.
  - Host tests 180 (2 new: escape, report). Cellbox text 2,583,767 B (+48,656); service-vfs text 477,363 B (+3,696).
- **Lane.**
  - New `scripts/phase09_trust_disk.py` writes the NVMe fixture disk: the lane config on P1, and on P5 a store with the real key for `10.0.2.2:PORT` and a different key for `host:PORT`. `gateway` (also 10.0.2.2) has no entry.
  - The runner attaches the disk and passes the fixture password to the driver in `/p09/ssh.conf`.
  - **Phase 09 lane 29/29 checks, 14 markers, PASS twice in a row:**
    - refusals (including `ssh ... | cat`);
    - `gateway`: not trusted, no prompt; `host`: HOST KEY CHANGED, no prompt;
    - wrong password: authentication failed, the command never ran;
    - exec: exact `a\0b\xffc`, `E` only on stderr, status 7;
    - typed stdin is echoed back by remote `cat`, then Ctrl-D ends it: `done`, status 3;
    - `kill -TERM $$` gives 255 with `signal TERM`;
    - PTY: `T=cellos-vt1` and `24 80`, a resize to `30 100`, then `~.` gives `disconnected` with 255;
    - the shell is still alive.
  - Independent checks: sshd logged exactly 4 `Accepted password` and exactly 1 `Failed password`; the CellosFS P5 volume mounted; the password appears nowhere in the raw serial log (text or hex); no weak-entropy warning, no panic.
- **Regression lanes (private TCG):**
  - Phase04 terminal 459/459; P03 67; P02 kernel lane 39/39 with 7/7 VFS self-tests.
  - P02 48 on the second run. The first boot never reached the shell prompt, alongside `X86-VMM-SMOKE: FAIL`. The kernel `COMPLETION-QUEUE` self-test also reported FAIL in that boot, but it reports FAIL in the passing rerun too, so it is not the cause. This matches the TCG boot flake recorded in S8-5.
  - Host: api 151, ostd 93, VFS 103, cellbox 180, vissh 16. Logs are in `reports/s9-3-evidence/`.
- **Failed attempts on the way:**
  1. Driver checks first matched the echoed command line ("never", "E") and the usage text ("password"). They now look only after the echo line and for `password: `.
  2. The foreground child's canonical input is not echoed locally, so typed text appears once, from the remote side.
  3. Ctrl-D arrives as a key event, not as Eof; it is now mapped to channel EOF.
  4. The first PTY attempt took the lease through `set_mode(raw|echo)`. The owner refused it because of the unread key-release events from the password Enter, and the client then panicked on its missing session slot. It now uses `RawInput` and drops stale input on `WouldBlock`.
  5. The trust disk run failed once because a provisioner receipt from the previous run was still there. The runner now removes it with the disk.
- **Deviations (for CP2).**
  1. Authorization for `TrustLookup` is the attested `net_connect` caller flag, not the contract's separate `Vt` right. The VFS has no attested program identity, so it cannot single out `/bin/cellbox-ssh` (`access/rules.rs` module note). Only cells with outbound-connect authority can ask, and the answer reveals one code, never the store.
  2. The store location `/srv/.cellos-trust/` and the v1 text format are this slice's choice. They are protected by new access rules, not by a separate trust worker process. The challenge/commit parts of the worker remain S9-4.
  3. Pipeline exec (`... | ssh --command ...`) is refused in this slice: a pipeline stage has no terminal for the password. Exact non-UTF-8 **stdin** bytes need it and move to S9-5 (publickey); exact stdout bytes are proven here.
  4. The completion record `cellbox.ssh.completion` (§4.3) is not emitted yet. `Dispatch` and the ending are tracked in `Outcome`, and the shell status follows §4.3. Emission is moved to S9-6 together with the uncertainty cases.
  5. Two Sunset patches (0003, 0004) beyond §3's S9-3 file set; 0004 fixes an upstream bug.
  6. `ssh-closed-port` from S9-2 is not in this lane any more (timeouts are S9-6).

## 10. S9-4 entry (2026-10-25) — CP1 approved by the user on 2026-10-25 (S9-4a/b/c as below)
**User decisions.**
- **D79-11:** S9-4 implements the candidate design in full (abi-vfs-telemetry §4):
  - VFS registers the challenge with service-input;
  - the client begins confirmation bound to its live session, lease and epoch;
  - the **input owner itself renders** the fixed prompt and captures a fresh typed `yes`+Enter (no paste);
  - VFS collects the attested decision and commits the store.
- **D79-12:** first trust persists only on a new, stronger D03 outcome **`CommittedDurable`**. The existing `Committed` ("synced, not crash-proof", D78-2) is not enough for trust.

**Entry facts (survey 2026-10-25).**
- service-input has SessionV1 selectors 1–28, authenticated by the kernel caller trailer plus session/lease/epoch (`cells/services/input/src/session.rs:168-250,344-578`). It has no challenge, owner-run prompt or capture operation.
- The input owner writes only child stdin/control streams, never terminal output.
- VFS declares `Send`/`LookupService` but not `LookupServiceBound`/`CellSessionV1`; input admits only session participants.
- D03 has `Committed`, `Conflict`, `NotPublished` and `Indeterminate` (`libs/api/src/services/ipc.rs`). CellosFS publication ends in `fs.sync()` (`backend_cellosfs.rs:307`).

**Proposed sub-slices (each with CP2).**
- **S9-4a `CommittedDurable`.**
  - Define the guarantee: after the reply, a power loss leaves the new store bytes, and before the reply it leaves the old or the new, never a torn mix. Audit and, if needed, fix the CellosFS commit order and the NVMe flush path (a device cache flush acknowledged before the reply).
  - Add the `SaveOutcome::CommittedDurable` variant, appended to the wire.
  - Evidence: a private **crash lane**. QEMU is SIGKILLed at many random points during repeated store commits with a write-back cache, then cold-booted, and the store is checked: every commit acknowledged before the kill survives, and nothing is ever torn.
- **S9-4b input owner confirmation.**
  - Three appended SessionV1 selectors:
    - `RegisterTrustChallenge`, admitted only from the bound live VFS service;
    - `BeginTrustConfirmation`, admitted only from the attested foreground child, with exact session, lease and epoch;
    - `CollectTrustDecision`, admitted only to the registering VFS, once.
  - A new owner output path draws the fixed prompt (host, port, algorithm, SHA256 fingerprint) on the session output.
  - Capture: owner-held input with paste refused, exactly `yes`+Enter accepts, anything else rejects. A 60 s expiry that is never extended, and baseline restore on every exit.
  - VFS gains `LookupServiceBound`/`CellSessionV1` to call input. The bounds are those of §4: one challenge per caller, 4 per worker, 32 system-wide.
  - Evidence: input host tests (forged, wrong-epoch, pasted, expired, replayed and wrong-worker cases) and the P02/P04 lanes unchanged.
- **S9-4c VFS challenge and commit, plus the client.**
  - `TrustLookup`'s Unknown result carries a challenge ref; new VFS requests begin the challenge and then complete it.
  - On an attested Accept, VFS re-reads the store, rejects on a store revision change, and commits through D03 with `CommittedDurable` only.
  - `cellbox ssh` asks VFS for the challenge and asks input to confirm. It continues only on the VFS receipt; Mismatch never prompts.
  - Lane: unknown `gateway` plus `yes` connects and the reconnect is KnownExact. `no`, a paste, expiry, a headless session, a forged receipt and two concurrent first trusts are each refused, and nothing is overwritten.

**Risks.**
- S9-4a can show that CellosFS or the NVMe driver needs real ordering or flush changes; crash experiments under TCG are slow.
- S9-4b changes the input-owner ABI, which is already in use by the shell, GUI Terminal and frontends; the Phase04/GUI lanes must stay green.

### 10.1 S9-4a results (2026-10-25) — scope revised by the user (D79-12 revised); CP2 approved by the user on 2026-10-25, including deviations 1–2
- **Audit.** The default CellosFS is not crash-consistent. There is no journal or mount recovery; the bitmap is overwritten before the superblock; replaced extents can be reused before commit; 4 KiB blocks are written as eight 512 B writes; a timed-out NVMe FLUSH is not aborted. The full findings, with file:line, are in the new separate plan [`.agents/261025-cellosfs-crash-safety/plan.md`](../261025-cellosfs-crash-safety/plan.md).
- **What changed, as the user decided** (store the file now; a function skeleton for CellosFS; the full filesystem in its own plan):
  - `api::ipc::SaveOutcome::CommittedDurable { inode, generation }` is appended, with the helper `SaveOutcome::published()`.
    - VFS `save.rs` and `conditional_save.rs`, the VFS self-test and the cellbox editor store treat both successful outcomes alike through `published()`.
    - No backend returns the new variant.
  - `CellosFs::sync_durable() -> Result<(), DurabilityUnavailable>` (default engine and the `g2-cow-experiment` wrapper) always refuses and writes nothing.
  - The `cellos-fs` crate doc no longer claims "Crash-Resilient … power-loss immunity".
- **Tests.** New `durable_commit_is_refused_and_the_volume_stays_usable`. Host results: cellos-fs 33 (default) and 39 (`g2-cow-experiment`), VFS 103, cellbox 180, api 151. The target build of cellbox and VFS passes.
- **Consequence for S9-4c.** First trust persists through D03 with `Committed` (synced, not crash-proof) and is reported that way. The S9-4c acceptance "CommittedDurable only" moves to the crash-safety plan.
- **Deviations (for CP2).**
  1. The approved S9-4a (durable protocol plus crash lane) is replaced by the user's narrower scope above.
  2. `cellos-fs` `lib.rs` doc correction.

### 10.2 S9-4b results (2026-10-25) — CP2 approved by the user on 2026-10-25, including deviations 1–6
- **What landed** (see CHANGELOG):
  - SessionV1 selectors 29 `RegisterTrustChallenge` (live VFS task only), 30 `BeginTrustConfirmation` (the descriptor's target as the committed foreground reader, with the exact lease and epoch) and 31 `CollectTrustDecision` (the registering principal; the final decision exactly once). The records are in `api::cell_session::trust`.
  - The input-owner capture is a three-phase quarantine:
    - **Arming:** until the controller acknowledges the prompt, every input is dropped and none counts.
    - **Armed:** only typed key/text events build `yes`+Enter; paste, SubmitRaw bytes, Ctrl-C, Escape or any other line rejects.
    - **Ending:** input is dropped until the controller acknowledges the end.
  - Expiry is 60 s and never extended. Restore, close, frontend loss, EndInput and a broken controller control ring all release; an undecided capture becomes cancelled. Begin discards input still queued for the child and answers its waiting poll.
  - Bounds: 1 per target, 4 per registrar, 32 in total.
  - D79-13 owner notices `TrustPrompt`/`TrustPromptEnd` (control kinds 9/10) go to the controller's own control ring. `TerminalSession::{next_owner_notice, acknowledge_owner_notice, begin_trust_confirmation}`.
  - The cellbox foreground relay suspends child output, draws the fixed prompt from the notice and acknowledges it, retrying the acknowledgement with backoff. If it gives up, it ends the line and keeps retrying `cancel_foreground` until it is done.
  - Client helpers `ostd::session::{register_trust_challenge, collect_trust_decision, trust_descriptor_sha256}`. VFS declares `LookupServiceBound`, the IPC exchange syscalls and `GetTime`.
- **Review.** An independent reviewer did three rounds. Fixed:
  - the prompt/capture race at begin and end (controller acknowledgements);
  - raw bytes and unframed physical-UART paste answering the prompt (raw never answers; serial sessions get `Unsupported`);
  - input queued before Begin leaking to the child;
  - incremental decoder state surviving release;
  - a failed notice acknowledgement leaving quarantine stuck;
  - a failed control-ring write leaving quarantine stuck;
  - a single-shot cancel after a handshake failure.
- **Tests.** Host: service-input 48 (trust 13), app-cellbox 183, terminal-sdk 53, api 150, vitui 24, terminal app 64, service-vfs 103, ostd 67. The target builds pass.
- **Lanes (private TCG):**
  - GUI 290;
  - P02 48; kernel lane 39/39 with 7/7 VFS self-tests; P03 67;
  - Phase 09 29/29, so the S9-3 sessions are unchanged.
  - Phase04 terminal first stopped at its pinned authority digest for `libs/api/src/abi/cell_session.rs`, which is the approved selector change. The digest was updated with a comment, and the rerun gives **459/459** (174 markers). Logs are in `reports/s9-4b-evidence/`.
- **Deviations (for CP2).**
  1. **Accepted limitation:** the attached frontend holder is the session's input authority (D02). A holder that synthesizes `yes`+Enter answers the prompt, as it can for any keystroke. Documented in `session_trust.rs`.
  2. **Accepted limitation:** bytes that input wrote to the child's kernel stdin ring before Begin stay readable during the prompt. The kernel exposes no ring occupancy (`kernel/src/task/cell_session.rs:873-881`); closing this needs a kernel query or a stream replacement.
  3. Confirmation is `Unsupported` on serial and headless sessions (no event provenance). First trust therefore needs a frontend session such as the GUI Terminal or a FrontendHost.
  4. If the shell never acknowledges the end notice, quarantine lasts until restore, close, frontend loss or EndInput. There is no timeout on that wait.
  5. VFS gains a new allowlist bit for `GetTime`.
  6. `scripts/qemu-phase04-terminal-test.sh`: the pinned digest of `cell_session.rs` is updated.

### 10.3 S9-4c results (2026-10-25) — CP2 approved by the user on 2026-10-25, including deviations 1–9
- **VFS first trust** (`cells/services/vfs/src/trust/first.rs`, `save::publish_owned`):
  - New `VfsRequest::TrustChallenge` (41) with reply `TrustChallenged` (17), and `TrustCommit` (42) answered with `Trust(u8)`. New codes `REFUSED` 5, `EXPIRED` 6, `PENDING` 7.
  - **Challenge:** only while the lookup is Unknown, and only for callers with the attested outbound-connect right.
    - The record binds caller, host, port, algorithm, full key, store observation, session, epoch, the descriptor SHA-256 and a 60 s deadline.
    - It is registered with input (S9-4b) with the vissh-identical `SHA256:` fingerprint. Bounds: 1 per caller, 4 in total.
  - **Commit:** only by the same caller.
    - An Accept must match every binding and VFS's own deadline.
    - The store is re-read: another writer's entry for the same host and port wins (same key gives KnownExact, a different key gives Mismatch, never an overwrite). Otherwise the line `HOST PORT ssh-ed25519 BASE64 interactive` is appended and published through D03 on VFS's own authority, with one retry on Conflict.
    - Only `Committed` counts, reported as synced, not crash-proof (D79-12 as revised). Indeterminate or NotPublished gives Unavailable.
- **Client.**
  - `vissh::Local::confirm_unknown` (only Unknown, only KnownExact continues, nothing sent before).
  - The cellbox ssh child calls `TrustChallenge` with its own session and epoch, then `begin_trust_confirmation`, then polls `TrustCommit`. The confirmation time is excluded from the connect deadline and from `--timeout`; auth gets a fresh 120 s afterwards.
  - Success prints `Host '…' (…) added to the trust store (synced, not crash-proof).`
  - Serial and headless sessions are refused with `confirmation needs an interactive frontend`.
- **Host tests:** service-vfs 109 (6 new), api 153, vissh 17, app-cellbox 184, service-input 48, terminal-sdk 53, ostd 93.
- **Lanes (private TCG):**
  - **Phase 09 32/32 (17 markers), PASS twice.** New markers:
    - `ssh-first-trust-no`: the owner prompt is shown with the fixture fingerprint and no password prompt; `no` refuses with 255.
    - `ssh-first-trust-paste-refused`: a paste is refused by input (Cancelled) and the confirmation is rejected.
    - `ssh-first-trust-yes`: typed `yes`, then the password, `trusted-ok`, status 0, "added to the trust store".
    - `ssh-first-trust-known-after`: the reconnect has no prompt and gives `again-ok`.
    - sshd logged exactly 6 accepted passwords and 1 failed.
  - Phase04 terminal 459/459 (with the `cell_session.rs` digest re-pinned for the ObjectRefV1 serde derive); GUI 290; P02 48; kernel lane 39/39 with 7/7 VFS self-tests; P03 67.
  - Logs are in `reports/s9-4c-evidence/`.
- **Failed attempt on the way.** The driver first treated the `Cancelled` returned by `submit_paste` during a capture as a failure. That is input's documented refusal, so the driver now accepts it and checks the refused outcome.
- **Deviations (for CP2).**
  1. The commit reply reuses `Trust(u8)` rather than a new variant.
  2. Bounds and malformed requests answer `Err(ERR_QUOTA)` / `Err(0xFF)`.
  3. Commit always re-reads the store.
  4. Confirmation time is also excluded from `--timeout`.
  5. If Begin fails (for example Unsupported on serial), that caller's VFS challenge stays until its 60 s deadline. A retry inside the window reports "host trust store unavailable" (QUOTA).
  6. Challenges do not survive a VFS restart.
  7. VFS's call to input can reach the SDK retry path that calls `SetTimer`, which VFS lacks; a transport failure would log the kernel's SetTimer-denied warning. It was not observed in the lanes.
  8. Persistence is `Committed` only (synced, not crash-proof), per D79-12 as revised; `CommittedDurable` waits for the crash-safety plan.
  9. The P04 runner's pinned digest of `cell_session.rs` is updated again.

## 11. S9-5 deferred and S9-6 results (2026-10-25) — S9-6 CP2 approved by the user on 2026-10-25, including deviations 1–10

- **D79-14 (user).** S9-5 is deferred. KMS has no SSH key class, its frame is 128 bytes, its protected persistence is fail-closed, `bcrypt-pbkdf` is not in the lock, and it admits only net, net-broker and supervisor callers. `--auth publickey`, pipeline exec and exact non-UTF-8 stdin stay open.
- **D79-15 (user).**
  - (a) Completion uses kernel slot 5, write-only.
  - (b) Transport loss exits 255, with outcome `unknown` after dispatch and `local_error` before.
  - (c) The `p09-faults` feature is lane-only.
  - (d) Window 65536 and max packet 16384; `--timeout` no longer pauses.
- **Results.**
  - Phase 09 lane: 102/102 (35 markers), twice in a row.
  - Regression: P04 459, GUI 290, P02 48, kernel 39/39 with 7/7 VFS self-tests, P03 complete.
  - Evidence: `reports/s9-6-evidence/`.
- **Deviations.**
  1. Sunset's rekey ceiling has no Sunset unit test, because its test build has a pre-existing `simplelog` error. It is covered through vissh.
  2. The record is shown by the shell builtin `completion` as a key=value line, not as an engine control-channel row. The builtin ledger goes from 59 to 60.
  3. A child with no record is settled as `local_error`/`no_record`.
  4. Before argv parses, the mode is guessed from `--command`.
  5. The error codes are names chosen by the implementation.
  6. Session loss (exit 125) is Stale, Revoked, BrokenPipe, WrongSession or WrongEpoch.
  7. Patch 0002 is narrowed: the strict-KEX marker is required in the initial KEXINIT only, which matches OpenSSH behaviour on rekey.
  8. The `p09-faults` hold at RequestSent comes before the request is written: the case shows `may_have_run` with 0 remote executions. Rekey-start also holds for 5 s.
  9. New `NetClient::tcp_close_graceful` (sends FIN). The fixture sets `PerSourcePenalties no`.
  10. The shell line editor silently drops input past 128 bytes; this is pre-existing and still open. The driver keeps its commands within 128 bytes.

**Closure (user, 2026-10-25).** Deviation 10 is fixed: lines up to 4096 bytes are accepted, and a longer line is refused whole. Phase 09 is closed as `closed-partial-private-evidence`: lane 103/103 twice, P04 459, P02 48. KMS persistence and S9-5 move to the roadmap entry in `.agents/plan-portfolio.md`.
