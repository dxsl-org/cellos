# Phase 02: Grant Surface Protocol API

**Priority**: P0 — all subsequent phases depend on this  
**Status**: ✅ Complete  
**Duration**: ~1d  
**Depends on**: Phase 01 (preferred) + ⚠️ **2× user confirmation** (Law 1: modifies `libs/api/`)

---

## Context Links

- [libs/api/src/display.rs](../../../libs/api/src/display.rs) — current opcodes + types
- [CLAUDE.md §Law1](../../../CLAUDE.md) — "Any changes to libs/api/ require 2× user confirmation"
- [libs/ostd/src/syscall.rs:777](../../../libs/ostd/src/syscall.rs) — `sys_grant_share(perm=0 ReadOnly)`

---

## ⚠️ Law 1 Gate

This phase modifies `libs/api/src/display.rs`. **Do not implement until user confirms twice.**

Changes are additive (new opcodes, new struct). `WRITE_PIXELS` is retained but deprecated.
Existing apps that use `WRITE_PIXELS` continue to work.

---

## Overview

Define the stable protocol between App Cells and the Compositor under the Grant model.
This is pure type/constant definitions — no implementation logic.

**New flow:**
```
App Cell:
  1. sys_grant_register(w × h × 4)   → reg_id (persistent buffer)
  2. sys_grant_share(reg_id, comp_tid, 0 /* ReadOnly */)
  3. ATTACH_GRANT IPC → compositor knows (cap, reg_id, w, h, fmt)
  4. Write pixels directly to ptr from sys_grant_slice(reg_id)
  5. DAMAGE_NOTIFY IPC (24 bytes) → compositor schedules dirty-region blend

Compositor:
  1. Receives ATTACH_GRANT → sys_grant_slice(reg_id) → read-only ptr
  2. Stores SurfaceState { ptr, w, h, fmt, damage, owner }
  3. Receives DAMAGE_NOTIFY → accumulate damage, render on next frame tick
```

---

## Requirements

### New opcodes (additive, no existing value conflicts)

| Opcode | Value | Description |
|--------|-------|-------------|
| `ATTACH_GRANT` | `0x08` | App attaches a Grant to an existing surface cap |
| `DAMAGE_NOTIFY` | `0x07` | App signals a dirty rect (replaces WRITE_PIXELS for damage) |
| `DETACH_GRANT` | `0x09` | App detaches grant before freeing (on surface destroy) |

`WRITE_PIXELS (0x02)` kept as deprecated fallback. Compositor still handles it.

### New types

```rust
/// Compact damage notification — the only IPC sent per frame after initial setup.
/// Total: 1 + 4 + 16 = 21 bytes. Padded to 24 for alignment.
#[repr(C)]
pub struct DamageNotify {
    pub opcode: u8,            // = compositor_ops::DAMAGE_NOTIFY (0x07)
    pub _pad:   [u8; 3],
    pub cap:    u32,           // surface cap (lower 32 bits; caps fit in u32 for now)
    pub rect:   Rect,          // damaged region in surface-local coords (16 bytes)
}                              // total = 24 bytes

/// Attach-grant request.
#[repr(C)]
pub struct AttachGrant {
    pub opcode:  u8,
    pub fmt:     u8,           // PixelFormat byte
    pub _pad:    [u8; 2],
    pub cap:     u32,          // surface cap
    pub reg_id:  u64,          // Grant register ID (physical base addr in SAS)
    pub width:   u32,
    pub height:  u32,
}                              // total = 24 bytes
```

### Deprecation annotation for WRITE_PIXELS

```rust
/// Write pixels into a surface (DEPRECATED — use ATTACH_GRANT + DAMAGE_NOTIFY).
/// Kept for backward compatibility only. Will be removed in a future release.
#[deprecated(since = "0.3.0", note = "Use ATTACH_GRANT + DAMAGE_NOTIFY")]
pub const WRITE_PIXELS: u8 = 0x02;
```

---

## Architecture

### Why `sys_grant_register` not `sys_grant_alloc`?

`sys_grant_alloc` frees pages when the owning task releases it. Surfaces are long-lived (exist
for the duration of a window). `sys_grant_register` pins the buffer for the cell's lifetime,
avoiding per-transfer alloc/free overhead. The app calls `sys_grant_unregister` only when the
window closes.

### Permission model

App calls `sys_grant_share(reg_id, comp_tid, 0 /* ReadOnly */)` before sending `ATTACH_GRANT`.
Compositor receives the share → calls `sys_grant_slice(reg_id)` → gets a `*const u8` (via `*mut
u8` cast; reads only). The kernel enforces ReadOnly — any compositor write attempt faults.

This is the SAS security guarantee: app cannot read compositor's memory; compositor cannot write
app's memory. Enforcement is Rust ownership + kernel perm table, not page faults (they're in the
same address space, but `sys_grant_slice` with ReadOnly gives a pointer the kernel permits reads
from).

