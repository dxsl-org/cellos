#![cfg_attr(not(test), no_std)]

#[cfg(feature = "runtime")]
pub mod mailbox;
#[cfg(feature = "runtime")]
use mailbox::BcmMailbox;
use types::{ViError, ViResult};

const RESPONSE_SUCCESS: u32 = 0x8000_0000;
const RESPONSE_BIT: u32 = 0x8000_0000;
const RESPONSE_LENGTH_MASK: u32 = 0x7fff_ffff;
const TAG_PHYSICAL_SIZE: u32 = 0x0004_8003;
const TAG_VIRTUAL_SIZE: u32 = 0x0004_8004;
const TAG_DEPTH: u32 = 0x0004_8005;
const TAG_PIXEL_ORDER: u32 = 0x0004_8006;
const TAG_ALPHA_MODE: u32 = 0x0004_8007;
const TAG_ALLOCATE_BUFFER: u32 = 0x0004_0001;
const TAG_GET_PITCH: u32 = 0x0004_0008;
const PIXEL_ORDER_BGR: u32 = 0;
const ALPHA_MODE_IGNORED: u32 = 2;

pub(crate) const PROPERTY_CHANNEL: u32 = 8;
pub(crate) const MAILBOX_READ_STATUS: usize = 0x18;
pub(crate) const MAILBOX_WRITE_STATUS: usize = 0x38;

pub(crate) fn matches_property_response(response: u32, bus_address: u32) -> bool {
    response & 0xF == PROPERTY_CHANNEL && response & !0xF == bus_address & !0xF
}

fn pixel_word(source: &[u8], offset: usize) -> Option<u32> {
    let pixel: [u8; 4] = source
        .get(offset..offset.checked_add(4)?)?
        .try_into()
        .ok()?;
    Some(u32::from_le_bytes(pixel))
}

/// Mailbox property buffers must be 16-byte aligned for VideoCore.
#[repr(C, align(16))]
pub struct PropertyBuffer<const N: usize> {
    pub data: [u32; N],
}

pub struct BcmFramebuffer {
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub fb_ptr: usize,
    pub fb_size: usize,
}

impl BcmFramebuffer {
    fn build_request(width: u32, height: u32) -> PropertyBuffer<34> {
        PropertyBuffer {
            data: [
                34 * 4,
                0,
                TAG_PHYSICAL_SIZE,
                8,
                0,
                width,
                height,
                TAG_VIRTUAL_SIZE,
                8,
                0,
                width,
                height,
                TAG_DEPTH,
                4,
                0,
                32,
                TAG_PIXEL_ORDER,
                4,
                0,
                PIXEL_ORDER_BGR,
                TAG_ALPHA_MODE,
                4,
                0,
                ALPHA_MODE_IGNORED,
                TAG_ALLOCATE_BUFFER,
                8,
                0,
                4096,
                0,
                TAG_GET_PITCH,
                4,
                0,
                0,
                0,
            ],
        }
    }

    fn parse_response(req: &PropertyBuffer<34>) -> ViResult<(u32, usize, u32, u32, u32)> {
        let expected = [
            (2, TAG_PHYSICAL_SIZE, 8),
            (7, TAG_VIRTUAL_SIZE, 8),
            (12, TAG_DEPTH, 4),
            (16, TAG_PIXEL_ORDER, 4),
            (20, TAG_ALPHA_MODE, 4),
            (24, TAG_ALLOCATE_BUFFER, 8),
            (29, TAG_GET_PITCH, 4),
        ];
        if req.data[0] != core::mem::size_of_val(&req.data) as u32
            || req.data[1] != RESPONSE_SUCCESS
            || req.data[33] != 0
        {
            return Err(ViError::InvalidInput);
        }
        for (index, tag, len) in expected {
            if req.data[index] != tag
                || req.data[index + 1] != len
                || req.data[index + 2] & RESPONSE_BIT == 0
                || req.data[index + 2] & RESPONSE_LENGTH_MASK != len
            {
                return Err(ViError::InvalidInput);
            }
        }
        let width = req.data[10];
        let height = req.data[11];
        let depth = req.data[15];
        let pixel_order = req.data[19];
        let alpha_mode = req.data[23];
        let fb_bus_addr = req.data[27];
        let fb_size = req.data[28] as usize;
        let pitch = req.data[32];
        if width == 0
            || height == 0
            || width > u16::MAX as u32
            || height > u16::MAX as u32
            || req.data[5] != width
            || req.data[6] != height
            || depth != 32
            || pixel_order != PIXEL_ORDER_BGR
            || alpha_mode != ALPHA_MODE_IGNORED
            || fb_bus_addr == 0
            || fb_size == 0
            || pitch == 0
        {
            return Err(ViError::NotFound);
        }
        Ok((fb_bus_addr, fb_size, pitch, width, height))
    }

