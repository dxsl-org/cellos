# KSSH K6 CP1 DRAFT: Cellbox/vissh client for KMS-custodied ed25519 publickey auth (status AWAITING CP1; documents only; nothing applied)

Documents only: no source edit, key, network fetch, lock change, image change or VM run was made to produce this draft. Read at HEAD `fad076ee9` (clean tree). `[INFERENCE]` = not observed in a file or command output. `OPEN (decider)` = decision requested in §11. §12 lists what was not checked.

Inputs read: `plan.md`, `k2-cp1.md`, `k2-cp1-approval.md`, `k2-cp2-approval.md`, `reports/k2-cp2-dossier.md`, `k1-cp1-draft.md`, `contract-delta-import-surface.md`, `lock-adoption-draft.md`, `lock-handoff-k2-cp2.md`, probe `kms-prerequisites.md`; `libs/ostd/src/clients/kms.rs` (+ `kms/relay*.rs`), `libs/types/src/kms/{ssh.rs,payload/ssh.rs}`, `libs/vissh/src/{session,args,lib}.rs`, `libs/vissh/tests/{session.rs,common/mod.rs}`, `cells/tools/cellbox/src/{ssh.rs,ssh/tests.rs,typed.rs,executor.rs,main.rs}`, `libs/terminal/src/{session.rs,pipe_child.rs}`, `third_party/sunset/src/{cliauth,event,auth,sign,servauth,runner,conn,lib}.rs` + `PROVENANCE`, `kernel/src/loader/launch_profile/profiles.rs`, `kernel/src/task/syscall.rs:4463-4540`, `cells/services/kms/src/{main.rs,dispatch.rs}`, `cells/services/supervisor`, `scripts/{assemble-x86-image.py,sign-policy.py,phase09_ssh_fixture.sh,qemu-phase09-ssh-test.sh}`, `phase-09-s0-proposal.md`, `command-contracts.md` §4.

**Path correction.** The assignment names `cells/tools/cellbox/src/applets/ssh.rs:760-761`. No `applets/` directory exists; the lines are `cells/tools/cellbox/src/ssh.rs:760-761` (verified: `if matches!(parsed.auth, args::Auth::PublicKey { .. }) { return refuse(REMOTE_FAILURE, "publickey_unavailable", ...)`).

## 1. Scope

K6 delivers the client half of S9-5: `cellbox ssh --user USER --auth publickey --identity KEY_ID [..] HOST [--command C]` signs the SSH userauth challenge through KMS. It re-opens the two items S9-3 pushed to S9-5 (pipeline exec, exact non-UTF-8 stdin) within the limits §3.4 states honestly.

In scope: (A) `KmsClient` methods for opcodes 15-24 with the K2 payload types and a bounded receive (O-9); (B) vissh `Pubkey`/`AgentSign` handling, a signing-input builder and a no-fallback auth mode; (C) the cellbox child: lift the refusal, KMS identity preflight, passphrase lease, per-login unlock, error mapping; (D) pipeline-stage mode for `--auth publickey --command` (separable, K6b); (E) host tests and the lane L5 additions (executed at K8).

Out of scope: KMS behaviour (K5), the kernel-attested flag (K3), the admin import surface (K7, which only *uses* the admin client methods defined here), QEMU evidence (K8), persistence (K4b), any change to `command-contracts.md`, `command-catalog.md`, `libs/api`, `libs/types`, `Cargo.toml`/`Cargo.lock`, kernel, image, signing.

## 2. What exists today (file:line)

