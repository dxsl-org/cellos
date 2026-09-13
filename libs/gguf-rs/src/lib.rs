//! GGUF **v3** reader for the Cellos native CPU inference engine (Spec 24).
//!
//! The reader borrows the caller-owned `.gguf` byte buffer and resolves the header, the metadata
//! key/value store, the tensor directory and the tensor-data section without copying the payload.
//! Every borrowed `&str` and every `tensor_data` slice is a subslice of that buffer.
//!
//! # Invariants enforced by this module
//!
//! * **No declared length is ever trusted.** Every read goes through a cursor that uses
//!   `checked_add`/`checked_mul` against the remaining buffer before touching bytes, so truncated
//!   and hostile inputs become [`GgufError`] values — never panics, never out-of-bounds indexing.
//! * **Tensor payloads stay inside the file.** The tensor-data section start is aligned up to
//!   `general.alignment` (default 32) and each tensor's `offset + byte_len` is validated against
//!   the buffer length while parsing, so [`GgufFile::tensor_data`] can only ever hand out bytes
//!   that belong to this file's data section. `tensor_data` also re-derives the length from the
//!   dtype and dimensions and refuses an entry whose `byte_len` disagrees with its shape.
//! * **Tensor shapes are legal or rejected.** GGML allows one to four dimensions per tensor; a
//!   directory entry claiming `n_dims == 0` or `n_dims > 4` is rejected with
//!   [`GgufError::BadTensorShape`], and a tensor whose first dimension is not a whole number of
//!   blocks for its dtype is rejected with [`GgufError::BadTensorData`] — never silently
//!   truncated.
//! * **Dtypes the build does not understand are reported, not misread.** An unknown GGML type id
//!   parses as [`GgmlDType::Unsupported`] with `byte_len == 0`, letting the engine refuse the
//!   model instead of decoding garbage; `tensor_data` and `dequant_row` both refuse such a tensor.
//! * **Parsing allocates only the directory indexes** — two `Vec`s sized by the (bounded) header
//!   counts. No allocation is leaked and none outlives the returned [`GgufFile`].
//!
//! # Why `dims` is [`Dims`] rather than `&'a [u64]`
//!
//! GGUF stores a tensor's dimensions as little-endian `u64`s at unaligned offsets inside the
//! image, but the caller only lends the reader `&'a [u8]`. Producing a `&'a [u64]` view of those
//! bytes would require an unaligned `unsafe` cast (the manifest denies `unsafe_code`), and
//! materializing them into a `Vec<u64>` owned by [`GgufFile`] would make the stored
//! `Vec<TensorInfo<'a>>` self-referential, which safe Rust cannot express. [`Dims`] therefore keeps
//! the (at most four) dimensions inline as a `Copy` value and derefs to `&[u64]`, so
//! `info.dims.len()`, `info.dims[i]`, `info.dims.iter()` and `let d: &[u64] = &info.dims;` all read
//! exactly like the frozen `&'a [u64]` interface while staying allocation-free and leak-free.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use core::ops::Deref;

use tensor_math::quant::{f16_to_f32, q8_0_block_to_f32, Q8_0_BLOCK_BYTES, Q8_0_BLOCK_WEIGHTS};

/// GGUF magic, the first four bytes of every file.
const MAGIC: [u8; 4] = *b"GGUF";

/// The only container version this reader accepts.
const SUPPORTED_VERSION: u32 = 3;

/// Fallback for `general.alignment` when the model does not declare it.
const DEFAULT_ALIGNMENT: u64 = 32;

/// Metadata key holding the tensor-data section alignment.
const GENERAL_ALIGNMENT: &str = "general.alignment";

/// GGML tensors have at most four dimensions; a directory entry claiming more is rejected.
const GGML_MAX_DIMS: usize = 4;

/// GGUF metadata value type ids (on-disk wire values).
mod value_type {
    /// Unsigned 8-bit metadata value type id.
    pub const UINT8: u32 = 0;
    /// Signed 8-bit metadata value type id.
    pub const INT8: u32 = 1;
    /// Unsigned 16-bit metadata value type id.
    pub const UINT16: u32 = 2;
    /// Signed 16-bit metadata value type id.
    pub const INT16: u32 = 3;
    /// Unsigned 32-bit metadata value type id.
    pub const UINT32: u32 = 4;
    /// Signed 32-bit metadata value type id.
    pub const INT32: u32 = 5;
    /// IEEE-754 single precision metadata value type id.
    pub const FLOAT32: u32 = 6;
    /// Boolean metadata value type id.
    pub const BOOL: u32 = 7;
    /// UTF-8 string metadata value type id.
    pub const STRING: u32 = 8;
    /// Homogeneous array metadata value type id.
    pub const ARRAY: u32 = 9;
    /// Unsigned 64-bit metadata value type id.
    pub const UINT64: u32 = 10;
    /// Signed 64-bit metadata value type id.
    pub const INT64: u32 = 11;
    /// IEEE-754 double precision metadata value type id.
    pub const FLOAT64: u32 = 12;
}

/// Everything that can go wrong while reading a GGUF image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GgufError {
    /// The buffer ended before a fixed-size field (or an aligned data section) was complete.
    TooShort,
    /// The first four bytes are not `GGUF`.
    BadMagic,
    /// The container version is not the supported GGUF v3.
    UnsupportedVersion(u32),
    /// A string's declared byte length overruns the buffer, or its bytes are not UTF-8.
    BadString,
    /// A metadata value type id (or array element type id) is unknown or unusable.
    BadValueType(u32),
    /// An array's element count does not fit the remaining buffer.
    BadArrayLength,
    /// The requested metadata key is absent.
    MissingKey,
    /// The requested tensor name is absent from the directory.
    TensorNotFound,
    /// The tensor dtype is not implemented by this build (holds the raw GGML type id).
    UnsupportedDType(u32),
    /// A tensor directory entry, or a tensor payload range, is inconsistent with the buffer.
    BadTensorData,
    /// A tensor's dimension count is outside the GGML range `1..=4` (holds the claimed count).
    BadTensorShape(u32),
}

/// GGML tensor data types this build understands.
///
/// Anything else parses but is reported as [`GgmlDType::Unsupported`] so the engine can refuse the
/// model instead of misreading weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GgmlDType {
    /// 32-bit IEEE-754 float, 4 bytes per weight.
    F32,
    /// IEEE-754 binary16, 2 bytes per weight.
    F16,
    /// 32-weight block quantization: 2-byte f16 scale plus 32 `i8` quants (34 bytes per block).
    Q8_0,
    /// A type id this build cannot decode; the raw on-disk id is preserved.
    Unsupported(u32),
}

impl GgmlDType {
    /// Map a raw GGML type id onto a dtype, preserving unknown ids as [`GgmlDType::Unsupported`].
    pub fn from_u32(raw: u32) -> Self {
        match raw {
            0 => GgmlDType::F32,
            1 => GgmlDType::F16,
            8 => GgmlDType::Q8_0,
            other => GgmlDType::Unsupported(other),
        }
    }

    /// The raw GGML type id this dtype maps back to.
    pub fn as_u32(self) -> u32 {
        match self {
            GgmlDType::F32 => 0,
            GgmlDType::F16 => 1,
            GgmlDType::Q8_0 => 8,
            GgmlDType::Unsupported(raw) => raw,
        }
    }