    #[cfg(feature = "runtime")]
    pub fn allocate(mailbox: &mut BcmMailbox, width: u32, height: u32) -> ViResult<Self> {
        let mut req = Self::build_request(width, height);
        if let Err(error) = mailbox.call(&mut req) {
            ostd::io::println("[bcm-display] framebuffer diagnostic: property call failed");
            return Err(error);
        }
        let (fb_bus_addr, fb_size, pitch, width, height) = match Self::parse_response(&req) {
            Ok(response) => response,
            Err(error) => {
                ostd::io::println("[bcm-display] framebuffer diagnostic: parse failed");
                diagnostic_response(&req);
                return Err(error);
            }
        };
        let fb_phys_addr = (fb_bus_addr & 0x3fff_ffff) as usize;
        let packed_dimensions = ((width as usize) << 16) | height as usize;
        if !ostd::syscall::sys_register_display_framebuffer(
            fb_phys_addr,
            fb_size,
            packed_dimensions,
            pitch as usize,
        ) {
            ostd::io::println("[bcm-display] framebuffer diagnostic: kernel registration rejected");
            diagnostic_number("[bcm-display] framebuffer base ", fb_phys_addr);
            diagnostic_number("[bcm-display] framebuffer size ", fb_size);
            diagnostic_number("[bcm-display] framebuffer width ", width as usize);
            diagnostic_number("[bcm-display] framebuffer height ", height as usize);
            diagnostic_number("[bcm-display] framebuffer pitch ", pitch as usize);
            return Err(ViError::PermissionDenied);
        }
        Ok(Self {
            width,
            height,
            pitch,
            fb_ptr: fb_phys_addr,
            fb_size,
        })
    }

    /// Copy one compositor rectangle into the registered VideoCore framebuffer.
    pub fn flush_rect(&self, src: &[u8], x: u32, y: u32, w: u32, h: u32) {
        if self.fb_ptr == 0
            || self.fb_ptr & 3 != 0
            || self.pitch & 3 != 0
            || w == 0
            || h == 0
            || src.is_empty()
        {
            return;
        }
        let Some(expected) = (w as usize)
            .checked_mul(h as usize)
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            return;
        };
        if src.len() < expected {
            return;
        }
        let x = x.min(self.width);
        let y = y.min(self.height);
        let width = w.min(self.width.saturating_sub(x)) as usize;
        let height = h.min(self.height.saturating_sub(y)) as usize;
        let Some(row_bytes) = width.checked_mul(4) else {
            return;
        };
        let Some(x_offset) = (x as usize).checked_mul(4) else {
            return;
        };
        if width == 0 || height == 0 || x_offset > self.pitch as usize {
            return;
        }
        for row in 0..height {
            let Some(src_offset) = row.checked_mul(row_bytes) else {
                return;
            };
            let Some(dst_offset) = (y as usize)
                .checked_add(row)
                .and_then(|line| line.checked_mul(self.pitch as usize))
                .and_then(|line| line.checked_add(x_offset))
            else {
                return;
            };
            let Some(dst_end) = dst_offset.checked_add(row_bytes) else {
                return;
            };
            let Some(src_end) = src_offset.checked_add(row_bytes) else {
                return;
            };
            if dst_end > self.fb_size || src_end > src.len() {
                return;
            }
            for column in 0..width {
                let Some(column_offset) = column.checked_mul(4) else {
                    return;
                };
                let Some(pixel_offset) = src_offset.checked_add(column_offset) else {
                    return;
                };
                let Some(pixel) = pixel_word(src, pixel_offset) else {
                    return;
                };
                let Some(destination) = self
                    .fb_ptr
                    .checked_add(dst_offset)
                    .and_then(|row_start| row_start.checked_add(column_offset))
                else {
                    return;
                };
                // SAFETY: framebuffer registration enforces a 4-byte-aligned
                // base and pitch. The checked rectangle keeps destination in the
                // registered range; volatile writes are required for Device-nGnRnE.
                unsafe {
                    core::ptr::write_volatile(destination as *mut u32, pixel);
                }
            }
        }
        #[cfg(feature = "runtime")]
        {
            static FIRST_FLUSH_COMPLETE: core::sync::atomic::AtomicBool =
                core::sync::atomic::AtomicBool::new(false);
            if !FIRST_FLUSH_COMPLETE.swap(true, core::sync::atomic::Ordering::Relaxed) {
                ostd::io::println("[bcm-display] first scanout flush completed");
            }
        }
    }
}

