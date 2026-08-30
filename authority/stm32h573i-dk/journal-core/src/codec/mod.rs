mod decode;
mod encode;
pub(crate) mod io;

pub use decode::decode_record;
pub(crate) use decode::{decode_authenticated_record, record_authenticates};
pub use encode::encode_record;

use crate::RecordError;
use authority_protocol::WireError;

pub const RECORD_MAX: usize = 1888;
pub(crate) const MAGIC: &[u8; 4] = b"SAJR";
pub(crate) const VERSION: u8 = 2;
pub(crate) const TAG_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    Wire(WireError),
    Record(RecordError),
    ProtectedRecord,
    Authentication,
}

impl From<WireError> for CodecError {
    fn from(value: WireError) -> Self {
        Self::Wire(value)
    }
}

impl From<RecordError> for CodecError {
    fn from(value: RecordError) -> Self {
        Self::Record(value)
    }
}