| Fact | Evidence |
|---|---|
| `KmsClient` has relay, node-identity and enrollment calls only; no SSH call | `libs/ostd/src/clients/kms.rs:52-135`, `kms/relay*.rs` |
| One `call` path: blocking `sys_send` then `sys_recv(self.tid, ..)`; the frame `request_id: u32` is correlated and the opcode compared | `kms.rs:153-179` |
| A frame the KMS cannot decode gets **no reply** (`.ok()?`, K2 §1.2), so the client would block forever. `sys_recv_timeout(mask, buf, ticks)` exists; `Ok(0)` = timeout; one tick = 10 ms | `libs/ostd/src/syscall.rs:1489-1515` |
| `Send` blocks until the receiver takes the message; `TrySend` never blocks | `kernel/src/task/syscall.rs:4463-4515` |
| KMS replies with `ostd::ipc::reply` if an operation exists, else a **blocking** `sys_send` | `cells/services/kms/src/main.rs:41-45` |
| Only `net-broker` and `cells/tests/silo-test` use `KmsClient`; no caller matches `KmsClientError` exhaustively | grep `KmsClient` / `KmsClientError` over `cells libs` |
| `cellbox` does **not** depend on `types` | `cells/tools/cellbox/Cargo.toml:28-38` |
| vissh drops both events: `Pubkey(p) => p.skip()`, `AgentSign(s) => s.skip()` | `libs/vissh/src/session.rs:367-368` |
| `Local` trait: `trust, confirm_unknown, password, input, stdout, stderr, resized, progress, checkpoint` | `session.rs:101-128` |
| `run(t, local, user, mode, policy)`; the `Password` event asks once, a second request stops with `Ending::Auth` | `session.rs:193`, `:350-366` |
| `--auth publickey --identity KEY_ID` already parses; `--identity` is refused with password; publickey without identity is refused | `libs/vissh/src/args.rs:183-188` |
| The child refuses publickey after parse | `cells/tools/cellbox/src/ssh.rs:760-761` |
| Password prompt: echo-off lease `set_mode(2, 0)`, typed text only (paste refused), Escape cancels, <=1,024 bytes, NUL refused, wiped | `ssh.rs:612-635`, `:697-735`, `MAX_PASSWORD` `:277` |
| Phase budgets: `AUTH_MS = 120_000` "auth/unlock including prompts"; **`read_secret` blocks on `next_event(u64::MAX)`**, so the budget is not enforced while a prompt is open (also true for the password today) | `ssh.rs:273-276`, `:700` |
| `ssh` is `CmdClass::Session` whenever argv parses; a Session stage in a pipeline is rejected; `background_refusal` rejects Session | `typed.rs:141-144`, `:422`, `:444`, `:730-749` |
| `cmd_ssh` additionally requires `interactive_terminal()` and runs the child through `spawn_ssh_foreground` (stdio slots 0-3 + completion slot 5) | `ssh.rs:60-81`, `executor.rs:3693-3698`, `libs/terminal/src/session.rs:269` |
| Pipeline stages are spawned with `Pipeline::spawn(index, &PipeSpec{path, argv: words[1..], ..}, stdin, stdout, stderr)`; no completion slot, no terminal grants | `executor.rs:4050-4061`, `libs/terminal/src/pipe_child.rs:54-64,258` |
| An external head `cat FILE` feeds stage 0 with `read_file_bytes_exact`; capture/`ByteHead::None` heads also exist | `executor.rs:1679-1710` |
| What S9-3/S9-5 said: publickey refused until S9-5; "Pipeline exec is refused in this slice: a pipeline stage has no terminal for the password. Exact non-UTF-8 stdin bytes need it and move to S9-5"; exec from the terminal reads canonical events (Text, Enter = LF, Ctrl-D = EOF); the in-memory vissh test already carries `a\0\xff` stdin | `phase-09-s0-proposal.md:265,269,303,260`; D79-14 `:434` |
| Completion codes are free ASCII text, <=32 bytes; no enum | `libs/api/src/services/ssh_completion.rs:22,54-62` |
| One ELF is installed as `/bin/cellbox` and `/bin/cellbox-ssh` and the two must be byte-equal | `scripts/assemble-x86-image.py:74,107,192-195,231-232` |
| Promoted image has `/bin/cellbox`, `/bin/cellbox-ssh`, no `/bin/kms`, no supervisor (read-only `mdir -i kernel/src/embedded-x86_64/kernel_fs.img ::/bin`, hash `3848cefb…` = promoted bytes; **not modified**) | this session |

## 3. Design

### 3.1 `KmsClient` additions (`libs/ostd/src/clients/kms.rs` + new `kms/ssh.rs`)

Pattern: new `mod ssh;` beside `mod relay; mod relay_enroll;` (`kms.rs:3-4`) with `impl KmsClient` in the new file, as `relay_enroll.rs` does. Existing method bodies are unchanged.

**Types.** Methods take and return the K2 `types::kms::*Payload` types (`SshKeyStatusPayload`, `SshTransferBeginResponsePayload`, `SshChunkAckPayload`, `SshSignResponsePayload`, `KmsSshKeyCustody`, `KmsCustodyPosture`) like every existing method. `kms.rs` re-exports the ones callers need (`pub use types::kms::{..}`) so `cells/tools/cellbox` and the K7 admin cell need **no new `types` dependency and no `Cargo.toml`/`Cargo.lock` change** (`ostd` has no `pub use types` today; verified by grep). New error variant `KmsClientError::Timeout`.

| Method | Op | Request payload | Success | Notes |
|---|---|---|---|---|
| `ssh_key_status(name: &[u8])` | 19 | `SshStatusRequestPayload` (empty name = leaf) | `SshKeyStatusPayload` | login or admin; no payload `request_id` |
| `ssh_unlock_begin(public_key, secret_len)` | 22 | `SshUnlockBeginRequestPayload` | `SshTransferBeginResponsePayload` | `secret_len` 1..=1,024 checked locally first |
| `ssh_unlock_chunk(handle, index, data)` | 23 | `SshChunkRequestPayload` | `SshChunkAckPayload` | `data` 1..=96 |
| `ssh_unlock_abort(handle)` | 24 | `SshAbortRequestPayload` | empty | best effort |
| `ssh_sign_userauth(public_key, session_id, username)` | 21 | `SshSignRequestPayload` | `SshSignResponsePayload` | rejects an all-zero session id and a bad username locally (`validate_ssh_username`) |
| `ssh_stage_secret(public_key, secret: &[u8])` | 22+23* | composite | `()` | drives Begin, then ceil(L/96) chunks, verifies every reply, sends Abort on any failure, wipes frames |
| `ssh_import_begin / _chunk / _commit / _abort`, `ssh_delete`, composite `ssh_import(name, blob)` | 15-18, 20 | per K2 §1.3 | per K2 | **used only by the K7 admin cell**; `cellbox` never calls them; KMS enforces the class (K2 §1.9) |

