# K7 CP1 + lock handoff approval

Date: 2026-10-30. Approver: accountable owner (user), "Duyệt CP1 + handoff K7" (including dev-dependency of the admin cell tests on service-kms, feature ssh-dev-custody).
- Text: k7-cp1.md (SHA-256 f1c02a8c1493988e4c327423f6579fd9906a50b971837a8e74ffdcd5eab9a108) over k7-cp1-draft.md. Amendment to the K3 handoff; no Law-1 surface (flags landed in K3, confirmation 2 in k3-cp2-approval.md).
- File-bounded exception: the k7-cp1.md section 3 paths only. Host and regression lanes only; no network, key generation (test seeds), QEMU/VM/image; DEV_POSTURE; serves Cellbox publickey, not an Intel C2C outcome.

## Amendment 2026-10-30 (user, "A: duyệt thêm policy.rs (chỉ sửa test)")
`kernel/src/policy.rs` is added to the closure for exactly one test edit: the K3 assertion in `signer_v8_output_decodes_with_kernel_rule` (no admin row) becomes "exactly one admin row, for /bin/kms-ssh-admin", with its comment. No production code in policy.rs changes.

## Amendment 2026-10-30 (user): F1 defect fix
`cells/tools/kms-ssh-admin/src/main.rs` (crate root) gains `#![forbid(unsafe_code)]` and the `unsafe` keyword in `src/tests/armor.rs` is removed (test content unchanged otherwise). Closure otherwise unchanged. Reason: cellos-sign F1 gate (see k8-cp1-approval.md).
