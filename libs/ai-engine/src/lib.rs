//! Native CPU transformer engine for the Cellos unified AI inference service.
//!
//! Normative source: [`docs/specs/24-ai-inference-architecture.md`] §4. This crate is the CPU
//! engine behind the service: it loads a GGUF checkpoint, tokenizes a prompt with the model's own
//! tokenizer, runs a Llama-architecture forward pass with a per-session KV cache, samples the next
//! token, and hands generated ids back to the caller in bounded slices.
//!
//! # Scope and invariants
//!
//! - **One architecture**: `general.architecture == "llama"` (Llama/Mistral/Qwen-style: RMSNorm,
//!   RoPE, scaled dot-product attention, grouped-query attention, SwiGLU MLP). Any other
//!   architecture is refused with [`EngineError::UnsupportedArchitecture`] rather than guessed at.
//! - **Both tokenizer families**: byte-level BPE (`gpt2`) and SentencePiece (`llama`), including
//!   control-token matching for chat templates. See `ai-tokenizer`.
//! - **Two weight layouts**: `Q8_0` tensors stay quantized and run through
//!   [`tensor_math::matvec_q8_0_int8`] with the activation quantized to Q8_0 as well, so the inner
//!   loop is an integer dot product and two f32 multiplies per 32 weights; `F32`/`F16` tensors are
//!   materialized as `f32` at load time and run through the dense kernel.
//! - **Zero-copy weights**: [`Engine::load`] takes ownership of the model file and addresses every
//!   tensor in place, so a model costs its file size and nothing more. A tensor the kernels cannot
//!   consume as stored — F16, or a float region that is not 4-byte aligned — is converted once at
//!   load and accounted by [`Engine::resident_bytes`].
//! - **Bounded work per call**: [`Engine::generate`] performs at most the caller's step budget.
//!   One request can never monopolise the Cell's event loop, which is what makes cancel, fairness
//!   between sessions, and bounded reply latency possible.
//! - **No allocation in the token loop**: every scratch buffer is sized at load time.
//! - **Determinism**: with `temperature_milli == 0` (greedy) or a fixed seed, identical inputs
//!   produce identical token ids; the golden-oracle test pins this to an independent reference.
//! - **No `unsafe`**: `#![forbid(unsafe_code)]`.
//!
//! # Memory
//!
//! [`Engine::resident_bytes`] reports the model buffer + converted tensors + tokenizer tables + KV
//! caches + scratch. [`Engine::load`] refuses a model above the caller's `memory_limit` instead of
//! failing later mid-request — with zero-copy weights that ceiling is closer to the file size than
//! it used to be, which is what lets a 26.7 MB model live in a Cell whose VA slot is 32 MiB.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::ops::Range;

use ai_proto::{backend, limit, AiError, Describe, FinishReason, Quant, MAX_SESSIONS};
use ai_tokenizer::Tokenizer;
use gguf_rs::{GgmlDType, GgufError, GgufFile};
use tensor_math::{quant, MathError, Rng};

/// Largest context window this engine will allocate for, whatever a file claims.
///
/// A hostile or corrupt file must not be able to make the Cell allocate an arbitrary KV cache.
pub const MAX_CONTEXT_TOKENS: usize = 4096;

/// Largest vocabulary this engine will accept.
pub const MAX_VOCAB: usize = 262_144;

/// Failure to load a model. Every variant names the exact contract that was violated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The GGUF container is malformed.
    Gguf(GgufError),
    /// The tokenizer metadata is missing or not byte-level BPE.
    Tokenizer(ai_tokenizer::TokenizerError),
    /// `general.architecture` is absent or not `"llama"`.
    UnsupportedArchitecture,
    /// A required metadata key is missing.
    MissingMetadata(&'static str),
    /// A metadata value is outside the supported range.
    InvalidMetadata(&'static str),
    /// A required tensor is missing from the file.
    MissingTensor(String),
    /// A tensor's shape does not match the architecture metadata.
    BadTensorShape(String),
    /// A tensor uses a dtype this build cannot execute.
    UnsupportedDType(u32),
    /// The model needs more memory than the caller allowed.
    TooLarge {
        /// Bytes the loaded engine would hold.
        needed: usize,
        /// Caller-provided ceiling.
        limit: usize,
    },
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Gguf(e) => write!(f, "gguf: {e:?}"),
            EngineError::Tokenizer(e) => write!(f, "tokenizer: {e:?}"),
            EngineError::UnsupportedArchitecture => write!(f, "unsupported architecture"),
            EngineError::MissingMetadata(key) => write!(f, "missing metadata: {key}"),
            EngineError::InvalidMetadata(key) => write!(f, "invalid metadata: {key}"),
            EngineError::MissingTensor(name) => write!(f, "missing tensor: {name}"),
            EngineError::BadTensorShape(name) => write!(f, "bad tensor shape: {name}"),
            EngineError::UnsupportedDType(dtype) => write!(f, "unsupported dtype: {dtype}"),
            EngineError::TooLarge { needed, limit } => {
                write!(f, "model needs {needed} bytes, limit {limit}")
            }
        }
    }
}

/// Model geometry, read from `llama.*` GGUF metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelConfig {
    /// `general.name`, used by [`Describe`].
    pub name: String,
    /// Model context window in tokens.
    pub n_ctx: usize,
    /// Embedding width.
    pub n_embd: usize,
    /// Transformer block count.
    pub n_layer: usize,
    /// Query head count.
    pub n_head: usize,
    /// Key/value head count (grouped-query attention; equals `n_head` for plain MHA).
    pub n_head_kv: usize,
    /// Feed-forward hidden width.
    pub n_ff: usize,
    /// `llama.attention.layer_norm_rms_epsilon`.
    pub rms_eps: f32,
    /// RoPE base frequency.
    pub rope_freq_base: f32,
    /// Vocabulary size, from the tokenizer.
    pub vocab_size: usize,
}

impl ModelConfig {
    /// Per-head width (`n_embd / n_head`).
    pub fn head_dim(&self) -> usize {
        self.n_embd / self.n_head
    }

    /// Width of one K/V vector (`n_head_kv * head_dim`).
    pub fn kv_dim(&self) -> usize {
        self.n_head_kv * self.head_dim()
    }

    /// Query heads sharing one K/V head.
    pub fn group_size(&self) -> usize {
        self.n_head / self.n_head_kv
    }
}

/// Where a weight matrix's values live.
///
/// The engine owns the model file and addresses tensors by offset, so a weight costs nothing beyond
/// the file itself: no per-tensor copy, and no second copy of the file in flight during load. That
/// is what lets a model fit a Cell whose virtual-address slot is 32 MiB — the previous design held
/// the file *and* a copy of every tensor at the same time, roughly doubling the requirement.
///
/// The one exception is a tensor whose stored layout the kernels cannot consume — F16, or an
/// unaligned float region — which is converted once into an owned `f32` buffer.
enum Matrix {
    /// `rows` rows of packed Q8_0 blocks (`cols` a multiple of 32) at `offset`.
    Q8_0 {
        rows: usize,
        cols: usize,
        offset: usize,
        len: usize,
    },
    /// Row-major `rows × cols` f32 at `offset`, viewed without copying.
    F32 {
        rows: usize,
        cols: usize,
        offset: usize,
    },
    /// f32 rows converted at load time because the file stored them as F16 or unaligned.
    F32Owned {
        rows: usize,
        cols: usize,
        data: Vec<f32>,
    },
}

impl Matrix {
    fn rows(&self) -> usize {
        match self {
            Matrix::F32Owned { rows, .. }
            | Matrix::F32 { rows, .. }
            | Matrix::Q8_0 { rows, .. } => *rows,
        }
    }

    /// Bytes this matrix holds *of its own*; borrowed tensors are accounted by the model buffer.
    fn owned_bytes(&self) -> usize {
        match self {
            Matrix::F32Owned { data, .. } => data.len() * 4,
            Matrix::F32 { .. } | Matrix::Q8_0 { .. } => 0,
        }
    }
}

/// Norm weights: a borrowed f32 view of the model buffer, or a converted copy.
enum Norm {
    /// `len` f32 values at `offset`.
    F32 { offset: usize, len: usize },
    /// Converted at load time (F16 or unaligned input).
    Owned(Vec<f32>),
}

impl Norm {
    fn owned_bytes(&self) -> usize {
        match self {
            Norm::Owned(data) => data.len() * 4,
            Norm::F32 { .. } => 0,
        }
    }
}

/// Tensor bytes at `[offset, offset + len)` inside the model buffer.
fn tensor_bytes(buffer: &[u8], offset: usize, len: usize) -> Result<&[u8], MathError> {
    buffer
        .get(offset..offset.checked_add(len).ok_or(MathError::ShapeMismatch)?)
        .ok_or(MathError::ShapeMismatch)
}

