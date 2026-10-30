# KSSH K2 CP2 lock handoff approval record

Recorded 2026-10-30. The accountable owner (the user, relayed in-session) selected **"Duyệt cả 4 mục (adopt, C-1/C-2, không chồng lấn, ghi sổ)"** for [lock-handoff-k2-cp2.md](lock-handoff-k2-cp2.md).

- Approved packet SHA-256 (whole file, UTF-8 including final newline, no normalization): see the digest table below; the packet is not edited after this record.
- Decision 1: K2 CP2 is **adopted as a bounded slice** under CELL-CONTRACT-0024 with the nine-path file-bounded exception of packet §3; it serves Cellbox Phase 09 S9-5 / CBX-24 publickey, **not an Intel C2C outcome**.
- Decision 2: **C-1** ruled: `260826-1605/phase-02:27` forbids adding authority-trust fields/opcodes; it does not forbid an unrelated purpose-specific class. K2 changes no byte of opcodes/payloads 9-14. **C-2** ruled: the Phase 06 pinned hashes for `model.rs` (`07531027…`) and `tests/frame.rs` (`7d62eaa9…`) become historical for those two files; the anchor is the pre-K2 closure table in `k2-cp1-approval.md`; the other pinned files stay byte-identical. Reciprocal notes C-1..C-3 approved for `260826-1605` and `260825-1726`.
- Decision 3: no overlapping owner for the nine paths; the "not examined" plans (`260825-sdk-delivery` Phase 02, `260712-1902-dice-attestation-identity` Phase 03, `260819-1409-cell-to-cell-anywhere-core` Phase 02, controlling `migration-ledger.md` E12) stay unexamined for overlap and are re-checked by the integrating owner at the start of K2 CP2 (stop if one claims a path of the closure).
- Decision 4: the adoption is recorded in the controlling plan, `plan-portfolio.md` and `cell-contract-0024-locks.md` as one scheduling change (done in the same commit as this record).

**Not authorized by this approval:** any source edit under `libs/` or `cells/` (K2 CP2 implementation needs its own go-ahead after the baseline capture), K3 kernel/flag work, K5, key generation, network, `Cargo.lock`, image/signing, lock release beyond the file-bounded exception, Law-1 confirmation 2.

## Digests
- `.agents/261030-1000-kms-ssh-key-custody/lock-handoff-k2-cp2.md`: `00ad044258ce67ea84f468ea9fff9a5fe68312523c67a6a9d6ab92d66bd50283`
- `.agents/261030-1000-kms-ssh-key-custody/k2-cp1.md`: `8c2421a6ad6cc3462ee20b5328f6abe6d635f8d5cea23927a4ff7b6947f1ace1`
- `.agents/261030-1000-kms-ssh-key-custody/k2-cp1-approval.md`: `c5dfdb9d5525f6cdae914c2a7434c1a04a84cf63c69d914473a61d0e9d97b33b`
