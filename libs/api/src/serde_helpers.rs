// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Serialization helpers for StateTransfer trait.
//!
//! Provides utilities to reduce boilerplate when implementing StateTransfer.

use crate::*;

/// This trait simplifies StateTransfer implementation by providing
/// automatic serialization for common types.
pub trait ViSerializable: Sized {
    /// Get the serialized size in bytes.
    fn serialized_size(&self) -> usize;

    /// Serialize into a buffer.
    fn serialize_into(&self, buffer: &mut [u8]) -> ViResult<usize>;

    /// Deserialize from a buffer.
    fn deserialize_from(buffer: &[u8]) -> ViResult<Self>;
}

// Implement for primitive types
impl ViSerializable for u8 {
    fn serialized_size(&self) -> usize {
        1
    }
    fn serialize_into(&self, buffer: &mut [u8]) -> ViResult<usize> {
        buffer[0] = *self;
        Ok(1)
    }
    fn deserialize_from(buffer: &[u8]) -> ViResult<Self> {
        Ok(buffer[0])
    }
}

impl ViSerializable for u16 {
    fn serialized_size(&self) -> usize {
        2
    }
    fn serialize_into(&self, buffer: &mut [u8]) -> ViResult<usize> {
        buffer[..2].copy_from_slice(&self.to_le_bytes());
        Ok(2)
    }
    fn deserialize_from(buffer: &[u8]) -> ViResult<Self> {
        let mut bytes = [0u8; 2];
        bytes.copy_from_slice(&buffer[..2]);
        Ok(u16::from_le_bytes(bytes))
    }
}

impl ViSerializable for u32 {
    fn serialized_size(&self) -> usize {
        4
    }
    fn serialize_into(&self, buffer: &mut [u8]) -> ViResult<usize> {
        buffer[..4].copy_from_slice(&self.to_le_bytes());
        Ok(4)
    }
    fn deserialize_from(buffer: &[u8]) -> ViResult<Self> {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&buffer[..4]);
        Ok(u32::from_le_bytes(bytes))
    }
}

impl ViSerializable for u64 {
    fn serialized_size(&self) -> usize {
        8
    }
    fn serialize_into(&self, buffer: &mut [u8]) -> ViResult<usize> {
        buffer[..8].copy_from_slice(&self.to_le_bytes());
        Ok(8)
    }
    fn deserialize_from(buffer: &[u8]) -> ViResult<Self> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&buffer[..8]);
        Ok(u64::from_le_bytes(bytes))
    }
}

impl ViSerializable for usize {
    fn serialized_size(&self) -> usize {
        8
    }
    fn serialize_into(&self, buffer: &mut [u8]) -> ViResult<usize> {
        buffer[..8].copy_from_slice(&self.to_le_bytes());
        Ok(8)
    }
    fn deserialize_from(buffer: &[u8]) -> ViResult<Self> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&buffer[..8]);
        Ok(usize::from_le_bytes(bytes))
    }
}

// Implement for arrays
impl<const N: usize> ViSerializable for [u8; N] {
    fn serialized_size(&self) -> usize {
        N
    }
    fn serialize_into(&self, buffer: &mut [u8]) -> ViResult<usize> {
        buffer[..N].copy_from_slice(self);
        Ok(N)
    }
    fn deserialize_from(buffer: &[u8]) -> ViResult<Self> {
        let mut arr = [0u8; N];
        arr.copy_from_slice(&buffer[..N]);
        Ok(arr)
    }
}

/// Helper for serializing slices with length prefix.
pub fn serialize_slice(slice: &[u8], buffer: &mut [u8]) -> ViResult<usize> {
    let len = slice.len();
    if buffer.len() < 8 + len {
        return Err(ViError::InvalidArgument);
    }

    // Write length
    buffer[..8].copy_from_slice(&len.to_le_bytes());
    // Write data
    buffer[8..8 + len].copy_from_slice(slice);

    Ok(8 + len)
}

/// Helper for deserializing slices with length prefix.
pub fn deserialize_slice<'a>(buffer: &'a [u8]) -> ViResult<(&'a [u8], usize)> {
    if buffer.len() < 8 {
        return Err(ViError::InvalidArgument);
    }

    let mut len_bytes = [0u8; 8];
    len_bytes.copy_from_slice(&buffer[..8]);
    let len = usize::from_le_bytes(len_bytes);

    if buffer.len() < 8 + len {
        return Err(ViError::InvalidArgument);
    }

    Ok((&buffer[8..8 + len], 8 + len))
}

/// Macro to implement StateTransfer for simple structs.
///
/// # Example
/// ```ignore
/// struct MyDriver {
///     counter: u64,
///     name: [u8; 32],
/// }
///
/// impl_state_transfer!(MyDriver, counter, name);
/// ```
#[macro_export]
macro_rules! impl_state_transfer {
    ($type:ty, $($field:ident),+) => {
        impl ViStateTransfer for $type {
            fn state_size(&self) -> usize {
                0 $(+ self.$field.serialized_size())+
            }

            fn serialize_state(&self, buffer: &mut [u8]) -> ViResult<usize> {
                let mut offset = 0;
                $(
                    offset += self.$field.serialize_into(&mut buffer[offset..])?;
                )+
                Ok(offset)
            }

            fn deserialize_state(&mut self, buffer: &[u8]) -> ViResult<()> {
                let mut offset = 0;
                $(
                    self.$field = ViSerializable::deserialize_from(&buffer[offset..])?;
                    offset += self.$field.serialized_size();
                )+
                Ok(())
            }
        }
    };
}
