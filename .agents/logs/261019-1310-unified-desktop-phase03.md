# Session Log: Unified Desktop Phase 03 Execution

**Date:** 2026-10-19  
**Plan:** `.agents/261019-1310-unified-desktop/`  
**Command:** `$hc-cook .agents/261019-1310-unified-desktop`  
**Stage Completed:** Phase 03 — Compositor-owned frames and native session lifecycle  
**Status:** Completed (Unblocks Phase 04 and Phase 05)

---

## 1. Route & Scope
- Plan target: `.agents/261019-1310-unified-desktop/phase-03-native-sessions.md`.
- Focus: Decouple frames and views in compositor; implement native tab management, generational lifecycle, and safe multi-app grouping without capability delegation.
- Governing specifications: [ADR-0027](../../docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md), `contracts.md`, and `abi-delta-ledger.md` (D-UI-01).

## 2. Phase 03 Build Deliverables
- **`libs/api/src/services/display.rs`**:
  - Defined view management opcodes in `api::display::compositor_ops`:
    - `CREATE_VIEW = 0x20`: Create view session and attach to frame (64-byte payload, 20-byte response).
    - `DESTROY_VIEW = 0x21`: Destroy view session and release surface (16-byte payload, 20-byte response).
    - `SELECT_VIEW = 0x22`: Select/activate tab view within frame (16-byte payload, 20-byte response).
    - `GROUP_VIEW = 0x23`: Move view into target frame (24-byte payload, 20-byte response).
    - `DETACH_VIEW = 0x24`: Detach view into floating frame at `(x, y)` (24-byte payload, 20-byte response).
    - `STAGE_GROUP_CONFIGURE = 0x25`: Stage group resize proposal across member views.
    - `COMMIT_GROUP_CONFIGURE = 0x26`: Commit staged configuration after all member ACKs.
  - Defined C-ABI structures with static compile-time assertions:
    - `ViewRef` (8 bytes): `view_id: u32`, `incarnation: u32`.
    - `FrameRef` (8 bytes): `frame_id: u32`, `generation: u32`.
    - `ViewOpResult` (1 byte enum): `Success` (0), `StaleGeneration` (1), `InvalidToken` (2), `AccessDenied` (3), `CapacityExhausted` (4), `FrameNotFound` (5), `ViewNotFound` (6), `Timeout` (7), `Refused` (8).
    - `ViewOpResponse` (20 bytes): `opcode: u8`, `result: ViewOpResult`, `_pad: [u8; 2]`, `frame_id: u32`, `frame_generation: u32`, `view_id: u32`, `view_incarnation: u32`.
    - `CreateViewRequest` (64 bytes, naturally 8-byte aligned with `file_grant_id: u64` at offset 32): `opcode: u8`, `role: SurfaceRole`, `_pad: [u8; 2]`, `frame_id: u32`, `frame_generation: u32`, `w: u32`, `h: u32`, `min_w: u32`, `min_h: u32`, `recipient_incarnation: u32`, `file_grant_id: u64`, `request_generation: u32`, `_reserved: u32`, `launch_token: [u8; 16]`.
    - Compile assertions: `size_of::<ViewRef>() == 8`, `size_of::<FrameRef>() == 8`, `size_of::<ViewOpResponse>() == 20`, `size_of::<CreateViewRequest>() == 64`.
- **`cells/services/compositor/src/surface_table/frame.rs`**:
  - Implemented `FrameRegistry`, `Frame`, and `View` data models.
  - Bounded capacities enforced upfront: `MAX_VIEWS_PER_FRAME = 32`, `MAX_FRAMES = 64`.
  - Generational invalidation: Monotonic `wrapping_add(1)` generation updates on frame creation, view attachment, detachment, grouping, destruction, and owner crash. Stale generations return `ViewOpResult::StaleGeneration`.
  - Multi-view per process and mixed-owner frames: Distinct processes (`owner_tid`) co-exist as tabs in a single frame without capability delegation or impersonation.
  - Same-frame grouping guard: `group_view` when `target_frame_id == source_frame_id` returns early success without panic.
  - Owner exit reclamation: `cleanup_owner(dead_tid)` sweeps only terminated task's views/surfaces, preserving surviving tabs and advancing frame generations.
- **`cells/services/compositor/src/message_handler/view.rs`**:
  - Authenticated IPC message dispatcher for `0x20..=0x24`.
  - Fixed-size payload validation and bounds checks.
  - Input focus routing: Queries authentic surface owner via `table.get(cap).owner`, ensuring focus cannot be stolen by unauthorized request senders.
  - Dirty rect tracking: Computes window decoration bounds on view reparenting and destruction, unioning them into `pending_dirty` for immediate repaint.
- **`cells/services/compositor/src/message_handler.rs`**:
  - Integrated `frames.cleanup_owner(dead_tid)` into task supervisor exit sweeps (`cleanup_owner`).

## 3. Verification & Review
- **Compositor Unit Tests (10/10 PASS):**
  - `cargo test --target x86_64-unknown-linux-gnu -p service-compositor`: PASS (10 passed, 0 failed).
  - Tests include: `test_group_and_detach_view`, `test_group_view_same_frame_does_not_panic`, `test_multi_view_creation_and_frame_association`, `test_owner_crash_cleanup_and_focus_preservation`, `test_select_view_and_stale_generation_rejection`.
- **Core Workspace Unit Test Suite (356/356 PASS):**
  - `cargo test --target x86_64-unknown-linux-gnu -p api -p service-compositor -p viui -p ocel -p desktop`: PASS (356 passed, 0 failed, 11 ignored).
- **Adversarial Audit (Phase03AuditorRound2):**
  - Verdict: **FULL ACCEPTANCE**.
  - All critical, high, and medium findings from Round 1 resolved (C-01 64-byte alignment, C-02 same-frame unwrap panic, C-03 static frame generations, C-04 typed wire bounds, S-01 dirty tracking & focus leak, S-02 geometry tracking, S-03 focus routing authentication).

## 4. Plan Reconciliation & State
- `abi-delta-ledger.md`: D-UI-01 CP2 marked `IMPLEMENTED & VERIFIED`.
- `phase-03-native-sessions.md`: Marked complete with deliverables and deviation log recorded.
- `plan.md`: Updated Phase 03 to completed; unblocked Phase 04 (Desktop Launcher / Taskbar) and Phase 05 (Six-Zone Snap & Placement).
- `CHANGELOG.md`, `docs/project-changelog.md`, `docs/display-api.md`, `docs/roadmap/current-focus.md`: Updated with complete Phase 03 deliverables.

## 5. Handoff
- Phase 03 is complete.
- Unblocked next phases:
  - Phase 04 — Desktop launcher, taskbar sessions and spotlight integration (`phase-04-desktop-launcher.md`).
  - Phase 05 — Six-zone placement and F11 fullscreen geometry (`phase-05-placement-fullscreen.md`).
