# CP1 approval — test/release key separation

Date: 2026-10-31. Approver: accountable owner (the user).

- Direction: "đồng ý tách khóa test và release … dùng khóa test để ký cho tất cả các cell native lên tier 1 … khi release thì tùy tình trạng, các cell này sẽ ở tier 1 hoặc tier 2".
- D1: release key custody = **CI secret** (existing `guard_prod_key` CI markers; public key file committed).
- D2: **every release cell is signed; tier follows the manifest class.** A release-posture kernel denies unsigned cells; dev posture keeps unsigned → Tier 2.
- Execution: "Duyệt, làm phase 1→5" — CP1 for the whole plan.

File-bounded exception: `kernel/src/signing.rs`, `kernel/src/loader/governed_spawn.rs` (unsigned branch only), `kernel/Cargo.toml`, `kernel/build.rs`, `scripts/cellos_sign/**` and its tests, `scripts/lib-sign-cells.sh`, lane scripts that sign cells, rollout-containment docs, `CHANGELOG.md`, `docs/security-model.md`. No `libs/api`/`libs/types` change; no Law-1 delta. Quarantined hashes stay blocked. All 38 lock relations keep their whole-plan scope.