    /// Bytes for one row of `cols` weights, or `None` when the dtype is unsupported or the element
    /// count is not a valid multiple (Q8_0 rows must be whole 32-weight blocks).
    pub fn row_bytes(self, cols: u64) -> Option<usize> {
        let cols = usize::try_from(cols).ok()?;
        match self {
            GgmlDType::F32 => cols.checked_mul(4),
            GgmlDType::F16 => cols.checked_mul(2),
            GgmlDType::Q8_0 => {
                if !cols.is_multiple_of(Q8_0_BLOCK_WEIGHTS) {
                    return None;
                }
                (cols / Q8_0_BLOCK_WEIGHTS).checked_mul(Q8_0_BLOCK_BYTES)
            }
            GgmlDType::Unsupported(_) => None,
        }
    }
}

/// The dimensions of one tensor, in GGML order (fastest-varying first).
///
/// GGML/GGUF allows at most **four** dimensions per tensor, so the value stores up to
/// [`Dims::MAX`] dimensions inline and a directory entry claiming more is rejected with
/// [`GgufError::BadTensorShape`] rather than truncated. The value owns its dimensions and derefs
/// to `&[u64]`, so it is used exactly like a slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dims {
    values: [u64; GGML_MAX_DIMS],
    len: u8,
}

impl Dims {
    /// The largest number of dimensions a GGML tensor can have (4).
    pub const MAX: usize = GGML_MAX_DIMS;

    /// Build from the first `len` values of `values`, clamping `len` to [`Dims::MAX`].
    fn from_parts(values: [u64; GGML_MAX_DIMS], len: usize) -> Self {
        let len = if len > GGML_MAX_DIMS {
            GGML_MAX_DIMS
        } else {
            len
        };
        Dims {
            values,
            len: len as u8,
        }
    }

    /// Number of dimensions.
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// `true` when the tensor has no dimensions.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The dimensions as a slice, in GGML order.
    pub fn as_slice(&self) -> &[u64] {
        self.values.get(..self.len()).unwrap_or(&[])
    }

    /// One dimension by index, or `None` when the index is past the last dimension.
    pub fn get(&self, index: usize) -> Option<u64> {
        self.as_slice().get(index).copied()
    }

    /// Iterate the dimensions in GGML order.
    pub fn iter(&self) -> core::slice::Iter<'_, u64> {
        self.as_slice().iter()
    }
}

impl Deref for Dims {
    type Target = [u64];

    fn deref(&self) -> &[u64] {
        self.as_slice()
    }
}

impl AsRef<[u64]> for Dims {
    fn as_ref(&self) -> &[u64] {
        self.as_slice()
    }
}

impl<const N: usize> PartialEq<[u64; N]> for Dims {
    fn eq(&self, other: &[u64; N]) -> bool {
        self.as_slice() == other.as_slice()
    }
}

/// One tensor's directory entry. `dims` is in GGML order (fastest-varying first).
#[derive(Debug, Clone, Copy)]
pub struct TensorInfo<'a> {
    /// Tensor name as stored in the file.
    pub name: &'a str,
    /// Dimensions in GGML order (fastest-varying first).
    pub dims: Dims,
    /// Quantization/format of the payload.
    pub dtype: GgmlDType,
    /// Offset of the payload relative to the tensor-data section start.
    pub offset: u64,
    /// Computed from `dtype` and `dims`; `0` when the dtype is unsupported.
    pub byte_len: usize,
}

/// One parsed metadata value.
///
/// Scalar variants borrow nothing; [`MetadataValue::Str`] and the data of
/// [`MetadataValue::Array`] are subslices of the parsed buffer.
#[derive(Debug, Clone, Copy)]
pub enum MetadataValue<'a> {
    /// `uint8` metadata value.
    U8(u8),
    /// `int8` metadata value.
    I8(i8),
    /// `uint16` metadata value.
    U16(u16),
    /// `int16` metadata value.
    I16(i16),
    /// `uint32` metadata value.
    U32(u32),
    /// `int32` metadata value.
    I32(i32),
    /// `float32` metadata value.
    F32(f32),
    /// `float64` metadata value.
    F64(f64),
    /// `bool` metadata value.
    Bool(bool),
    /// `uint64` metadata value.
    U64(u64),
    /// `int64` metadata value.
    I64(i64),
    /// UTF-8 string metadata value, borrowed from the file.
    Str(&'a str),
    /// Homogeneous array metadata value.
    ///
    /// `data` holds the raw little-endian element bytes, so it is `count * elem_size` bytes long
    /// for fixed-width element types; for string arrays it is the concatenated `u64 length || bytes`
    /// encoding of the elements. Element type and count are exposed so typed accessors can rebuild
    /// the values without copying the file.
    Array {
        /// Raw GGUF element type id of the array.
        elem_type: u32,
        /// Number of elements.
        count: u64,
        /// Raw little-endian element encodings, borrowed from the file.
        data: &'a [u8],
    },
}

/// A parsed GGUF v3 file borrowing the caller's byte buffer.
pub struct GgufFile<'a> {
    version: u32,
    tensor_count: u64,
    alignment: u64,
    metadata: Vec<(&'a str, MetadataValue<'a>)>,
    tensors: Vec<TensorInfo<'a>>,
    data_start: usize,
    bytes: &'a [u8],
}

impl<'a> GgufFile<'a> {
    /// Parse a complete GGUF v3 image.
    ///
    /// Returns an error for a bad magic, an unsupported version, any truncated field, a malformed
    /// string, an unknown metadata value type, an array whose count overruns the buffer, and a
    /// tensor whose payload range leaves the file. Never panics for any input, including truncated
    /// prefixes of a valid image.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, GgufError> {
        let mut reader = Reader::new(bytes);

        if reader.take(4)? != MAGIC {
            return Err(GgufError::BadMagic);
        }
        let version = reader.u32()?;
        if version != SUPPORTED_VERSION {
            return Err(GgufError::UnsupportedVersion(version));
        }
        let tensor_count = reader.u64()?;
        let metadata_count = reader.u64()?;

        // Bounded pre-allocation: the declared counts are hostile input, so capacity is capped and
        // the loops below fail on the first truncated read.
        let mut metadata: Vec<(&'a str, MetadataValue<'a>)> =
            Vec::with_capacity(capacity_hint(metadata_count));
        for _ in 0..metadata_count {
            let key = reader.string()?;
            let value = read_metadata_value(&mut reader)?;
            if !metadata.iter().any(|(existing, _)| *existing == key) {
                metadata.push((key, value));
            }
        }

        let mut tensors: Vec<TensorInfo<'a>> = Vec::with_capacity(capacity_hint(tensor_count));
        for _ in 0..tensor_count {
            let name = reader.string()?;
            // GGML tensors have 1..=4 dimensions; a file claiming anything else is rejected
            // instead of being silently truncated to the four inline slots.
            let claimed_dims = reader.u32()?;
            let n_dims = usize::try_from(claimed_dims)
                .map_err(|_| GgufError::BadTensorShape(claimed_dims))?;
            if n_dims == 0 || n_dims > GGML_MAX_DIMS {
                return Err(GgufError::BadTensorShape(claimed_dims));
            }
            let mut values = [0u64; GGML_MAX_DIMS];
            for index in 0..n_dims {
                let value = reader.u64()?;
                *values.get_mut(index).ok_or(GgufError::BadTensorData)? = value;
            }
            let dims = Dims::from_parts(values, n_dims);
            let dtype = GgmlDType::from_u32(reader.u32()?);
            let offset = reader.u64()?;
            let byte_len = match tensor_byte_len(dtype, dims.as_slice()) {
                Some(byte_len) => byte_len,
                // An unknown dtype has no known element width; the engine refuses such a model
                // through `GgmlDType::Unsupported`, so no payload length can be verified.
                None if matches!(dtype, GgmlDType::Unsupported(_)) => 0,
                None => return Err(GgufError::BadTensorData),
            };
            tensors.push(TensorInfo {
                name,
                dims,
                dtype,
                offset,
                byte_len,
            });
        }

