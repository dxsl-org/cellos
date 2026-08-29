# Desktop compositor + ViUI managed surface

Status: Complete at the RV64 QEMU software-evidence ceiling
Spec: [approved spec](../specs/260827-desktop-compositor-viui-managed-surface-spec.md)

- [x] Phase 01 — exact framebuffer damage (`phase-01-exact-damage.md`)
- [x] Phase 02 — managed lifecycle runner (`phase-02-managed-lifecycle-runner.md`)
- [x] Phase 03 — real viui-demo and evidence (`phase-03-viui-demo-evidence.md`)

Dependencies: existing `ostd::display::ViSurface` lifecycle API and compositor policy.
Exclusions: `libs/api`, taskbar/start menu, multi-window policy, client chrome, GPU/GLES, physical-board validation.
