# K4b CP1 + lock handoff approval

Date: 2026-10-30. Approver: accountable owner (user), "Duyệt K4b CP1 + handoff" (D-4b.11..16 and rulings C-K4b-1..3 as recommended). HEAD a12fd2d06.
- Text: k4b-cp1.md SHA-256 e44e301751b6fe96f4ea242f2f50e3c73e64fdd0a73ea86d6a5ff38a12b13fba. File-bounded exception for its closure (8 edited files, 5 new groups; no libs/api, libs/types, Cargo.lock). Opt-in feature ssh-dev-persist (implies ssh-dev-custody), DEV_POSTURE, no rollback or crash-durability claim.
- C-K4b-1: K4b's two-line edit of the K5-shared dispatch.rs lands before Phase 06 rebases.
- Build order: after K8 T0 lane; RT1 evidence needs its own K8 amendment.