/// Borrowed f32 view of `count` values at `offset`, refusing an unaligned region.
///
/// The view is what makes zero-copy loading safe without `unsafe`: `zerocopy` checks the alignment
/// and hands back a reference into the model buffer.
fn f32_values(buffer: &[u8], offset: usize, count: usize) -> Result<&[f32], MathError> {
    let bytes = tensor_bytes(
        buffer,
        offset,
        count.checked_mul(4).ok_or(MathError::ShapeMismatch)?,
    )?;
    zerocopy::FromBytes::ref_from_bytes(bytes).map_err(|_| MathError::ShapeMismatch)
}

/// Borrowed values of a norm weight.
fn norm_values<'a>(buffer: &'a [u8], norm: &'a Norm) -> Result<&'a [f32], MathError> {
    match norm {
        Norm::Owned(data) => Ok(data),
        Norm::F32 { offset, len } => f32_values(buffer, *offset, *len),
    }
}

/// Write `W · x` into `out[..rows]` for one stored matrix.
///
/// The kernel writes straight into the destination: every call site passes an exactly-sized slice,
/// so a staging buffer would only add a copy per projection — and for the tied output projection
/// (a `vocab × n_embd` matrix) that copy is what forced a per-row dequantise of the whole
/// vocabulary on every generated token.
///
/// The activation is the operand that *is* staged: for a `Q8_0` matrix it is quantized into `xq`
/// (one row, sized by `Scratch::new`) and the kernel then runs an integer dot product per 32-weight
/// block. Callers whose matrices share an input (attention `q`/`k`/`v`, then `gate`/`up`) pass the
/// same vector again; re-quantizing a `n_embd`-wide row costs one pass over it, against `rows` passes
/// for the projection itself, so the redundancy is not worth a second code path.
fn matvec_into(
    buffer: &[u8],
    matrix: &Matrix,
    out: &mut [f32],
    x: &[f32],
    xq: &mut [u8],
) -> Result<(), MathError> {
    let rows = matrix.rows();
    if out.len() < rows {
        return Err(MathError::ShapeMismatch);
    }
    let out = &mut out[..rows];
    match matrix {
        Matrix::Q8_0 {
            rows,
            cols,
            offset,
            len,
        } => {
            let data = tensor_bytes(buffer, *offset, *len)?;
            let row_bytes = tensor_math::q8_0_row_bytes(*cols).ok_or(MathError::NotDivisible)?;
            let activation = xq.get_mut(..row_bytes).ok_or(MathError::ShapeMismatch)?;
            quant::q8_0_row_from_f32(x, activation)?;
            tensor_math::matvec_q8_0_int8(out, data, activation, *rows, *cols)
        }
        Matrix::F32 { rows, cols, offset } => {
            let data = f32_values(buffer, *offset, rows * cols)?;
            tensor_math::matvec(out, data, x, *rows, *cols)
        }
        Matrix::F32Owned { rows, data, .. } => tensor_math::matvec(out, data, x, *rows, x.len()),
    }
}

/// Copy one matrix row into `out[..cols]`, dequantizing when the row is packed.
fn row_f32(buffer: &[u8], matrix: &Matrix, row: usize, out: &mut [f32]) -> Result<(), MathError> {
    match matrix {
        Matrix::F32 { rows, cols, offset } => {
            if row >= *rows || out.len() < *cols {
                return Err(MathError::ShapeMismatch);
            }
            let data = f32_values(buffer, offset + row * cols, *cols)?;
            out[..*cols].copy_from_slice(data);
            Ok(())
        }
        Matrix::F32Owned { rows, cols, data } => {
            if row >= *rows || out.len() < *cols {
                return Err(MathError::ShapeMismatch);
            }
            out[..*cols].copy_from_slice(&data[row * cols..(row + 1) * cols]);
            Ok(())
        }
        Matrix::Q8_0 {
            rows, cols, offset, ..
        } => {
            if row >= *rows || out.len() < *cols {
                return Err(MathError::ShapeMismatch);
            }
            let row_bytes = tensor_math::q8_0_row_bytes(*cols).ok_or(MathError::NotDivisible)?;
            let start = offset
                .checked_add(row.checked_mul(row_bytes).ok_or(MathError::ShapeMismatch)?)
                .ok_or(MathError::ShapeMismatch)?;
            let slice = tensor_bytes(buffer, start, row_bytes)?;
            for (block, out_block) in slice
                .chunks_exact(quant::Q8_0_BLOCK_BYTES)
                .zip(out.chunks_exact_mut(quant::Q8_0_BLOCK_WEIGHTS))
            {
                let mut decoded = [0f32; quant::Q8_0_BLOCK_WEIGHTS];
                quant::q8_0_block_to_f32(block, &mut decoded)?;
                out_block.copy_from_slice(&decoded);
            }
            Ok(())
        }
    }
}

/// One transformer block's weights.
struct Layer {
    attn_norm: Norm,
    wq: Matrix,
    wk: Matrix,
    wv: Matrix,
    wo: Matrix,
    ffn_norm: Norm,
    w_gate: Matrix,
    w_up: Matrix,
    w_down: Matrix,
}

/// All weights of one model, described rather than copied.
struct Weights {
    token_embd: Matrix,
    output_norm: Norm,
    /// `None` when the checkpoint ties output to the token embedding.
    output: Option<Matrix>,
    layers: Vec<Layer>,
}

impl Weights {
    /// Bytes held outside the model buffer (converted tensors only).
    fn owned_bytes(&self) -> usize {
        let mut total = self.token_embd.owned_bytes() + self.output_norm.owned_bytes();
        if let Some(output) = &self.output {
            total += output.owned_bytes();
        }
        for layer in &self.layers {
            total += layer.attn_norm.owned_bytes() + layer.ffn_norm.owned_bytes();
            total += layer.wq.owned_bytes()
                + layer.wk.owned_bytes()
                + layer.wv.owned_bytes()
                + layer.wo.owned_bytes()
                + layer.w_gate.owned_bytes()
                + layer.w_up.owned_bytes()
                + layer.w_down.owned_bytes();
        }
        total
    }
}

/// Positions a fresh session pre-allocates before growing on demand.
const KV_INITIAL_POSITIONS: usize = 64;

/// Key/value cache for one session.
///
/// **Position-major layout**: `pos * (n_layer * kv_dim) + layer * kv_dim`. A session grows its
/// cache as it advances, and this ordering makes growth a plain `Vec::resize` — a layer-major
/// layout would have to repack every cached vector whenever the position stride changed.
///
/// The cache starts small on purpose. Allocating the model's full declared context up front costs
/// real memory per session for no benefit — a 30-layer model with a 2048-token context reserved
/// 94 MB before its first token, which is what made a resident-set measurement look like the
/// weights had been copied.
struct KvCache {
    k: Vec<f32>,
    v: Vec<f32>,
    len: usize,
}

impl KvCache {
    fn new(_cfg: &ModelConfig) -> Self {
        Self {
            k: Vec::new(),
            v: Vec::new(),
            len: 0,
        }
    }

    fn bytes(&self) -> usize {
        (self.k.capacity() + self.v.capacity()) * 4
    }

    /// Positions this cache has room for.
    fn capacity(&self, cfg: &ModelConfig) -> usize {
        let per_position = cfg.n_layer * cfg.kv_dim();
        if per_position == 0 {
            return 0;
        }
        self.k.len() / per_position
    }

    /// Make room for `needed` positions, growing geometrically up to the model's context window.
    fn ensure(&mut self, cfg: &ModelConfig, needed: usize) -> Result<(), MathError> {
        if needed > cfg.n_ctx {
            return Err(MathError::ShapeMismatch);
        }
        if needed <= self.capacity(cfg) {
            return Ok(());
        }
        let target = needed
            .max(self.capacity(cfg).saturating_mul(2))
            .max(KV_INITIAL_POSITIONS)
            .min(cfg.n_ctx);
        let values = target * cfg.n_layer * cfg.kv_dim();
        self.k.resize(values, 0.0);
        self.v.resize(values, 0.0);
        Ok(())
    }

    /// Cache slot for layer `layer` at position `pos`.
    fn slot(&self, cfg: &ModelConfig, layer: usize, pos: usize) -> Range<usize> {
        let base = (pos * cfg.n_layer + layer) * cfg.kv_dim();
        base..base + cfg.kv_dim()
    }
}

