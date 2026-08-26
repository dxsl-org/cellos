# Phase 01 — Pre-compute bounding_rect at record time

**Status:** ✅ Done  
**Priority:** High  
**Effort:** Medium (struct refactor + executor update)

## Context Links

- Plan: [plan.md](plan.md)
- `libs/viui/src/gpu_cmd.rs` — `GpuCommandBuffer`, `GpuCmd::bounding_rect()`
- `libs/viui/src/executor.rs` — `CpuExecutor::execute()` damage filter loop

## Overview

`CpuExecutor::execute()` currently calls `cmd.bounding_rect()` on every command
every frame to check damage-rect overlap. `bounding_rect()` recalculates a
derived rect from the command's fields each time. Since commands don't change
between record and replay, this is pure wasted work per frame.

Fix: wrap each `GpuCmd` in a `RecordedCmd { cmd, bounds: Option<Rect> }` inside
`GpuCommandBuffer::push()`, computing bounds once at record time. The executor
reads `recorded.bounds` instead of calling `bounding_rect()`.

## Requirements

- `RecordedCmd { cmd: GpuCmd, bounds: Option<Rect> }` public struct in `gpu_cmd.rs`
- `GpuCommandBuffer` internal vec changes from `Vec<GpuCmd>` to `Vec<RecordedCmd>`
- `push(cmd: GpuCmd)` computes `cmd.bounding_rect()` before move, stores both
- `recorded_slice() -> &[RecordedCmd]` replaces `as_slice() -> &[GpuCmd]`
- `as_slice()` removed or kept as iterator adapter — remove to avoid confusion
- `CpuExecutor::execute()` updated to iterate `&RecordedCmd` and read `.bounds`
- `len()`, `is_empty()`, `clear()` still delegate to inner vec — no behavioural change
- No change to `GpuCmd` enum itself — `bounding_rect()` may stay for tests/docs

## Architecture

```
GpuCommandBuffer::push(cmd)
  → bounds = cmd.bounding_rect()       ← compute once at record time
  → self.cmds.push(RecordedCmd { cmd, bounds })

CpuExecutor::execute(&buf, damage)
  for rec in buf.recorded_slice():
    if let Some(b) = rec.bounds:        ← zero-compute read
        if !b.intersects(damage): continue
    match rec.cmd: ...                   ← dispatch as before
```

## Related Code Files

**Modify:**
- `libs/viui/src/gpu_cmd.rs`
  - Add `pub struct RecordedCmd { pub cmd: GpuCmd, pub bounds: Option<Rect> }`
  - Change `GpuCommandBuffer.cmds` from `Vec<GpuCmd>` to `Vec<RecordedCmd>`
  - `push()`: `let bounds = cmd.bounding_rect(); self.cmds.push(RecordedCmd { cmd, bounds })`
  - Replace `as_slice()` with `recorded_slice() -> &[RecordedCmd]`
  - Keep `len()`, `is_empty()`, `clear()` delegating to `self.cmds`
- `libs/viui/src/executor.rs`
  - `use crate::gpu_cmd::RecordedCmd;`
  - `execute()` signature stays the same — argument type `&GpuCommandBuffer`
  - Inner loop: `for rec in buf.recorded_slice()` → `match &rec.cmd` → use `rec.bounds`

## Implementation Steps

1. In `gpu_cmd.rs`, add `RecordedCmd` struct above `GpuCommandBuffer`
2. Change `GpuCommandBuffer.cmds: Vec<GpuCmd>` → `Vec<RecordedCmd>`
3. Update `push()`: extract bounds before move, wrap in `RecordedCmd`
4. Rename `as_slice()` → `recorded_slice()` returning `&[RecordedCmd]`
5. Update `len()`, `is_empty()`, `clear()` — they still call `self.cmds.*`
6. In `executor.rs`, import `RecordedCmd`, switch loop to `buf.recorded_slice()`
7. Replace `cmd.bounding_rect()` with `rec.bounds`
8. Replace `match cmd` → `match &rec.cmd`
9. `cargo check -p viui` — fix any remaining call sites
10. `cargo check -p viui-demo` — fix any use of `as_slice()` if present

## Todo List

- [x] Add `RecordedCmd` struct to `gpu_cmd.rs`
- [x] Change `GpuCommandBuffer.cmds` field type
- [x] Update `push()` to pre-compute and store bounds
- [x] Replace `as_slice()` with `recorded_slice()`
- [x] Update `executor.rs` loop to read `rec.bounds`
- [x] `cargo check -p viui` clean
- [x] `cargo check -p viui-demo` clean

## Success Criteria

- `cargo check -p viui` passes with zero new errors/warnings
- `cargo check -p viui-demo` passes
- `GpuCommandBuffer::push()` is the only call site of `bounding_rect()` at runtime
- Executor loop contains no `bounding_rect()` call

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `as_slice()` used outside viui | `cargo check -p viui-demo` + Grep for `as_slice` before rename |
| `DrawTextShort` bounds estimation is rough (width = len * 8px) | Same imprecision as before — damage filter is already conservative |

## Security Considerations

None — pure performance refactor, no data-flow changes.