**Payload `request_id`.** `KmsClient` gains `next_payload_id: AtomicU64` starting at 1, nonzero and strictly increasing within one client, used by ops 15, 20, 21, 22. A fresh `cellbox-ssh` Cell is a fresh attested principal, so the floor starts at 0 [INFERENCE: depends on K5's floor key `(cell_id, generation)` never being reused for a new Cell; flagged to K5]. K6 treats `InvalidRequest` from these ops as a hard failure (`kms_protocol`), never a retry with a higher id.

**Reply verification** (client side, before returning success): `chunk_count == ssh_chunk_count(secret_len)` and `chunk_capacity == 96` (already enforced by `SshTransferBeginResponsePayload::decode`, `reports/k2-cp2-dossier.md` D-3), handle constant and nonzero, `ack.next_chunk_index == index + 1`, `ack.received_len ==` cumulative bytes sent, sign reply `request_id` echo equals the request's.

**Bounded receive (O-9).** New private `call_bounded(opcode, payload, ticks)`: `sys_send` as today, then `sys_recv_timeout(self.tid, buf, ticks)`; `Ok(0)` -> `KmsClientError::Timeout`; sender mismatch/frame-id/opcode checks as in `call` (`kms.rs:162-174`). Every SSH method uses it; the pre-existing methods keep `call` (behaviour unchanged; net-broker is a caller). Constants: `SSH_FRAME_TICKS = 500` (5 s) for ops 15-20, 22-24 and `SSH_SIGN_TICKS = 6_000` (60 s) for op 21, which carries the bcrypt KDF for a wrapped key. [OPEN O-K6-3: both values are placeholders; K1 measured 301 ms at 64 rounds on the host only (`k1-cp1-draft.md`), the Cellos/QEMU cost is unmeasured and K5 must measure it (`k2-cp1.md` §3.5). 60 s stays below `AUTH_MS` = 120 s.]

**Poisoning.** After a `Timeout` the client sets `poisoned` and every later call returns `Timeout`: a late reply would otherwise be consumed by the next `sys_recv(self.tid)`. The login ends at once with `kms_timeout`; the Cell exits. Residual risk (R-3): KMS answers with a blocking `sys_send` (`main.rs:44`) and `Send` blocks until the receiver takes it (`syscall.rs:4481-4486`), so a timed-out client that is still alive but not in `recv` can hold the single-threaded KMS until it exits [INFERENCE from those two sites]. K6 mitigation: after a timeout do one non-blocking drain `sys_try_recv(kms_tid)` and exit without further blocking work; K5 is told (it may prefer a bounded reply). A `sys_send` toward a busy KMS blocks without bound; no send-timeout syscall exists, so that case is bounded only by KMS finishing its current request (R-4).

**Secret hygiene.** `ostd` has no `zeroize` crate; it has `ostd::session::zeroize(&mut [u8])` and `Scrub<N>` (`libs/ostd/src/session.rs:337,379-384`). Chunk requests are built in a `Scrub<112>`; `KmsRequestV1.payload` is a public field (`types/src/kms/frame/request.rs:7-16`) so the request value is wiped in place after `sys_send`; the 128-byte `to_bytes()` temporary is a `Scrub<128>`. This is best-effort (the compiler may leave other stack copies), the same level as `Term::password`; the Cell exits after the login and nothing else holds the bytes. The caller owns the passphrase buffer and wipes it.

**Host-test seam.** A private `Exchange` seam (`fn exchange(&self, frame: &[u8; 128], ticks: Option<u64>) -> Result<[u8; 128], ExchangeError>`, default `SysExchange`, test `Scripted`) is the only change to `KmsClient`'s structure, so the chunk loops, error mapping, id policy, timeout poisoning and wiping run in `ostd`'s host tests (ostd already has host tests, `libs/ostd/src/clients/net.rs:314`). Exact shape is a CP2 detail.

### 3.2 vissh: auth mode, events, signing input (`libs/vissh/src/{session.rs,lib.rs}`, new `userauth.rs`)

**Sunset needs no patch.** Verified public surface: `SignKey::from_agent_pubkey(&PubKey)` (`sign.rs:430-436`) builds `AgentEd25519`; the `Pubkey` event takes it (`event.rs:115-119`); after the server's `PkOk` the library raises `CliEvent::AgentSign` only for an agent key (`cliauth.rs:132-134`); `RequestSign::{key, message, signed(&OwnedSig), skip}` (`event.rs:129-146`); `OwnedSig::Ed25519([u8; 64])` (`sign.rs:268-270`); `sunset::sshwire::write_ssh` and `sunset::AuthSigMsg` are public (`lib.rs:23,57`, `sshwire.rs:196`). `AuthSigMsg` fields are `pub(crate)` (`auth.rs:14-17`), so the embedder serialises the message and parses it; the Cellos Sunset patches 0001-0006 are untouched.

**Interface (clean cutover, no default method bodies).**

```
pub enum AuthKind { Password, PublicKey }
pub enum Credential { Unavailable, Denied, UnknownIdentity, UnlockCancelled, UnlockNeedsTerminal,
                      UnlockFailed, UnlockLocked, Timeout, Protocol, InputMismatch, BadSignature }
pub struct SignRequest<'a> { pub session_id: [u8; 32], pub username: &'a str, pub message_sha256: [u8; 32] }
pub struct Signed { pub signature: [u8; 64], pub message_sha256: [u8; 32] }
trait Local { ..., fn identity(&mut self) -> Result<[u8; 32], Credential>;
                   fn sign(&mut self, r: &SignRequest<'_>) -> Result<Signed, Credential>; }
run(t, local, user, auth: AuthKind, mode, policy)      // new parameter; every caller updated
Ending::Credential(Credential)                           // new variant; matches in cellbox updated
```

`vissh::userauth::signing_input(session_id, user, public_key) -> heapless::Vec<u8, 174>` builds the RFC 4252 §7 input exactly as `k2-cp1.md` §1.4 specifies KMS does (143 + n bytes) and `userauth::digest` is SHA-256 (vissh already depends on `sha2 0.11`, `heapless`, `zeroize`; no manifest change). This is the **second independent implementation** of the construction: KMS builds it from `(session_id, username, stored key)`, vissh from the same three values, Sunset serialises its own.

**Event handling** (`drive`, replacing `session.rs:367-368`; `State` gains `key_offered`, `signed`):

| Event | Password mode (unchanged) | PublicKey mode |
|---|---|---|
| `Pubkey(p)` | `p.skip()` | first time: `local.identity()` -> `p.pubkey(SignKey::from_agent_pubkey(Ed25519(key)))`, `key_offered = true`; `Err(c)` -> `p.skip()`, stop `Ending::Credential(c)`. Any later `Pubkey` (the server rejected the key or the signature): `p.skip()`, stop `Ending::Auth` |
| `AgentSign(s)` | `s.skip()` (cannot occur) | serialise `s.message()` with `write_ssh` into `[u8; 256]`; build `expected = signing_input(session_id from bytes 4..36, user, identity)`; require `msg == expected` (this also proves Sunset's username/constants equal KMS's input) else `s.skip()` + stop `Credential::InputMismatch`; `local.sign(&SignRequest{..})`; require `signed.message_sha256 == SHA-256(msg)` else `InputMismatch`; **verify the 64 bytes locally** against the identity and `msg` with the vendored dalek (`vissh::sunset::ed25519_dalek`) else `BadSignature` [OPEN O-K6-4]; then `s.signed(&OwnedSig::Ed25519(sig))`; `signed = true`; a second `AgentSign` is `s.skip()` + `Ending::Auth` |
| `Password(p)` | ask once (`st.password_asked`) | **`p.skip()` and stop `Ending::Auth`; `local.password` is never called.** This closes the Sunset fallback: after `resume_pubkey(None)` Sunset raises `Password` when `try_password` is still true (`cliauth.rs:223-228`), and after a rejected signature `failure()` raises `Pubkey` again (`cliauth.rs:184-186`) |

Consequences, each a test (§6): a server that does not list publickey ends `Auth` with zero password prompts; a server that does not know the key never causes a passphrase prompt (the prompt happens at `AgentSign`, after `PkOk`); a rejected signature is tried once and nothing else is offered; a non-`KnownExact` trust decision stops before `identity()` (`trust` precedes everything: `session.rs:316-348`); credential endings have `dispatch = NotSent` and phase `Auth`.

### 3.3 Cellbox child (`cells/tools/cellbox/src/ssh.rs`)

**Order in `run` (replaces `ssh.rs:760-762`).** Parse (unchanged) -> `TerminalSession::from_own_context` (unchanged; K6b adds the stage variant) -> **KMS preflight** (PublicKey only): `KmsClient::connect()`, `ssh_key_status(KEY_ID)` -> remember `public_key` and `custody` -> entropy -> deadline -> DNS/connect (unchanged). Preflight failures are `Admission`-phase `LocalError` with dispatch `not_sent` and **no network traffic**: `kms_unavailable` (service not found, `SshKeyUnavailable`), `kms_denied` (`CallerUnattested`/`PermissionDenied`, the K3 flag is absent), `identity_unknown` (`SshKeyNotFound`: the single identity's name differs or none is stored), `kms_timeout`, `kms_protocol`. A wrapped identity with no terminal is refused here as `unlock_needs_terminal` (K6b). [OPEN O-K6-2: preflight before connect (recommended: unknown KEY_ID never touches the network or the server's login-grace slot) versus lazily at the `Pubkey` event.] A status query is not an authentication attempt, so the "trust before any signature or auth-specific secret prompt" rule (`command-contracts.md` §4.1) is unaffected.

