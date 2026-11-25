//! What the pointer looks like: the compositor's arrow, nothing, or an image
//! the surface under it gave (`SET_POINTER_IMAGE`).
//!
//! A surface's look applies where the surface takes pointer events (its
//! content, or while it holds a pointer capture); window decorations, menus
//! and drags show the arrow.

extern crate alloc;

use alloc::rc::Rc;
use alloc::vec::Vec;
use wire::display::Rect;

use crate::cursor_sprite::{cursor_pixel, hotspot, CURSOR_H, CURSOR_W};

/// Edge of the hardware cursor plane (virtio-gpu `CURSOR_RECT`).
pub const HW_EDGE: usize = 64;

/// A surface's pointer image, copied out of the owner's Grant.
pub struct PointerSprite {
    width: u32,
    height: u32,
    hot_x: u32,
    hot_y: u32,
    /// BGRA8888, rows packed, non-premultiplied alpha.
    pixels: Vec<u8>,
}

impl PointerSprite {
    /// `pixels` holds `width * height` BGRA pixels; `None` otherwise.
    pub fn new(width: u32, height: u32, hot_x: u32, hot_y: u32, pixels: Vec<u8>) -> Option<Self> {
        (pixels.len() == width as usize * height as usize * 4 && hot_x < width && hot_y < height)
            .then_some(Self {
                width,
                height,
                hot_x,
                hot_y,
                pixels,
            })
    }
}

#[derive(Clone, Default)]
pub enum PointerLook {
    #[default]
    Arrow,
    Hidden,
    Image(Rc<PointerSprite>),
}

impl PartialEq for PointerLook {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Arrow, Self::Arrow) | (Self::Hidden, Self::Hidden) => true,
            (Self::Image(a), Self::Image(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl PointerLook {
    /// `(width, height, hot_x, hot_y)`; zero-sized when hidden.
    fn geometry(&self) -> (u32, u32, u32, u32) {
        match self {
            Self::Arrow => {
                let (hot_x, hot_y) = hotspot();
                (CURSOR_W, CURSOR_H, hot_x as u32, hot_y as u32)
            }
            Self::Hidden => (0, 0, 0, 0),
            Self::Image(sprite) => (sprite.width, sprite.height, sprite.hot_x, sprite.hot_y),
        }
    }

    /// The screen area the look covers with its hot spot at `(x, y)`.
    pub fn rect(&self, x: i32, y: i32) -> Rect {
        let (w, h, hot_x, hot_y) = self.geometry();
        Rect {
            x: x - hot_x as i32,
            y: y - hot_y as i32,
            w,
            h,
        }
    }

    /// The pixel at `(row, col)` of the look's own area, `None` where it is
    /// transparent or outside.
    pub fn pixel(&self, row: u32, col: u32) -> Option<[u8; 4]> {
        match self {
            Self::Arrow => cursor_pixel(row, col),
            Self::Hidden => None,
            Self::Image(sprite) => {
                if row >= sprite.height || col >= sprite.width {
                    return None;
                }
                let at = (row * sprite.width + col) as usize * 4;
                let pixel = [
                    sprite.pixels[at],
                    sprite.pixels[at + 1],
                    sprite.pixels[at + 2],
                    sprite.pixels[at + 3],
                ];
                (pixel[3] != 0).then_some(pixel)
            }
        }
    }

    /// The look as a 64x64 BGRA hardware cursor sprite (top-left aligned,
    /// transparent elsewhere) and its hot spot.
    pub fn hw_sprite(&self) -> ([u8; HW_EDGE * HW_EDGE * 4], (u32, u32)) {
        let (w, h, hot_x, hot_y) = self.geometry();
        let mut sprite = [0u8; HW_EDGE * HW_EDGE * 4];
        for row in 0..h.min(HW_EDGE as u32) {
            for col in 0..w.min(HW_EDGE as u32) {
                if let Some(pixel) = self.pixel(row, col) {
                    let at = (row as usize * HW_EDGE + col as usize) * 4;
                    sprite[at..at + 4].copy_from_slice(&pixel);
                }
            }
        }
        (sprite, (hot_x, hot_y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ibeam() -> PointerLook {
        // 2x3, opaque except the middle row, hot spot at its centre.
        let mut pixels = alloc::vec![9u8; 2 * 3 * 4];
        pixels[2 * 4 + 3] = 0;
        pixels[3 * 4 + 3] = 0;
        PointerLook::Image(Rc::new(PointerSprite::new(2, 3, 1, 1, pixels).unwrap()))
    }

    #[test]
    fn an_image_sits_on_its_hot_spot_and_hidden_covers_nothing() {
        assert_eq!(
            ibeam().rect(100, 50),
            Rect {
                x: 99,
                y: 49,
                w: 2,
                h: 3
            }
        );
        assert_eq!(
            PointerLook::Arrow.rect(10, 10),
            Rect {
                x: 10,
                y: 10,
                w: 16,
                h: 16
            }
        );
        assert_eq!(PointerLook::Hidden.rect(10, 10).w, 0);
        assert_eq!(PointerLook::Hidden.pixel(0, 0), None);
    }

    #[test]
    fn transparent_and_outside_pixels_are_none() {
        let look = ibeam();
        assert_eq!(look.pixel(0, 1), Some([9, 9, 9, 9]));
        assert_eq!(look.pixel(1, 0), None, "alpha 0");
        assert_eq!(look.pixel(3, 0), None, "below the image");
        assert_eq!(look.pixel(0, 2), None, "right of the image");
    }

    #[test]
    fn the_hardware_sprite_is_the_image_in_the_top_left_corner() {
        let (sprite, hot) = ibeam().hw_sprite();
        assert_eq!(hot, (1, 1));
        assert_eq!(sprite[..4], [9, 9, 9, 9]);
        assert_eq!(
            sprite[HW_EDGE * 4..HW_EDGE * 4 + 4],
            [0; 4],
            "row 1 col 0 is transparent"
        );
        assert_eq!(
            sprite[2 * 4..2 * 4 + 4],
            [0; 4],
            "col 2 lies outside the image"
        );
        let (hidden, _) = PointerLook::Hidden.hw_sprite();
        assert!(hidden.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn a_sprite_must_hold_its_pixels_and_hot_spot() {
        assert!(PointerSprite::new(2, 2, 0, 0, alloc::vec![0; 15]).is_none());
        assert!(PointerSprite::new(2, 2, 2, 0, alloc::vec![0; 16]).is_none());
        assert!(PointerSprite::new(2, 2, 1, 1, alloc::vec![0; 16]).is_some());
    }
}
