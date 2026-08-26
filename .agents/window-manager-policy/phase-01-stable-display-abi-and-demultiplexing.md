---
phase: 1
title: "Stable display ABI and client demultiplexing"
status: pending
priority: P1
effort: 1d
dependencies: []
tier: thinking
---

# Phase 01: Stable display ABI and client demultiplexing

## Overview
Freeze the additive display protocol and make one client-side dispatcher preserve both existing compositor-forwarded input and new compositor lifecycle frames. This phase creates no alternative input endpoint and leaves old clients source- and wire-compatible.

## Requirements
- Functional: define title, configure acknowledge, close response, minimize/maximize/restore requests, and compositor-to-owner configure/close/state events; expose typed `ViSurface` operations and a bounded event pump.
- Non-functional: every public wire struct is `#[repr(C)]`, explicitly LE encoded/decoded, has a compile-time size assertion and documented byte layout; titles are valid UTF-8 of at most 64 bytes; no allocation or unbounded queue is introduced on the receive path.
- Compatibility: retain opcodes `0x01..0x09`, their replies, the legacy nine-byte create form, and `ostd::input::poll_events`'s existing `InputEvent` result.

## Architecture
Reserve client-to-compositor opcodes `0x0A SET_TITLE`, `0x0B CONFIGURE_ACK`, `0x0C CLOSE_RESPONSE`, `0x0D MINIMIZE`, `0x0E MAXIMIZE`, `0x0F RESTORE`, and `0x11 DETACH_REPLACED_GRANT`; keep `0x10 GET_SCREEN_SIZE`. All new cap fields are `u32`, matching `AttachGrant`; the compositor widens only after decode.

| Frame | Exact fixed LE layout | Contract |
|---|---|---|
| `SetTitle` (72 B) | `opcode:u8, len:u8, pad:[u8;2], cap:u32, title:[u8;64]` | owner only; `len ≤ 64`, trailing bytes zero, UTF-8 bytes `[0..len)`; empty title is allowed. |
| `ConfigureAck` (12 B) | `opcode:u8, pad:[u8;3], cap:u32, serial:u32` | commits only the matching staged Grant. |
| `CloseResponse` (12 B) | `opcode:u8, accept:u8, pad:[u8;2], cap:u32, serial:u32` | `accept` is exactly 0 or 1. |
| State request (8 B) | `opcode:u8, pad:[u8;3], cap:u32` | owner requests minimize/maximize/restore; maximization/restoration emits configure. |
| `DetachReplacedGrant` (16 B) | `opcode:u8, pad:[u8;3], cap:u32, reg_id:u64` | acknowledges a retired, no-longer-active Grant before the owner unregisters it. |
| `WindowConfigure` (28 B) | `opcode:0xA0, kind:u8, pad:[u8;2], cap:u32, serial:u32, x:i32, y:i32, w:u32, h:u32` | compositor event; kind is `Resize`, `Maximize`, or `Restore`. |
| `WindowCloseRequest` (12 B) | `opcode:0xA1, pad:[u8;3], cap:u32, serial:u32` | compositor event; only the matching response resolves it. |
| `WindowStateChanged` (12 B) | `opcode:0xA2, state:u8, pad:[u8;2], cap:u32, serial:u32` | compositor event; state is `Normal`, `Minimized`, `Maximized`, `Closing`. |

`WindowState`, `ConfigureKind`, and any boolean-like discriminants are `#[repr(u8)]` and decode only listed values. Use zeroed fixed arrays, reject non-zero reserved bytes, integer overflow, unknown opcodes, and frames shorter than their fixed layout.

`ostd::display` owns `poll_surface_events(max)` and a fixed per-cap compositor-event table: at most 32 compositor surfaces × a configure slot, a coalesced state slot, and a close slot (96 entries). Refactor `ostd::input::poll_events` to feed every frame received from the compositor into the same internal dispatcher: input (`0x10`) is returned as before; valid `0xA0..0xA2` occupies its cap/kind slot; unknown compositor frames are discarded. `poll_surface_events` first drains those slots, then receives only from the compositor and dispatches identically, so neither API consumes and loses the other class. A newer same-cap state replaces its predecessor; configure and close remain distinct, and a close cannot be lost because the compositor permits one close-pending state per live cap.

## Assumptions
- **Claim:** `spin` is already available to guard an SDK-local bounded queue if the public APIs can be called from more than one task.
  **Confidence:** high
  **How to verify:** inspect `libs/ostd/Cargo.toml` and the existing synchronization convention before implementation.

## Related Files
- Modify: `libs/api/src/services/display.rs` — public constants, enums, wire structs, docs, encoders/decoders, assertions.
- Modify: `libs/ostd/src/display.rs` — title/lifecycle API, staged replacement helper, typed surface events.
- Modify: `libs/ostd/src/input.rs` — route compositor-origin frames through the shared dispatcher without changing `poll_events` callers.
- Create if needed: a focused `libs/ostd/src/display_events.rs` (≤200 lines), exported from the existing module root.

## Implementation Steps
1. Add the listed opcodes, `MAX_TITLE_BYTES`, discriminants, `#[repr(C)]` structs, LE codec methods, exact-size assertions, and wire comments; do not rely on Rust field layout for IPC.
2. Add `ViSurface::{set_title, minimize, maximize, restore, respond_close, apply_configure}` and `SurfaceEvent`; `apply_configure` allocates/shares a fresh Grant, sends existing `ATTACH_GRANT`, then `CONFIGURE_ACK`, updates local pointer/dimensions only after a success reply, sends `DETACH_REPLACED_GRANT` for the old registration, then unregisters it.
3. Introduce one bounded per-cap compositor-frame dispatcher/table. Make both public poll functions use it; preserve frame order per sender and leave messages from every non-input/non-compositor sender queued.
4. Add unit coverage for all byte layouts, malformed/reserved data, title bounds/UTF-8, input/lifecycle interleaving, same-cap state coalescing, configure/close separation, and the 32-cap slot bound; preserve old input tests.

## Task List
- [ ] Freeze and test the fixed public wire layouts.
- [ ] Add the typed `ViSurface` lifecycle surface and replacement helper.
- [ ] Route compositor frames through one bounded dispatcher.
- [ ] Cover invalid and interleaved frames without changing old input behavior.

## Success Criteria
- [ ] Every listed struct has exact byte-size/LE round-trip tests and rejects invalid discriminants, pads, titles, and short frames.
- [ ] An old `poll_events` caller receives the same forwarded mouse/key frames when no lifecycle event exists.
- [ ] Interleaved `0x10`, configure, close, and state frames deliver input and exactly-once lifecycle events without consuming unrelated sender traffic.
- [ ] A successful configure helper never mutates or frees its old Grant until the compositor has committed the matching serial.
- [ ] Verification: `cargo test -p api -p ostd`.

## Security Considerations
Only the compositor TID may populate SDK lifecycle events; the dispatcher rejects lookalike frames from another sender. The client never trusts a cap/serial until it matches its live surface; titles and frame buffers have fixed bounds. The API does not add a capability or permit the compositor to write a Grant.

## Risk Notes
`sys_try_recv(compositor_tid)` cannot put a frame back, so the shared dispatcher is mandatory rather than optional. A global queue must be bounded and must not silently discard close requests; if task locality cannot be guaranteed, use the project-standard lock, not unsynchronized static state.

## Deviation Log
None.