        let alignment = metadata
            .iter()
            .find(|(key, _)| *key == GENERAL_ALIGNMENT)
            .and_then(|(_, value)| unsigned_value(*value))
            .unwrap_or(DEFAULT_ALIGNMENT);
        if alignment == 0 {
            return Err(GgufError::BadTensorData);
        }
        let data_start = align_up(reader.pos, alignment).ok_or(GgufError::BadTensorData)?;

        let data = DataSection { bytes, data_start };
        for info in &tensors {
            // Tensors with a known payload length must fit; an unsupported dtype cannot be sized
            // and is refused by `tensor_data` instead.
            if matches!(info.dtype, GgmlDType::Unsupported(_)) {
                continue;
            }
            if data.range(info.offset, info.byte_len).is_none() {
                return Err(GgufError::BadTensorData);
            }
        }

        Ok(GgufFile {
            version,
            tensor_count,
            alignment,
            metadata,
            tensors,
            data_start,
            bytes,
        })
    }

    /// The container version; always `3` for a successfully parsed file.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Number of tensor directory entries declared by the header.
    pub fn tensor_count(&self) -> u64 {
        self.tensor_count
    }

    /// `general.alignment`, default 32.
    pub fn alignment(&self) -> u64 {
        self.alignment
    }

    /// Look up a metadata value by key (`general.alignment`, `llama.block_count`, …).
    ///
    /// When a file repeats a key the first occurrence wins.
    pub fn metadata(&self, key: &str) -> Option<MetadataValue<'a>> {
        self.metadata
            .iter()
            .find(|(existing, _)| *existing == key)
            .map(|(_, value)| *value)
    }

    /// Unsigned integer metadata widened to `u32`; `None` for a missing key, a non-integer value,
    /// or an integer that does not fit.
    pub fn metadata_u32(&self, key: &str) -> Option<u32> {
        u32::try_from(unsigned_value(self.metadata(key)?)?).ok()
    }

    /// Unsigned integer metadata widened to `u64`; `None` for a missing key, a non-integer value,
    /// or a negative integer.
    pub fn metadata_u64(&self, key: &str) -> Option<u64> {
        unsigned_value(self.metadata(key)?)
    }

    /// `float32` metadata (a `float64` value is rounded to the nearest `f32`).
    pub fn metadata_f32(&self, key: &str) -> Option<f32> {
        match self.metadata(key)? {
            MetadataValue::F32(value) => Some(value),
            MetadataValue::F64(value) => Some(value as f32),
            _ => None,
        }
    }

    /// `bool` metadata.
    pub fn metadata_bool(&self, key: &str) -> Option<bool> {
        match self.metadata(key)? {
            MetadataValue::Bool(value) => Some(value),
            _ => None,
        }
    }

    /// String metadata, borrowed from the file.
    pub fn metadata_str(&self, key: &str) -> Option<&'a str> {
        match self.metadata(key)? {
            MetadataValue::Str(value) => Some(value),
            _ => None,
        }
    }

    /// String array metadata (`tokenizer.ggml.tokens`, `…merges`), borrowed from the file.
    ///
    /// Returns `None` when the key is missing, is not an array, or is not an array of strings. An
    /// empty array yields `Some(vec![])`, which keeps "absent" distinct from "present but empty".
    pub fn metadata_str_array(&self, key: &str) -> Option<Vec<&'a str>> {
        let (count, data) = match self.metadata(key)? {
            MetadataValue::Array {
                elem_type,
                count,
                data,
            } if elem_type == value_type::STRING => (count, data),
            _ => return None,
        };
        let count = usize::try_from(count).ok()?;
        let mut reader = Reader::new(data);
        let mut out = Vec::new();
        for _ in 0..count {
            out.push(reader.string().ok()?);
        }
        Some(out)
    }

    /// `float32` array metadata (`tokenizer.ggml.scores`); a `float64` array is rounded to `f32`.
    ///
    /// Returns `None` when the key is missing, is not an array, or has another element type.
    pub fn metadata_f32_array(&self, key: &str) -> Option<Vec<f32>> {
        let (elem_type, count, data) = match self.metadata(key)? {
            MetadataValue::Array {
                elem_type,
                count,
                data,
            } => (elem_type, count, data),
            _ => return None,
        };
        let count = usize::try_from(count).ok()?;
        let mut out = Vec::new();
        match elem_type {
            value_type::FLOAT32 => {
                let bytes = data.get(..count.checked_mul(4)?)?;
                for chunk in bytes.chunks_exact(4) {
                    let word: [u8; 4] = chunk.try_into().ok()?;
                    out.push(f32::from_le_bytes(word));
                }
            }
            value_type::FLOAT64 => {
                let bytes = data.get(..count.checked_mul(8)?)?;
                for chunk in bytes.chunks_exact(8) {
                    let word: [u8; 8] = chunk.try_into().ok()?;
                    out.push(f64::from_le_bytes(word) as f32);
                }
            }
            _ => return None,
        }
        Some(out)
    }

    /// Iterate the tensor directory in file order.
    pub fn tensors(&self) -> impl Iterator<Item = &TensorInfo<'a>> + '_ {
        self.tensors.iter()
    }

    /// Look up a tensor directory entry by name.
    pub fn tensor(&self, name: &str) -> Option<&TensorInfo<'a>> {
        self.tensors.iter().find(|info| info.name == name)
    }

    /// Raw little-endian tensor bytes, borrowed from the file.
    ///
    /// The payload length is re-derived from `info.dtype` and `info.dims` and must agree with
    /// `info.byte_len`; a directory entry whose length disagrees with its shape (a hand-built
    /// `TensorInfo`, or an entry from another file) is rejected with [`GgufError::BadTensorData`]
    /// instead of handing back a short slice. A tensor with an unsupported dtype has no known
    /// element width and is rejected with [`GgufError::UnsupportedDType`], as is a range that
    /// leaves the buffer.
    pub fn tensor_data(&self, info: &TensorInfo<'a>) -> Result<&'a [u8], GgufError> {
        let expected = match info.dtype {
            GgmlDType::Unsupported(raw) => return Err(GgufError::UnsupportedDType(raw)),
            dtype => {
                tensor_byte_len(dtype, info.dims.as_slice()).ok_or(GgufError::BadTensorData)?
            }
        };
        if expected != info.byte_len {
            return Err(GgufError::BadTensorData);
        }
        let bytes: &'a [u8] = self.bytes;
        let (start, end) = self
            .data_section()
            .range(info.offset, info.byte_len)
            .ok_or(GgufError::BadTensorData)?;
        bytes.get(start..end).ok_or(GgufError::BadTensorData)
    }

    /// Decode the first `count` weights of a tensor into the first `count` slots of `out`.
    ///
    /// Q8_0 blocks must not be split: `count` has to be a multiple of 32, because a partial block
    /// cannot be dequantized. `out` may be longer than `count`; extra slots are left untouched.
    pub fn dequant_row(
        &self,
        dtype: GgmlDType,
        data: &[u8],
        count: usize,
        out: &mut [f32],
    ) -> Result<(), GgufError> {
        let target = out.get_mut(..count).ok_or(GgufError::BadTensorData)?;
        match dtype {
            GgmlDType::F32 => {
                let needed = count.checked_mul(4).ok_or(GgufError::BadTensorData)?;
                let src = data.get(..needed).ok_or(GgufError::BadTensorData)?;
                for (slot, chunk) in target.iter_mut().zip(src.chunks_exact(4)) {
                    let word: [u8; 4] = chunk.try_into().map_err(|_| GgufError::BadTensorData)?;
                    *slot = f32::from_le_bytes(word);
                }
                Ok(())
            }
            GgmlDType::F16 => {
                let needed = count.checked_mul(2).ok_or(GgufError::BadTensorData)?;
                let src = data.get(..needed).ok_or(GgufError::BadTensorData)?;
                for (slot, chunk) in target.iter_mut().zip(src.chunks_exact(2)) {
                    let word: [u8; 2] = chunk.try_into().map_err(|_| GgufError::BadTensorData)?;
                    *slot = f16_to_f32(u16::from_le_bytes(word));
                }
                Ok(())
            }
            GgmlDType::Q8_0 => {
                if !count.is_multiple_of(Q8_0_BLOCK_WEIGHTS) {
                    return Err(GgufError::BadTensorData);
                }
                let blocks = count / Q8_0_BLOCK_WEIGHTS;
                let needed = blocks
                    .checked_mul(Q8_0_BLOCK_BYTES)
                    .ok_or(GgufError::BadTensorData)?;
                let src = data.get(..needed).ok_or(GgufError::BadTensorData)?;
                for (chunk, slot) in src
                    .chunks_exact(Q8_0_BLOCK_BYTES)
                    .zip(target.chunks_exact_mut(Q8_0_BLOCK_WEIGHTS))
                {
                    let mut block = [0.0f32; Q8_0_BLOCK_WEIGHTS];
                    q8_0_block_to_f32(chunk, &mut block).map_err(|_| GgufError::BadTensorData)?;
                    slot.copy_from_slice(&block);
                }
                Ok(())
            }
            GgmlDType::Unsupported(raw) => Err(GgufError::UnsupportedDType(raw)),
        }
    }

    /// View of the tensor-data section used for bounds checking.
    fn data_section(&self) -> DataSection<'a> {
        DataSection {
            bytes: self.bytes,
            data_start: self.data_start,
        }
    }
}

