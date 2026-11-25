//! Pointer image request: a surface owner chooses what the pointer looks like
//! while it is over the surface's content.

use super::{compositor_ops, DisplayProtocolError};

/// Exact wire size of [`SetPointerImage`].
pub const SET_POINTER_IMAGE_WIRE_SIZE: usize = 20;
/// Largest edge of a pointer image, in pixels (a hardware cursor plane's size).
pub const MAX_POINTER_EDGE: u16 = 64;

const KIND_ARROW: u8 = 0;
const KIND_HIDDEN: u8 = 1;
const KIND_IMAGE: u8 = 2;

/// What the pointer shows over a surface's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerImage {
    /// The compositor's own arrow (also what a surface has until it asks).
    Arrow,
    /// No pointer.
    Hidden,
    /// `width`x`height` BGRA8888 pixels, rows packed (`width * 4` bytes
    /// each), non-premultiplied alpha, at the start of `grant`: a Grant the
    /// sender owns and shares with the compositor. `(hot_x, hot_y)` is the
    /// pixel that sits on the pointer position. The compositor copies the
    /// pixels when it receives the request; later writes to the Grant do not
    /// show.
    Image {
        grant: u32,
        width: u16,
        height: u16,
        hot_x: u16,
        hot_y: u16,
    },
}

/// Fixed 20-byte owner request that sets the pointer image of a surface.
///
/// LE layout: `opcode:u8, kind:u8, reserved:[u8;2], cap:u32, grant:u32,
/// width:u16, height:u16, hot_x:u16, hot_y:u16`. `kind` is 0 (arrow), 1
/// (hidden) or 2 (image); for 0 and 1 every later field but `cap` is zero.
/// An image is 1..=[`MAX_POINTER_EDGE`] pixels on each edge with its hot spot
/// inside it. The compositor refuses a `cap` the sender does not own and a
/// Grant the sender does not own or that is too short. No reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetPointerImage {
    pub cap: u32,
    pub image: PointerImage,
}

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

impl SetPointerImage {
    pub fn encode(&self) -> [u8; SET_POINTER_IMAGE_WIRE_SIZE] {
        let mut bytes = [0; SET_POINTER_IMAGE_WIRE_SIZE];
        bytes[0] = compositor_ops::SET_POINTER_IMAGE;
        bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
        bytes[1] = match self.image {
            PointerImage::Arrow => KIND_ARROW,
            PointerImage::Hidden => KIND_HIDDEN,
            PointerImage::Image {
                grant,
                width,
                height,
                hot_x,
                hot_y,
            } => {
                bytes[8..12].copy_from_slice(&grant.to_le_bytes());
                bytes[12..14].copy_from_slice(&width.to_le_bytes());
                bytes[14..16].copy_from_slice(&height.to_le_bytes());
                bytes[16..18].copy_from_slice(&hot_x.to_le_bytes());
                bytes[18..20].copy_from_slice(&hot_y.to_le_bytes());
                KIND_IMAGE
            }
        };
        bytes
    }

    /// Decode the first 20 bytes of `bytes`.
    ///
    /// # Errors
    /// Short buffer, wrong opcode, an unknown kind, nonzero reserved or unused
    /// bytes, or an image size or hot spot out of range.
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        let Some(bytes) = bytes.get(..SET_POINTER_IMAGE_WIRE_SIZE) else {
            return Err(DisplayProtocolError::InvalidLength);
        };
        if bytes[0] != compositor_ops::SET_POINTER_IMAGE {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        if bytes[2..4] != [0, 0] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        let cap = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let unused_zero = bytes[8..].iter().all(|byte| *byte == 0);
        let image = match bytes[1] {
            KIND_ARROW if unused_zero => PointerImage::Arrow,
            KIND_HIDDEN if unused_zero => PointerImage::Hidden,
            KIND_ARROW | KIND_HIDDEN => return Err(DisplayProtocolError::NonZeroReserved),
            KIND_IMAGE => {
                let (width, height) = (le16(bytes, 12), le16(bytes, 14));
                let (hot_x, hot_y) = (le16(bytes, 16), le16(bytes, 18));
                let edge = 1..=MAX_POINTER_EDGE;
                if !edge.contains(&width)
                    || !edge.contains(&height)
                    || hot_x >= width
                    || hot_y >= height
                {
                    return Err(DisplayProtocolError::OutOfRange);
                }
                PointerImage::Image {
                    grant: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
                    width,
                    height,
                    hot_x,
                    hot_y,
                }
            }
            _ => return Err(DisplayProtocolError::InvalidDiscriminant),
        };
        Ok(Self { cap, image })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u16, height: u16, hot_x: u16, hot_y: u16) -> SetPointerImage {
        SetPointerImage {
            cap: 7,
            image: PointerImage::Image {
                grant: 0x0102_0304,
                width,
                height,
                hot_x,
                hot_y,
            },
        }
    }

    #[test]
    fn every_kind_round_trips() {
        for request in [
            SetPointerImage {
                cap: 7,
                image: PointerImage::Arrow,
            },
            SetPointerImage {
                cap: 7,
                image: PointerImage::Hidden,
            },
            image(64, 64, 63, 63),
            image(1, 1, 0, 0),
        ] {
            assert_eq!(SetPointerImage::decode(&request.encode()), Ok(request));
        }
        assert_eq!(
            image(24, 16, 4, 8).encode(),
            [0x14, 2, 0, 0, 7, 0, 0, 0, 4, 3, 2, 1, 24, 0, 16, 0, 4, 0, 8, 0]
        );
    }

    #[test]
    fn sizes_and_hot_spots_outside_the_image_are_refused() {
        for bad in [
            image(0, 8, 0, 0),
            image(65, 8, 0, 0),
            image(8, 8, 8, 0),
            image(8, 8, 0, 8),
        ] {
            assert_eq!(
                SetPointerImage::decode(&bad.encode()),
                Err(DisplayProtocolError::OutOfRange)
            );
        }
    }

    #[test]
    fn malformed_frames_are_refused() {
        let frame = image(8, 8, 1, 1).encode();
        assert_eq!(
            SetPointerImage::decode(&frame[..19]),
            Err(DisplayProtocolError::InvalidLength)
        );
        let mut bad = frame;
        bad[0] = 0x13;
        assert_eq!(
            SetPointerImage::decode(&bad),
            Err(DisplayProtocolError::InvalidOpcode)
        );
        bad = frame;
        bad[3] = 1;
        assert_eq!(
            SetPointerImage::decode(&bad),
            Err(DisplayProtocolError::NonZeroReserved)
        );
        bad = frame;
        bad[1] = 3;
        assert_eq!(
            SetPointerImage::decode(&bad),
            Err(DisplayProtocolError::InvalidDiscriminant)
        );
        // An arrow or hidden request carries nothing but the surface.
        bad = frame;
        bad[1] = 0;
        assert_eq!(
            SetPointerImage::decode(&bad),
            Err(DisplayProtocolError::NonZeroReserved)
        );
    }
}
