# KSSH K0 contract delta: SSH key import surface and unlock model (PROPOSED, not approved, nothing edited)

Scope: documents only. This proposes how the KSSH program reconciles the retained Cellbox contract and the S9-5 proposal. It edits neither `command-contracts.md` nor any proposal; approval of this delta is the K0 gate.

## 1. The conflict (verified, 2026-10-30)

| Source | Text | Effect |
|---|---|---|
| `261020-1750-cellbox-cli/command-contracts.md:142` | "...Encrypted OpenSSH Ed25519 parser/decryption is a **credential-owner import/unlock path**, not `ssh -i` or an ambient VFS read ... Unlock asks at most once under the same secret-entry bound: the actual foreground client captures the secret under its separate no-echo lease and passes it only to the authenticated credential owner for bounded unlock; parsing/decryption/private-key custody remain inside that owner ... **No keyboard-interactive, agent, RSA/ECDSA, key generation, import command or secret persistence is added to the CLI.** Actual KMS hook/parser/runtime integration remains its named owner prerequisite, not permission for cellbox to hold raw signing material." | binding: the CLI gets no import command and no secret persistence; the client only forwards a passphrase for unlock |
| `261020-1750-cellbox-cli/phase-09-s0-proposal.md:108` (S9-5) | "Import is done by a separate admin path, such as a `cellbox ssh-key import` builtin or a provisioning tool. The exact import surface is a sub-decision at S9-5 CP1." | the builtin candidate contradicts the contract; the provisioning-tool candidate does not |
| `command-contracts.md:136` | `-i` raw identity-file reads, OpenSSH config/import files rejected | the client never reads key files |

## 2. Proposed delta

1. **No `ssh-key import` (or any key-management) command is added to Cellbox.** The S9-5 builtin candidate is withdrawn. The contract line 142 stands unchanged.
2. **Import is a credential-owner operation outside the CLI** (decision D-K3). The owner is KMS plus an authenticated administrative caller; candidate surfaces, to be decided at K7's CP1 (not here): (a) a supervisor-authorised KMS operation, (b) a dedicated admin cell with its own launch row and policy entry, (c) an offline provisioning tool that produces a record for T1 only (image-time provisioning stores key material in an image: needs its own security review and is not acceptable for T0).
3. **The login path stays contract-shaped:** `cellbox ssh --auth publickey --identity KEY_ID` forwards at most one secret (passphrase, same 1,024-byte bound, separate no-echo lease) to KMS for bounded unlock when the stored key is passphrase-wrapped; KMS signs the session-bound userauth blob and returns 64 bytes; no key bytes, no key material and no secret persistence in `cellbox`.
4. **Unlock model (D-K5) is constrained by the contract and decided at K2's CP1.** The contract's "unlock asks at most once ... bounded unlock" implies a stored passphrase-wrapped key unlocked per login (or per bounded session), while the S9-5 proposal implied decrypting at import and re-sealing under the KMS root. The two give different guarantees: per-login unlock keeps the key encrypted at rest under the user's passphrase and needs a transient unlocked state in KMS; decrypt-once relies on the KMS root for confidentiality and removes the per-login prompt, which then conflicts with line 142's unlock step. K2's CP1 must choose and, if decrypt-once, propose the corresponding contract wording change; K0 does not choose.
5. **Accepted formats unchanged:** `openssh-key-v1` Ed25519, at most 64 KiB, unencrypted inside owner custody, or bcrypt (rounds 1..64, salt 16..64 B) + AES-256-CTR only; everything else rejects before expensive work. (Parser choice D-K4 stays with K1's CP1.)
6. **Caller admission:** a new kernel-attested caller flag for `/bin/cellbox-ssh` (D-K6); the admin/import caller is a different, separately admitted caller and never `cellbox-ssh`.

## 3. Consequences recorded now

- S9-5 text in `phase-09-s0-proposal.md` stays historical; its import-builtin candidate is superseded by this delta once approved.
- No change to `command-catalog.md`: no new command appears (digest `f06f38ce…` untouched).
- Any wording change to `command-contracts.md:142` (only if D-K5 = decrypt-once) is a contract delta needing its own approval at K2.

## 4. Not decided or authorized here

D-K4 (parser), D-K5 (unlock model), K7 import surface choice, any code, key, network fetch, lock release or image change.