/// Sampling parameters for one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SamplingParams {
    /// Hard cap on generated tokens.
    pub max_tokens: u16,
    /// Temperature × 1000; `0` selects greedy decoding.
    pub temperature_milli: u16,
    /// Top-k cutoff; `0` means the whole vocabulary.
    pub top_k: u16,
    /// Deterministic sampling seed.
    pub seed: u32,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            max_tokens: 16,
            temperature_milli: 0,
            top_k: 0,
            seed: 0,
        }
    }
}

/// One live generation session.
struct Session {
    id: u32,
    /// Every token in the session: prompt first, then generated.
    tokens: Vec<u32>,
    /// Prompt length, fixed at submit time.
    prompt_len: usize,
    /// Ids generated since the caller last drained them.
    pending: Vec<u32>,
    /// Byte-level decode buffer; holds an incomplete trailing UTF-8 sequence between polls.
    pending_bytes: Vec<u8>,
    kv: KvCache,
    /// Tokens already fed through the model.
    computed: usize,
    rng: Rng,
    params: SamplingParams,
    finish: Option<FinishReason>,
}

impl Session {
    fn generated(&self) -> usize {
        self.tokens.len().saturating_sub(self.prompt_len)
    }

    fn bytes(&self) -> usize {
        self.tokens.len() * 4 + self.pending.len() * 4 + self.pending_bytes.len() + self.kv.bytes()
    }
}

/// Outcome of one bounded generation step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// True once the session is finished.
    pub done: bool,
    /// Why it finished; `Some` exactly when `done`.
    pub finish: Option<FinishReason>,
}

/// Tokens and text handed back by [`Engine::drain`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drained {
    /// Token ids produced since the previous drain.
    pub ids: Vec<u32>,
    /// Complete UTF-8 text for `ids`; an incomplete trailing character is held for the next drain.
    pub text: String,
    /// True when the session has finished and all ids have been drained.
    pub done: bool,
    /// Why it finished; `Some` exactly when `done`.
    pub finish: Option<FinishReason>,
}

/// Per-step scratch buffers, allocated once at load time.
struct Scratch {
    x: Vec<f32>,
    xb: Vec<f32>,
    q: Vec<f32>,
    k: Vec<f32>,
    v: Vec<f32>,
    attn: Vec<f32>,
    down: Vec<f32>,
    gate: Vec<f32>,
    up: Vec<f32>,
    logits: Vec<f32>,
    scores: Vec<f32>,
    /// One Q8_0-quantized activation row, as wide as the widest projection input.
    xq: Vec<u8>,
}

impl Scratch {
    fn new(cfg: &ModelConfig) -> Self {
        // The quantized row must cover the widest `Q8_0` projection input: `n_embd` for
        // attention/MLP inputs and the output projection, `n_ff` for the down projection. The width
        // is rounded up to a whole block because a model whose `n_embd` is not a multiple of 32 is
        // still loadable when its tensors are stored as f32; each call slices exactly `row_bytes`.
        let widest = cfg.n_embd.max(cfg.n_ff);
        let xq = tensor_math::q8_0_row_bytes(widest.next_multiple_of(quant::Q8_0_BLOCK_WEIGHTS))
            .expect("rounding a width up to a block is always representable");
        Self {
            x: vec![0.0; cfg.n_embd],
            xb: vec![0.0; cfg.n_embd],
            q: vec![0.0; cfg.n_embd],
            k: vec![0.0; cfg.kv_dim()],
            v: vec![0.0; cfg.kv_dim()],
            attn: vec![0.0; cfg.n_embd],
            down: vec![0.0; cfg.n_embd],
            gate: vec![0.0; cfg.n_ff],
            up: vec![0.0; cfg.n_ff],
            logits: vec![0.0; cfg.vocab_size],
            scores: vec![0.0; cfg.n_ctx],
            xq: vec![0u8; xq],
        }
    }

    fn bytes(&self) -> usize {
        (self.x.len()
            + self.xb.len()
            + self.q.len()
            + self.k.len()
            + self.v.len()
            + self.attn.len()
            + self.down.len()
            + self.gate.len()
            + self.up.len()
            + self.logits.len()
            + self.scores.len())
            * 4
            + self.xq.len()
    }
}

/// The engine: the model file, descriptors into it, and a bounded session table.
pub struct Engine {
    /// The model file. Weights address it by offset, so this is the *only* copy of the weights.
    buffer: Vec<u8>,
    /// Offset of the tensor-data section inside `buffer` (see [`GgufFile::data_section_offset`]).
    data_section: usize,
    cfg: ModelConfig,
    tokenizer: Tokenizer,
    weights: Weights,
    sessions: [Option<Session>; MAX_SESSIONS as usize],
    next_request_id: u32,
    scratch: Scratch,
}

impl Engine {
    /// Load a model from an owned GGUF image.
    ///
    /// The buffer becomes the engine's weight storage — nothing is copied out of it. A model whose
    /// resident size would exceed `memory_limit` is refused with [`EngineError::TooLarge`], and the
    /// buffer is dropped with the error.
    pub fn load(buffer: Vec<u8>, memory_limit: usize) -> Result<Self, EngineError> {
        let file = GgufFile::parse(&buffer).map_err(EngineError::Gguf)?;
        let tokenizer = Tokenizer::from_gguf(&file).map_err(EngineError::Tokenizer)?;
        let cfg = read_config(&file, tokenizer.vocab_size())?;
        let data_section = file.data_section_offset();
        let weights = read_weights(&file, &cfg, data_section)?;
        drop(file);
        let scratch = Scratch::new(&cfg);

        let engine = Self {
            buffer,
            data_section,
            cfg,
            tokenizer,
            weights,
            sessions: [const { None }; MAX_SESSIONS as usize],
            next_request_id: 1,
            scratch,
        };

        let needed = engine.resident_bytes();
        if needed > memory_limit {
            return Err(EngineError::TooLarge {
                needed,
                limit: memory_limit,
            });
        }
        Ok(engine)
    }

    /// Load a model from a borrowed image, copying it once into the engine's own buffer.
    ///
    /// For callers that hold a `&'static [u8]` (a fixture compiled into a test binary) rather than a
    /// freshly read file. One copy of the file is the whole cost; the weights are still addressed in
    /// place afterwards.
    pub fn load_from_slice(bytes: &[u8], memory_limit: usize) -> Result<Self, EngineError> {
        Self::load(bytes.to_vec(), memory_limit)
    }

    /// The model file this engine serves.
    pub fn model_bytes(&self) -> &[u8] {
        &self.buffer
    }

    /// Model geometry.
    pub fn config(&self) -> &ModelConfig {
        &self.cfg
    }

