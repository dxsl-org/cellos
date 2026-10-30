# K8 RT1 dossier (T1)

Date 2026-10-30. Authority: k8-rt1-approval.md. Ceiling: private fixture image, QEMU 10.2.0/TCG, loopback Docker OpenSSH_9.9p2, ssh-dev-persist KMS, DEV_POSTURE, F1 private per-run waiver (7 pre-existing items).

Result: RT1 CLASS A (one run, exit 0): five boots on one disk, 261 checks, 0 failed, first sign epoch and first persistence epoch passed; archive scan 34 files, 865 needles, controls ok; census drift PASS; no ciphertext or record header in serial/logs. Evidence: `k8-evidence/rt1/`. Preceding B0 (3 sequences, class A, no evidence value): `k8-rt1-dossier-part1.md`.

Shows: a passphrase-wrapped key imported at boot 1 persists on the P5 CellosFS volume and logs in after reload without re-import; delete persists; record at rest equals the host's independent prediction; a disk reverted to the boot-1 image resurrects the deleted key (rollback NOT protected, by design at DEV_POSTURE).

Not claimed: crash/power-loss durability (no clean shutdown path on this lane; boots end by SIGTERM), rollback resistance, confidentiality at rest beyond passphrase + bcrypt (dev root label is public), production, zeroization, limiter persistence across reboot, RG/GUI.

Disposition of the CBX-24 T1 statement requires an explicit user decision.
