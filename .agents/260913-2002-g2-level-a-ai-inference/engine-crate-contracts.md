# Engine crate contracts (frozen for parallel implementation)

These signatures are the frozen interface between the engine crates. Implementations may add
private helpers, but a public item that differs from this document breaks the engine integration.

All three crates:

- `#![cfg_attr(not(test), no_std)]` at the crate root; `extern crate alloc;` when allocating.
- `#![forbid(unsafe_code)]` is enforced by `[lints.rust] unsafe_code = "deny"` in the manifest.
- **No** edits to the root `Cargo.toml` (crates are already registered) and **no** edits outside the
  crate's own directory.
- Test command (the workspace default target is bare metal):
  `cargo test -p <crate> --target x86_64-unknown-linux-gnu`
- No panics on malformed input: every parse/decode path returns `Result`. `expect`/`unwrap`/indexing
  that can go out of bounds on hostile bytes is prohibited outside tests.
- Doc comments on every public item; module docs state the invariant each module enforces.

---

## 1. `libs/tensor-math`

```rust
/// Errors that are the caller's fault (shape mismatch), never arithmetic faults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathError { ShapeMismatch, NotDivisible, Empty }

/// Q8_0 block layout constants and decoders (GGML: 32 weights per 34-byte block).
pub mod quant {
    pub const Q8_0_BLOCK_WEIGHTS: usize = 32;
    pub const Q8_0_BLOCK_BYTES: usize = 34;
    /// Decode one Q8_0 block (2-byte f16 scale + 32 i8) into `out`.
    pub fn q8_0_block_to_f32(block: &[u8], out: &mut [f32; Q8_0_BLOCK_WEIGHTS]) -> Result<(), MathError>;
    /// IEEE-754 binary16 bits → f32 (subnormals, inf, nan included).
    pub fn f16_to_f32(bits: u16) -> f32;
}

/// out[r] = dot(W[r], x), W row-major `rows × cols`, x.len() == cols.
pub fn matvec(out: &mut [f32], w: &[f32], x: &[f32], rows: usize, cols: usize) -> Result<(), MathError>;

/// Same as `matvec` with Q8_0-quantized rows; `w` is `rows × row_bytes` where
/// `row_bytes = cols / 32 * 34` (cols must be a multiple of 32).
pub fn matvec_q8_0(out: &mut [f32], w: &[u8], x: &[f32], rows: usize, cols: usize) -> Result<(), MathError>;

/// Row bytes for a Q8_0 tensor with `cols` columns.
pub fn q8_0_row_bytes(cols: usize) -> Option<usize>;

/// out[i] = x[i] / sqrt(mean(x²) + eps) * weight[i]
pub fn rms_norm(out: &mut [f32], x: &[f32], weight: &[f32], eps: f32) -> Result<(), MathError>;

/// In-place Llama NORMAL RoPE over one head vector: pairs (i, i + half) rotated by
/// `pos * freq_base^(-2i/dim)`.
pub fn rope_normal(head: &mut [f32], pos: usize, freq_base: f32) -> Result<(), MathError>;

/// Numerically stable in-place softmax (subtract max).
pub fn softmax_in_place(x: &mut [f32]);

/// SiLU(x) = x * sigmoid(x).
pub fn silu(x: f32) -> f32;

/// gate[i] = silu(gate[i]) * up[i] — Llama-family SwiGLU (`silu(gate) * up`).
pub fn swiglu_in_place(gate: &mut [f32], up: &[f32]) -> Result<(), MathError>;

/// Inner product of two equal-length slices.
pub fn dot(a: &[f32], b: &[f32]) -> Result<f32, MathError>;

/// out[i] += src[i] * scale
pub fn scaled_add_in_place(out: &mut [f32], src: &[f32], scale: f32) -> Result<(), MathError>;

/// out[i] += src[i]
pub fn add_in_place(out: &mut [f32], src: &[f32]) -> Result<(), MathError>;

/// Index of the largest value (first wins on ties); `None` for an empty slice.
pub fn argmax(x: &[f32]) -> Option<usize>;

/// Deterministic PRNG (xorshift64*) used by sampling.
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self;
    pub fn next_u32(&mut self) -> u32;
    /// Uniform in [0, 1).
    pub fn next_f32(&mut self) -> f32;
}

/// Greedy or top-k sampling. `temperature <= 0.0` selects argmax; otherwise the logits are
/// divided by temperature, restricted to the `k` largest (all when `k == 0`), softmaxed, and one
/// index is drawn from `rng`.
pub fn sample_top_k(logits: &mut [f32], k: usize, temperature: f32, rng: &mut Rng) -> Option<usize>;
```

