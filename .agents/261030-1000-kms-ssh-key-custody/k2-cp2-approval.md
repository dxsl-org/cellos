# KSSH K2 CP2 approval record (Law-1 confirmation 2, exact ABI delta)

Recorded 2026-10-30. The accountable maintainer (the user) selected **"Duyệt confirmation 2 + 2 điểm lưu ý"** for the implemented K2 CP2 delta described in [reports/k2-cp2-dossier.md](reports/k2-cp2-dossier.md), committed as `d97695ca3`.

**Approved:** the append-only ABI exactly as implemented in the nine paths below (`KmsKeyAlgorithm::SshEd25519 = 3`, opcodes 15-24, errors 27-38, `KmsSshKeyCustody`, `KmsCustodyPosture`, payload codecs, validators, host tests) and the `service-kms` dispatch arms that answer `SshKeyUnavailable` after attested-caller validation. Opcodes/payloads 1-14 and errors 1-26 byte-identical (17 untouched frozen files unchanged). Evidence: `types` 54 -> 74 passed, `service-kms` 59 -> 65 passed, service-kms release builds for riscv64gc, x86_64 and aarch64 bare-metal targets.

**Two noted points accepted provisionally (to be revisited at K3/K7):** (1) the arms call `authorize_supervisor(sender, caller, Some(sender))`, a vacuous supervisor comparison, plus a new `cell_id == 0` refusal, because `validated_caller` is private to `auth.rs` (outside the closure); whether a Cell with `cell_id = 0` can legitimately be attested is unverified; (2) public type names carry the repo's `Payload` suffix instead of the bare names in `k2-cp1.md`.

**Not approved / still gated:** any KMS behaviour (K5), the kernel-attested caller flag (K3, own handoff), `libs/ostd`/Cellbox client work (K6), the admin import surface (K7), persistence (K4b), `Cargo.lock`, keys, network, images. The arms are status-honest stubs of unimplemented behaviour and must not be reported as K5 progress.

## Digests after implementation (SHA-256 of whole files)
| Path | SHA-256 |
|---|---|
| `libs/types/src/kms/model.rs` | `c859156bf84c62b161719f76b847a01623173207f8d11fa074038ef701d7e07e` |
| `libs/types/src/kms/ssh.rs` | `dc547a3d42e25e5bdf9a26769711747790db4aebde20ca689886409d9ae470e3` |
| `libs/types/src/kms/payload/ssh.rs` | `da81a6453c9a0a1696cec099006f1303be04ae7e9c22ac8305ea7806886d3ed0` |
| `libs/types/src/kms/payload.rs` | `7077d2db7ef77b8da317602b2b0b80ad5375fd31bbd97974cecb369646dd4c0d` |
| `libs/types/src/kms.rs` | `f92fc159437b6684b00faa79e65715b2064bab789b282f6390fa227c357f5172` |
| `libs/types/src/kms/tests/ssh.rs` | `6aac80b1c7e050be9c429ab3b82b6cb442f6f4f7b84d16816097b6402bcc18ec` |
| `libs/types/src/kms/tests/mod.rs` | `ef5733a43a40d37df005b585795be6ac0490d56d85f4f3e173ffad01dba0a798` |
| `libs/types/src/kms/tests/frame.rs` | `643a6a606b5ab6adf4bd7ce6303ae705401de128cf1be458f2f81df8d891e02c` |
| `cells/services/kms/src/dispatch.rs` | `2c19b290c252b3d850eae71035aac35a05dedc6ce605897bd8f9a8a38b586b06` |
| `.agents/261030-1000-kms-ssh-key-custody/reports/k2-cp2-dossier.md` | `c15be39bfa5a1738bf66fe9d60b876ff83339ccda0c971f1805f1b1f9c32abd6` |
