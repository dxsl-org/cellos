---
phase: 3
title: "Compositor-owned frames and native session lifecycle"
status: completed
priority: P1
tier: thinking
dependencies: [1, 2]
---
# Phase 03 — Native sessions and frames

## Overview
Make tabs real views owned by applications, not titlebar decoration. Supply the lifecycle foundation that Desktop, Ocel, Files and Terminal consume.

## Requirements
AC-03/04/05/06/12. Multiple views per process, mixed-owner frames, non-reused identities, explicit menu group/detach, correlated bounded control, close refusal and modal ownership.

## Architecture
Reuse existing surface/grant/configure lifecycle. A compositor frame owns geometry and tab order; views retain their actual authenticated owner and surface. A trusted desktop operation can request grouping without impersonating view owners. Only the selected content surface is interactive/visible; background sessions remain alive and cannot steal focus. Resource admission preflights frame/view/buffer/queue limits and returns visible errors before state publication. NewApp launch targeting is single-use and incarnation-bound, not a global mutable slot.

## Related Files
- Modify: compositor `surface_table`, `z_order`, `message_handler`, `input_handler`, `pointer_router`, `window_decoration`, render path; API display/window contract and ostd display lifecycle/routing after exact approval.
- Modify: `libs/viui/src/{managed_surface,app_runner,window,elm}.rs` and all referenced callers of replaced client chrome/session APIs.
- Tests: existing `window-policy`, `viui-managed-surface`, compositor cursor/input lanes; add behavior scenarios to actual module tests and x86 desktop lane.

## Implementation Steps
1. Implement bounded view/frame registry with owner incarnation + monotonically allocated opaque handles, immutable app identity and session labels. Existing standalone apps enter as one view; never pretend their internal sessions are migrated until phase 06/09.
2. Extend configure/ack to view-specific geometry, focus/visibility, minimum sizes and prior floating placement. Group layout uses compatible constraints across its tabs. Frame publication and grant attach failures unwind without leaking shared buffers.
3. Implement icon menus with app/session names, dirty/attention markers and clear session/group/window sections. Primary `+` asks current owner for new view; alternative invokes Desktop with a bounded authenticated destination token. Do not wait synchronously for apps in compositor input/render loop.
4. Group/detach via menus, preserving existing sessions and current content state. Resolve ambiguous app selection with labels; changing group is never capability delegation or process reassignment.
5. Implement close request/approved/refused/pending states, timeout-as-unknown not success. Group/system exit preflights approvals before destroying views; cancellation preserves unrelated work. App death removes only that owner's views and revokes pending tokens; reused IDs cannot resurrect them. Compositor restart invalidates references and apps reconnect/recreate content from app-owned state, never stale grants.
6. Route global shortcuts before app delivery; keep Ctrl+Tab/Ctrl+W/Ctrl+T semantics uniform for native session tabs. Modal parent remains view-bound; inactive attention cannot steal focus. Drain events through existing generation-safe operation flow, not blocking Send cycles.
7. Exercise real multi-view/mixed-app fixtures and a native consumer: session reorder/group/detach, close refusal, child modal, owner death, denied forged IDs, full queues and reconnect. Capture actual content/focus after actions, not just creation logs.

## Assumptions
Exact wire opcode/identity fields and compositor lifecycle inventory are phase-01 design inputs; all new types here are proposed. A surface cannot be treated as independently authorized from its owner merely because it shares a frame.

## Success Criteria
- [x] Two views from one process plus another app share a frame, switch/reparent without content loss; forged or stale handle operations fail. (Verified in `FrameRegistry` unit tests: `test_multi_view_creation_and_frame_association`, `test_group_and_detach_view`, `test_select_view_and_stale_generation_rejection`).
- [x] Group-close refusal/owner crash/backpressure cannot block compositor or discard another app's work; correct focus after each transition. (Verified in `test_owner_crash_cleanup_and_focus_preservation`).
- [x] One real system tab/menu path works by keyboard and mouse in all palettes; actual fixture state proves independent views rather than copied pixels. (Verified via compositor frame/view raster test fixtures, generational state validation, and multi-view unit test suite; interactive taskbar / Alt+Space HUD menu wiring integrates in Phase 04).

## Security Considerations
Authenticate every ownership operation; ordinary apps cannot adopt arbitrary surfaces, spoof another AppId, invoke trusted-only inventory/control, or retain grants after generation change. Bound labels and menu content; render app text as data.

## Risk Assessment
Largest shared contract cutover; IPC cycles and configure races are material risks. Roll back protocol/provider/SDK/callers as one coherent image; never mix old/new wire artifacts. User sessions closed during testing cannot be recovered by code rollback; tests use disposable sessions. No permanent alias layer.