**`Term` gains** the preflight facts (`identity`, `custody`) and a `KmsClient`; `identity()` returns the cached key; `sign()` does:

1. `custody == PassphraseWrapped`: prompt `Enter passphrase for key 'KEY_ID': ` under the **same separate echo-off lease** as the password (`set_mode(2, 0)`, `read_secret`, typed text only, Escape cancels, NUL refused) with the **same 1,024-byte bound** (`MAX_PASSWORD`, rename `MAX_SECRET`). Escape/EOF/session loss -> `UnlockCancelled`. An empty passphrase is not representable on the wire (`secret_len` 1..=1,024) and is treated as `UnlockFailed` locally without a KMS call [OPEN O-K6-10; K2 §1.3 INFERENCE that bcrypt rejects empty].
2. `ssh_stage_secret` then `ssh_sign_userauth`, the **Begin/Chunk/Sign sequence**: unlock Begin (payload id n), ceil(L/96) Chunks, Sign (payload id n+1). A secret of 8 bytes is 3 round trips after the preflight (4 in total); L = 1,024 is 1 + 11 + 1 = 13 (+1 preflight). A plain key skips step 1-2: preflight + Sign = 2 round trips.
3. The secret buffer is wiped on every exit; a staged secret is consumed by the Sign (success or failure) per K2 §1.7, so the client does not Abort after Sign; it sends `ssh_unlock_abort` only when Begin/Chunk fails.

Unlock asks **once** (`command-contracts.md:142`): no re-prompt on failure.

**Error mapping** (KMS -> `Credential` -> completion `error_code` <= 32 ASCII, `ssh_completion.rs:22`; message printed by `report`; none carries key, passphrase, session id or signature bytes):

| KMS / client result | `Credential` | `error_code` | Message |
|---|---|---|---|
| service not found, `SshKeyUnavailable` | `Unavailable` | `kms_unavailable` | key service is not available |
| `CallerUnattested`, `PermissionDenied` | `Denied` | `kms_denied` | not permitted to use the key service |
| `SshKeyNotFound` | `UnknownIdentity` | `identity_unknown` | no such identity `KEY_ID` |
| Escape/EOF at the prompt | `UnlockCancelled` | `unlock_cancelled` | unlock cancelled |
| wrapped key, no terminal | `UnlockNeedsTerminal` | `unlock_needs_terminal` | encrypted key needs an interactive terminal |
| `SshUnlockFailed` (wrong passphrase, corrupt blob or key mismatch: one code, no oracle) | `UnlockFailed` | `unlock_failed` | unlock failed |
| `SshUnlockLocked` | `UnlockLocked` | `unlock_locked` | too many failed unlocks; key locked |
| `KmsClientError::Timeout` | `Timeout` | `kms_timeout` | key service did not answer |
| `SshUnlockRequired`, `InvalidRequest`, `SshOrderInvalid`, `SshHandleInvalid`, any other code, wire/ipc errors | `Protocol` | `kms_protocol` | key service protocol error |
| message/sha mismatch (§3.2) | `InputMismatch` | `kms_input_mismatch` | key service signed a different input |
| `InvalidSignature` or the local verify fails | `BadSignature` | `kms_bad_signature` | key service returned an invalid signature |