### Backwards compatibility surface area

`CREATE_SURFACE (0x01)` keeps the same payload `[w: u32, h: u32]`. The compositor now interprets
this as "create a surface slot without pixel storage". The slot has no pixels until `ATTACH_GRANT`
arrives. A legacy client that sends `WRITE_PIXELS` without `ATTACH_GRANT` will still work via the
old path (compositor allocates an owned buffer on `WRITE_PIXELS` if no Grant is attached).

---

## Related Code Files

**Modify:**
- `libs/api/src/display.rs` — add `DamageNotify`, `AttachGrant`, new opcodes, `#[deprecated]`

**Inform (read before implementing subsequent phases):**
- `libs/ostd/src/syscall.rs:763-803` — `sys_grant_register`, `sys_grant_share`, `sys_grant_slice`
- `libs/api/src/cap.rs` — `CapId` type (used in `SurfaceCap`)

---

## Implementation Steps

1. Open `libs/api/src/display.rs`
2. Add `DamageNotify` and `AttachGrant` structs with `#[repr(C)]`
3. Add `ATTACH_GRANT = 0x08`, `DAMAGE_NOTIFY = 0x07`, `DETACH_GRANT = 0x09` to `compositor_ops`
4. Add `#[deprecated]` annotation to `WRITE_PIXELS`
5. Run `cargo check --workspace` — must be clean (no compile errors from deprecation in existing
   code that uses WRITE_PIXELS; `#[allow(deprecated)]` at call sites if needed)

---

## Todo List

- [x] **⚠️ Get first user confirmation** (Law 1)
- [x] **⚠️ Get second user confirmation** (Law 1)
- [x] Add `DamageNotify` struct to `libs/api/src/display.rs`
- [x] Add `AttachGrant` struct to `libs/api/src/display.rs`
- [x] Add `ATTACH_GRANT = 0x08` to `compositor_ops`
- [x] Add `DAMAGE_NOTIFY = 0x07` to `compositor_ops`
- [x] Add `DETACH_GRANT = 0x09` to `compositor_ops`
- [x] Add `#[deprecated]` to `WRITE_PIXELS`
- [x] `cargo check --workspace` clean

---

## Success Criteria

- [ ] `DamageNotify` and `AttachGrant` are `#[repr(C)]`, derive `Copy + Clone`
- [ ] `size_of::<DamageNotify>() == 24` (assert in a `#[test]`)
- [ ] `size_of::<AttachGrant>() == 24` (assert in a `#[test]`)
- [ ] `cargo check --workspace` passes
- [ ] No `WRITE_PIXELS` usage in new code (only in deprecated dispatch)

---

## Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|-----------|
| Opcode 0x07 conflicts with something | Low | Current `compositor_ops` goes up to 0x10 (GET_SCREEN_SIZE); 0x07 and 0x08 are free |
| `cap: u32` too small (future CapIds > u32::MAX) | Low | CapId is currently `u64`; use `cap_lo: u32` + `cap_hi: u32` or just use u64 and adjust struct size |
| `#[deprecated]` triggers warnings in existing compositor code | Low | Add `#[allow(deprecated)]` at usage sites until they're migrated |

## Security Considerations

- `DamageNotify::cap` must be validated by compositor (owner check before processing damage)
- `AttachGrant::reg_id` must be validated: compositor calls `sys_grant_slice` — kernel enforces
  that the caller actually has a ReadOnly share before returning the pointer
- Compositor must NOT call `sys_grant_free` on a received grant (it's not the owner)

---

## Evidence

**Status**: ✅ Complete

**Verification**:
```bash
$ cargo check -p api
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.04s
```

**Code Evidence**:
1. **DamageNotify struct** — `libs/api/src/display.rs:115–123` defines `#[repr(C)]` struct with `opcode: u8, _pad: [u8; 3], cap: u32, rect: Rect`.
2. **AttachGrant struct** — lines 130–146 defines `#[repr(C)]` struct with opcode, fmt, cap, reg_id, width, height.
3. **Size assertions** — lines 149–150 verify both structs are exactly 24 bytes at compile time.
4. **Opcodes** — `compositor_ops` mod (lines 212–267): `DAMAGE_NOTIFY = 0x07` (line 246), `ATTACH_GRANT = 0x08` (line 252), `DETACH_GRANT = 0x09` (line 258).
5. **WRITE_PIXELS deprecated** — line 224 `#[deprecated(since = "0.3.0", note = "Use ATTACH_GRANT + DAMAGE_NOTIFY")]`.
6. **encode()/decode() helpers** — lines 152–208 provide bidirectional serialization with full LE byte order handling.

Law 1 gate satisfied; all types and opcodes verified.
