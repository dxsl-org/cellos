# Session Log: Unified Desktop Phase 05 Execution

**Date:** 2026-10-19  
**Plan:** `.agents/261019-1310-unified-desktop/`  
**Command:** `$hc-cook .agents/261019-1310-unified-desktop`  
**Stage Completed:** Phase 05 — Six-zone snap and F11 fullscreen  
**Status:** Completed (Unblocks Phase 06 and Phase 09)

---

## 1. Route & Scope
- Plan target: `.agents/261019-1310-unified-desktop/phase-05-placement-fullscreen.md`.
- Focus: Six-zone snap layout calculations with integer grid math, usable work-area exclusion for the 36 DIP bottom taskbar, group minimum-size preflighting and refusal, content-only F11 fullscreen mode, and prior placement/floating rect restoration.
- Governing specifications: [ADR-0027](../../docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md), `contracts.md`, and `abi-delta-ledger.md` (D-UI-02).

## 2. Phase 05 Build Deliverables
- **`libs/api/src/services/display.rs`**:
  - `SnapZone` enum with 6 layout targets: `LeftHalf = 1`, `RightHalf = 2`, `TopLeftQuarter = 3`, `BottomLeftQuarter = 4`, `TopRightQuarter = 5`, `BottomRightQuarter = 6`, and `from_u8`.
  - `FramePlacement` enum: `Floating`, `Snapped(SnapZone)`, `Maximized`, `Fullscreen`.
  - Wire opcodes added to `compositor_ops`:
    - `SNAP_FRAME = 0x27`: payload `[frame_id: u32, frame_generation: u32, zone: u8, _pad: [u8; 3]]` (12 bytes).
    - `TOGGLE_FULLSCREEN = 0x28`: payload `[frame_id: u32, frame_generation: u32, enable: u8, _pad: [u8; 3]]` (12 bytes).
- **`cells/services/compositor/src/surface_table/frame.rs`**:
  - `calculate_snap_rect(zone, work_area)`: Integer grid division using shared midpoint endpoints (`half_w = work_area.w / 2`, `other_half_w = work_area.w - half_w`, `half_h = work_area.h / 2`, `other_half_h = work_area.h - half_h`) eliminating pixel gaps and overlap across odd display resolutions.
  - `FrameRegistry::snap_frame(frame_id, frame_gen, zone, work_area)`:
    - Generational verification against stale operations.
    - Group minimum-size preflighting: computes maximum `min_w` and `min_h` across all views in the target frame.
    - If snap zone width < `max_min_w` or height < `max_min_h`, safely returns `Err(ViewOpResult::Refused)` and retains existing floating placement and geometry without mutation.
    - Saves prior floating rect (`saved_floating_rect`), transitions placement to `Snapped(zone)`, and updates backing surface geometry.
  - `FrameRegistry::toggle_fullscreen(frame_id, frame_gen, enable, screen_w, screen_h)`:
    - On enter: saves current placement (`saved_placement`) and floating rect (`saved_floating_rect`), updates placement to `FramePlacement::Fullscreen`, and expands bounds to full display dimensions `(0, 0, screen_w, screen_h)`. Hides host window chrome while preserving background session views.
    - On exit: accurately restores prior placement (`Floating` or `Snapped(zone)`) and prior dimensions (restoring `saved_floating_rect` or recalculating the snapped rectangle against the work area).
- **`cells/services/compositor/src/message_handler/view.rs`**:
  - Parametrized view handler with dynamic screen dimensions `(screen_w, screen_h)`.
  - Computed usable work area excluding the 36 DIP bottom taskbar (`work_area.h = screen_h.saturating_sub(36)`).
  - Dispatched `SNAP_FRAME` (`0x27`) and `TOGGLE_FULLSCREEN` (`0x28`), returning typed `ViewOpResponse` and unioning old and new frame rects into `pending_dirty` for full repainting.

## 3. Verification & Review
- **Compositor Placement & Snap Unit Tests (PASS):**
  - `cargo test --target x86_64-unknown-linux-gnu -p service-compositor`: PASS (12 passed, 0 failed).
  - Tests include: `surface_table::frame::tests::test_six_zone_snap_geometry_and_min_size_refusal`, `surface_table::frame::tests::test_f11_fullscreen_toggle_and_state_restoration`.
- **Workspace Unit Test Suite (359/359 PASS):**
  - `cargo test --target x86_64-unknown-linux-gnu -p api -p service-compositor -p viui -p ocel -p desktop`: PASS (359 passed, 0 failed, 11 ignored).
- **Bare-Metal x86_64 Cross-Compilation:**
  - `RUSTFLAGS="-C relocation-model=pic" cargo build --release --target x86_64-unknown-none -Z build-std=core,alloc -p service-compositor`: Clean build with PIC relocation model.
- **Security & Sign Policy Gate:**
  - `python3 scripts/cellos-sign --check`: PASS (pinned to `nightly-2026-05-01`, zero unallowlisted unsafe code across 123 crates, `#![forbid(unsafe_code)]` enforced).
- **Integration Test Runner:**
  - `tests/integration/tests/desktop-x86.rs` compiles without error.

## 4. Plan Reconciliation & State
- `abi-delta-ledger.md`: `D-UI-02` (Six-Zone Snap & F11 Fullscreen Geometry) marked `IMPLEMENTED & VERIFIED`.
- `phase-05-placement-fullscreen.md`: All acceptance criteria marked verified (`[x]`), status set to `completed`, and deliverables recorded.
- `plan.md`: Updated Phase 05 to `completed`; unblocked Phase 06 (`Ocel native session and reader cutover`) and Phase 09 (`GUI Terminal and shell-session streams`).
- `verification.md`: Recorded Phase 05 test and build evidence; updated status row to `Verified passing`.

## 5. Handoff
- Phase 05 is complete and fully verified.
- Unblocked next phases:
  - Phase 06 — Ocel native session and reader cutover (`phase-06-ocel-cutover.md`), satisfying dependencies [2, 3, 4, 5].
  - Phase 09 — GUI Terminal and shell-session streams (`phase-09-terminal-streams.md`), satisfying dependencies [1, 2, 3, 4, 5].
