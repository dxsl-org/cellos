# Phase 14 — Complete Input Service

**Effort:** 80h | **Priority:** P2 | **Status:** pending | **Blockers:** Phase 05, Phase 13

## Overview

Phase 05 fixed the immediate keyboard deadlock; this phase builds the proper input service: a dedicated `input` Cell that owns event ingestion from VirtIO input (keyboard, mouse), maps scancodes to Unicode + key events, maintains focus routing, and dispatches to focused Cells. Compositor (Phase 16) and shell (Phase 17) consume the dispatched events through this service.

## Context Links

- `docs/04-hardware.md` — input subsystem
- `cells/services/input/src/lib.rs` — current stub
- `cells/drivers/input/src/lib.rs` — VirtIO input driver
- `kernel/src/task/drivers/input_map.rs` — scancode → keysym

## Key Insights

- The input cell DOES NOT execute in IRQ context. The kernel-side VirtIO input driver fires IRQ, enqueues raw events into a kernel buffer, then wakes the input cell which drains via `Recv`. This keeps cells/services/input under `#![forbid(unsafe_code)]`.
- Focus model: a single "focused cell" id at a time (managed by compositor in Phase 16; until then, shell is always focused by boot policy). InputDispatcher sends events to whichever cell is currently focused.
- Event type: a tagged union `InputEvent::{Key{scancode, keysym, char, modifiers, state}, MousePosition, MouseButton, MouseScroll}` with a `timestamp_ns` for ordering.
- Layout: support US QWERTY in v1.0; layout abstraction lets us add more later. Modifier state (Shift, Ctrl, Alt, Meta, Caps, Num, Scroll) tracked centrally to avoid per-consumer duplication.

## Requirements

**Functional**
- Input Cell receives raw VirtIO input events from kernel via IPC
- Translates scancodes → keysyms + Unicode chars (US QWERTY)
- Tracks modifier state
- Routes to focused Cell via IPC `KeyEvent` / `MouseEvent` messages
- API: `set_focus(cell_id)`, `get_focus() → cell_id` (compositor uses these)
- Event log to `/var/log/input.log` (debug only) for forensics

**Non-functional**
- < 1ms median input-event-to-dispatch latency
- Handles 1000 events/sec without drops
- Zero `unsafe`

## Architecture

```
Hardware (QEMU virt VirtIO input devices: keyboard, mouse)
      │ IRQ
      ▼
Kernel-side virtio_input driver (Phase 04 + Phase 05 stable)
      │ enqueues raw events to kernel buffer
      │
      ▼
Input Cell (IPC Recv on input-source endpoint)
      │ pulls raw event
      ├─ scancode → keysym (input_map.rs reused)
      ├─ apply modifier state
      ├─ build InputEvent with timestamp
      │
      ▼ (route)
   Focused Cell (shell, compositor surface, etc.)
      │ Recv KeyEvent
      ▼ handle (e.g., shell appends to line buffer)
```

## Related Code Files

**Modify:**
- `cells/services/input/src/lib.rs` — full dispatcher implementation
- `cells/drivers/input/src/lib.rs` — driver glue (currently kernel does most; cells/drivers/input is a thin wrapper for now)
- `kernel/src/task/drivers/virtio_input.rs` — expose IPC source endpoint for raw events
- `kernel/src/task/drivers/input_map.rs` — extend with full US QWERTY map including special keys + modifiers
- `libs/api/src/syscall.rs` — add focus syscalls (or expose via input cell IPC; pick latter for capability cleanliness)
- `cells/apps/shell/src/async_utils.rs` — switch from raw byte read to KeyEvent receive
- `cells/apps/shell/src/shell.rs` — handle KeyEvent variants (Backspace, Enter, arrows, Ctrl+C)

**Create:**
- `libs/api/src/input.rs` — `InputEvent`, `KeyEvent`, `MouseEvent`, `KeySym`, `KeyState`, `Modifiers`, `MouseButton` types
- `cells/services/input/src/layout_us_qwerty.rs` — scancode table
- `cells/services/input/src/dispatcher.rs` — focus routing + event queue
- `cells/services/input/src/modifier_state.rs` — modifier tracking
- `tests/integration/input_dispatch.rs` — boot, inject keys, verify routed to focused cell
- `docs/input-api.md` — IPC schema + key codes

## Implementation Steps

1. **Define event types in `libs/api/src/input.rs`**:
   ```rust
   #[repr(C)]
   pub enum InputEvent {
       Key(KeyEvent),
       MouseMove { x: i32, y: i32, dx: i32, dy: i32 },
       MouseButton { button: MouseButton, state: KeyState },
       MouseScroll { dx: i32, dy: i32 },
   }
   #[repr(C)]
   pub struct KeyEvent {
       pub timestamp_ns: u64,
       pub scancode: u32,
       pub keysym: KeySym,        // virtual key (Enter, A, F1, …)
       pub character: Option<char>, // Unicode if printable
       pub modifiers: Modifiers,    // bitflags
       pub state: KeyState,         // Press / Release / Repeat
   }
   bitflags! { pub struct Modifiers: u8 { const SHIFT=1; const CTRL=2; const ALT=4; const META=8; … } }
   ```
