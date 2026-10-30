# KSSH K7 CP1 DRAFT: admin import surface for the KMS SSH identity (status AWAITING CP1; documents only; nothing applied)

Documents only: no source edit, key, network fetch, lock change, image change or VM run was made to produce this draft. Read at HEAD `fad076ee9` (clean tree). `[INFERENCE]` = not observed in a file or command output. `OPEN (decider)` = decision requested in §9. §10 lists what was not checked.

Inputs read: `plan.md`, `k2-cp1.md` (§1.2, §1.9, §2.1, §3.3, §4.5, §6, §9), `k2-cp1-approval.md`, `k2-cp2-approval.md`, `reports/k2-cp2-dossier.md` (D-5, D-6), `contract-delta-import-surface.md`, `lock-adoption-draft.md`, `lock-handoff-k2-cp2.md`, probe `kms-prerequisites.md`; `command-contracts.md:132,136,142`; `phase-09-s0-proposal.md:104-112`; `cells/services/kms/src/{main,auth,dispatch}.rs`; `cells/services/supervisor/src/{main,protocol}.rs` + `Cargo.toml`; `cells/tools/sys-tools/src/bin/hotswap.rs`; `kernel/src/loader/launch_profile/{profiles,targets}.rs`; `libs/api/src/abi/caller_identity.rs`; `scripts/{sign-policy,assemble-x86-image,phase09_trust_disk}.py`; `libs/terminal/src/session.rs`; `.agents/cell-contract-0024-locks.md`; read-only `mdir -i kernel/src/embedded-x86_64/kernel_fs.img ::/bin` (sha256 `3848cefb…`, the promoted bytes; **not modified**).

## 1. Question and recommendation

D-K3 fixed that import is a non-CLI credential-owner path; K0 listed three candidates (`contract-delta-import-surface.md` §2.2): (a) supervisor-authorised KMS operation, (b) dedicated admin cell, (c) offline provisioning tool (T1 only). O-2 (Admin class realisation) was deferred to K7 (`k2-cp1.md` §9.4).

**Recommendation: (b), a dedicated admin cell `/bin/kms-ssh-admin`, with a second kernel-attested flag `CALLER_FLAG_KMS_SSH_ADMIN = 1 << 7` carried in the same Law-1/kernel/policy change as K3's sign flag.** (c) is not a surface for the program (it cannot serve T0); it MAY later be a K8 lane convenience for T1. (a) is rejected: it does not remove the need for a user-facing cell (the supervisor cannot read the console), it adds a third holder of key bytes inside a Tier-1 cell, and it is not simpler on the image or lock side. Evidence in §2.

## 2. Candidate comparison (source evidence)

### 2.1 The two questions the assignment asks of (a)

**Can the supervisor carry a 64 KiB chunked import?** Mechanically yes: its inbound unit is an `AppEvent::Message` of up to `IPC_BUF_SIZE = 4096` bytes (`libs/api/src/services/ipc.rs:23`) and a KMS frame is 128 bytes (K2); 64 KiB is 683 chunks, 685 KMS round trips (`k2-cp1.md` §1.8), each through a relay: tool -> supervisor -> KMS -> supervisor -> tool. The supervisor handler is synchronous and would block in `recv` for the whole import, stalling hotswap/snapshot service meanwhile [INFERENCE: `handler` at `supervisor/src/main.rs:43` returns per message; a blocking relay inside it is the only stateless form]. Nothing in the supervisor holds KMS client code today: no `KmsClient` caller exists outside net-broker and a test cell (grep), and `supervisor/Cargo.toml:11-14` depends only on `types`, `api`, `ostd`.

