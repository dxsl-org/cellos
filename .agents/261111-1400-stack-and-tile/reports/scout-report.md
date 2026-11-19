# Scout report — Stack & Tile (2026-11-11)

Paths relative to `cells/services/compositor/src/` unless noted.

## Frames today (exist, unused)
- `surface_table/frame.rs:15-41` — `Frame { generation, x, y, w, h, views: Vec<ViewRef>, selected_view, min_w, min_h, placement, saved rect/placement, owner_tid }`; `View { incarnation, frame_id, surface_cap, owner_tid }`; limits 32 views/frame, 64 frames (`:11-12`).
- Ops (IPC only, dispatched `message_handler.rs:105-111`, handlers `message_handler/view.rs`): `create_view :61`, `destroy_view :151`, `select_view :207` (no generation bump `:238`), `group_view :252` (moves origin only, no resize/min merge), `detach_view :369`, `snap_frame :482` (origin only; taskbar 36px hard-coded `view.rs:305`), `toggle_fullscreen :536` (x not reset `:566`). STAGE/COMMIT_GROUP_CONFIGURE: opcodes only (`libs/api/src/services/display.rs`).
- Every op is owner-gated (`frame.rs:108,163,227,275,309,389,494,549`); a test asserts the mixed-owner denial (`:763-767`). No client in libs/ or cells/ sends any view op; `ostd::display` only sends `CREATE_SURFACE` (`libs/ostd/src/display/ipc.rs:17`).

## What ignores frames
- `render.rs` (per-cap decoration + blit), `pointer_router/target.rs:17-39` (hit-test every cap), `window_decoration.rs` (`tab_rect`, `shapes`, `hit_test`, `menu_button_rect` take one `content: Rect` → exactly one tab), window menu `window_decoration/menu.rs` (`MENU_ITEMS` fixed 3), `OpenMenu` per cap (`pointer_router.rs`).
- Drag moves one surface (`pointer_router/input.rs` `Capture::Drag`); `DESTROY_SURFACE` never touches frames (owner death does, `message_handler.rs:152`).

## Contracts that constrain the design
- Configure: `surface_table/configure.rs:60-91` — `begin_configure` refuses a surface that is not window-managed, not visible-for-paint, already has a pending configure, or has a retired Grant. One pending configure per surface; commit on matching ack with a staged Grant (`:111-157`). A silent owner never acks (window-policy `wm-silent`).
  → hidden stack members must stay `WindowState::Normal` (hidden by frame selection, not by state), or they can never be configured.
- Resize gesture: `pointer_router/resize.rs` `propose` → `begin_configure(Resize, rect)`; min content 64x48 (`resize.rs:7-8`).
- Z-order: `z_order.rs` has `push/remove/raise(cap)` and iterators only — no group raise.
- Focus: `input_handler.rs` `request_focus`, `focus_new_window`; keys go to the selected cap's owner (`forward_key`).
- Panels never join frames (only `SurfaceRole::Interactive` is window-managed, `surface_table/state.rs`).
- `.agents/261019-1310-unified-desktop/contracts.md:20,30-31`: frame membership is presentation only (no capability transfer); Trusted Desktop may request grouping; group configure is one transaction; hidden tabs never expose stale-sized content or pointer transforms.
- ADR-0027 (`docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md`): tab = icon + app name; icon menu = session actions, grouping/detaching, placement; grouping via explicit menu commands first, drag deferred; closing a tab ≠ closing a group.

## Current chrome (this session)
- Tab = `[icon box 20px at tab+(4,3)] title`; `Hit::{Menu, Title, Resize, Content}`; window menu Maximize/Restore, Minimize, Close; double-click title toggles maximize (`pointer_router/input.rs`).
- Soft shadow `SHADOW_MARGIN` 14, `GRIP` 6 (`window_decoration.rs`).

## Test assets
- Host: `cargo test -p service-compositor --target x86_64-unknown-linux-gnu` (48 tests incl. decoration geometry, router hover/menu, appearance).
- QEMU: `tests/integration/tests/window-policy.rs` (RV64, `window-policy-probe` roles back/front/background/wm-primary/wm-silent/wm-close, 160x160 surfaces, pixel + serial assertions); `scripts/qemu-phase04-gui-test.sh` + `scripts/phase04_gui_drive.py` (x86 KVM, 295 checks).
