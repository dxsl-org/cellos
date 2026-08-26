use super::alloc::vec;
use api::display::SurfaceRole;

use super::*;

fn surface(x: i32, y: i32, w: u32, h: u32, pixels: &[u8]) -> SurfaceState {
    let mut surface = SurfaceState::new(x, y, w, h, 0, SurfaceRole::Background);
    surface.write_pixels(0, 0, w, h, pixels);
    surface
}

fn pixel_at(fb: &ScreenFb, x: usize, y: usize) -> [u8; 4] {
    let offset = (y * fb.width as usize + x) * 4;
    [
        fb.pixels[offset],
        fb.pixels[offset + 1],
        fb.pixels[offset + 2],
        fb.pixels[offset + 3],
    ]
}

#[test]
fn clipped_blit_copies_the_matching_source_pixels() {
    let mut fb = ScreenFb::new(5, 4);
    fb.pixels.fill(0x5a);
    let surface = surface(
        1,
        1,
        3,
        2,
        &[
            10, 0, 0, 255, 20, 0, 0, 255, 30, 0, 0, 255, 40, 0, 0, 255, 50, 0, 0, 255, 60, 0, 0,
            255,
        ],
    );

    fb.blit_surface_clipped(
        &surface,
        Rect {
            x: 2,
            y: 1,
            w: 1,
            h: 2,
        },
    );

    assert_eq!(pixel_at(&fb, 2, 1), [20, 0, 0, 255]);
    assert_eq!(pixel_at(&fb, 2, 2), [50, 0, 0, 255]);
}

#[test]
fn clipped_blit_preserves_pixels_outside_damage() {
    let mut fb = ScreenFb::new(4, 3);
    fb.pixels.fill(0x5a);
    let surface = surface(0, 0, 4, 3, &vec![0xab; 4 * 3 * 4]);

    fb.blit_surface_clipped(
        &surface,
        Rect {
            x: 1,
            y: 1,
            w: 2,
            h: 1,
        },
    );

    for y in 0..3 {
        for x in 0..4 {
            let expected = if y == 1 && (x == 1 || x == 2) {
                [0xab; 4]
            } else {
                [0x5a; 4]
            };
            assert_eq!(pixel_at(&fb, x, y), expected);
        }
    }
}

#[test]
fn clipped_blit_translates_negative_surface_coordinates_at_screen_edge() {
    let mut fb = ScreenFb::new(3, 3);
    fb.pixels.fill(0x5a);
    let surface = surface(
        -1,
        -1,
        3,
        3,
        &[
            0, 0, 0, 255, 1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255, 5, 0, 0, 255, 6,
            0, 0, 255, 7, 0, 0, 255, 8, 0, 0, 255,
        ],
    );

    fb.blit_surface_clipped(
        &surface,
        Rect {
            x: -1,
            y: -1,
            w: 3,
            h: 3,
        },
    );

    assert_eq!(pixel_at(&fb, 0, 0), [4, 0, 0, 255]);
    assert_eq!(pixel_at(&fb, 1, 0), [5, 0, 0, 255]);
    assert_eq!(pixel_at(&fb, 0, 1), [7, 0, 0, 255]);
    assert_eq!(pixel_at(&fb, 1, 1), [8, 0, 0, 255]);
    assert_eq!(pixel_at(&fb, 2, 2), [0x5a; 4]);
}