/// The tensor-data section: buffer plus the aligned start offset.
#[derive(Clone, Copy)]
struct DataSection<'a> {
    bytes: &'a [u8],
    data_start: usize,
}

impl DataSection<'_> {
    /// Absolute `(start, end)` range of a payload, or `None` when it leaves the buffer.
    fn range(&self, offset: u64, byte_len: usize) -> Option<(usize, usize)> {
        let offset = usize::try_from(offset).ok()?;
        let start = self.data_start.checked_add(offset)?;
        let end = start.checked_add(byte_len)?;
        if end > self.bytes.len() {
            return None;
        }
        Some((start, end))
    }
}

/// A bounds-checked cursor over the file bytes.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Start a cursor at the beginning of `bytes`.
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }

    /// Consume `n` bytes; fails with [`GgufError::TooShort`] instead of reading past the end.
    fn take(&mut self, n: usize) -> Result<&'a [u8], GgufError> {
        let bytes: &'a [u8] = self.bytes;
        let end = self.pos.checked_add(n).ok_or(GgufError::TooShort)?;
        let slice = bytes.get(self.pos..end).ok_or(GgufError::TooShort)?;
        self.pos = end;
        Ok(slice)
    }

    /// Consume exactly `N` bytes as a fixed-size array.
    fn array<const N: usize>(&mut self) -> Result<[u8; N], GgufError> {
        <[u8; N]>::try_from(self.take(N)?).map_err(|_| GgufError::TooShort)
    }

    /// Read one unsigned byte.
    fn u8(&mut self) -> Result<u8, GgufError> {
        Ok(self.array::<1>()?[0])
    }

    /// Read a little-endian `u16`.
    fn u16(&mut self) -> Result<u16, GgufError> {
        Ok(u16::from_le_bytes(self.array::<2>()?))
    }

    /// Read a little-endian `u32`.
    fn u32(&mut self) -> Result<u32, GgufError> {
        Ok(u32::from_le_bytes(self.array::<4>()?))
    }

    /// Read a little-endian `u64`.
    fn u64(&mut self) -> Result<u64, GgufError> {
        Ok(u64::from_le_bytes(self.array::<8>()?))
    }

    /// Read a little-endian `f32`.
    fn f32(&mut self) -> Result<f32, GgufError> {
        Ok(f32::from_le_bytes(self.array::<4>()?))
    }

    /// Read a little-endian `f64`.
    fn f64(&mut self) -> Result<f64, GgufError> {
        Ok(f64::from_le_bytes(self.array::<8>()?))
    }

    /// Read a length-prefixed UTF-8 string.
    ///
    /// A declared length that overruns the buffer and invalid UTF-8 both report
    /// [`GgufError::BadString`]; a missing length prefix reports [`GgufError::TooShort`].
    fn string(&mut self) -> Result<&'a str, GgufError> {
        let len = self.u64()?;
        let len = usize::try_from(len).map_err(|_| GgufError::BadString)?;
        let bytes = self.take(len).map_err(|_| GgufError::BadString)?;
        core::str::from_utf8(bytes).map_err(|_| GgufError::BadString)
    }

    /// Borrow `bytes[start..end]` with the file lifetime; used for string-array spans.
    fn span(&self, start: usize, end: usize) -> Result<&'a [u8], GgufError> {
        let bytes: &'a [u8] = self.bytes;
        bytes.get(start..end).ok_or(GgufError::BadArrayLength)
    }
}

/// Read one metadata value, including its wire type id.
fn read_metadata_value<'a>(reader: &mut Reader<'a>) -> Result<MetadataValue<'a>, GgufError> {
    let raw_type = reader.u32()?;
    match raw_type {
        value_type::UINT8 => Ok(MetadataValue::U8(reader.u8()?)),
        value_type::INT8 => Ok(MetadataValue::I8(reader.u8()? as i8)),
        value_type::UINT16 => Ok(MetadataValue::U16(reader.u16()?)),
        value_type::INT16 => Ok(MetadataValue::I16(reader.u16()? as i16)),
        value_type::UINT32 => Ok(MetadataValue::U32(reader.u32()?)),
        value_type::INT32 => Ok(MetadataValue::I32(reader.u32()? as i32)),
        value_type::FLOAT32 => Ok(MetadataValue::F32(reader.f32()?)),
        value_type::BOOL => Ok(MetadataValue::Bool(reader.u8()? != 0)),
        value_type::STRING => Ok(MetadataValue::Str(reader.string()?)),
        value_type::UINT64 => Ok(MetadataValue::U64(reader.u64()?)),
        value_type::INT64 => Ok(MetadataValue::I64(reader.u64()? as i64)),
        value_type::FLOAT64 => Ok(MetadataValue::F64(reader.f64()?)),
        value_type::ARRAY => {
            let elem_type = reader.u32()?;
            let count = reader.u64()?;
            let data = match fixed_element_size(elem_type) {
                Some(size) => {
                    let total = count
                        .checked_mul(size as u64)
                        .and_then(|total| usize::try_from(total).ok())
                        .ok_or(GgufError::BadArrayLength)?;
                    reader.take(total).map_err(|_| GgufError::BadArrayLength)?
                }
                None if elem_type == value_type::STRING => {
                    let start = reader.pos;
                    for _ in 0..count {
                        reader.string().map_err(|err| match err {
                            GgufError::TooShort => GgufError::BadArrayLength,
                            other => other,
                        })?;
                    }
                    reader.span(start, reader.pos)?
                }
                None => return Err(GgufError::BadValueType(elem_type)),
            };
            Ok(MetadataValue::Array {
                elem_type,
                count,
                data,
            })
        }
        other => Err(GgufError::BadValueType(other)),
    }
}

