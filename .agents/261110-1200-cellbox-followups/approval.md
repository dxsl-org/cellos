# Follow-up CP1s approval

Date: 2026-10-30. Approver: accountable owner (user).
- (1) phase02-marker-cp1.md: approved as recommended (D-P2-1..10 incl. the optional 5-line CI step); adopted bounded slice with reciprocal notes R1-R5; no frontmatter change to the Phase 02 plan; P01 pins not refreshed.
- (2) guest-pipeline-cp1.md: approved as recommended (D-GP-1..10) and authorized to run: offline gates, <=2 rehearsal boots, 1 evidence boot, <=1 confirmation boot, tokens T3P_*; lane-only closure; private key in the guest sshd; no product edit.
- (3) reap-and-rst-cp1.md (NET-GCD): approved as recommended (D-GCD-1..) with its handoff; implement AFTER (2) lands (lane pin list); host tests only, no QEMU.
- (4) other-services-owner-death-cp1.md: OSD-1 + OSD-3 approved with the section 9 handoff (OSD-2/KMS not adopted); host tests + bare-metal builds, no QEMU.
- File-bounded exceptions are exactly each CP1's closure; any other path stops the slice. Law-1 not touched by any item. Digests: phase02-marker.md 99d637a37ed2b46c;guest-pipeline.md eddbb9888a66147f;reap-and-rst.md 89e8daf5cca2a5c9;other-services-owner-death.md a3a4f6fe2e796405;
- Not authorized: push by agents, canonical promotion, production claims.

## Amendment 2026-10-31 (user): F-GP-1
"A host-only ngay; B chạy luôn nếu A không lý giải": Slice A of fgp1-diagnosis.md (host-only tests, libs/vissh/tests/session.rs and only what the CP1 draft lists) is authorized now. Slice B (<=3 diag boots: 1 rehearsal, 1 evidence, 1 confirmation; D-FD-1..5 as recommended in the draft; new tokens; lane re-pin of the four Net files after NET-GCD inside B's lane closure) is authorized to run only if Slice A does not explain F-GP-1 (i.e. A is green and the pcap comparison is still needed). No product source change; a product defect found is reported, not fixed.