2. **Set up the IPC endpoint** between kernel virtio_input and input cell:
   - Kernel registers an endpoint `InputRawSource` at boot
   - Input cell subscribes via `Recv` to this endpoint
   - On IRQ, kernel posts raw event (scancode + state) to endpoint
3. **Build the scancode map** `cells/services/input/src/layout_us_qwerty.rs`:
   - Static array indexed by scancode → `(KeySym, char_unshifted, char_shifted)`
   - Cover keys 0..0x80 (standard) + 0x80..0xE0 (extended)
4. **Modifier tracker** `cells/services/input/src/modifier_state.rs`:
   - State machine: on Shift press → SHIFT set; on release → cleared
   - Sticky keys: Caps Lock, Num Lock, Scroll Lock toggled on press
5. **Dispatcher** `cells/services/input/src/dispatcher.rs`:
   - `current_focus: AtomicU64` (CellId, settable via IPC `SetFocus`)
   - On each translated InputEvent, IPC Send to `current_focus`
   - If focus invalid (cell exited): log + drop event
6. **Cell loop** `cells/services/input/src/lib.rs::main`:
   - Recv from InputRawSource
   - Translate via layout + modifier tracker
   - Dispatch to focused cell
7. **Update shell** `cells/apps/shell/src/async_utils.rs`:
   - `read_byte` → `read_key() -> KeyEvent`
   - Shell REPL handles KeyEvent variants: printable char → append, Enter → execute, Backspace → erase, Ctrl+C → cancel line, arrow keys → history nav (basic; full readline later in Phase 17)
8. **Event log** (debug-only):
   - If `/var/log/input.log` writable, append each event JSON-style
   - Compile-out in release with `#[cfg(feature = "log_input")]`
9. **Integration test** `tests/integration/input_dispatch.rs`:
   - Inject "Hello\n" via QEMU monitor sendkey
   - Assert shell echoes "Hello" and processes Enter
   - Inject Ctrl+C; assert shell cancels current line
10. **Document** `docs/input-api.md` with KeySym enum, IPC schema, focus rules.

## Todo List

- [ ] Define `InputEvent`, `KeyEvent`, etc. in `libs/api/src/input.rs`
- [ ] Set up kernel↔input-cell IPC endpoint for raw events
- [ ] Build US QWERTY layout table
- [ ] Implement modifier state tracker (incl. sticky lock keys)
- [ ] Implement dispatcher with focus routing
- [ ] Implement input cell main loop
- [ ] Update shell to consume KeyEvent
- [ ] Handle Backspace, Enter, Ctrl+C, basic arrows in shell
- [ ] Add /var/log/input.log (debug feature)
- [ ] Integration test: shell receives "Hello\n" + Ctrl+C
- [ ] Document `docs/input-api.md`
- [ ] CI green

## Success Criteria

- Shell receives translated KeyEvents (not raw scancodes) with correct char + modifiers
- 1000 events/sec sustained without drops (CI soak)
- Median latency < 1ms from IRQ → focused cell receive
- Ctrl+C reliably cancels current shell line
- Focus switching works (set_focus syscall changes destination)
- Zero `unsafe` in input cell

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Scancode set differences between QEMU keyboard backends (set 1 vs set 2) | Med | Med | Probe at startup; default to set 1 (BIOS legacy) on QEMU virt |
| Mouse event semantics differ between virtio-mouse and virtio-tablet | Med | Low | Use tablet (absolute) for v1.0; document |
| Focus changes during burst lose events for prior focus | Low | Low | Drain queue before focus change in dispatcher |
| Modifier state desync if release event lost | Med | Low | Reset modifiers on focus change or every 5s timer pulse |
| Phase 16 compositor wants to take focus management — overlap | Cert | Low | Input cell EXPOSES the API; compositor CALLS it; clean ownership |

## Security Considerations

- Input events visible to focused cell only — non-focused cells cannot snoop
- Defense-in-depth: kernel never grants raw input events to any cell except the input cell
- Audit log (debug feature) does not leak to non-privileged cells; lives in `/var/log` writable only by input cell

## Rollback

Phase 14 layers above Phase 05's fix. Reverting falls back to shell directly Recv'ing raw bytes from kernel — usable but loses translation. Compositor (Phase 16) hard-depends on input cell, so revert affects Phase 16 sequencing.

## Next Steps

Phase 16 (compositor) consumes the focus API. Phase 17 (shell) gains full keybindings (arrows for history, Ctrl-A/E for line nav). Phase 22 (benchmarks) measures latency.
