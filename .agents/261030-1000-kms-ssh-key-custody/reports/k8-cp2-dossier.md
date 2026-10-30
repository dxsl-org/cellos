# K8 CP2 dossier (T0 lane)

Date 2026-10-30. Authority: k8-cp1-approval.md (incl. Docker and F1 amendments). Ceiling: private fixture image, fixture kernel with the K3 delta, QEMU 10.2.0/TCG, loopback Docker OpenSSH_9.9p2, DEV_POSTURE. No promotion, no rerun.

| Set | Result | Checks | Evidence |
|---|---|---|---|
| RT0 (T0, ssh-dev-custody) | CLASS A, first sign epoch passed | 58 checks, 0 failed; archive scan 23 files, 640 needles, controls ok | `k8-evidence/rt0/` (run by the main agent, one run, exit 0) |
| RT0f (lane-only kssh-faults cellbox) | CLASS A, first sign epoch passed | 47 checks, 0 failed; archive scan 23 files, 636 needles, controls ok | `k8-evidence/rt0f/` (one run, exit 0) |

Preceding (no evidence value): H0 gate and five B0 smoke boots, `k8-cp2-dossier-part1.md`. KDF on TCG (wrong-passphrase interval): 1 round 63-72 ms, 4 rounds 92-104 ms, 16 rounds 244-267 ms, 64 rounds 865 ms (K5 limit 10 s met). Census 470/470 equal before and after.

Disclosures: (1) F1 gate of cellos-sign is red at HEAD for the tracked tree (7 items remaining after the K7 fix); K8 images were signed under a private per-run waiver (`f1-waiver-unsafe-allowlist.diff`). (2) Kernel self-test `COMPLETION-QUEUE: FAIL` appears in every boot incl. the S10 L5 log; pre-existing, not gated. (3) S05 server-side refusal of a wrong-session signature is not evidenced (K6 verifies the KMS reply locally). (4) Zeroization/RAM residue, rollback, crash durability, production and T1/RT1 are not claimed. (5) Boot-B volatility check and RG deferred to the RT1 amendment.

Disposition: "CBX-24 publickey half met at DEV_POSTURE ceiling (T0)" is NOT recorded; it needs an explicit user disposition.
