use crate::sync::Spinlock;
use crate::task::drivers::font;
use crate::task::drivers::virtio_gpu::{GpuContext, GPU_CONTEXT};
use log::info;

pub struct FramebufferConsole {
    width: u32,
    height: u32,
    cursor_x: u32,
    cursor_y: u32,
    fg_color: u32, // BGR
    bg_color: u32, // BGR
}

pub static FB_CONSOLE: Spinlock<Option<FramebufferConsole>> = Spinlock::new(None);

impl FramebufferConsole {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            cursor_x: 0,
            cursor_y: 0,
            fg_color: 0xFFFFFFFF, // White BGR
            bg_color: 0x00600000, // Navy Blue BGR
        }
    }

    pub fn init() {
        {
            let mut console_guard = FB_CONSOLE.lock();
            if let Some(gpu_ctx) = GPU_CONTEXT.lock().as_mut() {
                let console = FramebufferConsole::new(gpu_ctx.width, gpu_ctx.height);

                // Initial clear
                console.clear_screen(gpu_ctx);
                console.draw_cursor(gpu_ctx);

                *console_guard = Some(console);

                // Explicit flush
                let _ = gpu_ctx.gpu.flush();
            }
        }

        // Welcome Message
        info!("Graphics Console: Online (Resolution probed)");
        Self::write_str("\n ViOS Graphical Console initialized.\n");
        Self::write_str(" Welcome to ViOS v0.2.0 (RISC-V 64)\n\n");
        Self::write_str(" vios> ");
    }

    pub fn write_str(s: &str) {
        let mut console_guard = FB_CONSOLE.lock();
        if let Some(console) = console_guard.as_mut() {
            if let Some(gpu) = GPU_CONTEXT.lock().as_mut() {
                // Clear old cursor
                console.draw_rect(
                    gpu,
                    console.cursor_x,
                    console.cursor_y,
                    font::FONT_WIDTH as u32,
                    font::FONT_HEIGHT as u32,
                    console.bg_color,
                );

                for c in s.chars() {
                    console.write_char(gpu, c);
                }

                // Draw new cursor
                console.draw_cursor(gpu);
                let _ = gpu.gpu.flush();
            }
        }
    }

    pub fn draw_rect(&self, ctx: &mut GpuContext, x: u32, y: u32, w: u32, h: u32, color: u32) {
        let fb = ctx.framebuffer();
        for row in 0..h {
            let py = y + row;
            if py >= self.height {
                break;
            }
            for col in 0..w {
                let px = x + col;
                if px >= self.width {
                    break;
                }
                let idx = (py as usize * self.width as usize + px as usize) * 4;
                if idx + 4 <= fb.len() {
                    let c = color;
                    fb[idx] = (c >> 16) as u8;
                    fb[idx + 1] = (c >> 8) as u8;
                    fb[idx + 2] = c as u8;
                    fb[idx + 3] = 0x00;
                }
            }
        }
    }

    fn draw_cursor(&self, ctx: &mut GpuContext) {
        self.draw_rect(
            ctx,
            self.cursor_x,
            self.cursor_y,
            font::FONT_WIDTH as u32,
            font::FONT_HEIGHT as u32,
            self.fg_color,
        );
    }

    fn clear_screen(&self, ctx: &mut GpuContext) {
        let fb = ctx.framebuffer();
        for i in (0..fb.len()).step_by(4) {
            let c = self.bg_color;
            fb[i] = (c >> 16) as u8; // Blue
            fb[i + 1] = (c >> 8) as u8; // Green
            fb[i + 2] = c as u8; // Red
            fb[i + 3] = 0x00; // Alpha
        }
    }

    fn write_char(&mut self, ctx: &mut GpuContext, c: char) {
        if c == '\n' {
            self.newline(ctx);
            return;
        }

        if c == '\r' {
            self.cursor_x = 0;
            return;
        }

        if self.cursor_x + font::FONT_WIDTH as u32 >= self.width {
            self.newline(ctx);
        }

        self.draw_char(ctx, self.cursor_x, self.cursor_y, c, self.fg_color);
        self.cursor_x += font::FONT_WIDTH as u32;
    }

    fn newline(&mut self, ctx: &mut GpuContext) {
        self.cursor_x = 0;
        self.cursor_y += font::FONT_HEIGHT as u32;
        if self.cursor_y + font::FONT_HEIGHT as u32 >= self.height {
            self.scroll_up(ctx);
            self.cursor_y -= font::FONT_HEIGHT as u32;
        }
    }

    fn scroll_up(&mut self, ctx: &mut GpuContext) {
        let fb = ctx.framebuffer();
        let stride = self.width as usize * 4;
        let row_bytes = stride * font::FONT_HEIGHT;
        let total_bytes = fb.len();

        fb.copy_within(row_bytes..total_bytes, 0);

        // Clear last row
        let clear_start = total_bytes - row_bytes;
        for i in (clear_start..total_bytes).step_by(4) {
            let c = self.bg_color;
            fb[i] = (c >> 16) as u8;
            fb[i + 1] = (c >> 8) as u8;
            fb[i + 2] = c as u8;
            fb[i + 3] = 0x00;
        }
    }

    fn draw_char(&self, ctx: &mut GpuContext, x: u32, y: u32, c: char, color: u32) {
        let glyph = font::get_glyph(c);
        let fb = ctx.framebuffer();

        for row in 0..font::FONT_HEIGHT {
            let row_data = glyph.get(row).unwrap_or(&0); // Safety check
            let py = y as usize + row;
            if py >= self.height as usize {
                break;
            }

            for col in 0..font::FONT_WIDTH {
                if (row_data >> (7 - col)) & 1 != 0 {
                    let px = x as usize + col;
                    if px >= self.width as usize {
                        break;
                    }

                    let idx = (py * self.width as usize + px) * 4;
                    if idx + 4 <= fb.len() {
                        let c = color;
                        fb[idx] = (c >> 16) as u8;
                        fb[idx + 1] = (c >> 8) as u8;
                        fb[idx + 2] = c as u8;
                        fb[idx + 3] = 0x00;
                    }
                }
            }
        }
    }
}

pub struct ConsoleWrapper<'a> {
    pub console: &'a mut FramebufferConsole,
    pub gpu: &'a mut GpuContext,
}

impl<'a> core::fmt::Write for ConsoleWrapper<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            self.console.write_char(self.gpu, c);
        }
        let _ = self.gpu.gpu.flush();
        Ok(())
    }
}