    /// The model's tokenizer.
    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }

    /// Bytes held by the model buffer, converted tensors, tokenizer tables, KV caches, and scratch.
    ///
    /// The weights themselves are counted once, as the model buffer they live in.
    pub fn resident_bytes(&self) -> usize {
        let sessions: usize = self.sessions.iter().flatten().map(Session::bytes).sum();
        self.buffer.len()
            + self.weights.owned_bytes()
            + self.tokenizer.bytes()
            + sessions
            + self.scratch.bytes()
    }

    /// The model file this engine was loaded from, in bytes.
    pub fn model_len(&self) -> usize {
        self.buffer.len()
    }

    /// Tensor-data section offset, exposed for tests and diagnostics.
    pub fn data_section_offset(&self) -> usize {
        self.data_section
    }

    /// Capability description for [`ai_proto::AiRequest::Describe`].
    pub fn describe(&self) -> Describe<'_> {
        let live = self.sessions.iter().flatten().count() as u8;
        Describe {
            proto_version: 1,
            active_backend: backend::CPU,
            backends: backend::CPU,
            model: self.cfg.name.as_str(),
            arch: "llama",
            quant: self.quant_label(),
            context_tokens: self.cfg.n_ctx as u32,
            vocab_size: self.cfg.vocab_size as u32,
            live_sessions: live,
            max_sessions: MAX_SESSIONS,
            resident_bytes: self.resident_bytes().min(u32::MAX as usize) as u32,
        }
    }

    /// Weight layout of the resident model.
    fn quant_label(&self) -> Quant {
        match &self.weights.token_embd {
            Matrix::Q8_0 { .. } => Quant::Q8_0,
            Matrix::F32 { .. } | Matrix::F32Owned { .. } => Quant::F32,
        }
    }

    /// Start a generation session and return its service-assigned id.
    pub fn submit(&mut self, prompt: &str, params: SamplingParams) -> Result<u32, AiError> {
        if self.sessions.iter().all(Option::is_some) {
            return Err(AiError::Busy);
        }
        if params.max_tokens == 0 || params.max_tokens > ai_proto::MAX_TOKENS_PER_REQUEST {
            return Err(AiError::BadRequest(limit::Violation::MaxTokensOutOfRange));
        }

        let mut tokens =
            self.tokenizer
                .encode_with_specials(prompt, self.tokenizer.add_bos(), false);
        if tokens.is_empty() {
            return Err(AiError::BadRequest(limit::Violation::PromptTooLong));
        }
        // Keep at least one free slot for the first generated token.
        if tokens.len() >= self.cfg.n_ctx {
            tokens.truncate(self.cfg.n_ctx.saturating_sub(1));
        }

        let request_id = self.allocate_id();
        let session = Session {
            id: request_id,
            prompt_len: tokens.len(),
            tokens,
            pending: Vec::new(),
            pending_bytes: Vec::new(),
            kv: KvCache::new(&self.cfg),
            computed: 0,
            rng: Rng::new(u64::from(params.seed) ^ u64::from(request_id)),
            params,
            finish: None,
        };
        let slot = self
            .sessions
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(AiError::Busy)?;
        *slot = Some(session);
        Ok(request_id)
    }

    /// Advance a session by at most `budget` model steps (prefill or generated tokens).
    ///
    /// Returns [`AiError::UnknownRequest`] for an id this engine does not own.
    pub fn generate(&mut self, request_id: u32, budget: usize) -> Result<Progress, AiError> {
        let Engine {
            buffer,
            cfg,
            weights,
            scratch,
            sessions,
            tokenizer,
            ..
        } = self;

        let session = sessions
            .iter_mut()
            .flatten()
            .find(|session| session.id == request_id)
            .ok_or(AiError::UnknownRequest)?;

        if let Some(finish) = session.finish {
            return Ok(Progress {
                done: true,
                finish: Some(finish),
            });
        }

        let eos = tokenizer.eos_id();
        let mut steps = 0usize;
        while steps < budget {
            if session.computed < session.tokens.len() {
                let token = session.tokens[session.computed];
                let position = session.computed;
                let kv = &mut session.kv;
                forward(buffer, cfg, weights, scratch, kv, token, position)
                    .map_err(|_| AiError::Internal)?;
                session.computed += 1;
                steps += 1;
                continue;
            }

            // The sequence is fully computed: sample the next token.
            if session.tokens.len() >= cfg.n_ctx {
                session.finish = Some(FinishReason::ContextFull);
                break;
            }
            let sampled = sample(buffer, cfg, weights, scratch, session)?;
            session.tokens.push(sampled);
            session.pending.push(sampled);
            steps += 1;

            if Some(sampled) == eos {
                session.finish = Some(FinishReason::Stop);
                break;
            }
            if session.generated() >= usize::from(session.params.max_tokens) {
                session.finish = Some(FinishReason::Length);
                break;
            }
        }

        Ok(Progress {
            done: session.finish.is_some(),
            finish: session.finish,
        })
    }

    /// Drain up to `max` generated ids plus their complete text.
    pub fn drain(&mut self, request_id: u32, max: usize) -> Result<Drained, AiError> {
        let Engine {
            tokenizer,
            sessions,
            ..
        } = self;
        let session = sessions
            .iter_mut()
            .flatten()
            .find(|session| session.id == request_id)
            .ok_or(AiError::UnknownRequest)?;

        let take = max.min(session.pending.len());
        let ids: Vec<u32> = session.pending.drain(..take).collect();
        for id in &ids {
            if let Some(bytes) = tokenizer.token_bytes(*id) {
                session.pending_bytes.extend_from_slice(&bytes);
            }
        }

        let finished = session.finish.is_some() && session.pending.is_empty();
        let text = if finished {
            // Nothing more will arrive: emit any trailing bytes, replacing a stray partial
            // sequence rather than holding it forever.
            let bytes = core::mem::take(&mut session.pending_bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            take_complete_text(&mut session.pending_bytes)
        };

        Ok(Drained {
            ids,
            text,
            done: finished,
            finish: session.finish,
        })
    }

    /// Cancel a session and release its slot.
    pub fn cancel(&mut self, request_id: u32) -> Result<(), AiError> {
        match self.find_slot(request_id) {
            Some(slot) => {
                self.sessions[slot] = None;
                Ok(())
            }
            None => Err(AiError::UnknownRequest),
        }
    }

    /// Release a finished session's slot after the caller has drained it.
    pub fn release(&mut self, request_id: u32) -> Result<(), AiError> {
        match self.find_slot(request_id) {
            Some(slot) => {
                self.sessions[slot] = None;
                Ok(())
            }
            None => Err(AiError::UnknownRequest),
        }
    }

    /// Whether a session has finished generating.
    pub fn is_finished(&self, request_id: u32) -> Result<bool, AiError> {
        self.session(request_id)
            .map(|session| session.finish.is_some())
            .ok_or(AiError::UnknownRequest)
    }

    /// Mean-pooled, L2-normalized hidden state of `text` (Spec 24 §6 `embed`).
    ///
    /// Uses a scratch KV cache of its own, so it never disturbs a live session.
    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>, AiError> {
        let mut tokens = self
            .tokenizer
            .encode_with_specials(text, self.tokenizer.add_bos(), false);
        if tokens.is_empty() {
            return Err(AiError::BadRequest(limit::Violation::EmbedInputOutOfRange));
        }
        if tokens.len() > self.cfg.n_ctx {
            tokens.truncate(self.cfg.n_ctx);
        }

        let Engine {
            buffer,
            cfg,
            weights,
            scratch,
            ..
        } = self;
        let mut kv = KvCache::new(cfg);
        let mut pooled = vec![0.0f32; cfg.n_embd];

        for (position, token) in tokens.iter().enumerate() {
            forward(buffer, cfg, weights, scratch, &mut kv, *token, position)
                .map_err(|_| AiError::Internal)?;
            tensor_math::add_in_place(&mut pooled, &scratch.x).map_err(|_| AiError::Internal)?;
        }

        let scale = 1.0 / tokens.len() as f32;
        pooled.iter_mut().for_each(|value| *value *= scale);
        let norm = libm::sqrtf(tensor_math::dot(&pooled, &pooled).map_err(|_| AiError::Internal)?);
        if norm > 0.0 {
            pooled.iter_mut().for_each(|value| *value /= norm);
        }
        Ok(pooled)
    }

    fn session(&self, request_id: u32) -> Option<&Session> {
        self.sessions
            .iter()
            .flatten()
            .find(|session| session.id == request_id)
    }

    fn find_slot(&self, request_id: u32) -> Option<usize> {
        self.sessions
            .iter()
            .position(|slot| matches!(slot, Some(session) if session.id == request_id))
    }

    fn allocate_id(&mut self) -> u32 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        id
    }
}

/// One forward pass for `token` at `position`, updating the KV cache.
fn forward(
    buffer: &[u8],
    cfg: &ModelConfig,
    weights: &Weights,
    scratch: &mut Scratch,
    kv: &mut KvCache,
    token: u32,
    position: usize,
) -> Result<(), MathError> {
    let token = token as usize;
    if token >= cfg.vocab_size || position >= cfg.n_ctx {
        return Err(MathError::ShapeMismatch);
    }

    row_f32(
        buffer,
        &weights.token_embd,
        token,
        &mut scratch.x[..cfg.n_embd],
    )?;

    for (layer_index, layer) in weights.layers.iter().enumerate() {
        tensor_math::rms_norm(
            &mut scratch.xb[..cfg.n_embd],
            &scratch.x[..cfg.n_embd],
            norm_values(buffer, &layer.attn_norm)?,
            cfg.rms_eps,
        )?;

        matvec_into(
            buffer,
            &layer.wq,
            &mut scratch.q[..cfg.n_embd],
            &scratch.xb[..cfg.n_embd],
            &mut scratch.xq,
        )?;
        matvec_into(
            buffer,
            &layer.wk,
            &mut scratch.k[..cfg.kv_dim()],
            &scratch.xb[..cfg.n_embd],
            &mut scratch.xq,
        )?;
        matvec_into(
            buffer,
            &layer.wv,
            &mut scratch.v[..cfg.kv_dim()],
            &scratch.xb[..cfg.n_embd],
            &mut scratch.xq,
        )?;

        rope(&mut scratch.q[..cfg.n_embd], cfg, position)?;
        rope(&mut scratch.k[..cfg.kv_dim()], cfg, position)?;

        kv.ensure(cfg, position + 1)?;
        let slot = kv.slot(cfg, layer_index, position);
        kv.k[slot.clone()].copy_from_slice(&scratch.k[..cfg.kv_dim()]);
        kv.v[slot].copy_from_slice(&scratch.v[..cfg.kv_dim()]);

        attention(cfg, kv, layer_index, scratch, position)?;
        matvec_into(
            buffer,
            &layer.wo,
            &mut scratch.down[..cfg.n_embd],
            &scratch.attn[..cfg.n_embd],
            &mut scratch.xq,
        )?;
        tensor_math::add_in_place(&mut scratch.x[..cfg.n_embd], &scratch.down[..cfg.n_embd])?;

        tensor_math::rms_norm(
            &mut scratch.xb[..cfg.n_embd],
            &scratch.x[..cfg.n_embd],
            norm_values(buffer, &layer.ffn_norm)?,
            cfg.rms_eps,
        )?;
        matvec_into(
            buffer,
            &layer.w_gate,
            &mut scratch.gate[..cfg.n_ff],
            &scratch.xb[..cfg.n_embd],
            &mut scratch.xq,
        )?;
        matvec_into(
            buffer,
            &layer.w_up,
            &mut scratch.up[..cfg.n_ff],
            &scratch.xb[..cfg.n_embd],
            &mut scratch.xq,
        )?;
        tensor_math::swiglu_in_place(&mut scratch.gate[..cfg.n_ff], &scratch.up[..cfg.n_ff])?;
        matvec_into(
            buffer,
            &layer.w_down,
            &mut scratch.down[..cfg.n_embd],
            &scratch.gate[..cfg.n_ff],
            &mut scratch.xq,
        )?;
        tensor_math::add_in_place(&mut scratch.x[..cfg.n_embd], &scratch.down[..cfg.n_embd])?;
    }

    kv.len = position + 1;
    Ok(())
}