**Who supplies the key bytes and the passphrase, and how does the user get a key to the supervisor?** *Nobody and no way.* The supervisor has no console: manifest `uart = false` (`main.rs:164-171`), no terminal-SDK or VFS client, and its only inbound path is a message from a task whose **name** matches (`sender_has_exact_name`, `main.rs:135-147`; the only reviewed names are `hotswap` for OP_HOTSWAP, `main.rs:57-61`, and an empty list for snapshot, `:40`). A key can reach it only through a user-facing tool cell, exactly like `hotswap` (`sys-tools/src/bin/hotswap.rs`: CLI sends a request, waits for a status with a 1,600-tick timeout). That tool is the "import command": it must read the key from the console (or a file), so (a) = (b)'s cell **plus** a supervisor relay. The passphrase is never part of import under D-K5 (the key stays wrapped; the unlock is per login, `k2-cp1.md` §3.4), so only the key blob (possibly plain) travels.

### 2.2 Matrix

| Criterion | (a) supervisor-authorised op | (b) dedicated admin cell | (c) offline provisioning (T1 only) |
|---|---|---|---|
| Who talks to KMS | supervisor (`authorize_supervisor`, `auth.rs:120-130`, exists) | the admin cell (new flag, `libs/api/.../caller_identity.rs:45-75` has bits 6..31 free) | nobody at runtime; a host script writes a sealed record |
| Needs a user-facing program | **yes** (feeder tool) + relay in supervisor | yes (the cell itself) | host script, no cell |
| Hops key bytes cross | console -> tool -> supervisor -> KMS (3 cells hold the blob) | console -> admin cell -> KMS (2) | disk image only |
| Contract `command-contracts.md:142` "no import command ... added to the CLI" | tool is still a typed command; Cellbox itself unchanged (reading A) | same, Cellbox itself unchanged (reading A); K0 explicitly listed this candidate | no runtime command at all |
| "No secret persistence" | none outside KMS | none outside KMS | persists the wrapped blob in an image/disk artifact; plain key would persist cleartext in an image: **not acceptable** for a user's real key; fixtures only |
| Parse/decrypt/custody inside the owner | blob transits supervisor (Tier-1, holds SupervisorCap) | blob transits one small purpose cell | the host tool must understand the KMS record format (second implementation) |
| Law 1 | none new (no flag) | one new constant in `libs/api` (Law 1: two confirmations, but can ride K3's) | none |
| Admin class in `k2-cp1.md` §2.1 | "existing `authorize_supervisor`, no second flag" | "second flag `1 << 7`, repeats the K3 sub-CP for that path; admin and sign flags never on one Cell" | n/a |
| Kernel/policy files | still a launch row for the tool (`profiles.rs:65` hotswap analogue) + `sign-policy.py` row; no CapSet bit | launch row (Path only, `profiles.rs:75` analogue), CapSet bit, policy v8 (shared with K3), audit narrowing bit, `sign-policy.py` | none |
| Locked source touched | `cells/services/supervisor/**` (rows: `260712-0800-supervisory-cell-migration` `locks.md:130`, `260605-2107-full-reliability-track` `:123`; which governs is [INFERENCE]), plus KMS (K5) | `libs/api` (Law 1), kernel permission rows (shared with K3: `260603-1803-phase-h-vfs-hardening` `:120`, controlling P05), KMS (K5) | none; needs K4b's record v2 |
| Image | supervisor **absent** from the promoted image (`mdir`: 30 files, no `supervisor`, no `hotswap`, no `kms`) -> add supervisor + tool + KMS | add admin cell + KMS | image/disk carries the key material |
| T0 (volatile, re-import every boot, `plan.md` tiers) | works | works | **cannot**: nothing persists to import from, and K0 says image-time provisioning "is not acceptable for T0" (`contract-delta-import-surface.md` §2.2) |
| T1 | works | works | works but depends on the K4b record v2, a dev root key co-resident with the disk (D-K7) and a re-implementation of the KMS load-time allowlist [INFERENCE] |
| Attack surface | supervisor gains a message path carrying secrets, authenticated by **task name** only; one relay bug sits in the cell that can hotswap/freeze services | new cell authenticated by a kernel-attested flag; surface = armor decode + chunk driver (small, host-testable); an attacker needs the flag path `/bin/kms-ssh-admin` through the shell edge | image artifacts; a stale or leaked fixture disk |
| Effort | M plus a supervisor lock handoff | M, of which the flag/kernel part is shared with K3 (marginal S-M if bundled) | S after K4b (K4b itself M) |
| Verdict | reject | **recommend** | not a program surface; optional K8 T1 aid |

Reading A of line 142: "the CLI" is the cellbox command surface. Cellbox gets no builtin, alias, catalog entry, completion or argv parsing of key material; the shell merely launches `/bin/kms-ssh-admin` by a reviewed path as it launches `/bin/hotswap` and `/bin/cellbox-ssh` (`profiles.rs:65,75`). The residual interpretive risk (the user typing `kms-ssh-admin import` is still a command) is stated for decision O-K7-1; K0 already approved "a dedicated admin cell with its own launch row and policy entry" as a candidate.

### 2.3 Why (b) and not "lighter"

A supervisor relay saves exactly one thing: the second flag and policy bump. Against it: Tier-1 TCB growth, a name-based (not kernel-attested) check for the most sensitive KMS operation, supervisor and hotswap tool both missing from the image, and two more lock rows. The flag is one constant and one CapSet bool on a path the kernel already supports (`config_write` precedent: `cap.rs:267,538-540`, `caller_identity.rs:73-75`). **If K3 lands with a single bit, K7 forces a second Law-1 + policy round.** K3Design was told (peer note) to carry both bits in one change; the user decides at K3/K7 CP1.

## 3. Design of the recommended surface

### 3.1 Cell and launch

- Crate `cells/tools/kms-ssh-admin` (package name per the `app-*`/`service-*` convention, CP2 detail), binary and install path `/bin/kms-ssh-admin` (13 bytes; `ProcessInfo.name` is 32, `libs/api/src/abi/syscall.rs:1461`). `declare_manifest!` all false (no net, spawn, block_io, uart). `declare_syscalls!`: the terminal-session child set (`Send`, `Recv`, `RecvTimeout`, `LookupService`, `Log`, `GetTime`, cap/grant syscalls) enumerated at CP2 from `cells/tests/phase02-session-child` and `cells/tools/cellbox/src/main.rs:27-63`, minus `GetRandom`, net and spawn [INFERENCE: set not enumerated here].
- Launched only by the shell edge: `LaunchRoute::Path if target == "/bin/kms-ssh-admin" => KMS_SSH_ADMIN` in `cellbox_profile` (`profiles.rs:62-76` form), **not** in `reviewed_user_target_ceiling` (`targets.rs:29`), so init autoload, desktop and the ELF route cannot start it (same property the code states for `/bin/cellbox-ssh`, `profiles.rs:70-74`). New ceiling `KMS_SSH_ADMIN = LEGACY + kms_ssh_admin`; **never combined with `ssh_sign`**.
- It gets the foreground terminal like any foreground child (`spawn_foreground`, `libs/terminal/src/session.rs`; test cells already use `TerminalSession::from_own_context`: `phase02-session-child`, `phase03-filter`, `top-load`). Without a terminal it refuses (exit 125).

### 3.2 Admission (K5 enforces; K3/K7 supply the flag)

`CALLER_FLAG_KMS_SSH_ADMIN = 1 << 7` mirrors `CapSet::kms_ssh_admin` of the sender's Cell root, exported by `attested_identity_of` (`syscall.rs:2889`), requested by the exact path `/bin/kms-ssh-admin` through a helper kept outside `with_path_caps`' body (`cap.rs:481-483`: `sign-policy.py` reads that body as the P-TRUST list), bounded by the launch ceiling and the signed policy. KMS classes per `k2-cp1.md` §1.9: ops 15-18, 20 admin only; 19 admin or login; 21-24 login only; admin never signs (refusal rows 2-4, §4.5). The K2 dispatch arms' vacuous `authorize_supervisor(sender, caller, Some(sender))` and the `cell_id == 0` refusal (`reports/k2-cp2-dossier.md` D-5, accepted provisionally in `k2-cp2-approval.md`) are **replaced** by the flag check in K5's closure; K7 supplies only the constant and the kernel bit. Whether a Cell with `cell_id = 0` can be attested remains unverified (K3 task, `k2-cp2-approval.md` point 1).

### 3.3 Command grammar and behaviour

```
kms-ssh-admin status [NAME]      leaf status (no NAME) or one key's facts          op 19
kms-ssh-admin import NAME        key text on the echo-off lease                    ops 15,16*,17 (+18 on failure)
kms-ssh-admin delete NAME        typed confirmation, then                          ops 19, 20
```

No `list` (K2 §6), no key export, no passphrase prompt and no parsing of key contents: a wrapped key is imported **as is** (D-K5; the passphrase never enters this path). Exit codes: 0 ok, 1 refused by KMS (typed error name printed), 2 usage, 124 KMS timeout, 125 no terminal or session lost, 130 cancelled.

**Import input.** Echo-off lease (`set_mode(2, 0)` as the ssh passphrase prompt, `cells/tools/cellbox/src/ssh.rs:612-635`), typed or bracketed-paste events accepted (the ssh `read_secret` refuses paste, `ssh.rs:710`; this reader must accept it, so it is its own small reader), Escape cancels. The shell's command-line history sees only `kms-ssh-admin import NAME`. Reading ends at the `-----END OPENSSH PRIVATE KEY-----` line; raw text cap 98,304 bytes (covers a 65,536-byte blob: base64 87,382 B + line breaks + headers) in a zeroized buffer. **Armor decode only:** exactly one `-----BEGIN OPENSSH PRIVATE KEY-----`/`END` block, standard base64 alphabet, canonical padding, LF or CRLF line breaks inside the body, nothing else outside the block, decoded length 1..=65,536; anything else is refused before any KMS frame. The `openssh-key-v1\0` magic and every structural check stay in KMS (D-K4; `k2-cp1.md` §4.5 row 17). Base64 may use `base64ct` (already in `Cargo.lock:664-666`) or a strict own decoder (precedent `cells/services/vfs/src/trust.rs:144`); no new external crate either way [OPEN O-K7-5].

**Frame sequence** (K2 §1.3, §1.7; `request_id` per K6 §3.1 rule, strictly increasing within the run):

| Step | Op | Content | Reply check |
|---|---|---|---|
| 1 | 15 Begin | id n, `total_len = N`, `name_len`, `SHA-256(blob)`, name | handle != 0, `chunk_count == ceil(N/96)`, capacity 96 |
| 2 | 16 Chunk x ceil(N/96) | index 0.., canonical lengths | ack `next == i+1`, `received ==` cumulative |
| 3 | 17 Commit | handle, `chunk_count`, same SHA-256 | `SshKeyStatusPayload` of the new key |
| on any error | 18 Abort | handle | best effort |

N = 250 (OpenSSH plain reference, `k2-cp1.md` §4.4) is 1 + 3 + 1 = 5 round trips; N = 290 (AES-256-CTR reference) is 6; N = 65,536 is 685. Timeouts use K6's bounded call (`SSH_FRAME_TICKS`); a timeout poisons the client and ends the run.

**Output (nonsecret only):** name, custody (`plain` | `passphrase-wrapped`), `SHA256:<base64>` fingerprint (the value `ssh-keygen -lf` prints, `k2-cp1.md` §1.3 `fingerprint_sha256`), posture label (`DEV_POSTURE volatile: lost when the key service restarts` for T0; `DEV_POSTURE persistent: no rollback or crash-durability claim` for T1), **the blob length and SHA-256 prefix the admin cell computed** so the operator can compare with the host's decoded blob. That last line matters: an import of a *wrapped* key is not decryptability-checked (K2 O-15), and console paste can drop or alter a character; structure-breaking damage is caught by KMS's pre-KDF checks, but a flipped ciphertext byte would show only as `unlock_failed` at the first login. A plain key adds `WARNING: unencrypted key held by the key service (development posture)`. Key text, blob bytes, SHA of secrets other than the public-blob digest above, and fingerprints of anything else are never printed or logged (`sys_log` is not used with input-derived text).

**Replace/delete.** One identity per store (LR-1 = Y): `import` over an existing identity is refused (`SshStoreFull`/`SshKeyExists` -> "delete the existing identity first"). `delete NAME` calls status to obtain the public key, prints name and fingerprint, requires the user to type `delete` + Enter on the terminal (Escape cancels), then op 20 with `public_key` and `name` (`k2-cp1.md` §1.3 delete rule).

**KMS result mapping:** `SshKeyFormatRejected` -> "key format refused (openssh-key-v1 ed25519; none, or bcrypt + aes256-ctr only)"; `SshImportIntegrity` -> "transfer corrupted; try again"; `SshImportPending` -> abort and report; `SshKeyUnavailable` -> "key service not serving"; `PermissionDenied`/`CallerUnattested` -> "not an admin caller" (flag or policy missing); other codes -> their name.

### 3.4 Boundaries kept

The admin cell holds the blob only in zeroized buffers for the duration of one run (best-effort wipe, same level as K6 §3.1), never writes it to VFS, log or completion record, has no network, no entropy, no spawn, and no sign right. It never calls ops 21-24. A compromised admin cell can import or delete the identity (T0/T1 stated limit); it cannot sign.

## 4. Lock handoff packet content (amendment form, for the accountable owner; **not submitted, nothing released**)

K7 does not edit `cells/services/kms/**` (K5 owns admission), `libs/types/**`, or Cellbox. Its locked-path work is the kernel/Law-1 part below. If K3's handoff is approved first, K7 is an **amendment to it** (the Phase 02 "A1" pattern, `lock-handoff-k2-cp2.md` §3 last paragraph) adding exactly the admin rows; otherwise K7 needs its own packet and a second policy version bump (not recommended).

1. **Slice identity.** KSSH K7: admin import surface for the single ed25519 SSH identity. Serves Cellbox S9-5/CBX-24 publickey (no import path = no key to sign with); **not an Intel C2C outcome** (same statement as `lock-handoff-k2-cp2.md` §1). Controlling plan `.agents/261011-1200-cell-contract-authority/plan.md`.
2. **ADR-0024 delta.**

| Principle | Effect of K7 |
|---|---|
| Cell principal distinct from Task | admin identity is the kernel-attested trailer + one more bit; no new identity concept |
| Shared per-Cell capability space | one `CapSet` bool; no handle crosses Cells (import handles are per-principal, K2) |
| Explicit delegation/attenuation | the right is requested by an exact path only, bounded by the launch ceiling and signed policy; not delegable; never co-held with `ssh_sign` |
| Revocation generations | live generation checked by KMS before any store access (K2/K5) |
| Kernel substrate vs policy Cells | the right is a kernel `CapSet` field like `config_write`; the cell is an ordinary Tier-2 policy cell |
| Durable state vs execution | none stored by the cell; T0/T1 custody is KMS's (K4b) |
| Caller/test/doc migration | callers: KMS K5 checks the flag; kernel tests assert only the shell edge carries it; docs: CHANGELOG, `docs/code-standards.md` KMS boundary already says "an admin caller is never `cellbox-ssh`" |
| No ambient credential | no UID/root; the console user is the admin only through the shell edge to one path |

3. **Exact file closure requested (pre-edit SHA-256 now; all existing files hashed at HEAD `fad076ee9`).**

| Path | Action | SHA-256 now |
|---|---|---|
| `libs/api/src/abi/caller_identity.rs` | edit: constant `CALLER_FLAG_KMS_SSH_ADMIN = 1 << 7`, reserved range 8..31, test (**Law 1: two confirmations**, bundled with K3's) | `924a0325566282747d8b8afd3023ef5a7e24ec7f3b57fe6cb3aefc9033ddac8f` |
| `kernel/src/task/cap.rs` | edit: `CapSet::kms_ssh_admin`, helper, `intersect`, `apply_to`, `from_task`, flag helper, tests | `fc4b8bfc7857d6a07549055966d43716c009ee3c2a067b0cf139f0264a45803f` |
| `kernel/src/task/tcb.rs` | edit: per-task field | `2a83ff56af16b9f2afd1e13dac7439ba1b2b3e0ffbbd3c63b00b61fe2f57f0a9` |
| `kernel/src/task/syscall.rs` | edit: `attested_identity_of` ORs the flag; `CapSet` literals in fixtures | `5cc2ef44f945df8db637b1959e36213e852ed2a878b485ba370959ae8934b4d0` |
| `kernel/src/policy.rs` | edit: the second bool inside K3's v8 entry (`CAP_BYTES_V8`), legacy decode `false` | `e1e07e7184f261711093083098e06d39e6747754908a7ee16a016001af54c779` |
| `kernel/src/audit.rs` | edit: narrowing-bit doc | `a731029f1bc49e4df704769f947966821c5fb500b79e2d5726fecdd40bc6e020` |
| `kernel/src/loader/launch_profile/profiles.rs` | edit: Path-only row for `/bin/kms-ssh-admin` | `12ff2f739cd7440d2d6b9bce22bef32460b764df6411aff396ebe6a3a93134e2` |
| `kernel/src/loader/launch_profile/targets.rs` | edit: `KMS_SSH_ADMIN` ceiling constant | `45834caee5979f219a8df2027958ff6969a5b9d6068da07dea7ce7fb5306a48f` |
| `kernel/src/loader/launch_profile/tests.rs` | edit: only the shell edge launches it; init/desktop/terminal/ELF route refused; never with `ssh_sign` | `b3d19ebd2670207afdb607c023729643c6ac56edfa2453d5294b2ae37f718170` |
| `scripts/sign-policy.py` | edit: `/bin/kms-ssh-admin` row and request helper (v8, shared with K3) | `70a6fb27bdff390a4a42e3eaeebe11fd48aaa619488950d512da97c6ed308598` |
| `Cargo.toml` (workspace members) | edit: add `cells/tools/kms-ssh-admin` | `3c30c55d79ca8ce93781f38cc1739e0fdfdb2e557e2383fa1fcecd7f615153e9` |
| `Cargo.lock` | edit: one new local package stanza; **no external package added** (deps `ostd`, `api`, `sha2`, `zeroize`, optionally `base64ct`, all already locked) | `02d35d446b90fe569fab50d721a634258d6bb733ea789c45ddc8534ee8ca555a` |
| `cells/tools/kms-ssh-admin/**` (`Cargo.toml`, `build.rs`, `src/main.rs`, `src/armor.rs`, `src/import.rs`, `src/tests`) | new | ABSENT |
| `libs/ostd/src/clients/kms/ssh.rs` | **K6's file**: admin methods used here | ABSENT (K6) |
| `CHANGELOG.md` | entry | `55d62ca6b96e8c41fdb5fa93a847eedbf4b497b9c22f27bd9b40f4f03ab2cf63` |

Not in the closure: `cells/services/kms/**` (K5 admission), `libs/types/**`, `libs/ostd` beyond K6's file, `cells/tools/cellbox/**`, `scripts/assemble-x86-image.py`'s `HEADLESS_CELLS`/`DESKTOP_CELLS` (canonical profiles; the lane script packages the admin cell into its **private** image, as it already packages `/bin/cellbox-ssh`, `qemu-phase09-ssh-test.sh` header), `kernel/src/main.rs`, any image or signing path, the promoted `POLICY.BIN` (v7 stays until a promotion CP; the kernel keeps decoding v7 entries with the new bits false, K3 O-11).

4. **Locked rows and overlap checks (read-only, to repeat at slice start).** `260603-1803-phase-h-vfs-hardening` (kernel permission; `locks.md:120`) and controlling P05 for the kernel files; Law 1 for `caller_identity.rs`; no KMS-row edit by K7. Last commits and plan-overlap grep for the kernel files are to be re-run at slice start; the K3 packet's own overlap findings apply. The new crate path is in no inventory row [INFERENCE: not grepped exhaustively].
5. **Retained gates.** Law-1 confirmation 1 (design) and 2 (exact delta) for the one constant, bundled with K3's; no production/physical claim; `DEV_POSTURE` labels; the admin and sign flags are never held by one Cell (kernel test + K5 refusal row 4).
6. **Scheduling (text only).** One scheduling change records "KSSH K7 (amendment to K3)" with acceptance ceiling "admin cell + flag + host tests, DEV_POSTURE"; WIP stays one slice.
7. **Execution boundary and rollback.** Single integrating owner for the paths above; no network, no key generation, no VM; baseline host tests first (kernel launch-profile tests, `cargo test -p api`, new crate tests); rollback = source reversal to the hashes above; no persistent effect exists.
8. **Decisions requested from the owner.** Adopt K7 as a bounded amendment (or its own slice); rule that the second bit may share K3's v8 bump; confirm no overlapping owner; approve recording the change as one scheduling change.

## 5. CP1 outline (the sections the final K7 CP1 contains)

1. Decision record: O-2 resolved = (b); readings O-K7-1 (line 142) and O-K7-2 (second flag in K3's bump).
2. Admission: flag, ceiling, path, class matrix, refusal rows (K2 §4.5 rows 1-4 now enforced).
3. Cell contract: grammar §3.3, exit codes, output lines (exact text), armor rules, caps, secret hygiene, what is never printed.
4. Frame-level sequences for import, delete, status with request-id, timeout and abort rules (K6 client).
5. Test plan: host tests (below); refusal matrix; lane markers for K8.
6. File closure with hashes and the §4 packet.
7. Risks, out of scope, effort, entry gates, decisions.

## 6. Tests and evidence

**Host tests (no VM, no network, no key generation at CP1; throw-away keys only at K8 CP2).** The cell's logic is written over a small trait implemented by `KmsClient` and by a scripted fake (the K6 `Exchange` seam, `k6-cp1-draft.md` §3.1), so these run on the host:

| Area | Cases |
|---|---|
| Armor | the two OpenSSH reference blobs in `k2-cp1.md` §4.4 (plain 250 B `1d3eb512…`, aes256-ctr 290 B `b5fd6cdd…`): decode of the armored files yields those digests (files read at test time from the registry copy or supplied by the maintainer; copying them into the repo is a licensing decision K1/K5 already flagged, **not made here**); refusals: missing/extra/mismatched header or footer, two blocks, text outside the block, non-alphabet bytes, bad or non-canonical padding, CR-only breaks, empty body, 65,537-byte decoded, text over 98,304 B |
| Chunk driver | exact frame bytes for N = 1, 96, 97, 250, 290, 65,535, 65,536 against K2's canonical chunking vectors (`k2-cp1.md` §4.2); order, handle and cumulative checks; Abort on every failure; timeout poisons; frame buffers and the text buffer wiped (inspect the scrubbed buffers) |
| Delete | status first, public key taken from the reply, typed confirmation required, Escape cancels, wrong word cancels; no op 20 without it |
| Output | exact lines for plain/wrapped/T0/T1; no input-derived byte appears in any output or log string (property test over random inputs) |
| Mapping | every KMS error code the cell can see maps to one fixed message and exit code |
| Class guard | the cell's source calls no op 21-24 (grep-style test) |
| Kernel (K3/K7 shared) | launch-profile tests: only the shell Path edge launches `/bin/kms-ssh-admin`; never `ssh_sign`+admin on one ceiling; `attested_identity_of` flag round trip; policy decode v1-v7 -> false |

**Lane (K8, private candidate, S10-3 style; never canonical promotion):** the lane driver types `kms-ssh-admin import lane` and the armored throw-away key, paced for the serial path [console burst throughput unmeasured: INFERENCE that fast bursts can drop bytes; the printed length/SHA line is the check]; markers: import plain, import wrapped, import with a corrupted character (refused by KMS or `unlock_failed` later, both recorded), import over an existing identity refused, delete then re-import, import by a non-admin caller (`/bin/cellbox-ssh` or the shell) -> `PermissionDenied`, admin cell attempting a sign -> `PermissionDenied`, no key text in the serial log (text and hex), KMS restart empties the T0 store (status shows no key), `status` agrees with the fingerprint sshd logs at login.

## 7. Risks

R-1 Console is the admin: anyone at the foreground session can import or delete (dev posture; no separate admin authentication exists; D02-style trusted confirmation for delete is a typed word only). R-2 Plain keys are handled in cleartext by the admin cell and held by KMS (T0 RAM, T1 under the co-resident dev root). R-3 Paste damage on wrapped keys is detected only at login (§3.3 mitigation: printed length/SHA). R-4 Console burst limits unmeasured. R-5 A second flag costs a Law-1 round if K3 shipped one bit. R-6 K5 must replace the vacuous `authorize_supervisor` arms (D-5) with the flag check; until then the admin path answers `SshKeyUnavailable`. R-7 Terminal quota: the admin cell is one more foreground child under the session's 32-child quota (`executor.rs:3826-3827`). R-8 `cell_id = 0` attestation question stays open (K3).

## 8. Out of scope

KMS behaviour and admission code (K5), the sign flag and policy v8 itself (K3), `KmsClient` methods (K6), persistence (K4b), QEMU evidence (K8), a key list/export command, a passphrase path, offline provisioning (c) as a program surface, canonical image/policy changes, any change to `command-contracts.md` or `command-catalog.md`.

## 9. Effort, entry gates, decisions

**Effort:** M (about 1-2 weeks): cell + host tests about 1 week after K6's client methods exist; kernel/flag/launch/policy work about a week if done inside K3's change (S-M marginal), separate otherwise.

**Entry gates:** this CP1 approved; K3 CP1 either carries both bits (O-K7-2) or K7's own packet is approved; K2 done (yes); K6 admin client methods available (host-only seam is enough to start); K5 CP2 for runtime acceptance; the K7 handoff amendment approved before any file in §4 is edited; K8 for QEMU evidence. No network, key, image or VM authorization is requested.

**Decisions requested from the user:**

| ID | Decision | Recommendation |
|---|---|---|
| O-2 (resolves `k2-cp1.md` O-2) | Admin class realisation | (b) dedicated admin cell |
| O-K7-1 | Accept reading A of `command-contracts.md:142` (an external admin program launched by reviewed path is not "an import command added to the CLI"; Cellbox gets nothing) | accept |
| O-K7-2 | Carry `CALLER_FLAG_KMS_SSH_ADMIN` in K3's single Law-1/policy-v8 change | yes |
| O-K7-3 | Import input is console only (typed/paste, echo-off); no file or pipe input (a file would persist the key text outside KMS) | accept |
| O-K7-4 | Raw text cap 98,304 B = no narrowing of the contract's 64 KiB; measured console limits are documented, not enforced | accept |
| O-K7-5 | Armor decode with `base64ct` (locked) or an own strict decoder | `base64ct`, strictness checked by tests |
| O-K7-6 | (c) offline provisioning stays outside the program (optional K8 T1 helper after K4b) | accept |
| O-K7-7 | Delete confirmation is a typed word on the terminal | accept |

## 10. Not checked

No compile, test, lane or VM run; the cell's exact syscall allowlist and the child-quota behaviour under repeated runs; the full lock inventory for `cells/tools/kms-ssh-admin/**` and for the supervisor paths (rows named from `locks.md:120,123,130`, which row governs the supervisor source is inferred); whether the kernel clears freed pages; serial/GUI paste behaviour and throughput of the input owner for 500 B-90 KB bursts; the kernel boot-ceiling and `boot_ceiling/selftest.rs` literals that may need the new `CapSet` field (listed only from the Q-precedent, `k2-cp1.md` §2.2); `kernel/src/audit.rs` and `boot_ceiling.rs` bodies; the supervisor's syscall allowlist (no `declare_syscalls!` in its crate; default behaviour unverified); how `ProcessInfo.name` is derived (affects only the rejected candidate (a)); whether any plan already owns `cells/tools/kms-ssh-admin`; the K3 draft (written in parallel; this draft assumes only that it can carry two bits).
