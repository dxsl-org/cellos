# K8 CP1 (T0 lane) + ownership handoff approval

Date: 2026-10-30. Approver: accountable owner (user), "Duyệt K8 CP1 + handoff theo khuyến nghị" (D8-1..7 as recommended). HEAD a12fd2d06.
- Text: k8-cp1.md SHA-256 5366add455ac9b7766f0f645eaba25afb3c8a0edbddf5e83d546ba55c34517e0. Authorizes exactly section 18 A-D and nothing in E: private lane edits (section 4), H0, B0 (<=3 boots per variant), one RT0 and one RT0f evidence boot, no rerun; no image build/pull, no network, no KVM, no promotion, no T1/RT1, no RG; TEST keys under /tmp/cellbox-phase09-kssh-*; no commit of keys/images; DEV_POSTURE; QEMU/TCG only.
- No locked relation in the edit closure (grep, [INFERENCE]); no Law-1 file.
- CBX-24 disposition ("publickey half met at DEV_POSTURE ceiling") is made only by an explicit user disposition after K8 CP2.
