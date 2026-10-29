# KSSH K2 CP1 approval record (Law-1 confirmation 1, design)

Recorded 2026-10-30 from the maintainer's answers relayed in the session: **LR-1 = option Y** (exactly one SSH identity per KMS store; the caller selects no key), **LR-2 = option P** (kernel-attested live caller identity accepted in place of a registered service TID for this purpose-specific right), **confirmation 1 approved for `k2-cp1.md`** including the §9.3 values (O-3, O-4 = 4, O-5 = 5, O-7, O-8, O-15), D-K5 = per-login unlock bounded to one signature, O-10 = B, O-14 = admin-chosen KEY_ID (1..64). Deferred with owners: O-2 (K7), O-6 (K5 CP1), O-9 (K6), O-11 (K3 CP1), O-12 (K3/K8).

**This record authorizes:** the design in `k2-cp1.md` for `libs/types/src/kms/**` and the O-10 = B dispatch follow-up **as confirmation 1 only**. **It does not authorize** code, keys, network, `Cargo.lock`, lock release, image or kernel changes. Before any K2 code: the lock handoff for `libs/types/src/kms/**` and `cells/services/kms/**`; after implementation, confirmation 2 on the exact ABI delta with evidence (a new record).

Digest form: SHA-256 of the whole UTF-8 file including the final newline, no normalization, no digest stored inside the hashed files. Verify with `sha256sum <path>` from the repository root.

Source HEAD at binding (parent of the commit that adds this record and the final `k2-cp1.md`): `07d8e894b18990c8c17e9b6b76db09b8c95e4421`; the only uncommitted change at binding was `k2-cp1.md` itself (LR-1 = Y wording and status line), committed together with this record; no source file under `libs/` or `cells/` was modified.

## Bound documents
| Path | SHA-256 |
|---|---|
| `.agents/261030-1000-kms-ssh-key-custody/k2-cp1.md` | `8c2421a6ad6cc3462ee20b5328f6abe6d635f8d5cea23927a4ff7b6947f1ace1` |
| `.agents/261030-1000-kms-ssh-key-custody/plan.md` | `8372e9df3c490b4ff9026274f061f521f64e53bd2a17a1cf95eb6ce3a6b8eb61` |
| `.agents/261030-1000-kms-ssh-key-custody/contract-delta-import-surface.md` | `2fbbc6d8a2fad10e812bcb0aa6b39cd9bfaa08f8ef4020dacdffe05b22cdb13e` |
| `.agents/261030-1000-kms-ssh-key-custody/lock-adoption-draft.md` | `bd4b80d291d9fa13b32b5a071a210eb5d12e02e69c2125d960e77aa77ef60de6` |
| `.agents/261030-1000-kms-ssh-key-custody/k1-cp1-draft.md` | `6b611483d622fe0cb1156c92fd4565fb7dc20a5dd55fbe00cf7877d7e2560ff2` |
| `.agents/261020-1750-cellbox-cli/reports/s10-10-evidence/probe/kms-prerequisites.md` | `9d27e6549ed139452b497ac2cdfe3b949bcb619bee0064676e9cecbad933a791` |
| `.agents/261020-1750-cellbox-cli/command-contracts.md` | `4ff1c3be46a39312d400877e0655a715b024563f2d84db7aecf27947efb2090d` |
| `docs/code-standards.md` | `9f87c3246074d5da7e7140e79c5108107189f6d0bade5fd664ec75ca2cde5424` |

## K2 file closure, pre-edit state (for the later confirmation-2 delta)
| Path | SHA-256 before any K2 edit |
|---|---|
| `libs/types/src/kms/model.rs` | `07531027d431bbfa7e08681b44d47d866a01fc1162621ae7b7f39ef3182db709` |
| `libs/types/src/kms/ssh.rs` | `ABSENT (new file)` |
| `libs/types/src/kms/payload/ssh.rs` | `ABSENT (new file)` |
| `libs/types/src/kms/payload.rs` | `915a061f0965b8c62367f1d94910c82a63598a3f68b587dbeefa80be728969b4` |
| `libs/types/src/kms.rs` | `27609f78be155cea1b8da772a88fa75618b146556b56588e415c8e02589e5291` |
| `libs/types/src/kms/tests/ssh.rs` | `ABSENT (new file)` |
| `libs/types/src/kms/tests/mod.rs` | `9f30fe0976e6815a3b0c57d39652ce547b673da3e4701f42e408b2c95cf3c0f4` |
| `libs/types/src/kms/tests/frame.rs` | `7d62eaa96ad4b0f0ad7d2a93e7f9f769b84c73e38d9973c63723a8e147778e04` |
| `cells/services/kms/src/dispatch.rs` | `9506f0f27c9d91a5890203ac608d4c81d4eb7b90c2e64141e83965baaf84c98a` |

Note: `k2-cp1.md` is hashed at binding; any later edit of it voids this approval and needs a new record. `plan.md` and the other input documents may receive status updates; their hashes above identify the text approved at this binding.
