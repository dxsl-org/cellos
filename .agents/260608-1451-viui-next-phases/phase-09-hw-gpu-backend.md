# Phase 09 — HW GPU Backend

**Status:** Planned  
**Stage:** G2  
**Priority:** Medium  
**Estimate:** 3-4 ngày  
**Depends on:** Phase 07 (layout v2 stable before GPU layer)

---

## Context

Current: `GpuRenderer<E: CommandExecutor>` — Architecture done (P07). Default executor = `CpuExecutor` (CPU rasterization). 

`HwGpuExecutor` deferred từ P07 — implements `CommandExecutor` bằng real GPU API.

Target hardware G2: VisionFive2 / RK3588 2D accelerator. Fallback: OpenGL ES via EGL (Linux SBC).

---

## HwGpuExecutor design

```rust
// libs/viui/src/hw_gpu.rs (no_std incompatible — G2 only, std feature)

pub struct HwGpuExecutor {
    // Option A: 2D Accelerator (board-specific DMA engine)
    // Option B: OpenGL ES 2.0 (via EGL + GLES2 crate)
    // Option C: Vulkan (too heavy for G2 embedded)
}

impl CommandExecutor for HwGpuExecutor {
    fn execute(&mut self, cmds: &[RecordedCmd], target: &mut dyn FrameBuffer) {
        for cmd in cmds {
            match &cmd.cmd {
                GpuCmd::FillRect { rect, color } => {
                    // Use DMA fill or GLES glClear(scissor)
                }
                GpuCmd::DrawText { .. } => {
                    // Blit pre-rasterized glyph atlas texture
                }
                GpuCmd::DrawImage { .. } => {
                    // Blit texture
                }
            }
        }
    }
}
```

### Strategy selection

| Platform | Strategy |
|----------|-----------|
| QEMU virt | `CpuExecutor` (default, always works) |
| VisionFive2 | StarFive JH7110 2D Accelerator (DMA blit) |
| RK3588 | RK2D / OpenGL ES 3.0 |
| x86_64 QEMU | OpenGL ES via Mesa software |

### Feature flags

```toml
# libs/viui/Cargo.toml
[features]
default   = []
hw-gpu    = ["dep:gles2"]        # GLES2 executor
jh7110-2d = []                   # StarFive 2D DMA
```

App chọn backend via feature flag → zero-cost abstraction.

---

## Glyph atlas on GPU

Key optimization: upload `GlyphAtlas` to GPU texture once → text rendering = texture blit (vs CPU pixel-by-pixel):

```rust
// lib/viui/src/font_context.rs (extend)
pub struct GpuGlyphAtlas {
    texture_id: u32,   // GPU texture handle
    cpu_atlas:  GlyphAtlas,
    dirty:      bool,
}
```

Upload on first draw, re-upload khi new glyphs added.

---

## Success Criteria

- `HwGpuExecutor` on OpenGL ES renders viui-demo at ≥ 60fps
- Glyph atlas uploaded as GPU texture — text render O(blit) not O(pixel)
- `CpuExecutor` still works as fallback
- Feature flags compile cleanly: `cargo check --features hw-gpu`