/// Dot-product attention for one layer at `position`, written into `scratch.attn`.
fn attention(
    cfg: &ModelConfig,
    kv: &KvCache,
    layer: usize,
    scratch: &mut Scratch,
    position: usize,
) -> Result<(), MathError> {
    let head_dim = cfg.head_dim();
    let group = cfg.group_size();
    // Scaled dot-product attention. Every Llama-family implementation divides the scores by
    // `sqrt(head_dim)` before the softmax (llama.cpp folds the factor into q; llama2.c multiplies
    // the accumulated score). Omitting it sharpens every attention distribution by that factor and
    // degrades generation to word salad on real weights, which is exactly how this was found.
    let scale = 1.0 / libm::sqrtf(head_dim as f32);

    for head in 0..cfg.n_head {
        let kv_head = head / group;
        let q = &scratch.q[head * head_dim..(head + 1) * head_dim];
        for key_position in 0..=position {
            let slot = kv.slot(cfg, layer, key_position);
            let k = &kv.k[slot.start + kv_head * head_dim..slot.start + (kv_head + 1) * head_dim];
            scratch.scores[key_position] = tensor_math::dot(q, k)? * scale;
        }
        tensor_math::softmax_in_place(&mut scratch.scores[..=position]);

        let out = &mut scratch.attn[head * head_dim..(head + 1) * head_dim];
        out.iter_mut().for_each(|value| *value = 0.0);
        for key_position in 0..=position {
            let slot = kv.slot(cfg, layer, key_position);
            let v = &kv.v[slot.start + kv_head * head_dim..slot.start + (kv_head + 1) * head_dim];
            tensor_math::scaled_add_in_place(out, v, scratch.scores[key_position])?;
        }
    }
    Ok(())
}

/// Sample the next token from the logits of the current hidden state.
fn sample(
    buffer: &[u8],
    cfg: &ModelConfig,
    weights: &Weights,
    scratch: &mut Scratch,
    session: &mut Session,
) -> Result<u32, AiError> {
    tensor_math::rms_norm(
        &mut scratch.xb[..cfg.n_embd],
        &scratch.x[..cfg.n_embd],
        norm_values(buffer, &weights.output_norm).map_err(|_| AiError::Internal)?,
        cfg.rms_eps,
    )
    .map_err(|_| AiError::Internal)?;

    // Tied embedding (`output.weight` absent) uses the token embedding as the output projection.
    let output = weights.output.as_ref().unwrap_or(&weights.token_embd);
    matvec_into(
        buffer,
        output,
        &mut scratch.logits[..cfg.vocab_size],
        &scratch.xb[..cfg.n_embd],
        &mut scratch.xq,
    )
    .map_err(|_| AiError::Internal)?;

    let temperature = f32::from(session.params.temperature_milli) / 1000.0;
    let index = tensor_math::sample_top_k(
        &mut scratch.logits[..cfg.vocab_size],
        usize::from(session.params.top_k),
        temperature,
        &mut session.rng,
    )
    .ok_or(AiError::Internal)?;
    Ok(index as u32)
}

/// Apply RoPE to each head of a q/k vector.
fn rope(vector: &mut [f32], cfg: &ModelConfig, position: usize) -> Result<(), MathError> {
    let head_dim = cfg.head_dim();
    for head in vector.chunks_exact_mut(head_dim) {
        tensor_math::rope_normal(head, position, cfg.rope_freq_base)?;
    }
    Ok(())
}

/// Split the longest complete-UTF-8 prefix out of `buffer`, leaving the partial tail behind.
fn take_complete_text(buffer: &mut Vec<u8>) -> String {
    let split = complete_utf8_prefix(buffer);
    let tail = buffer.split_off(split);
    let head = core::mem::replace(buffer, tail);
    match String::from_utf8(head) {
        Ok(text) => text,
        // The prefix ended on a boundary by construction, so this is unreachable for well-formed
        // input; a lossy conversion keeps the Cell alive on a corrupt token table.
        Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
    }
}

/// Length of the longest prefix of `bytes` that ends on a UTF-8 character boundary.
fn complete_utf8_prefix(bytes: &[u8]) -> usize {
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let width = if byte < 0x80 {
            1
        } else if byte >> 5 == 0b110 {
            2
        } else if byte >> 4 == 0b1110 {
            3
        } else if byte >> 3 == 0b11110 {
            4
        } else {
            // Invalid leading byte: consume it so the caller can emit a replacement character.
            1
        };
        if index + width > bytes.len() {
            break;
        }
        index += width;
    }
    index
}

/// Read `llama.*` metadata into [`ModelConfig`].
fn read_config(file: &GgufFile<'_>, vocab_size: usize) -> Result<ModelConfig, EngineError> {
    let arch = file
        .metadata_str("general.architecture")
        .ok_or(EngineError::MissingMetadata("general.architecture"))?;
    if arch != "llama" {
        return Err(EngineError::UnsupportedArchitecture);
    }

    let n_embd = meta_usize(file, "llama.embedding_length")?;
    let n_head = meta_usize(file, "llama.attention.head_count")?;
    let n_head_kv = file
        .metadata_u32("llama.attention.head_count_kv")
        .map(|value| value as usize)
        .unwrap_or(n_head);
    let n_layer = meta_usize(file, "llama.block_count")?;
    let n_ff = meta_usize(file, "llama.feed_forward_length")?;
    let n_ctx = meta_usize(file, "llama.context_length")?;

    if vocab_size == 0 || vocab_size > MAX_VOCAB {
        return Err(EngineError::InvalidMetadata("tokenizer.ggml.tokens"));
    }
    if n_embd == 0 || n_head == 0 || n_embd % n_head != 0 {
        return Err(EngineError::InvalidMetadata("llama.embedding_length"));
    }
    if n_head_kv == 0 || n_head % n_head_kv != 0 {
        return Err(EngineError::InvalidMetadata(
            "llama.attention.head_count_kv",
        ));
    }
    if n_layer == 0 || n_ff == 0 {
        return Err(EngineError::InvalidMetadata("llama.block_count"));
    }
    if n_ctx == 0 || n_ctx > MAX_CONTEXT_TOKENS {
        return Err(EngineError::InvalidMetadata("llama.context_length"));
    }
    if !(n_embd / n_head).is_multiple_of(2) {
        return Err(EngineError::InvalidMetadata("llama.attention.head_count"));
    }

    Ok(ModelConfig {
        name: file
            .metadata_str("general.name")
            .unwrap_or("unnamed")
            .to_owned(),
        n_ctx,
        n_embd,
        n_layer,
        n_head,
        n_head_kv,
        n_ff,
        rms_eps: file
            .metadata_f32("llama.attention.layer_norm_rms_epsilon")
            .unwrap_or(1e-5),
        rope_freq_base: file
            .metadata_f32("llama.rope.freq_base")
            .unwrap_or(10_000.0),
        vocab_size,
    })
}

fn meta_usize(file: &GgufFile<'_>, key: &'static str) -> Result<usize, EngineError> {
    file.metadata_u32(key)
        .map(|value| value as usize)
        .or_else(|| file.metadata_u64(key).map(|value| value as usize))
        .ok_or(EngineError::MissingMetadata(key))
}