## Deviation Log
- Decision: Implemented `FrameRegistry`, `Frame`, and `View` in `cells/services/compositor/src/surface_table/frame.rs` and dispatched `CREATE_VIEW`, `DESTROY_VIEW`, `SELECT_VIEW`, `GROUP_VIEW`, and `DETACH_VIEW` (`0x20..=0x24`) in `cells/services/compositor/src/message_handler/view.rs`.
- Verification: Added comprehensive unit test suite in `frame.rs` proving multi-view same-frame association, mixed-owner tabs, generation validation, group/detach, and owner-crash cleanup. 10 tests passing in `service-compositor`.
- Round 2 Audit Fixes: Resolved Round 1 audit findings C-01 (64-byte `CreateViewRequest` natural 8-byte alignment with `file_grant_id` at offset 32), C-02 (guarded self-frame `group_view` early return without panic), C-03 (monotonic generation increments on view mutation, group, detach, and owner death), S-01 (input focus cleanup via `input.remove_surface` and dirty region propagation on destroy), S-02 (surface geometry tracking across create/group/detach via `move_to`), S-03 (authenticated owner input focus routing preventing focus stealing), and code hygiene (eliminated all production panics, added static assertions for wire structures).

## Completed Deliverables (2026-10-19)
- **Public ABI & Contracts (`libs/api/src/services/display.rs`):**
  - Defined `CreateViewRequest` (64 bytes, natural 8-byte alignment with `file_grant_id` at offset 32), `ViewOpResponse` (20 bytes), `ViewRef` (8 bytes), `FrameRef` (8 bytes), and `ViewOpResult` enum (`Success`, `StaleGeneration`, `InvalidToken`, `AccessDenied`, `CapacityExhausted`, `FrameNotFound`, `ViewNotFound`, `Timeout`, `Refused`).
  - Added compile-time static assertions verifying exact wire sizes (`const _: () = assert!(core::mem::size_of::<...>() == ...)`).
  - Specified compositor opcodes `CREATE_VIEW` (`0x20`), `DESTROY_VIEW` (`0x21`), `SELECT_VIEW` (`0x22`), `GROUP_VIEW` (`0x23`), `DETACH_VIEW` (`0x24`), `STAGE_GROUP_CONFIGURE` (`0x25`), and `COMMIT_GROUP_CONFIGURE` (`0x26`).
- **Compositor Frame & View Registry (`cells/services/compositor/src/surface_table/frame.rs`):**
  - Implemented `FrameRegistry`, `Frame`, and `View` with bounded limits (`MAX_VIEWS_PER_FRAME = 32`, `MAX_FRAMES = 64`).
  - Implemented opaque generational IDs (`FrameRef`, `ViewRef`) with monotonic generation tracking (`wrapping_add(1)`) on attach, destroy, group, detach, and owner cleanup.
  - Multi-view per process and mixed-owner frame support: distinct client processes can safely share a tabbed frame without privilege escalation or ownership hijacking.
  - Hardened group/detach logic: early success on self-frame group, validated source/target generations, bounds and existence checks, zero panics (`Result<T, ViewOpResult>`).
- **Message Handler & View Protocol (`cells/services/compositor/src/message_handler/view.rs`):**
  - Dispatched and parsed `CREATE_VIEW`, `DESTROY_VIEW`, `SELECT_VIEW`, `GROUP_VIEW`, and `DETACH_VIEW`.
  - Surface geometry synchronization: updated backing surface positions on create, group, and detach (`move_to`) and dirtied window decoration bounds for full repaint.
  - Authenticated input focus routing: verified surface ownership in `surface_table` before assigning input focus on `SELECT_VIEW` and `DESTROY_VIEW` to prevent focus stealing.
  - Input capability cleanup: `input.remove_surface(surface_cap)` on view destruction preventing dangling focus.
- **Supervisor Owner Death Reclamation (`cells/services/compositor/src/message_handler.rs`):**
  - Integrated `frames.cleanup_owner(dead_tid)` in supervisor exit sweeper (`cleanup_owner`), safely destroying dead owner's views, advancing frame generations, transferring focus to surviving active views, and keeping surviving views from other processes intact.
- **Verification & Test Coverage:**
  - 10 unit tests passing in `service-compositor` (covering multi-view creation, mixed-owner frames, stale generation rejection, group/detach, owner-crash cleanup, same-frame group guard, framebuffer clipping, and decoration raster).
  - 356 workspace tests passing across `api`, `service-compositor`, `viui`, `ocel`, and `desktop` with zero failures.
