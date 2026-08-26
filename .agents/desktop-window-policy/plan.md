---
title: "Two-Surface Raise and Keyboard-Focus Policy"
status: verified
created: 2026-08-25
branch: main
---

# Two-Surface Raise and Keyboard-Focus Policy

## Context Links
- `826f32f9` (`fix(desktop): route pointer input through compositor`)
- `cells/services/compositor/src/{main.rs,input_handler.rs,pointer_router.rs,z_order.rs}`
- `cells/services/input/src/dispatcher.rs`; `tests/integration/tests/compositor-cursor.rs`

## Overview
One left press on an exposed non-top interactive surface selects its owner, raises it, restores the compositor input endpoint, then sends the local press. The compositor sends later keys only to that owner.

## Key Insights
- `ZOrder::raise`, hit testing, `PointerRouter::focused_owner`, and capture already exist.
- `fb-console` is full-screen and does not consume pointer events; it needs a compositor-enforced background role so it cannot activate or raise.
- Shell or GUI `request_focus` calls replace the input endpoint, so click activation must resend compositor `SetFocus` before the key oracle.

## Requirements
- A hit left press on an interactive surface sets capture/keyboard owner, raises, damages, reasserts compositor focus, then delivers locally.
- Background surfaces never activate, capture, receive routed pointer events, or raise from desktop clicks.
- Captured move/release stay with the owner; keys go only to `focused_owner`.

## Architecture
`input → compositor router → selected interactive (cap, owner) → raise + dirty + SetFocus(compositor) → owner`; later keyboard frame → selected owner. Role is stored in compositor surface state and sent at surface creation.

## Related Code Files
- `libs/api/src/services/display.rs`, `libs/ostd/src/display.rs`, `cells/apps/fb-console/src/main.rs`.
- `cells/services/compositor/src/{input_handler.rs,pointer_router.rs,main.rs,surface_table.rs}`.
- New `cells/tests/window-policy-probe`, workspace/package entries, `gen_disk.ps1`, and integration QMP test.

## Implementation Steps
1. Add interactive/background surface role, retain interactive default, and mark `fb-console` background.
2. Persist compositor TID and reuse the existing fire-and-forget `SetFocus` request at successful activation.
3. On interactive left press: establish capture/owner, raise, dirty, reassert focus, then send pointer input.
4. Add a deterministic two-owner probe and one QEMU visual/input test.

## Todo List
- [x] Add non-interactive background role and mark `fb-console` and VMM scanout.
- [x] Implement atomic selection/raise/focus transition.
- [x] Add/package two-surface probe and QEMU evidence.

## Success Criteria
One test sees front then back in an overlap pixel, back-only press/captured release/key markers after selection, and no background activation.

## Risk Assessment
Premature delivery can expose old stacking; missed damage makes raise invisible; missing reassertion leaves keys with shell. Full-screen background activation would obscure all windows.

## Security Considerations
Focus derives only from compositor hit targets and owner-checked surfaces. The role only reduces a surface's own input eligibility; it does not grant authority.

## Next Steps
Keep decorations, drag/resize, close lifecycle, and general window management
as separately approved policy work.