#[cfg(feature = "runtime")]
fn diagnostic_response(response: &PropertyBuffer<34>) {
    for (label, index) in [
        ("[bcm-display] response bytes ", 0),
        ("[bcm-display] response status ", 1),
        ("[bcm-display] physical width ", 5),
        ("[bcm-display] physical height ", 6),
        ("[bcm-display] virtual width ", 10),
        ("[bcm-display] virtual height ", 11),
        ("[bcm-display] depth ", 15),
        ("[bcm-display] alpha mode ", 23),
        ("[bcm-display] framebuffer bus address ", 27),
        ("[bcm-display] framebuffer bytes ", 28),
        ("[bcm-display] pitch ", 32),
    ] {
        diagnostic_number(label, response.data[index] as usize);
    }
}

#[cfg(feature = "runtime")]
fn diagnostic_number(label: &str, value: usize) {
    ostd::io::print(label);
    ostd::io::print_usize(value);
    ostd::io::println("");
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    fn response() -> PropertyBuffer<34> {
        let mut request = BcmFramebuffer::build_request(640, 480);
        request.data[1] = RESPONSE_SUCCESS;
        for index in [4, 9, 14, 18, 22, 26, 31] {
            request.data[index] = RESPONSE_BIT | request.data[index - 1];
        }
        request.data[10] = 640;
        request.data[11] = 480;
        request.data[15] = 32;
        request.data[23] = ALPHA_MODE_IGNORED;
        request.data[27] = 0x5f00_1000;
        request.data[28] = 640 * 480 * 4;
        request.data[32] = 640 * 4;
        request
    }
    #[test]
    fn accepts_complete_structured_response() {
        assert!(BcmFramebuffer::parse_response(&response()).is_ok());
    }

    #[test]
    fn request_layout_matches_videocore_wire_contract() {
        let request = BcmFramebuffer::build_request(640, 480);
        assert_eq!(request.data[0], 136);
        assert_eq!(request.data[16], TAG_PIXEL_ORDER);
        assert_eq!(request.data[19], PIXEL_ORDER_BGR);
        assert_eq!(request.data[20], TAG_ALPHA_MODE);
        assert_eq!(request.data[23], ALPHA_MODE_IGNORED);
        assert_eq!(request.data[24], TAG_ALLOCATE_BUFFER);
        assert_eq!(request.data[33], 0);
    }
    #[test]
    fn rejects_missing_response_bit() {
        let mut response = response();
        response.data[26] = 0;
        assert_eq!(
            BcmFramebuffer::parse_response(&response),
            Err(ViError::InvalidInput)
        );
    }

    #[test]
    fn rejects_missing_alpha_mode_response_bit() {
        let mut response = response();
        response.data[22] = 4;
        assert_eq!(
            BcmFramebuffer::parse_response(&response),
            Err(ViError::InvalidInput)
        );
    }
    #[test]
    fn rejects_duplicate_or_reordered_tag() {
        let mut response = response();
        response.data[29] = TAG_ALLOCATE_BUFFER;
        assert_eq!(
            BcmFramebuffer::parse_response(&response),
            Err(ViError::InvalidInput)
        );
    }

    #[test]
    fn rejects_alpha_mode_that_keeps_compositor_pixels_transparent() {
        let mut response = response();
        response.data[23] = 0;
        assert_eq!(
            BcmFramebuffer::parse_response(&response),
            Err(ViError::NotFound)
        );
    }

    #[test]
    fn rejects_changed_pixel_order() {
        let mut response = response();
        response.data[19] = 1;
        assert_eq!(
            BcmFramebuffer::parse_response(&response),
            Err(ViError::NotFound)
        );
    }
    #[test]
    fn rejects_invalid_returned_dimensions() {
        let mut response = response();
        response.data[10] = 0;
        assert_eq!(
            BcmFramebuffer::parse_response(&response),
            Err(ViError::NotFound)
        );
    }

    #[test]
    fn accepts_only_the_submitted_property_response() {
        let bus_address = 0xc123_4000;
        assert!(matches_property_response(
            bus_address | PROPERTY_CHANNEL,
            bus_address
        ));
        assert!(!matches_property_response(bus_address | 7, bus_address));
        assert!(!matches_property_response(0xc123_5008, bus_address));
    }

    #[test]
    fn mailbox_status_offsets_keep_read_and_write_fifos_distinct() {
        assert_eq!(MAILBOX_READ_STATUS, 0x18);
        assert_eq!(MAILBOX_WRITE_STATUS, 0x38);
    }

    #[test]
    fn packs_unaligned_source_bytes_without_unaligned_load() {
        assert_eq!(pixel_word(&[0, 1, 2, 3, 4], 1), Some(0x0403_0201));
        assert_eq!(pixel_word(&[0, 1, 2], 0), None);
    }
}