Required tests: shape errors on every kernel; `matvec` against a naive hand-computed case;
`matvec_q8_0` equals `matvec` on dequantized weights; f16 edge cases (0, 1.0, -2.5, inf, nan);
`rms_norm` identity with unit weight; `rope_normal` preserves vector norm and is exactly identity at
position 0; `softmax_in_place` sums to 1 and is invariant to a constant offset; `sample_top_k` with
`temperature = 0` returns `argmax`, with `k = 1` always returns the max index, and with a fixed seed
is reproducible.

---

## 2. `libs/gguf-rs`

Reads a GGUF **v3** byte buffer. The caller owns the bytes (`&[u8]`); the reader never copies the
whole file and never trusts a length before checking it against the buffer.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GgufError {
    TooShort, BadMagic, UnsupportedVersion(u32), BadString, BadValueType(u32),
    BadArrayLength, MissingKey, TensorNotFound, UnsupportedDType(u32), BadTensorData,
}

/// GGML tensor data types this build understands. Anything else parses but is reported as
/// `Unsupported(u32)` so the engine can refuse the model instead of misreading weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GgmlDType { F32, F16, Q8_0, Unsupported(u32) }

impl GgmlDType {
    pub fn from_u32(raw: u32) -> Self;
    pub fn as_u32(self) -> u32;
    /// Bytes for one row of `cols` weights, or `None` when the dtype is unsupported or the
    /// element count is not a valid multiple.
    pub fn row_bytes(self, cols: u64) -> Option<usize>;
}

/// One tensor's directory entry. `dims` is in GGML order (fastest-varying first).
#[derive(Debug, Clone, Copy)]
pub struct TensorInfo<'a> {
    pub name: &'a str,
    pub dims: &'a [u64],
    pub dtype: GgmlDType,
    pub offset: u64,     // relative to the tensor-data section start
    pub byte_len: usize, // computed from dtype and dims
}

