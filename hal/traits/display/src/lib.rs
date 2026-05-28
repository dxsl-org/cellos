#![no_std]

pub trait Framebuffer {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn set_pixel(&mut self, x: u32, y: u32, color: u32);

    fn clear(&mut self, color: u32) {
        for y in 0..self.height() {
            for x in 0..self.width() {
                self.set_pixel(x, y, color);
            }
        }
    }
}

/// Common Display Info Struct
#[derive(Debug, Clone, Copy)]
pub struct DisplayInfo {
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
}