/// Fixed byte width of a metadata element type, or `None` for variable-width types.
fn fixed_element_size(elem_type: u32) -> Option<usize> {
    match elem_type {
        value_type::UINT8 | value_type::INT8 => Some(1),
        value_type::UINT16 | value_type::INT16 => Some(2),
        value_type::UINT32 | value_type::INT32 | value_type::FLOAT32 => Some(4),
        value_type::BOOL | value_type::UINT64 | value_type::INT64 | value_type::FLOAT64 => Some(8),
        _ => None,
    }
}

/// Byte length of a tensor computed from its dtype and GGML-ordered dimensions.
///
/// `None` when the dtype is unsupported or the first dimension is not a valid row for that dtype.
fn tensor_byte_len(dtype: GgmlDType, dims: &[u64]) -> Option<usize> {
    let (&row, rest) = dims.split_first()?;
    let mut total = dtype.row_bytes(row)?;
    for &dim in rest {
        total = total.checked_mul(usize::try_from(dim).ok()?)?;
    }
    Some(total)
}

/// Interpret a metadata value as an unsigned integer, rejecting negatives and non-integers.
fn unsigned_value(value: MetadataValue<'_>) -> Option<u64> {
    match value {
        MetadataValue::U8(number) => Some(number as u64),
        MetadataValue::U16(number) => Some(number as u64),
        MetadataValue::U32(number) => Some(number as u64),
        MetadataValue::U64(number) => Some(number),
        MetadataValue::I8(number) => u64::try_from(number).ok(),
        MetadataValue::I16(number) => u64::try_from(number).ok(),
        MetadataValue::I32(number) => u64::try_from(number).ok(),
        MetadataValue::I64(number) => u64::try_from(number).ok(),
        _ => None,
    }
}

/// Round `pos` up to the next multiple of `alignment`, or `None` on overflow.
fn align_up(pos: usize, alignment: u64) -> Option<usize> {
    let alignment = usize::try_from(alignment).ok()?;
    if alignment == 0 {
        return None;
    }
    let padded = pos.checked_add(alignment.checked_sub(1)?)?;
    (padded / alignment).checked_mul(alignment)
}

