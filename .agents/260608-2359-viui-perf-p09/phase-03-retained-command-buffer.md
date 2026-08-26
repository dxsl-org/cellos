# Phase 03 — GpuCommandBuffer Retained Reuse

**Plan**: [plan.md](plan.md)
**Status**: Planned
**Priority**: P3 — eliminates 1 Vec alloc per render call

---

## Problem

`gpu_renderer.rs:40`:
```rust
let mut buf = GpuCommandBuffer::new();
```
Called on every `render()` invocation. Allocates a fresh `Vec<GpuCmd>` even
when the previous frame's buffer would have identical capacity. After the first
frame, the allocator may or may not reuse the same memory block — not guaranteed.

On embedded targets without a slab allocator, repeated small Vec allocs fragment
the heap over time.

---

## Solution

Move `buf: GpuCommandBuffer` into `GpuRenderer<E>` as a retained field. Each
`render()` call does `self.buf.clear()` (keeps Vec capacity, O(n cmds) to zero
the length pointer) instead of `GpuCommandBuffer::new()` (O(1) alloc + later
O(n) free).

After the warm-up frame, zero heap allocations for the command buffer.

---

## Requirements

- `GpuCommandBuffer::clear(&mut self)` added — clears commands, retains Vec capacity
- `GpuRenderer::new()` now creates an initial empty buffer stored in `self.buf`
- `render()` calls `self.buf.clear()` then records new commands into it
- Borrow checker: mutable borrow of `self.buf` (via GpuCanvas) releases before
  `self.executor.execute(&self.buf, damage)` (immutable borrow) — NLL field split
- `GpuRenderer::into_executor()` unchanged in behavior
- `cargo check -p viui` + `cargo check -p viui-demo` pass

---

## Architecture

### `libs/viui/src/gpu_cmd.rs` — add `clear()`

```rust
impl GpuCommandBuffer {
    pub fn new()   -> Self { Self { cmds: Vec::new() } }
    pub fn push(&mut self, cmd: GpuCmd) { self.cmds.push(cmd); }
    pub fn as_slice(&self) -> &[GpuCmd] { &self.cmds }
    pub fn len(&self) -> usize { self.cmds.len() }
    pub fn is_empty(&self) -> bool { self.cmds.is_empty() }

    /// Clear all commands while retaining Vec capacity for the next frame.
    pub fn clear(&mut self) { self.cmds.clear(); }
}
```

### `libs/viui/src/gpu_renderer.rs` — retained `buf` field

```rust
pub struct GpuRenderer<E: CommandExecutor> {
    executor: E,
    width:    u32,
    height:   u32,
    buf:      GpuCommandBuffer,  // retained across frames
}

impl<E: CommandExecutor> GpuRenderer<E> {
    pub fn new(executor: E, width: u32, height: u32) -> Self {
        Self { executor, width, height, buf: GpuCommandBuffer::new() }
    }

    pub fn into_executor(self) -> E { self.executor }
}

impl<E: CommandExecutor> ViRenderer for GpuRenderer<E> {
    fn render(&mut self, damage: Option<Rect>, draw: &mut dyn FnMut(&mut dyn ViCanvas)) {
        self.buf.clear();  // reuse Vec capacity — zero alloc after first frame
        {
            let mut canvas = GpuCanvas::new(&mut self.buf, self.width, self.height);
            draw(&mut canvas);
        }
        // NLL field split: `self.buf` borrow (immutable) + `self.executor` borrow (mutable)
        self.executor.execute(&self.buf, damage);
    }

    fn size(&self) -> (u32, u32) { (self.width, self.height) }
}
```

**Borrow analysis**: After the inner `{}` block, the `GpuCanvas<'buf>` that holds
`&'buf mut self.buf` is dropped. Then `self.executor.execute(&self.buf, damage)`
takes `&self.buf` (immutable) and `&mut self.executor` — two different fields;
NLL field borrow splitting allows this. No explicit borrow splitting needed.

---

## Related Code Files

**Modify:**
- `libs/viui/src/gpu_cmd.rs` — add `clear()` method
- `libs/viui/src/gpu_renderer.rs` — add `buf` field, update `new()`, update `render()`

---

## Implementation Steps

1. `gpu_cmd.rs`: add `pub fn clear(&mut self) { self.cmds.clear(); }`
2. `gpu_renderer.rs`: add `buf: GpuCommandBuffer` field to struct
3. `gpu_renderer.rs`: init `buf: GpuCommandBuffer::new()` in `new()`
4. `gpu_renderer.rs`: replace `let mut buf = GpuCommandBuffer::new();` with `self.buf.clear();`
5. `gpu_renderer.rs`: replace `&buf` with `&self.buf` in `execute()` call
6. `cargo check -p viui` — verify NLL field split works
7. `cargo check -p viui-demo`

---

## Todo List

- [ ] gpu_cmd.rs: add clear()
- [ ] gpu_renderer.rs: add buf field + update new()
- [ ] gpu_renderer.rs: update render() to use self.buf.clear()
- [ ] cargo check -p viui passes
- [ ] cargo check -p viui-demo passes

---

## Success Criteria

- `render()` body no longer contains `GpuCommandBuffer::new()` (verifiable)
- `cargo check -p viui` passes (NLL field split)
- Warm-up frame (first render): one Vec alloc (inside `GpuCommandBuffer::new()` in `new()`)
- Subsequent frames: zero Vec allocs for command buffer

---

## Risk

- **Borrow checker rejection**: If Rust doesn't accept the NLL field split, fix by
  using `let buf = &self.buf; self.executor.execute(buf, damage);`. The borrow of
  `self.buf` by `GpuCanvas` is released at `}` so the subsequent `&self.buf`
  is an immutable borrow of a now-free field. Should compile cleanly.
- **`_assert_gpu_renderer_api()` in viui-demo**: Uses `core::mem::size_of::<GpuRenderer<CpuExecutor>>()`.
  The struct grew by `size_of::<GpuCommandBuffer>()` = `size_of::<Vec<GpuCmd>>()` = 24 bytes.
  The assert only checks the type compiles, not the size — no change needed.