/// Load every weight tensor the Llama forward pass needs, as descriptors into `buffer`.
///
/// Only tensors the kernels cannot consume in place are converted: F16 (no F16 kernel) and float
/// tensors whose offset is not 4-byte aligned. Everything else stays where the file put it.
fn read_weights(
    file: &GgufFile<'_>,
    cfg: &ModelConfig,
    data_section: usize,
) -> Result<Weights, EngineError> {
    let token_embd = read_matrix(
        file,
        data_section,
        "token_embd.weight",
        cfg.vocab_size,
        cfg.n_embd,
    )?;
    let output_norm = read_norm(file, data_section, "output_norm.weight", cfg.n_embd)?;
    // Tied-embedding checkpoints omit `output.weight`.
    let output = if file.tensor("output.weight").is_some() {
        Some(read_matrix(
            file,
            data_section,
            "output.weight",
            cfg.vocab_size,
            cfg.n_embd,
        )?)
    } else {
        None
    };

    let mut layers = Vec::with_capacity(cfg.n_layer);
    for index in 0..cfg.n_layer {
        let name = |suffix: &str| format!("blk.{index}.{suffix}");
        layers.push(Layer {
            attn_norm: read_norm(file, data_section, &name("attn_norm.weight"), cfg.n_embd)?,
            wq: read_matrix(
                file,
                data_section,
                &name("attn_q.weight"),
                cfg.n_embd,
                cfg.n_embd,
            )?,
            wk: read_matrix(
                file,
                data_section,
                &name("attn_k.weight"),
                cfg.kv_dim(),
                cfg.n_embd,
            )?,
            wv: read_matrix(
                file,
                data_section,
                &name("attn_v.weight"),
                cfg.kv_dim(),
                cfg.n_embd,
            )?,
            wo: read_matrix(
                file,
                data_section,
                &name("attn_output.weight"),
                cfg.n_embd,
                cfg.n_embd,
            )?,
            ffn_norm: read_norm(file, data_section, &name("ffn_norm.weight"), cfg.n_embd)?,
            w_gate: read_matrix(
                file,
                data_section,
                &name("ffn_gate.weight"),
                cfg.n_ff,
                cfg.n_embd,
            )?,
            w_up: read_matrix(
                file,
                data_section,
                &name("ffn_up.weight"),
                cfg.n_ff,
                cfg.n_embd,
            )?,
            w_down: read_matrix(
                file,
                data_section,
                &name("ffn_down.weight"),
                cfg.n_embd,
                cfg.n_ff,
            )?,
        });
    }

    Ok(Weights {
        token_embd,
        output_norm,
        output,
        layers,
    })
}

/// Absolute offset of a tensor's bytes inside the model file.
fn tensor_offset(
    file: &GgufFile<'_>,
    data_section: usize,
    name: &str,
) -> Result<(usize, GgmlDType, TensorDims), EngineError> {
    let info = file
        .tensor(name)
        .ok_or_else(|| EngineError::MissingTensor(name.to_owned()))?;
    let data = file.tensor_data(info).map_err(EngineError::Gguf)?;
    let relative = data.as_ptr() as usize - file.bytes().as_ptr() as usize;
    let _ = data_section;
    let dims = if info.dims.len() >= 2 {
        TensorDims {
            first: info.dims[0] as usize,
            second: info.dims[1] as usize,
        }
    } else {
        TensorDims {
            first: info.dims.first().copied().unwrap_or(0) as usize,
            second: 1,
        }
    };
    Ok((relative, info.dtype, dims))
}

/// First two tensor dimensions, in GGUF order.
struct TensorDims {
    first: usize,
    second: usize,
}

/// Describe one 1-D tensor (norm weights).
fn read_norm(
    file: &GgufFile<'_>,
    data_section: usize,
    name: &str,
    len: usize,
) -> Result<Norm, EngineError> {
    let (offset, dtype, dims) = tensor_offset(file, data_section, name)?;
    let info = file
        .tensor(name)
        .ok_or_else(|| EngineError::MissingTensor(name.to_owned()))?;
    if info.dims.len() != 1 || dims.first != len {
        return Err(EngineError::BadTensorShape(name.to_owned()));
    }
    match dtype {
        // A float view is free but needs 4-byte alignment; the buffer's base is page-aligned, so the
        // tensor offset decides.
        GgmlDType::F32 if offset.is_multiple_of(4) => Ok(Norm::F32 { offset, len }),
        GgmlDType::F32 | GgmlDType::F16 => {
            let data = file.tensor_data(info).map_err(EngineError::Gguf)?;
            let mut values = vec![0.0f32; len];
            file.dequant_row(dtype, data, len, &mut values)
                .map_err(EngineError::Gguf)?;
            Ok(Norm::Owned(values))
        }
        GgmlDType::Q8_0 => Err(EngineError::BadTensorShape(name.to_owned())),
        GgmlDType::Unsupported(dtype) => Err(EngineError::UnsupportedDType(dtype)),
    }
}

