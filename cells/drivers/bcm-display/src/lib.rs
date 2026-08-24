#![cfg_attr(not(test), no_std)]

#[cfg(feature = "runtime")]
pub mod mailbox;

#[cfg(feature = "runtime")]
use mailbox::BcmMailbox;
use types::{ViError, ViResult};

// Mailbox property buffers are plain u32 arrays, kept here so request/response
// parsing stays testable without the ostd-backed transport in `mailbox`.
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
    // Property-request layout (30 words). Response words land in each tag's
    // value slots, so the read indices in `parse_response` are coupled to this
    // array: Allocate buffer -> data[23] bus address, data[24] size;
    // Get pitch -> data[28].
    fn build_request(width: u32, height: u32) -> PropertyBuffer<30> {
        PropertyBuffer {
            data: [
                30 * 4,     // Total buffer size (30 u32s = 120 bytes)
                0x00000000, // Request code
                // Tag 1: Set physical width/height (0x00048003)
                0x00048003,
                8,
                0,
                width,
                height,
                // Tag 2: Set virtual width/height (0x00048004)
                0x00048004,
                8,
                0,
                width,
                height,
                // Tag 3: Set depth (0x00048005)
                0x00048005,
                4,
                0,
                32,
                // Tag 4: Set pixel order (0x00048006) - 0 = BGR (BGRA in 32bpp)
                0x00048006,
                4,
                0,
                0,
                // Tag 5: Allocate buffer (0x00040001)
                0x00040001,
                8,
                0,
                4096, // align
                0,
                // Tag 6: Get pitch (0x00040008)
                0x00040008,
                4,
                0,
                0,
                // End tag
                0x00000000,
            ],
        }
    }

    fn parse_response(req: &PropertyBuffer<30>) -> ViResult<(u32, usize, u32)> {
        let fb_bus_addr = req.data[23];
        let fb_size = req.data[24] as usize;
        let pitch = req.data[28];
        if fb_bus_addr == 0 || fb_size == 0 || pitch == 0 {
            return Err(ViError::NotFound);
        }
        Ok((fb_bus_addr, fb_size, pitch))
    }

    #[cfg(feature = "runtime")]
    pub fn allocate(mailbox: &BcmMailbox, width: u32, height: u32) -> ViResult<Self> {
        let mut req = Self::build_request(width, height);

        mailbox.call(&mut req)?;

        let (fb_bus_addr, fb_size, pitch) = Self::parse_response(&req)?;

        // Convert VideoCore bus address to ARM physical address (strip high 2 bits / VC alias)
        let fb_phys_addr = (fb_bus_addr & 0x3FFF_FFFF) as usize;

        Ok(Self {
            width,
            height,
            pitch,
            fb_ptr: fb_phys_addr,
            fb_size,
        })
    }

    /// Copy a pixel rect from the compositor buffer into the VideoCore
    /// framebuffer.
    ///
    /// `src.len()` is the sender's byte count; if it cannot cover `w*h*4`
    /// (including overflow of `w*h*4` itself), the call is silently ignored to
    /// prevent out-of-bounds reads. The rect is clamped to the framebuffer.
    pub fn flush_rect(&self, src: &[u8], x: u32, y: u32, w: u32, h: u32) {
        if self.fb_ptr == 0 || w == 0 || h == 0 || src.is_empty() {
            return;
        }
        // Reject before any arithmetic can overflow or any read happens.
        let Some(expected) = (w as usize)
            .checked_mul(h as usize)
            .and_then(|px| px.checked_mul(4))
        else {
            return;
        };
        if src.len() < expected {
            return;
        }
        let x = x.min(self.width);
        let y = y.min(self.height);
        let w = w.min(self.width.saturating_sub(x)) as usize;
        let h = h.min(self.height.saturating_sub(y)) as usize;
        if w == 0 || h == 0 {
            return;
        }
        let bytes_per_row = w * 4;
        for row in 0..h {
            let src_offset = row * bytes_per_row;
            let dst_offset = (y as usize + row) * (self.pitch as usize) + (x as usize) * 4;
            if dst_offset + bytes_per_row <= self.fb_size && src_offset + bytes_per_row <= src.len()
            {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        src[src_offset..].as_ptr(),
                        (self.fb_ptr + dst_offset) as *mut u8,
                        bytes_per_row,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    // Guards the mailbox tag layout: any tag insertion/removal shifts the
    // response indices in `parse_response` and must update them together.
    #[test]
    fn response_offsets_match_tag_layout() {
        let req = BcmFramebuffer::build_request(640, 480);
        assert_eq!(req.data[0] as usize, std::mem::size_of_val(&req.data));
        assert_eq!(req.data[20], 0x0004_0001, "allocate-buffer tag moved");
        assert_eq!(req.data[25], 0x0004_0008, "get-pitch tag moved");
        assert_eq!(req.data[29], 0x0000_0000, "end tag moved");
    }

    #[test]
    fn parses_videocore_response_words() {
        let mut req = BcmFramebuffer::build_request(640, 480);
        req.data[23] = 0x5F00_1000; // VC bus address of framebuffer
        req.data[24] = 640 * 480 * 4;
        req.data[28] = 640 * 4;
        let (addr, size, pitch) =
            BcmFramebuffer::parse_response(&req).expect("valid response must parse");
        assert_eq!(addr, 0x5F00_1000); // raw VC bus address; alias stripped in `allocate`
        assert_eq!(size, 640 * 480 * 4);
        assert_eq!(pitch, 640 * 4);
    }

    #[test]
    fn rejects_zeroed_response() {
        let req = BcmFramebuffer::build_request(640, 480);
        assert!(matches!(
            BcmFramebuffer::parse_response(&req),
            Err(ViError::NotFound)
        ));
    }
}
