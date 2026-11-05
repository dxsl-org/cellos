extern crate std;

use api::display::{PixelFormat, SurfaceRole};

use super::SurfaceState;

const REG_ID: usize = 0x8000_0000;

fn grant_surface(w: u32, h: u32) -> SurfaceState {
    let mut surface = SurfaceState::new(0, 0, w, h, 7, SurfaceRole::Interactive);
    assert!(surface.attach_grant(REG_ID, w, h, PixelFormat::Bgra8888));
    surface
}

#[test]
fn live_grant_is_resolved_for_each_blit() {
    let mut backing = std::vec![0x11u8; 2 * 2 * 4];
    let ptr = backing.as_mut_ptr();
    let mut surface = grant_surface(2, 2);
    let mut asked = None;
    let pixels = surface.resolve_pixels(|reg_id| {
        asked = Some(reg_id);
        Some((ptr, 16))
    });
    assert_eq!(pixels, &[0x11u8; 16][..]);
    assert_eq!(asked, Some(REG_ID));
    assert_eq!(surface.grant_ids()[0], Some(REG_ID));
}

#[test]
fn released_grant_detaches_instead_of_painting_freed_frames() {
    let mut surface = grant_surface(2, 2);
    assert!(surface.resolve_pixels(|_| None).is_empty());
    assert_eq!(surface.grant_ids()[0], None);
    // Detached for good: a later blit never asks the kernel again.
    assert!(surface
        .resolve_pixels(|_| panic!("detached surface re-resolved its grant"))
        .is_empty());
}

#[test]
fn shrunk_grant_detaches() {
    let mut backing = std::vec![0u8; 8];
    let ptr = backing.as_mut_ptr();
    let mut surface = grant_surface(2, 2);
    assert!(surface.resolve_pixels(|_| Some((ptr, 8))).is_empty());
    assert_eq!(surface.grant_ids()[0], None);
}

#[test]
fn write_pixels_rejects_rectangles_outside_the_surface() {
    let data = [0xabu8; 487];
    let mut surface = SurfaceState::new(0, 0, 4, 4, 7, SurfaceRole::Interactive);
    // Negative origin used to wrap into a huge destination offset.
    surface.write_pixels(0, -1, 1, 1, &data);
    // A 0x10000-square rectangle used to wrap the expected length to zero.
    surface.write_pixels(0, 0, 0x1_0000, 0x1_0000, &data);
    // Partially outside the surface.
    surface.write_pixels(3, 3, 2, 2, &data);
    assert!(surface.damage.is_none());
    assert!(surface.resolve_pixels(|_| None).is_empty());

    surface.write_pixels(1, 1, 2, 2, &data);
    let pixels = surface.resolve_pixels(|_| None);
    assert_eq!(pixels.len(), 4 * 4 * 4);
    assert_eq!(&pixels[(4 + 1) * 4..(4 + 3) * 4], &[0xab; 8]);
    assert_eq!(&pixels[..4 * 4], &[0; 16]);
}

#[test]
fn write_pixels_never_allocates_past_the_owned_buffer_cap() {
    let data = [0xabu8; 16];
    let mut surface = SurfaceState::new(0, 0, 2048, 2048, 7, SurfaceRole::Interactive);
    surface.write_pixels(0, 0, 2, 2, &data);
    assert!(surface.damage.is_none());
    assert!(surface.resolve_pixels(|_| None).is_empty());
}
