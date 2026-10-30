# K3 CP2 approval (Law-1 confirmation 2)

Date: 2026-10-30. Approver: accountable owner (the user), reply "duyệt confirmation 2 + các deviation trên".

- Implementation commit: 47a9d5bda (HEAD at approval: 47a9d5bda776ed468cbf7f40859bfd982d3ba4ed).
- Dossier: reports/k3-cp2-dossier.md, SHA-256 d4bb2458a174e7267afd5663ee5068067b73af0c8515772e9f63851984d9bf02.
- Approved exact delta: libs/api/src/abi/caller_identity.rs +65/-5 (CALLER_FLAG_KMS_SSH_SIGN=1<<6, CALLER_FLAG_KMS_SSH_ADMIN=1<<7, reserved bits doc 8..31; trailer layout, magic, parser unchanged).
- Accepted deviations: 1 (signer 22 fields, V8_CURL 39 bytes), 3 (admin path named only, no edge, ungrantable), 4 (H-1 strips both rights), 5 (narrowed_bits helper, bake-time lint, literal ssh-edge ceiling in self-test).
- Deferred: end-to-end flag proof to K8; CI clippy gate already red at HEAD in untouched crates, no clippy baseline was taken (inferred, not proven).
- Evidence: kernel host 284->292 (default), api 153->154, signer 71 OK; QEMU/lane not run. DEV_POSTURE only.
