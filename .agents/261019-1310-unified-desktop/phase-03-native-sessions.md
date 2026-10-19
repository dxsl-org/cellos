---
phase: 3
title: "Compositor-owned frames and native session lifecycle"
status: pending
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
- [ ] Two views from one process plus another app share a frame, switch/reparent without content loss; forged or stale handle operations fail.
- [ ] Group-close refusal/owner crash/backpressure cannot block compositor or discard another app's work; correct focus after each transition.
- [ ] One real system tab/menu path works by keyboard and mouse in all palettes; actual fixture state proves independent views rather than copied pixels.

## Security Considerations
Authenticate every ownership operation; ordinary apps cannot adopt arbitrary surfaces, spoof another AppId, invoke trusted-only inventory/control, or retain grants after generation change. Bound labels and menu content; render app text as data.

## Risk Assessment
Largest shared contract cutover; IPC cycles and configure races are material risks. Roll back protocol/provider/SDK/callers as one coherent image; never mix old/new wire artifacts. User sessions closed during testing cannot be recovered by code rollback; tests use disposable sessions. No permanent alias layer.

## Deviation Log
Append during execution.