/// Cap a declared element count before using it as a `Vec` capacity hint.
fn capacity_hint(count: u64) -> usize {
    const MAX_PREALLOC: usize = 1024;
    let count = usize::try_from(count).unwrap_or(MAX_PREALLOC);
    if count > MAX_PREALLOC {
        MAX_PREALLOC
    } else {
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tensor_math::quant::Q8_0_BLOCK_BYTES;

    const ALIGNMENT: usize = 32;
    const ALIGNMENT_U32: u32 = 32;

    /// Little-endian byte-image builder used to write GGUF fixtures inside the tests.
    #[derive(Default)]
    struct Builder {
        buf: Vec<u8>,
    }

    impl Builder {
        fn new() -> Self {
            Builder { buf: Vec::new() }
        }

        fn raw(&mut self, bytes: &[u8]) {
            self.buf.extend_from_slice(bytes);
        }

        fn u8(&mut self, value: u8) {
            self.buf.push(value);
        }

        fn u16(&mut self, value: u16) {
            self.raw(&value.to_le_bytes());
        }

        fn u32(&mut self, value: u32) {
            self.raw(&value.to_le_bytes());
        }

        fn u64(&mut self, value: u64) {
            self.raw(&value.to_le_bytes());
        }

        fn i8(&mut self, value: i8) {
            self.u8(value as u8);
        }

        fn i16(&mut self, value: i16) {
            self.u16(value as u16);
        }

        fn i32(&mut self, value: i32) {
            self.u32(value as u32);
        }

        fn i64(&mut self, value: i64) {
            self.u64(value as u64);
        }

        fn f32(&mut self, value: f32) {
            self.raw(&value.to_le_bytes());
        }

        fn f64(&mut self, value: f64) {
            self.raw(&value.to_le_bytes());
        }

        fn string(&mut self, value: &str) {
            self.u64(value.len() as u64);
            self.raw(value.as_bytes());
        }

        fn pad_to(&mut self, alignment: usize) {
            while self.buf.len() % alignment != 0 {
                self.buf.push(0);
            }
        }
    }

    /// Encode a `uint32` metadata value including its wire type id.
    fn enc_u32(value: u32) -> Vec<u8> {
        let mut bytes = Builder::new();
        bytes.u32(value_type::UINT32);
        bytes.u32(value);
        bytes.buf
    }

    /// One tensor directory entry for a fixture image.
    struct TensorSpec {
        name: &'static str,
        dims: Vec<u64>,
        dtype: u32,
        offset: u64,
    }

    /// Metadata value bytes covering every [`MetadataValue`] kind.
    fn full_metadata() -> Vec<(&'static str, Vec<u8>)> {
        let mut u8_value = Builder::new();
        u8_value.u32(value_type::UINT8);
        u8_value.u8(0xAB);

        let mut i8_value = Builder::new();
        i8_value.u32(value_type::INT8);
        i8_value.i8(-7);

        let mut u16_value = Builder::new();
        u16_value.u32(value_type::UINT16);
        u16_value.u16(0xBEEF);

        let mut i16_value = Builder::new();
        i16_value.u32(value_type::INT16);
        i16_value.i16(-1234);

        let mut i32_value = Builder::new();
        i32_value.u32(value_type::INT32);
        i32_value.i32(-70000);

        let mut f32_value = Builder::new();
        f32_value.u32(value_type::FLOAT32);
        f32_value.f32(-2.5);

        let mut f64_value = Builder::new();
        f64_value.u32(value_type::FLOAT64);
        f64_value.f64(1.0 / 3.0);

        let mut bool_value = Builder::new();
        bool_value.u32(value_type::BOOL);
        bool_value.u8(1);

        let mut u64_value = Builder::new();
        u64_value.u32(value_type::UINT64);
        u64_value.u64(0x0123_4567_89AB_CDEF);

        let mut i64_value = Builder::new();
        i64_value.u32(value_type::INT64);
        i64_value.i64(-9_000_000_000);

        let mut str_value = Builder::new();
        str_value.u32(value_type::STRING);
        str_value.string("hello gguf \u{00e9}");

        // Array of strings: ["alpha", "beta"].
        let mut str_array = Builder::new();
        str_array.u32(value_type::ARRAY);
        str_array.u32(value_type::STRING);
        str_array.u64(2);
        str_array.string("alpha");
        str_array.string("beta");

        // Empty string array: present but with no elements.
        let mut empty_str_array = Builder::new();
        empty_str_array.u32(value_type::ARRAY);
        empty_str_array.u32(value_type::STRING);
        empty_str_array.u64(0);

        // Array of f32: [1.0, -2.5, 3.25].
        let mut f32_array = Builder::new();
        f32_array.u32(value_type::ARRAY);
        f32_array.u32(value_type::FLOAT32);
        f32_array.u64(3);
        f32_array.f32(1.0);
        f32_array.f32(-2.5);
        f32_array.f32(3.25);

        // Array of u32: [7, 9].
        let mut u32_array = Builder::new();
        u32_array.u32(value_type::ARRAY);
        u32_array.u32(value_type::UINT32);
        u32_array.u64(2);
        u32_array.u32(7);
        u32_array.u32(9);

        let mut u32_value = Builder::new();
        u32_value.u32(value_type::UINT32);
        u32_value.u32(0xDEAD_BEEF);

        vec![
            ("general.alignment", enc_u32(ALIGNMENT_U32)),
            ("test.u8", u8_value.buf),
            ("test.i8", i8_value.buf),
            ("test.u16", u16_value.buf),
            ("test.i16", i16_value.buf),
            ("test.i32", i32_value.buf),
            ("test.f32", f32_value.buf),
            ("test.f64", f64_value.buf),
            ("test.bool", bool_value.buf),
            ("test.u64", u64_value.buf),
            ("test.i64", i64_value.buf),
            ("test.str", str_value.buf),
            ("test.str_array", str_array.buf),
            ("test.empty_str_array", empty_str_array.buf),
            ("test.f32_array", f32_array.buf),
            ("test.u32_array", u32_array.buf),
            ("test.u32", u32_value.buf),
        ]
    }

    /// The tensor directory of the fixture image.
    fn full_tensors() -> Vec<TensorSpec> {
        vec![
            // 8 f32 weights (dims [4, 2]) at data offset 0: 32 bytes.
            TensorSpec {
                name: "a.f32",
                dims: vec![4, 2],
                dtype: 0,
                offset: 0,
            },
            // 2 Q8_0 blocks (dims [32, 2]) at data offset 32: 68 bytes.
            TensorSpec {
                name: "b.q8",
                dims: vec![32, 2],
                dtype: 8,
                offset: 32,
            },
            // 3 f16 weights at data offset 100: 6 bytes.
            TensorSpec {
                name: "c.f16",
                dims: vec![3],
                dtype: 1,
                offset: 100,
            },
            // Q4_0 (type id 2) is not implemented: parsed but reported as Unsupported.
            TensorSpec {
                name: "d.q4",
                dims: vec![32],
                dtype: 2,
                offset: 106,
            },
        ]
    }

    /// Payload of the fixture image: f32 row, two Q8_0 blocks, three f16 weights.
    fn full_data() -> Vec<u8> {
        let mut data = Builder::new();
        for index in 0..8u32 {
            data.f32(index as f32);
        }

        // Block 1: scale 1.0, quants 0..32.
        data.u16(0x3C00);
        for index in 0..32u8 {
            data.u8(index);
        }
        // Block 2: scale 0.5, quants -16..16.
        data.u16(0x3800);
        for index in 0..32i16 {
            data.i8((index - 16) as i8);
        }

        // f16 weights: 1.0, -2.5, 0.5.
        data.u16(0x3C00);
        data.u16(0xC100);
        data.u16(0x3800);
        data.buf
    }

    /// Assemble a GGUF image from raw metadata bytes and tensor specs.
    fn build_image(
        version: u32,
        magic: &[u8; 4],
        metadata: &[(&str, Vec<u8>)],
        tensors: &[TensorSpec],
        data: &[u8],
    ) -> Vec<u8> {
        let mut image = Builder::new();
        image.raw(magic);
        image.u32(version);
        image.u64(tensors.len() as u64);
        image.u64(metadata.len() as u64);
        for (key, value) in metadata {
            image.string(key);
            image.raw(value);
        }
        for tensor in tensors {
            image.string(tensor.name);
            image.u32(tensor.dims.len() as u32);
            for &dim in &tensor.dims {
                image.u64(dim);
            }
            image.u32(tensor.dtype);
            image.u64(tensor.offset);
        }
        image.pad_to(ALIGNMENT);
        image.raw(data);
        image.buf
    }

    /// The valid fixture image: GGUF v3, every metadata kind, four directory entries.
    fn valid_image() -> Vec<u8> {
        build_image(3, b"GGUF", &full_metadata(), &full_tensors(), &full_data())
    }

    #[test]
    fn parses_v3_header() {
        let bytes = valid_image();
        let file = GgufFile::parse(&bytes).expect("valid image must parse");
        assert_eq!(file.version(), 3);
        assert_eq!(file.tensor_count(), 4);
        assert_eq!(file.alignment(), 32);
    }

    #[test]
    fn reads_every_metadata_kind() {
        let bytes = valid_image();
        let file = GgufFile::parse(&bytes).expect("valid image must parse");
        assert!(file.metadata("missing.key").is_none());
        assert!(matches!(
            file.metadata("test.u8"),
            Some(MetadataValue::U8(0xAB))
        ));
        assert!(matches!(
            file.metadata("test.i8"),
            Some(MetadataValue::I8(-7))
        ));
        assert!(matches!(
            file.metadata("test.u16"),
            Some(MetadataValue::U16(0xBEEF))
        ));
        assert!(matches!(
            file.metadata("test.i16"),
            Some(MetadataValue::I16(-1234))
        ));
        assert!(matches!(
            file.metadata("test.u32"),
            Some(MetadataValue::U32(0xDEAD_BEEF))
        ));
        assert!(matches!(
            file.metadata("test.i32"),
            Some(MetadataValue::I32(-70000))
        ));
        assert!(matches!(file.metadata("test.f32"), Some(MetadataValue::F32(v)) if v == -2.5));
        assert!(
            matches!(file.metadata("test.f64"), Some(MetadataValue::F64(v)) if (v - 1.0 / 3.0).abs() < f64::EPSILON)
        );
        assert!(matches!(
            file.metadata("test.bool"),
            Some(MetadataValue::Bool(true))
        ));
        assert!(
            matches!(file.metadata("test.u64"), Some(MetadataValue::U64(v)) if v == 0x0123_4567_89AB_CDEF)
        );
        assert!(matches!(
            file.metadata("test.i64"),
            Some(MetadataValue::I64(-9_000_000_000))
        ));
    }

    #[test]
    fn reads_typed_metadata_accessors() {
        let bytes = valid_image();
        let file = GgufFile::parse(&bytes).expect("valid image must parse");
        assert_eq!(file.metadata_u32("test.u32"), Some(0xDEAD_BEEF));
        assert_eq!(file.metadata_u32("general.alignment"), Some(32));
        assert_eq!(file.metadata_u32("test.u8"), Some(0xAB));
        assert_eq!(file.metadata_u32("test.i8"), None, "negative must not wrap");
        assert_eq!(
            file.metadata_u32("test.u64"),
            None,
            "value does not fit u32"
        );
        assert_eq!(file.metadata_u32("test.str"), None);
        assert_eq!(file.metadata_u32("missing.key"), None);

        assert_eq!(file.metadata_u64("test.u64"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(file.metadata_u64("test.i64"), None);
        assert_eq!(file.metadata_u64("test.i32"), None);

        assert_eq!(file.metadata_f32("test.f32"), Some(-2.5));
        assert_eq!(file.metadata_f32("test.str"), None);
        assert!(file.metadata_f32("test.f64").is_some());

        assert_eq!(file.metadata_bool("test.bool"), Some(true));
        assert_eq!(file.metadata_bool("test.u8"), None);

        assert_eq!(file.metadata_str("test.str"), Some("hello gguf \u{00e9}"));
        assert_eq!(file.metadata_str("test.u8"), None);
    }

    #[test]
    fn reads_array_metadata() {
        let bytes = valid_image();
        let file = GgufFile::parse(&bytes).expect("valid image must parse");
        assert_eq!(
            file.metadata_str_array("test.str_array"),
            Some(vec!["alpha", "beta"])
        );
        assert_eq!(
            file.metadata_str_array("test.empty_str_array"),
            Some(Vec::new()),
            "present but empty must be distinguishable from absent"
        );
        assert!(file.metadata_str_array("test.f32_array").is_none());
        assert!(file.metadata_str_array("missing.key").is_none());
        assert_eq!(
            file.metadata_f32_array("test.f32_array"),
            Some(vec![1.0, -2.5, 3.25])
        );
        assert!(file.metadata_f32_array("test.u32_array").is_none());
        assert!(matches!(
            file.metadata("test.u32_array"),
            Some(MetadataValue::Array {
                elem_type: value_type::UINT32,
                count: 2,
                data,
            }) if data == [7, 0, 0, 0, 9, 0, 0, 0]
        ));
    }

    #[test]
    fn reads_tensor_directory() {
        let bytes = valid_image();
        let file = GgufFile::parse(&bytes).expect("valid image must parse");
        let names: Vec<&str> = file.tensors().map(|info| info.name).collect();
        assert_eq!(names, ["a.f32", "b.q8", "c.f16", "d.q4"]);
        assert!(file.tensor("missing.tensor").is_none());

        let f32_tensor = file.tensor("a.f32").expect("tensor a.f32");
        assert_eq!(f32_tensor.dims, [4, 2]);
        assert_eq!(f32_tensor.dims.len(), 2);
        assert_eq!(f32_tensor.dims.get(0), Some(4));
        assert_eq!(f32_tensor.dims.get(2), None);
        assert_eq!(f32_tensor.dims.iter().copied().sum::<u64>(), 6);
        assert_eq!(f32_tensor.dtype, GgmlDType::F32);
        assert_eq!(f32_tensor.offset, 0);
        assert_eq!(f32_tensor.byte_len, 32);

        let q8_tensor = file.tensor("b.q8").expect("tensor b.q8");
        assert_eq!(q8_tensor.dims.as_slice(), [32, 2]);
        assert_eq!(q8_tensor.dtype, GgmlDType::Q8_0);
        assert_eq!(q8_tensor.byte_len, 2 * Q8_0_BLOCK_BYTES);

        let f16_tensor = file.tensor("c.f16").expect("tensor c.f16");
        assert_eq!(f16_tensor.dtype, GgmlDType::F16);
        assert_eq!(f16_tensor.byte_len, 6);

        // The unsupported type keeps its raw id and reports a zero-length payload.
        let q4_tensor = file.tensor("d.q4").expect("tensor d.q4");
        assert_eq!(q4_tensor.dtype, GgmlDType::Unsupported(2));
        assert_eq!(q4_tensor.dtype.as_u32(), 2);
        assert_eq!(q4_tensor.byte_len, 0);

        assert_eq!(file.tensor_data(f32_tensor), Ok(&full_data()[..32]));
    }

    #[test]
    fn dequantizes_rows_from_tensor_data() {
        let bytes = valid_image();
        let file = GgufFile::parse(&bytes).expect("valid image must parse");

        let f32_tensor = *file.tensor("a.f32").expect("tensor a.f32");
        let payload = file.tensor_data(&f32_tensor).expect("f32 payload");
        let mut f32_out = [0.0f32; 8];
        file.dequant_row(GgmlDType::F32, payload, 8, &mut f32_out)
            .expect("F32 dequant");
        assert_eq!(f32_out, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);

        let f16_tensor = *file.tensor("c.f16").expect("tensor c.f16");
        let payload = file.tensor_data(&f16_tensor).expect("f16 payload");
        let mut f16_out = [0.0f32; 3];
        file.dequant_row(GgmlDType::F16, payload, 3, &mut f16_out)
            .expect("F16 dequant");
        assert_eq!(f16_out, [1.0, -2.5, 0.5]);
    }

    #[test]
    fn dequant_row_q8_0_matches_block_composition() {
        let bytes = valid_image();
        let file = GgufFile::parse(&bytes).expect("valid image must parse");
        let q8_tensor = *file.tensor("b.q8").expect("tensor b.q8");
        let payload = file.tensor_data(&q8_tensor).expect("q8 payload");

        let mut expected = [0.0f32; 64];
        for (index, chunk) in payload.chunks_exact(Q8_0_BLOCK_BYTES).enumerate() {
            let mut block = [0.0f32; Q8_0_BLOCK_WEIGHTS];
            q8_0_block_to_f32(chunk, &mut block).expect("block decode");
            expected[index * Q8_0_BLOCK_WEIGHTS..(index + 1) * Q8_0_BLOCK_WEIGHTS]
                .copy_from_slice(&block);
        }

        let mut decoded = [0.0f32; 64];
        file.dequant_row(GgmlDType::Q8_0, payload, 64, &mut decoded)
            .expect("Q8_0 dequant");
        assert_eq!(decoded, expected);
        assert_eq!(decoded[0], 0.0);
        assert_eq!(decoded[31], 31.0);
        assert_eq!(decoded[32], -8.0);
        assert_eq!(decoded[63], 7.5);

        // Only the first block is decoded when `count` asks for a single block.
        let mut single = [0.0f32; 32];
        file.dequant_row(GgmlDType::Q8_0, payload, 32, &mut single)
            .expect("single block");
        assert_eq!(single, expected[..32]);

        // `out` shorter than `count`, a split block, and short payloads are rejected.
        let mut too_small = [0.0f32; 4];
        assert_eq!(
            file.dequant_row(GgmlDType::Q8_0, payload, 32, &mut too_small),
            Err(GgufError::BadTensorData)
        );
        let mut split = [0.0f32; 16];
        assert_eq!(
            file.dequant_row(GgmlDType::Q8_0, payload, 16, &mut split),
            Err(GgufError::BadTensorData)
        );
        let mut short = [0.0f32; 32];
        assert_eq!(
            file.dequant_row(
                GgmlDType::Q8_0,
                &payload[..Q8_0_BLOCK_BYTES - 1],
                32,
                &mut short
            ),
            Err(GgufError::BadTensorData)
        );
        assert_eq!(
            file.dequant_row(GgmlDType::F32, &payload[..4], 8, &mut [0.0f32; 8]),
            Err(GgufError::BadTensorData)
        );
        assert_eq!(
            file.dequant_row(GgmlDType::Unsupported(2), payload, 0, &mut []),
            Err(GgufError::UnsupportedDType(2))
        );
    }

    #[test]
    fn dtype_ids_and_row_bytes() {
        assert_eq!(GgmlDType::from_u32(0), GgmlDType::F32);
        assert_eq!(GgmlDType::from_u32(1), GgmlDType::F16);
        assert_eq!(GgmlDType::from_u32(8), GgmlDType::Q8_0);
        assert_eq!(GgmlDType::from_u32(30), GgmlDType::Unsupported(30));
        assert_eq!(GgmlDType::F32.as_u32(), 0);
        assert_eq!(GgmlDType::F16.as_u32(), 1);
        assert_eq!(GgmlDType::Q8_0.as_u32(), 8);
        assert_eq!(GgmlDType::Unsupported(30).as_u32(), 30);

        assert_eq!(GgmlDType::F32.row_bytes(5), Some(20));
        assert_eq!(GgmlDType::F16.row_bytes(5), Some(10));
        assert_eq!(GgmlDType::Q8_0.row_bytes(64), Some(2 * Q8_0_BLOCK_BYTES));
        assert_eq!(GgmlDType::Q8_0.row_bytes(33), None);
        assert_eq!(GgmlDType::Unsupported(2).row_bytes(32), None);
        assert_eq!(GgmlDType::F32.row_bytes(u64::MAX), None);
    }

    #[test]
    fn rejects_bad_magic() {
        let bytes = valid_image();
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert_eq!(GgufFile::parse(&bad).err(), Some(GgufError::BadMagic));
        assert_eq!(
            GgufFile::parse(&bytes[..3]).err(),
            Some(GgufError::TooShort)
        );
    }

    #[test]
    fn rejects_unsupported_version() {
        let bytes = build_image(2, b"GGUF", &[], &[], &[]);
        assert_eq!(
            GgufFile::parse(&bytes).err(),
            Some(GgufError::UnsupportedVersion(2))
        );
        let bytes = build_image(4, b"GGUF", &[], &[], &[]);
        assert_eq!(
            GgufFile::parse(&bytes).err(),
            Some(GgufError::UnsupportedVersion(4))
        );
    }

    #[test]
    fn rejects_truncated_header() {
        let bytes = valid_image();
        for prefix in [3usize, 4, 8, 12, 16, 20] {
            let parsed = GgufFile::parse(&bytes[..prefix]);
            assert!(parsed.is_err(), "prefix {prefix} must not parse");
        }
    }

    #[test]
    fn rejects_unknown_value_type() {
        let metadata = vec![("test.bad", vec![99u8])];
        let bytes = build_image(3, b"GGUF", &metadata, &[], &[]);
        assert_eq!(
            GgufFile::parse(&bytes).err(),
            Some(GgufError::BadValueType(99))
        );

        // An array of unknown element type is rejected the same way.
        let mut array = Builder::new();
        array.u32(value_type::ARRAY);
        array.u32(99);
        array.u64(0);
        let metadata = vec![("test.bad_array", array.buf)];
        let bytes = build_image(3, b"GGUF", &metadata, &[], &[]);
        assert_eq!(
            GgufFile::parse(&bytes).err(),
            Some(GgufError::BadValueType(99))
        );
    }

    #[test]
    fn rejects_array_length_overrun() {
        // Declares 100 u32 elements with no payload bytes behind them.
        let mut array = Builder::new();
        array.u32(value_type::ARRAY);
        array.u32(value_type::UINT32);
        array.u64(100);
        let metadata = vec![("test.array", array.buf)];
        let bytes = build_image(3, b"GGUF", &metadata, &[], &[]);
        assert_eq!(
            GgufFile::parse(&bytes).err(),
            Some(GgufError::BadArrayLength)
        );

        // A string array whose second element is cut off is rejected too. The image ends inside
        // the array, so the trailing element's length prefix is not present.
        let mut image = Builder::new();
        image.raw(b"GGUF");
        image.u32(3);
        image.u64(0);
        image.u64(1);
        image.string("test.str_array");
        image.u32(value_type::ARRAY);
        image.u32(value_type::STRING);
        image.u64(2);
        image.string("only-one");
        assert_eq!(
            GgufFile::parse(&image.buf).err(),
            Some(GgufError::BadArrayLength)
        );
    }

    #[test]
    fn rejects_malformed_strings() {
        // Invalid UTF-8 payload.
        let mut value = Builder::new();
        value.u32(value_type::STRING);
        value.u64(2);
        value.u8(0xFF);
        value.u8(0xFE);
        let metadata = vec![("test.bad", value.buf)];
        let bytes = build_image(3, b"GGUF", &metadata, &[], &[]);
        assert_eq!(GgufFile::parse(&bytes).err(), Some(GgufError::BadString));

        // Declared length overruns the buffer.
        let mut value = Builder::new();
        value.u32(value_type::STRING);
        value.u64(4096);
        value.u8(b'x');
        let metadata = vec![("test.bad", value.buf)];
        let bytes = build_image(3, b"GGUF", &metadata, &[], &[]);
        assert_eq!(GgufFile::parse(&bytes).err(), Some(GgufError::BadString));
    }

    #[test]
    fn rejects_tensor_payload_past_data_section() {
        let mut tensors = full_tensors();
        tensors[0].offset = 0xFFFF;
        let bytes = build_image(3, b"GGUF", &full_metadata(), &tensors, &full_data());
        assert_eq!(
            GgufFile::parse(&bytes).err(),
            Some(GgufError::BadTensorData)
        );

        // The last supported tensor is two bytes too long for the data section.
        let mut tensors = full_tensors();
        tensors[2].offset = full_data().len() as u64 - 2;
        let bytes = build_image(3, b"GGUF", &full_metadata(), &tensors, &full_data());
        assert_eq!(
            GgufFile::parse(&bytes).err(),
            Some(GgufError::BadTensorData)
        );

        // A directory entry with more than four dimensions is rejected, not truncated.
        let mut tensors = full_tensors();
        tensors[0].dims = vec![1, 1, 1, 1, 1];
        let bytes = build_image(3, b"GGUF", &full_metadata(), &tensors, &full_data());
        assert_eq!(
            GgufFile::parse(&bytes).err(),
            Some(GgufError::BadTensorShape(5))
        );

        // A tensor with no dimensions has no row and is rejected as well.
        let mut tensors = full_tensors();
        tensors[0].dims = Vec::new();
        let bytes = build_image(3, b"GGUF", &full_metadata(), &tensors, &full_data());
        assert_eq!(
            GgufFile::parse(&bytes).err(),
            Some(GgufError::BadTensorShape(0))
        );

        // A Q8_0 row that is not a whole number of 32-weight blocks is rejected.
        let mut tensors = full_tensors();
        tensors[1].dims = vec![33, 2];
        let bytes = build_image(3, b"GGUF", &full_metadata(), &tensors, &full_data());
        assert_eq!(
            GgufFile::parse(&bytes).err(),
            Some(GgufError::BadTensorData)
        );
    }

    #[test]
    fn rejects_zero_alignment() {
        let metadata = vec![("general.alignment", enc_u32(0))];
        let bytes = build_image(3, b"GGUF", &metadata, &[], &[]);
        assert_eq!(
            GgufFile::parse(&bytes).err(),
            Some(GgufError::BadTensorData)
        );
    }

    #[test]
    fn tensor_data_rejects_foreign_ranges() {
        let bytes = valid_image();
        let file = GgufFile::parse(&bytes).expect("valid image must parse");
        let mut tensor = *file.tensor("a.f32").expect("tensor a.f32");
        tensor.offset = u64::MAX;
        assert_eq!(file.tensor_data(&tensor), Err(GgufError::BadTensorData));
        tensor.offset = 0;
        tensor.byte_len = usize::MAX;
        assert_eq!(file.tensor_data(&tensor), Err(GgufError::BadTensorData));

        // A shape that disagrees with the recorded length is refused, never sliced short.
        let mut tensor = *file.tensor("a.f32").expect("tensor a.f32");
        tensor.byte_len = 16;
        assert_eq!(file.tensor_data(&tensor), Err(GgufError::BadTensorData));
        let mut tensor = *file.tensor("b.q8").expect("tensor b.q8");
        tensor.byte_len = Q8_0_BLOCK_BYTES;
        assert_eq!(file.tensor_data(&tensor), Err(GgufError::BadTensorData));

        // An unsupported dtype has no known element width, so no payload can be addressed.
        let unsupported = *file.tensor("d.q4").expect("tensor d.q4");
        assert_eq!(
            file.tensor_data(&unsupported),
            Err(GgufError::UnsupportedDType(2))
        );
    }

    #[test]
    fn accepts_tensor_free_image() {
        // Header plus metadata only: no tensors, so no data section is required.
        let metadata = vec![("general.alignment", enc_u32(ALIGNMENT_U32))];
        let bytes = build_image(3, b"GGUF", &metadata, &[], &[]);
        let file = GgufFile::parse(&bytes).expect("tensor-free image must parse");
        assert_eq!(file.tensor_count(), 0);
        assert_eq!(file.tensors().count(), 0);
        assert!(file.tensor("anything").is_none());
    }

    #[test]
    fn never_panics_on_truncated_prefixes() {
        let bytes = valid_image();
        let mut accepted = 0usize;
        for prefix in 0..=bytes.len() {
            if GgufFile::parse(&bytes[..prefix]).is_ok() {
                accepted += 1;
            }
        }
        // Only the complete image can be valid: the data section must be present in full.
        assert_eq!(accepted, 1);
    }

    #[test]
    fn reads_tensor_payload_at_aligned_offset() {
        let bytes = valid_image();
        let file = GgufFile::parse(&bytes).expect("valid image must parse");
        let q8_tensor = *file.tensor("b.q8").expect("tensor b.q8");
        let payload = file.tensor_data(&q8_tensor).expect("q8 payload");
        assert_eq!(payload.len(), 2 * Q8_0_BLOCK_BYTES);
        assert_eq!(payload, &full_data()[32..100]);
    }
}