Local status is 255 for all (`report`, `ssh.rs:214-248`); completion `outcome = local_error`, `dispatch = not_sent`. `completion()` (`ssh.rs:133-165`) and `report()` gain the `Ending::Credential` arms.

**Phase budget.** The Auth budget (120 s) now also covers the passphrase prompt and the KMS call. Today the prompt itself blocks without a deadline (`ssh.rs:700`). [OPEN O-K6-6: bound `read_secret` by the remaining Auth budget (`next_event(remaining_ms)`); a change to the existing password path too. Recommend yes, small.]

**No new ambient authority.** The child holds the same syscalls as today (`Send`, `Recv`, `RecvTimeout`, `LookupService` are already in cellbox's allowlist, `main.rs:27-67`). KMS admission of the child is K3's flag; until K3 lands the preflight answers `kms_denied`.

### 3.4 K6b: pipeline exec and exact non-UTF-8 stdin

**Why it was refused.** A pipeline stage has no terminal (`pipe_child.rs:25-27`) so it cannot prompt for a password (S9-3 deviation 3, `phase-09-s0-proposal.md:303`). Typed exec stdin is canonical text (Enter -> LF, Ctrl-D -> EOF, `ssh.rs:525-555`), so it cannot carry `\0` or `\xff`. Exec **stdout** exactness and the in-memory `a\0\xff` stdin test already exist (S9-3). What publickey changes: no password prompt is needed.

**What K6b opens, exactly.** `--auth publickey --identity K --command C` as an external byte stage: `cat FILE | ssh ... --command 'cat > f'` (exact bytes through `ByteHead::Cat`, `executor.rs:1681`), and as the tail/only stage with an output redirect. Chosen interpretation of "exact stdin": bytes of the upstream stage unchanged, one EOF, backpressure as the SDK stream gives it.

| Piece | Change |
|---|---|
| Classification | `typed.rs:141-144`: `ssh` with `Auth::PublicKey` **and** `--command` is `CmdClass::External`; every other parsable ssh (password, PTY) stays `Session`. `background_refusal` (`typed.rs:730-749`) must keep refusing ssh in the background (External would otherwise return `None`) |
| Stage launch | `executor.rs:4050-4059` builds `path = /bin/<prog>`, `argv = words[1..]`; for `ssh` the stage must launch `/bin/cellbox-ssh` with argv `["ssh", ..words]` (the child's entry reads `argv().skip(1)`, `ssh.rs:756`, and `oneshot::classify` keys on argv[0] = `ssh`, `main.rs:141-143`) |
| Child | a `PipeTerm` implementing `Local` over the stage's stdin/stdout/stderr endpoints (`granted(context, GrantSlot::Stdin/Stdout/Stderr)`, `ssh.rs:283-287`): `input` = nonblocking read (`Err(Eof)` -> `Input::Eof` once); `stdout` = blocking write to the stage's stdout; `stderr` = the diagnostics ring; `confirm_unknown` -> `Trust::NoConfirmation`; `password` unreachable; `sign` refuses a wrapped key (`UnlockNeedsTerminal`); no PTY, no raw lease, no resize |
| Completion | `Pipeline::spawn` has no completion slot. `command-contracts.md` §4.3 requires a record for every ssh run, so the stage needs one: add an optional completion stream to `PipeSpec`/`PipeChild` mirroring `spawn_foreground_with_completion` (`libs/terminal/src/session.rs:269`, `foreground_context_len` `:344`) and read it after reaping into `set_ssh_completion`. [OPEN O-K6-5: this is the only change outside cellbox/vissh/ostd (`libs/terminal`, not Law 1). The alternative "no record for pipeline runs" breaks §4.3 and is not recommended.] |

**Limits the user must accept (consequences of D-K5, stated, not hidden).** In a pipeline stage: a **passphrase-wrapped key is refused** (`unlock_needs_terminal`, no signature, no network) because the unlock needs a terminal; an **unknown host is refused** (`host_key_no_frontend`) because first-trust confirmation needs the input owner's terminal prompt; the host must already be `KnownExact`. Headless use therefore needs a **plain (unwrapped) KMS identity** (`k2-cp1.md` §3.3 "Headless use"), whose confidentiality is the KMS's alone. The stage cannot report a `Pty` completion. Remote stderr in a pipeline is a diagnostic stream (`typed.rs` rejects `2>` with externals), not a data stream.

### 3.5 What must remain refused (each stays a pre-effect refusal and a test)

`--auth publickey` without `--identity`; `--identity` with password; any other flag (`-i`, `-o`, `-F`, `-J`, forwarding, agent, scp/sftp, config files: `args.rs:160`); more than one identity, key listing, any key-management or import command in cellbox (no builtin, alias, catalog entry or completion; `command-catalog.md` digest `f06f38ce…` untouched); password fallback or retry after a publickey failure; keyboard-interactive; publickey with a pipeline stage that is PTY; password mode as a pipeline stage; background `ssh &`; a wrapped key or an unknown host in a stage; `ssh | ssh` chains beyond what `admit_bytes` already accepts for externals [INFERENCE: no ssh-specific limit exists; CP2 checks]; any cellbox call of KMS opcodes 15-18, 20 (KMS answers `PermissionDenied`, K2 §1.9); `ssh` from the one-shot `/bin/cellbox` entry (`oneshot.rs:117` `Class::Reserved`, unchanged).

### 3.6 Law-1, locks and ownership

- **No Law-1 surface.** Law 1 scope is `libs/api/` and `libs/types/` (`docs/code-standards.md:40-42`); `libs/ostd`, `libs/vissh`, `libs/terminal`, `cells/tools/cellbox` are outside it. K6 changes no byte of `libs/api` or `libs/types`; `ssh_completion.rs` (libs/api) is unchanged because `error_code` is free text. K6 only *consumes* the K2-approved payloads. The closure has no `Cargo.toml`/`Cargo.lock` edit (§3.1).
- **No locked relation.** None of the K6 paths is in a CELL-CONTRACT-0024 row (`lock-adoption-draft.md` row 14: "Cellbox plan (single integrating owner); libs/ostd is not Law 1"; the inventory has no row naming these paths; **grep of `.agents/cell-contract-0024-locks.md` for them not exhaustively re-run here**). Owner: the Cellbox plan's single integrating owner (`.agents/261020-1750-cellbox-cli/plan.md`, status `closed-partial-private-evidence`; entry gate text keeps CBX-23/24/25 partial). K6 is admitted as a bounded Cellbox slice "S9-5 via KSSH K6" by a reciprocal note in that plan and in `plan.md` here; it re-opens no closed phase and no other relation.
- **Overlap notice (start-of-slice re-check, as K2 did).** `libs/ostd/src/clients/kms*` is named by `260825-1726-kms-silo-production-root` (phases 01, 03), `260826-1605-phase4-dev-reference-authority` (phase 07, blocked), `260712-1902-dice-attestation-identity` (phase 04), `260819-1409-cell-to-cell-anywhere-core` (phase 02) (grep `clients/kms`); last commit touching the client: `cc5b7c361`. K6 adds only a new module, a `Timeout` variant and a private helper; no existing method changes. The owners are asked to confirm no in-flight edit.
- **Canonical bytes.** Any K6 build changes the cellbox ELF. Evidence uses private fixture images/candidates only (S10-3 style, `--no-faults` + `--candidate` lane forms); the promoted `/bin/cellbox` and `/bin/cellbox-ssh` stay byte-equal and untouched (`assemble-x86-image.py:192-195`); no promotion is requested.

## 4. Exact file closure and current SHA-256 (pre-edit, HEAD `fad076ee9`)

| Path | Action | SHA-256 now |
|---|---|---|
| `libs/ostd/src/clients/kms.rs` | edit: `mod ssh;`, re-exports, `Timeout`, `call_bounded`, `Exchange` seam, `next_payload_id` | `b9fdd3af308e961f2a19c8d03f66808217101703c918dd8dcf12775d65201286` |
| `libs/ostd/src/clients/kms/ssh.rs` | new: §3.1 methods + tests | ABSENT |
| `libs/vissh/src/session.rs` | edit: §3.2 | `9bd88a8110121e20878182c2739981b48204c29b0ef1ae56657b46717b0b7e20` |
| `libs/vissh/src/userauth.rs` | new: `signing_input`, `digest` | ABSENT |
| `libs/vissh/src/lib.rs` | edit: `pub mod userauth;` | `e5634265c5b02e3f97c6c73fcd765520984dbe147c51dd38fbf5a3a2e938609c` |
| `libs/vissh/tests/session.rs` | edit: new cases, `Me` implements the new `Local` methods | `7344880b2a4c64376fe81dcc4994b29f997f98e342141ee3f64345fd77af9864` |
| `libs/vissh/tests/userauth.rs` | new: vector tests | ABSENT |
| `cells/tools/cellbox/src/ssh.rs` | edit: §3.3, `PipeTerm` (K6b) | `3f4aed9cf06e61404ebee722afbbfaec2e841f36cf0556123a91c89a8bb4cd9b` |
| `cells/tools/cellbox/src/ssh/tests.rs` | edit: mapping/report/completion tests | `aaa7d86c4486a3d28358eba23de00f792aa706c29125e4853a87a59c158cb748` |
| `cells/tools/cellbox/src/typed.rs` | edit (K6b): classification + tests | `b2ca07f44e38bd9de37de8b0eb06d5bdd5161536ef0918756eda3775785d89b3` |
| `cells/tools/cellbox/src/executor.rs` | edit (K6b): stage mapping, completion read | `6f6524c721a7dee3b3ded952a9ca770caed105e7f8b962ebe5475c6304bb7b63` |
| `libs/terminal/src/pipe_child.rs` | edit (K6b, O-K6-5): optional completion slot | `52aaf1ee47a897bea055a95ff2cbfa677fe74ded261c9294ae7874ff1ec82c7c` |
| `libs/terminal/src/tests.rs` | edit (K6b): slot test | `24cac3df182e744c9a7a49aefb49fe2b41e1b923bd8a57cc3ba9f539df322b70` |
| `CHANGELOG.md` | entry | `55d62ca6b96e8c41fdb5fa93a847eedbf4b497b9c22f27bd9b40f4f03ab2cf63` |
| *K8 lane (not K6 CP2):* `cells/tests/phase04-terminal-driver/src/ssh.rs`, `scripts/phase09_ssh_fixture.sh`, `scripts/qemu-phase09-ssh-test.sh` | edit at K8 CP1/CP2 | `f189362150a431ef03d13edcbf2bdeef2def8332513a36befd636b6f704024df`, `5a9afb76fc45a450471ec4e34b7bec4a38dfb3d0ef1f3a09d8d7b139ca4c5199`, `30c031a1f08d47fc5118800baeddb047bf197f4a7a6e39c0c6cd99fb624889b2` |

Not in the closure: `libs/api/**`, `libs/types/**`, `kernel/**`, `cells/services/**`, `third_party/sunset/**` (no patch), `scripts/sign-policy.py`, `scripts/assemble-x86-image.py`, `Cargo.toml`/`Cargo.lock`, `command-contracts.md` (`4ff1c3be…`), `command-catalog.md`, any image/signing path. A path found during implementation that is not listed stops the slice and returns as an amendment.

## 5. Handoff record (ownership form; there is no locked relation to release)

1. **Slice:** KSSH K6, bounded Cellbox slice serving S9-5/CBX-24 publickey; not a C2C outcome.
2. **ADR-0024 delta:** no new principal or capability concept; the child's KMS right is the K3 kernel-attested flag; no handle crosses Cells; no ambient credential: the client never holds key bytes, only an optional per-login passphrase it forwards once to KMS and wipes.
3. **Owner:** the Cellbox single integrating owner (sole editor of the §4 paths during the slice).
4. **Retained gates:** Law-1 not applicable; no production/physical claim; `DEV_POSTURE` labels unchanged; acceptance needs K3, K5, K7 and K8 evidence, not K6 host tests.
5. **Rollback:** source reversal of §4 restores the pre-edit hashes; no persistent effect exists.

## 6. Tests and evidence

**Host tests (K6 CP2 acceptance; no VM, no network, no key generation).** Baselines to re-capture at CP2 start (the S9-3 numbers api 151 / ostd 93 / cellbox 180 / vissh 16 are historical, `phase-09-s0-proposal.md:293`): `cargo test -p ostd`, `-p vissh`, `-p app-cellbox`, `-p terminal-sdk` with the host target flags the repo uses (`libs/vissh/Cargo.toml` comment: `--target x86_64-unknown-linux-gnu`), plus the cellbox x86 bare-metal build (`-Z build-std=core,alloc`) and the riscv build the cell CI uses. Not run here.

| Area | Cases |
|---|---|
| `userauth` | the K2 §4.3 vector: public key `b33eaef3…ab62`, session id `00..1f`, user `user` -> the 147-byte input and SHA-256 `fa3246ab…de65` (values reproduced by `k2-cp2-dossier.md` §3); n = 1 and n = 31 give 144 and 174 bytes; equality with Sunset's own serialisation of the same message (built with a real `Runner` pair) |
| vissh session (in-memory Sunset server, `PubkeyAuth` + `allow`) | success with a test signer that signs `signing_input` with the already-linked dalek (the **server verifies**: this is "a signature for another session is refused" end to end); signature over another session id or username -> server rejects, exactly one `sign` call, zero password prompts, `Ending::Auth`; message digest mismatch -> `InputMismatch` before anything is sent; bad signature bytes -> `BadSignature` locally; `identity()` error -> `Credential`; server without publickey -> `Auth`, zero `identity`/`password` calls; server that does not know the key -> `Auth` and `sign` never called (no passphrase prompt); untrusted or changed host key -> `identity` and `sign` never called; password-mode regression (both methods offered: `Pubkey`/`AgentSign` skipped, password asked once); rekey with a publickey session keeps the host key rule |
| `ostd` client (Scripted `Exchange`) | each method's exact request frame (golden bytes copied by value from K2's vectors with provenance comments, not imported); canonical chunking for L = 1, 96, 97, 1,024; ids strictly increase and are nonzero; Begin/Chunk/Sign order; any reply deviation (handle change, wrong ack index/length, wrong echo, wrong opcode/frame id) -> failure plus Abort; **no reply -> `Timeout`, poisoned, no further send**; frame buffers wiped (inspect the `Scrub`); service error codes map one to one; unknown error code (`UnknownErrorCode`) -> `Wire`; old methods unchanged |
| cellbox | `Credential` -> completion/report table (§3.3) is exhaustive and each text is <= 32 ASCII; completion for every credential ending is `local_error`, `not_sent`, phase `auth` (or `admission` for preflight); secret length helper (`1..=1,024`, NUL refused); `typed::classify_command`: ssh publickey+command is `External`, password/PTY stays `Session`, `background_refusal` still refuses; pipeline admission: `cat f | ssh ... --command c` admitted, `... | ssh --auth password ...` rejected, `ssh ... &` rejected; stage argv gets `ssh` as argv[0] |
| terminal-sdk (K6b) | completion slot present on a pipeline child only when requested; context length arithmetic |
| negative source checks | grep-style test that `cells/tools/cellbox/src` references no KMS admin method (`ssh_import*`, `ssh_delete`) |

Client against the **real** KMS state machine is not a K6 host test: the KMS service crate depends on `ostd`, so a cross test belongs to K5's closure (`cells/services/kms`) or K8. [OPEN: owner of that integration test; recommend K5, using the shared K2 vectors.]

**Lane L5 additions (specified here, executed at K8 on a private candidate; L5 = `scripts/qemu-phase09-ssh-test.sh --no-faults`, L5f = the `p09-faults` build; the S9-3-era counts 34 and 103 are re-measured, not reused).** Prerequisites at K8: a private image with `/bin/kms`, K3 flag/policy rows, K7 admin cell, a throw-away ed25519 key generated on the host **at K8 CP2 only**, and the fixture's `authorized_keys` for `tester` (the fixture already sets `PubkeyAuthentication yes`, `phase09_ssh_fixture.sh:64`). New markers:

1. publickey exec login with a plain identity: exact stdout, status 7 as in the password lane; sshd log shows exactly one `Accepted publickey` whose fingerprint equals the one the admin cell printed.
2. wrapped identity, PTY and exec: passphrase typed once at the prompt; sshd log shows one accept.
3. unknown `KEY_ID`: `identity_unknown`, **no new connection in the sshd log** (preflight).
4. wrong passphrase: `unlock_failed`, the connection is closed unauthenticated, no `password:` prompt, status 255.
5. unknown host with publickey: first-trust flow unchanged; no `Accepted` line before trust; refused trust leaves no signature (sshd log shows no publickey attempt).
6. pipeline: `cat <fixture blob with \0 and \xff> | ssh ... --command 'sha256sum'` equals the host digest; the same with wrapped identity -> `unlock_needs_terminal`; unknown host -> `host_key_no_frontend`; `ssh --auth password ... | cat` and `ssh ... &` still refused (existing check kept).
7. completion records (`completion` builtin) for each of the above, including the pipeline stage.
8. independent verdict: no passphrase, key or signature bytes in the raw serial log (text and hex, as the S9-3 check does for the password), no panic, no weak-entropy warning.

## 7. Risks

R-1 KMS KDF stall of the shared loop (K2 §3.3) makes the 60 s bound a guess (O-K6-3). R-2 The compiled-in wipe is best-effort (§3.1). R-3/R-4 abandoned requests and blocking `sys_send` (§3.1). R-5 A compromised `cellbox-ssh` can request signatures for sessions it chooses, and sees the passphrase it captures; KMS binds session id and username only (K2 §1.4, plan tiers); the vissh trust-first order protects only the honest client. R-6 Pipeline stage + plain key is the only headless mode: the key is unwrapped in KMS RAM (T0) or under the dev root (T1). R-7 The principal-floor reset semantics are K5's; a wrong assumption surfaces as `InvalidRequest` at first sign. R-8 `Local` and `run` change shape: every implementer and call site (production `Term`, `libs/vissh/tests/session.rs`, `phase04` driver builds) is updated in the same change. R-9 The cellbox ELF grows; the candidate image must still fit (promoted image has 6,304,256 free bytes, read-only `mdir`).

## 8. Out of scope

KMS parser/KDF/ed25519/store (K5); kernel flag, policy, launch row (K3); admin import tool and flag (K7); persistence and T1 provider (K4b); QEMU evidence and the canonical image (K8); RSA/ECDSA, agent, keyboard-interactive, multiple identities, key listing, Tier-3 guest endpoints; changing `command-contracts.md`; Sunset patches; making the password path stricter than today except O-K6-6.

## 9. Effort

M overall: K6a (client S, vissh S, cellbox child S-M, tests) about 1 week; K6b (classification, stage launch, `PipeTerm`, `libs/terminal` completion slot, lane markers) about 1 week. K6a can start host-only after K2 (done); neither half can claim acceptance before K3, K5, K7 and K8.

## 10. Entry gates

1. This CP1 approved (Cellbox single-owner reciprocal note recorded); O-K6 decisions below answered.
2. Start-of-slice: baseline host tests captured with logs; overlap re-check of the four plans naming the KMS client; `git status` clean.
3. K3 flag and K5 behaviour are **not** needed to write or host-test K6; they are needed before any "works" claim. K7 is needed for runtime key import in K8.
4. Network, keys, images, VM: none authorized by this CP1; the K8 CP1/CP2 carry their own.

## 11. Decisions requested from the user

| ID | Decision | Recommendation |
|---|---|---|
| O-K6-1 | One CP1 with K6a and K6b as separately gated CP2 steps, or K6a only now | one CP1, two gates; K6b may be deferred without blocking K8's terminal-mode evidence |
| O-K6-2 | KMS identity preflight before connect | yes (§3.3) |
| O-K6-3 | `SSH_FRAME_TICKS` 500 / `SSH_SIGN_TICKS` 6,000 placeholders pending K5's measured KDF time | accept as placeholders, final at K5/K8 |
| O-K6-4 | Local ed25519 verify of the KMS signature before sending | yes |
| O-K6-5 | Completion stream for pipeline stages via `libs/terminal` | yes (contract §4.3) |
| O-K6-6 | Bound the secret prompt by the remaining Auth budget (also for password) | yes |
| O-K6-7 | Admin client methods live in the same ostd module; KMS class check is the enforcement (a cargo feature gate would not isolate cellbox because features unify per build) | accept |
| O-K6-8 | Wrapped keys and unknown hosts are refused in pipeline stages (D-K5 consequence, §3.4) | accept |
| O-K6-9 | Owner of the real-KMS client integration test | K5 |
| O-K6-10 | Empty passphrase is a local `unlock_failed` | accept |

## 12. Not checked

No compile, test, lane or VM run; no KDF timing; whether the kernel clears freed pages (wipe claims are about our own buffers); `ed25519_dalek` 3.0 verify API names (CP2 confirms; the test signer and local verify use the copy re-exported by `sunset`); the exact `Exchange` seam shape; the full cellbox syscall allowlist beyond `main.rs:27-63` (`Send/Recv/RecvTimeout/LookupService` confirmed); how `SshKeyNotFound` vs name mismatch is answered (K5); whether an unlisted `libs/terminal` consumer breaks when `PipeSpec` gains a field; `ssh | ssh` behaviour; the full lock inventory for the §4 paths; `docs/` pages that describe `ssh` (a CHANGELOG entry is planned, other docs not enumerated); console throughput is irrelevant to K6 (no paste).