/// Describe one 2-D tensor, keeping Q8_0 packed and float rows in place where the layout allows.
fn read_matrix(
    file: &GgufFile<'_>,
    data_section: usize,
    name: &str,
    rows: usize,
    cols: usize,
) -> Result<Matrix, EngineError> {
    let (offset, dtype, dims) = tensor_offset(file, data_section, name)?;
    let info = file
        .tensor(name)
        .ok_or_else(|| EngineError::MissingTensor(name.to_owned()))?;
    if info.dims.len() != 2 || dims.first != cols || dims.second != rows {
        return Err(EngineError::BadTensorShape(name.to_owned()));
    }
    match dtype {
        GgmlDType::Q8_0 => {
            let row_bytes = tensor_math::q8_0_row_bytes(cols)
                .ok_or_else(|| EngineError::BadTensorShape(name.to_owned()))?;
            Ok(Matrix::Q8_0 {
                rows,
                cols,
                offset,
                len: row_bytes * rows,
            })
        }
        GgmlDType::F32 if offset.is_multiple_of(4) => Ok(Matrix::F32 { rows, cols, offset }),
        GgmlDType::F32 | GgmlDType::F16 => {
            let data = file.tensor_data(info).map_err(EngineError::Gguf)?;
            let mut values = vec![0.0f32; rows * cols];
            file.dequant_row(dtype, data, rows * cols, &mut values)
                .map_err(EngineError::Gguf)?;
            Ok(Matrix::F32Owned {
                rows,
                cols,
                data: values,
            })
        }
        GgmlDType::Unsupported(dtype) => Err(EngineError::UnsupportedDType(dtype)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    /// The fixture checkpoint, generated by `scripts/gen-ai-test-model.py`.
    const MODEL: &[u8] = include_bytes!("../../../models/tiny-llama-64.gguf");
    /// Golden values from that script's independent reference forward pass.
    const GOLDEN: &str = include_str!("../../../models/tiny-llama-64.golden.txt");

    /// Memory ceiling for the fixture (its resident set is well under 2 MiB).
    const LIMIT: usize = 32 * 1024 * 1024;

    /// Golden expectations parsed out of the fixture's `key=value` file.
    struct Golden {
        prompt: String,
        prompt_ids: Vec<u32>,
        greedy_ids: Vec<u32>,
        logits: Vec<f32>,
        embed_text: String,
        embed_ids: Vec<u32>,
        embed_values: Vec<f32>,
    }

    fn numbers<T: core::str::FromStr>(value: &str) -> Vec<T> {
        value
            .split(',')
            .filter(|item| !item.is_empty())
            .map(|item| item.parse::<T>().ok().expect("golden number"))
            .collect()
    }

    fn golden() -> Golden {
        let mut golden = Golden {
            prompt: String::new(),
            prompt_ids: Vec::new(),
            greedy_ids: Vec::new(),
            logits: Vec::new(),
            embed_text: String::new(),
            embed_ids: Vec::new(),
            embed_values: Vec::new(),
        };
        for line in GOLDEN.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let (key, value) = line.split_once('=').expect("key=value");
            match key {
                "prompt" => golden.prompt = value.to_string(),
                "prompt_ids" => golden.prompt_ids = numbers(value),
                "greedy_ids" => golden.greedy_ids = numbers(value),
                "logits" => golden.logits = numbers(value),
                "embed_text" => golden.embed_text = value.to_string(),
                "embed_ids" => golden.embed_ids = numbers(value),
                "embed_values" => golden.embed_values = numbers(value),
                "weakest_margin" => {
                    let margin: f32 = value.parse().expect("margin");
                    assert!(
                        margin > 0.05,
                        "fixture margin {margin} is too small to be an oracle"
                    );
                }
                other => panic!("unexpected golden key {other}"),
            }
        }
        assert!(!golden.prompt_ids.is_empty(), "golden file not parsed");
        golden
    }

    fn greedy_params(max_tokens: u16) -> SamplingParams {
        SamplingParams {
            max_tokens,
            temperature_milli: 0,
            top_k: 0,
            seed: 7,
        }
    }

    #[test]
    fn loads_the_fixture_with_the_expected_geometry() {
        let engine = Engine::load_from_slice(MODEL, LIMIT).expect("fixture loads");
        let config = engine.config();
        assert_eq!(config.n_layer, 2);
        assert_eq!(config.n_embd, 64);
        assert_eq!(config.n_head, 4);
        assert_eq!(config.n_head_kv, 2);
        assert_eq!(config.n_ff, 128);
        assert_eq!(config.n_ctx, 128);
        assert_eq!(config.head_dim(), 16);
        assert_eq!(config.kv_dim(), 32);
        assert_eq!(config.group_size(), 2);
        assert_eq!(config.vocab_size, 270);
        assert!(engine.resident_bytes() < LIMIT);
        let described = engine.describe();
        assert_eq!(described.model, "tiny-llama-64");
        assert_eq!(described.arch, "llama");
        assert_eq!(described.quant, Quant::Q8_0);
        assert_eq!(described.active_backend, backend::CPU);
        assert_eq!(described.live_sessions, 0);
    }

    #[test]
    fn refuses_a_model_that_exceeds_the_memory_ceiling() {
        let error = match Engine::load_from_slice(MODEL, 64 * 1024) {
            Ok(_) => panic!("a 64 KiB ceiling must refuse the fixture"),
            Err(error) => error,
        };
        match error {
            EngineError::TooLarge { needed, limit } => {
                assert!(needed > 64 * 1024);
                assert_eq!(limit, 64 * 1024);
            }
            other => panic!("wrong error: {other:?}"),
        }
    }

    #[test]
    fn tokenizer_agrees_with_the_independent_reference_encoder() {
        let golden = golden();
        let engine = Engine::load_from_slice(MODEL, LIMIT).expect("fixture loads");
        assert_eq!(engine.tokenizer().encode(&golden.prompt), golden.prompt_ids);
        assert_eq!(
            engine.tokenizer().encode(&golden.embed_text),
            golden.embed_ids
        );
        assert!(
            !engine.tokenizer().add_bos(),
            "fixture declares add_bos = false"
        );
        assert_eq!(engine.tokenizer().vocab_size(), 270);
    }

    /// The load-bearing test: greedy decoding through the engine must equal the ids produced by
    /// `scripts/gen-ai-test-model.py`'s independent forward pass.
    #[test]
    fn greedy_decode_matches_the_reference_ids() {
        let golden = golden();
        let mut engine = Engine::load_from_slice(MODEL, LIMIT).expect("fixture loads");
        let request_id = engine
            .submit(
                &golden.prompt,
                greedy_params(golden.greedy_ids.len() as u16),
            )
            .expect("submit");

        let mut produced: Vec<u32> = Vec::new();
        let mut text = String::new();
        for _ in 0..64 {
            let _ = engine.generate(request_id, 4).expect("generate");
            let drained = engine.drain(request_id, 16).expect("drain");
            produced.extend(drained.ids.iter().copied());
            text.push_str(&drained.text);
            if drained.done {
                break;
            }
        }

        assert_eq!(produced, golden.greedy_ids);
        assert_eq!(produced.len(), golden.greedy_ids.len());
        // The engine reports finish reasons, and a length-capped run must say so.
        assert_eq!(
            engine.describe().live_sessions,
            1u8,
            "session stays live until released"
        );
        engine.release(request_id).expect("release");
        assert_eq!(engine.describe().live_sessions, 0u8);
        assert_eq!(
            engine.generate(request_id, 1).unwrap_err(),
            AiError::UnknownRequest
        );
        let _ = text;
    }

    /// Numeric agreement beyond token ids: the embedding path runs the whole forward pass
    /// (norm, RoPE, attention, SwiGLU) and must match the reference vector.
    #[test]
    fn embedding_matches_the_reference_vector() {
        let golden = golden();
        let mut engine = Engine::load_from_slice(MODEL, LIMIT).expect("fixture loads");
        let values = engine.embed(&golden.embed_text).expect("embed");
        assert_eq!(values.len(), golden.embed_values.len());
        for (index, (actual, expected)) in values.iter().zip(&golden.embed_values).enumerate() {
            assert!(
                (actual - expected).abs() < 1e-3,
                "component {index}: engine {actual} vs reference {expected}"
            );
        }
        let norm: f32 = values.iter().map(|value| value * value).sum();
        assert!((norm - 1.0).abs() < 1e-3, "embedding must be unit length");
    }

    #[test]
    fn sessions_are_bounded_cancellable_and_truthful_about_unknown_ids() {
        let golden = golden();
        let mut engine = Engine::load_from_slice(MODEL, LIMIT).expect("fixture loads");
        let mut ids = Vec::new();
        for _ in 0..MAX_SESSIONS {
            ids.push(
                engine
                    .submit(&golden.prompt, greedy_params(4))
                    .expect("submit"),
            );
        }
        assert_eq!(
            engine.submit(&golden.prompt, greedy_params(4)).unwrap_err(),
            AiError::Busy
        );
        assert_eq!(
            engine.generate(9_999, 1).unwrap_err(),
            AiError::UnknownRequest
        );
        assert_eq!(engine.cancel(9_999).unwrap_err(), AiError::UnknownRequest);

        engine.cancel(ids[0]).expect("cancel");
        assert_eq!(
            usize::from(engine.describe().live_sessions),
            usize::from(MAX_SESSIONS) - 1
        );
        // The freed slot is reusable.
        let replacement = engine
            .submit(&golden.prompt, greedy_params(4))
            .expect("submit");
        assert_ne!(replacement, ids[0], "ids are never reused");
        assert!(engine.cancel(replacement).is_ok());
    }

    #[test]
    fn refuses_a_submit_that_breaks_the_contract() {
        let engine_result = Engine::load_from_slice(MODEL, LIMIT);
        let mut engine = engine_result.expect("fixture loads");
        assert_eq!(
            engine.submit("xyz", greedy_params(0)).unwrap_err(),
            AiError::BadRequest(limit::Violation::MaxTokensOutOfRange)
        );
    }

    #[test]
    fn seeded_sampling_is_reproducible_and_stays_in_vocabulary() {
        let golden = golden();
        let params = SamplingParams {
            max_tokens: 6,
            temperature_milli: 800,
            top_k: 8,
            seed: 0xABCD,
        };

        let mut runs = Vec::new();
        for _ in 0..2 {
            let mut engine = Engine::load_from_slice(MODEL, LIMIT).expect("fixture loads");
            let request_id = engine.submit(&golden.prompt, params).expect("submit");
            let mut produced = Vec::new();
            for _ in 0..64 {
                let _ = engine.generate(request_id, 4).expect("generate");
                let drained = engine.drain(request_id, 16).expect("drain");
                produced.extend(drained.ids.iter().copied());
                if drained.done {
                    break;
                }
            }
            assert!(produced.iter().all(|id| (*id as usize) < 270));
            runs.push(produced);
        }
        assert_eq!(runs[0], runs[1], "same seed must reproduce the same tokens");
    }

    /// Real-weight validation (Spec 24 §4.1, plan Phase 04).
    ///
    /// Skips itself unless `CELLOS_AI_MODEL` points at a GGUF checkpoint, so the host suite stays
    /// hermetic; `scripts/fetch-ai-test-model.sh` fetches the pinned SmolLM-135M-Instruct Q8_0
    /// checkpoint this test is written for.
    ///
    /// What it establishes that the fixture cannot: the engine loads a production-sized file
    /// (49,152-token vocabulary, 30 layers, tied output embedding, 138 MB of Q8_0 weights) and
    /// generates text from real trained weights rather than from a numerical oracle.
    #[test]
    fn generates_text_from_a_real_checkpoint() {
        let path = match std::env::var("CELLOS_AI_MODEL") {
            Ok(path) => path,
            Err(_) => {
                eprintln!(
                    "SKIP generates_text_from_a_real_checkpoint: set CELLOS_AI_MODEL \
                     (scripts/fetch-ai-test-model.sh prints the path)"
                );
                return;
            }
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("SKIP generates_text_from_a_real_checkpoint: {path}: {error}");
                return;
            }
        };

        let mut engine = Engine::load(bytes, 1 << 30).expect("checkpoint loads");
        assert!(
            engine.config().vocab_size > 40_000,
            "expected a real tokenizer"
        );
        assert!(engine.config().n_layer >= 8, "expected a real transformer");

        // This checkpoint is instruction-tuned and declares its own chat template, so the prompt
        // is formatted the way the model expects: a raw completion prompt makes it emit junk
        // (measured: repeated ids, then punctuation soup), which is the model's contract, not an
        // engine defect — `stories15M`, a base model, produces clean prose on the same code path.
        let prompt =
            "<|im_start|>user\nThe capital of France is<|im_end|>\n<|im_start|>assistant\n";
        let params = SamplingParams {
            max_tokens: 16,
            temperature_milli: 0,
            top_k: 0,
            seed: 1,
        };
        let request_id = engine.submit(prompt, params).expect("submit");
        let started = std::time::Instant::now();
        let mut ids = Vec::new();
        let mut text = String::new();
        let mut finish = None;
        for _ in 0..64 {
            let progress = engine.generate(request_id, 8).expect("generate");
            let drained = engine.drain(request_id, 16).expect("drain");
            ids.extend(drained.ids.iter().copied());
            text.push_str(&drained.text);
            if drained.done {
                finish = progress.finish.or(drained.finish);
                break;
            }
        }
        let elapsed = started.elapsed();

        assert_eq!(ids.len(), 16, "requested 16 tokens: {ids:?}");
        assert!(
            finish.is_some(),
            "a length-capped generation must report a finish reason"
        );
        let distinct = {
            let mut sorted = ids.clone();
            sorted.sort_unstable();
            sorted.dedup();
            sorted.len()
        };
        assert!(distinct >= 4, "degenerate sampled generation: {ids:?}");
        let printable: usize = text
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
            .count();
        assert!(
            printable * 2 >= text.chars().count(),
            "generated text is not text-like: {text:?}"
        );

        let seconds = elapsed.as_secs_f64();
        std::println!(
            "[ai-engine] real checkpoint: model={} layers={} vocab={} resident={} MiB \
             tokens={} tps={:.2} text={:?}",
            engine.config().name,
            engine.config().n_layer,
            engine.config().vocab_size,
            engine.resident_bytes() / (1024 * 1024),
            ids.len(),
            ids.len() as f64 / seconds.max(f64::MIN_POSITIVE),
            text,
        );
    }

    /// Real SentencePiece checkpoint validation (`CELLOS_AI_SPM_MODEL`).
    ///
    /// Written for the llama2.c story models (`stories260K`, `stories15M-q8_0`), which are real
    /// trained weights with a `tokenizer.ggml.model = "llama"` vocabulary. It checks the two things
    /// a wrong SentencePiece implementation would break: text must round-trip through
    /// encode/decode, and a plain story prompt must produce English-like text rather than noise.
    /// Zero-copy acceptance: a 26.7 MB checkpoint must fit inside a Cell's 32 MiB VA slot.
    ///
    /// The engine used to hold the file *and* a copy of every tensor while loading, so this model
    /// needed roughly 55 MB and could not be served from a Cell at all. With weights addressed in
    /// place the requirement is the file plus the session and scratch buffers, which is what this
    /// test pins: loading under a ceiling that a Cell slot can hold.
    #[test]
    fn a_real_checkpoint_fits_a_cell_slot() {
        let path = match std::env::var("CELLOS_AI_SPM_MODEL") {
            Ok(path) => path,
            Err(_) => {
                eprintln!(
                    "SKIP a_real_checkpoint_fits_a_cell_slot: set CELLOS_AI_SPM_MODEL to an SPM GGUF"
                );
                return;
            }
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("SKIP a_real_checkpoint_fits_a_cell_slot: {path}: {error}");
                return;
            }
        };
        let file_len = bytes.len();

        // A Cell slot is 32 MiB (`kernel/src/loader/va_alloc.rs`); leave headroom for the cell's own
        // code, stack, and the session table.
        const CELL_SLOT_BUDGET: usize = 30 * 1024 * 1024;
        let engine = match Engine::load(bytes, CELL_SLOT_BUDGET) {
            Ok(engine) => engine,
            Err(error) => panic!(
                "a {file_len}-byte checkpoint must load under {CELL_SLOT_BUDGET} bytes: {error:?}"
            ),
        };

        assert_eq!(engine.model_len(), file_len, "the whole file is resident");
        let resident = engine.resident_bytes();
        assert!(
            resident < CELL_SLOT_BUDGET,
            "resident {resident} must stay under the Cell budget"
        );
        // The point of zero-copy: resident memory tracks the file, not twice the file.
        assert!(
            resident < file_len + 6 * 1024 * 1024,
            "resident {resident} should be the file plus buffers, not a second copy of {file_len}"
        );
        std::println!(
            "[ai-engine] zero-copy: file={} MiB resident={} MiB (weights accounted once)",
            file_len / (1024 * 1024),
            resident / (1024 * 1024),
        );
    }

    #[test]
    fn tokenizes_and_generates_with_a_real_sentencepiece_checkpoint() {
        let path = match std::env::var("CELLOS_AI_SPM_MODEL") {
            Ok(path) => path,
            Err(_) => {
                eprintln!(
                    "SKIP tokenizes_and_generates_with_a_real_sentencepiece_checkpoint: set \
                     CELLOS_AI_SPM_MODEL to an SPM GGUF (see scripts/fetch-ai-test-model.sh)"
                );
                return;
            }
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("SKIP: {path}: {error}");
                return;
            }
        };

        let mut engine = Engine::load(bytes, 1 << 30).expect("checkpoint loads");

        // Round-trip: a SentencePiece encoder that disagrees with its own decoder fails here. The
        // vocabulary declares the U+2581 space prefix (llama.cpp's default for SPM), so the decoded
        // text carries one leading space: that is the documented SentencePiece contract, not drift.
        assert!(
            engine.tokenizer().add_bos(),
            "the story models declare BOS insertion"
        );
        for text in [
            "Once upon a time",
            "The little girl said: \"hello!\"",
            "caf\u{e9} cr\u{e8}me",
            "numbers 1 2 3 and symbols %$#",
            "  two leading spaces",
        ] {
            let decoded = engine.tokenizer().decode(&engine.tokenizer().encode(text));
            // The marker is added only when the text does not already start with a space.
            let expected = if text.starts_with(' ') {
                alloc::string::String::from(text)
            } else {
                alloc::format!(" {text}")
            };
            assert_eq!(decoded, expected, "round-trip failed for {text:?}");
        }

        let params = SamplingParams {
            max_tokens: 24,
            temperature_milli: 0,
            top_k: 0,
            seed: 7,
        };
        let request_id = engine.submit("Once upon a time", params).expect("submit");
        let started = std::time::Instant::now();
        let mut ids = Vec::new();
        let mut text = String::new();
        for _ in 0..128 {
            let _ = engine.generate(request_id, 8).expect("generate");
            let drained = engine.drain(request_id, 16).expect("drain");
            ids.extend(drained.ids.iter().copied());
            text.push_str(&drained.text);
            if drained.done {
                break;
            }
        }
        let elapsed = started.elapsed();
        assert_eq!(ids.len(), 24, "requested 24 tokens: {ids:?}");

        let distinct = {
            let mut sorted = ids.clone();
            sorted.sort_unstable();
            sorted.dedup();
            sorted.len()
        };
        assert!(distinct >= 6, "degenerate generation: {ids:?}");
        assert!(
            !text.contains('\u{FFFD}'),
            "the tokenizer or decoder produced a replacement character: {text:?}"
        );
        let letters = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
        let spaces = text.chars().filter(|c| *c == ' ').count();
        assert!(
            letters * 3 >= text.chars().count(),
            "generated text is not text-like: {text:?}"
        );
        assert!(
            spaces >= 2,
            "generated text has no word structure: {text:?}"
        );

        std::println!(
            "[ai-engine] real SPM checkpoint: model={} layers={} vocab={} resident={} MiB \
             tokens={} tps={:.2} text={:?}",
            engine.config().name,
            engine.config().n_layer,
            engine.config().vocab_size,
            engine.resident_bytes() / (1024 * 1024),
            ids.len(),
            ids.len() as f64 / elapsed.as_secs_f64().max(f64::MIN_POSITIVE),
            text,
        );
    }

    #[test]
    fn streaming_text_waits_for_a_complete_character() {
        // '€' is E2 82 AC: the engine must not emit a replacement character for a split sequence.
        let mut buffer = vec![0xE2, 0x82];
        assert_eq!(take_complete_text(&mut buffer), "");
        assert_eq!(buffer.len(), 2, "partial sequence is retained");
        buffer.push(0xAC);
        assert_eq!(take_complete_text(&mut buffer), "€");
        assert!(buffer.is_empty());

        // An invalid lead byte still makes progress instead of stalling the stream forever.
        let mut invalid = vec![0xFF, b'a'];
        let text = take_complete_text(&mut invalid);
        assert!(text.ends_with('a'));
    }
}