#[derive(Debug, Clone, Copy)]
pub enum MetadataValue<'a> {
    U8(u8), I8(i8), U16(u16), I16(i16), U32(u32), I32(i32), F32(f32), F64(f64), Bool(bool),
    U64(u64), I64(i64),
    Str(&'a str),
    /// `data` is the raw little-endian element bytes; element type and count are exposed.
    Array { elem_type: u32, count: u64, data: &'a [u8] },
}

pub struct GgufFile<'a> { /* private */ }

impl<'a> GgufFile<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, GgufError>;
    pub fn version(&self) -> u32;
    pub fn tensor_count(&self) -> u64;
    /// `general.alignment`, default 32.
    pub fn alignment(&self) -> u64;
    pub fn metadata(&self, key: &str) -> Option<MetadataValue<'a>>;
    pub fn metadata_u32(&self, key: &str) -> Option<u32>;
    pub fn metadata_u64(&self, key: &str) -> Option<u64>;
    pub fn metadata_f32(&self, key: &str) -> Option<f32>;
    pub fn metadata_bool(&self, key: &str) -> Option<bool>;
    pub fn metadata_str(&self, key: &str) -> Option<&'a str>;
    /// String array metadata (`tokenizer.ggml.tokens`, `…merges`), borrowed from the file.
    pub fn metadata_str_array(&self, key: &str) -> Option<Vec<&'a str>>;
    /// f32 array metadata (`tokenizer.ggml.scores`).
    pub fn metadata_f32_array(&self, key: &str) -> Option<Vec<f32>>;
    pub fn tensors(&self) -> impl Iterator<Item = &TensorInfo<'a>> + '_;
    pub fn tensor(&self, name: &str) -> Option<&TensorInfo<'a>>;
    /// Raw little-endian tensor bytes.
    pub fn tensor_data(&self, info: &TensorInfo<'a>) -> Result<&'a [u8], GgufError>;
    /// Decode the first `count` weights of a tensor into f32 (Q8_0 blocks must not be split).
    pub fn dequant_row(&self, dtype: GgmlDType, data: &[u8], count: usize, out: &mut [f32]) -> Result<(), GgufError>;
}
```

Required tests: a hand-built minimal GGUF byte image (write it in the test) covering v3 header,
string/u32/f32/bool/array metadata, one F32 and one Q8_0 tensor; reject bad magic, truncated
header, unknown value type, array length that overruns the buffer, offset+length past the tensor
data section; `dequant_row` Q8_0 matches `tensor_math::quant::q8_0_block_to_f32` composition.

---

## 3. `libs/ai-tokenizer`

Byte-level BPE (GPT-2 family) built from a parsed GGUF file.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenizerError {
    /// `tokenizer.ggml.model` is absent or is not a byte-level BPE model ("gpt2").
    UnsupportedModel,
    MissingTokens,
    MissingMerges,
    DuplicateMerge,
    /// A token id in the metadata points outside the token array.
    BadSpecialToken,
    Gguf(gguf_rs::GgufError),
}

/// Owns its tables (no lifetime): the engine owns the model buffer and cannot borrow out of it.
pub struct Tokenizer { /* private */ }

impl Tokenizer {
    /// `tokenizer.ggml.model` must be `"gpt2"`; anything else (notably `"llama"` SentencePiece)
    /// returns `UnsupportedModel` — the engine supports byte-level BPE only in this slice.
    /// Copies the vocabulary, merge table, and special-token ids into owned storage.
    pub fn from_gguf(file: &gguf_rs::GgufFile<'_>) -> Result<Self, TokenizerError>;
    pub fn vocab_size(&self) -> usize;
    /// Number of loaded BPE merges (used by the engine's memory accounting).
    pub fn merge_count(&self) -> usize;
    pub fn bos_id(&self) -> Option<u32>;
    pub fn eos_id(&self) -> Option<u32>;
    /// Raw token text for one id, or `None` when the id is out of range.
    pub fn token_str(&self, id: u32) -> Option<&str>;
    /// Decoded bytes of one token (inverse byte-level mapping applied), `None` when out of range.
    pub fn token_bytes(&self, id: u32) -> Option<Vec<u8>>;
    /// Encode text into token ids (no BOS/EOS inserted).
    pub fn encode(&self, text: &str) -> Vec<u32>;
    /// Encode and optionally wrap with BOS/EOS where the model declares them.
    pub fn encode_with_specials(&self, text: &str, add_bos: bool, add_eos: bool) -> Vec<u32>;
    /// Decode ids to bytes; ids out of range are skipped.
    pub fn decode_bytes(&self, ids: &[u32]) -> Vec<u8>;
    /// Decode ids to a `String`, lossily joining partial UTF-8 sequences.
    pub fn decode(&self, ids: &[u32]) -> String;
}
```

Semantics that must hold:

- The byte→unicode mapping is the canonical GPT-2 `bytes_to_unicode` table (printable ASCII and
  Latin-1 ranges map to themselves; remaining bytes map to U+0100… in order).
- Pre-tokenization follows the GPT-2 split: contractions (`'s`, `'t`, `'re`, `'ve`, `'m`, `'ll`,
  `'d`), optional leading space + letters, digits, other non-space runs, trailing whitespace runs.
  Use `char::is_alphabetic` / `char::is_numeric` / `char::is_whitespace`; document any divergence
  from the regex in the module docs.
- BPE merges are applied by rank (lowest rank first) until no merge applies.
- Unknown bytes never panic: a symbol absent from the vocabulary falls back to its constituent
  byte tokens when they exist, otherwise it is skipped.

Required tests (no real model needed): build a GGUF byte image in the test with a small hand-written
vocabulary + merges; assert round-trip `decode(encode(s)) == s` for ASCII, accented Latin-1,
multi-byte UTF-8, leading/duplicate whitespace; assert a known merge produces the merged token id;
assert `UnsupportedModel` for `tokenizer.ggml.model = "llama"`; assert out-of-range ids are skipped.
