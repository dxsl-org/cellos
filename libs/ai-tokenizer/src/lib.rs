//! Byte-level BPE tokenizer (GPT-2 family) built from GGUF metadata.
//!
//! A [`Tokenizer`] is built once from a parsed [`GgufFile`] and then owns everything it needs:
//! the vocabulary strings, the merge table and the byte-level alphabet. The GGUF file it was
//! built from is not referenced afterwards, so the engine may drop or move the model buffer
//! while a tokenizer is alive.
//!
//! # Invariants
//!
//! * Construction succeeds only when `tokenizer.ggml.model` is `"gpt2"`; every other model
//!   (notably `"llama"` SentencePiece) is refused with [`TokenizerError::UnsupportedModel`]
//!   instead of being misread as byte-level BPE.
//! * Every id returned by [`Tokenizer::encode`] is a valid index into `tokenizer.ggml.tokens`,
//!   and `tok.decode(&tok.encode(text)) == text` whenever the vocabulary contains byte-level
//!   tokens for every byte of `text` (which a GPT-2 vocabulary always does).
//! * No input, however malformed or hostile, panics: failures are [`TokenizerError`] values,
//!   out-of-range ids are skipped, bytes without a vocabulary token are skipped, and decoding
//!   rejoins incomplete UTF-8 sequences lossily.
//!
//! # Memory
//!
//! The tables are owned, so a tokenizer costs roughly `len(vocab) + len(merges)` times a small
//! constant: one `String` per vocabulary entry, one `u32` per vocabulary entry for the reverse
//! index (the token strings are not duplicated to build it), and one map node per merge. For a
//! 50k-token GPT-2 vocabulary with ~50k merges that is a few megabytes; [`Tokenizer::bytes`]
//! reports the estimate and [`Tokenizer::merge_count`] the merge count, both of which the engine
//! adds to its `resident_bytes()`. Sharing the tokenizer instead of cloning it is the intended way
//! to avoid paying that cost twice.
//!
//! # Byte-level alphabet
//!
//! Text is treated as bytes. Each byte maps to one `char` through the canonical GPT-2
//! `bytes_to_unicode` table: the printable ASCII range `0x21..=0x7E`, `0xA1..=0xAC` and
//! `0xAE..=0xFF` map to themselves, and the remaining 68 bytes (`0x00..=0x20`, `0x7F..=0xA0` and
//! `0xAD`) map to U+0100..=U+0143 in increasing byte order. Vocabulary entries are strings of
//! mapped characters (so a space in text is the single char `Ġ`, U+0120); [`Tokenizer::token_bytes`]
//! and [`Tokenizer::decode`] apply the inverse mapping.
//!
//! # Pre-tokenization
//!
//! Chunks are produced left to right with the same alternatives, in the same order, as the
//! reference GPT-2 regex
//!
//! ```text
//! 's|'t|'re|'ve|'m|'ll|'d| ?\p{L}+| ?\p{N}+| ?[^\s\p{L}\p{N}]+|\s+(?!\S)|\s+
//! ```
//!
//! so contractions, an optional literal space plus a run of letters, a run of digits, a run of
//! anything else that is not whitespace, and whitespace runs are grouped as in GPT-2. Two
//! classes come from `core` instead of a regex engine, and that is the only divergence:
//!
//! * `char::is_alphabetic` is the Unicode `Alphabetic` property, which also holds for
//!   `Other_Alphabetic` marks (e.g. U+0345 COMBINING GREEK YPOGEGRAMMENI) and for `Nl` letter
//!   numbers (e.g. U+2167 ROMAN NUMERAL EIGHT) that are not `\p{L}`. Such a code point groups
//!   with letters here, and with the `other` or `digit` alternative in the reference regex.
//! * `char::is_numeric` is exactly `Nd | Nl | No`, i.e. `\p{N}`, and `char::is_whitespace` is
//!   exactly `White_Space`, i.e. `\s`, so those two classes do not diverge.
//!
//! The whitespace alternative (`\s+(?!\S)|\s+`) is emulated exactly rather than approximated: a
//! whitespace run followed by a non-whitespace character yields a chunk with everything but the
//! final whitespace character, and that final character either becomes the optional leading space
//! of the following group (when it is a literal U+0020 space) or a chunk of its own. A whitespace
//! run at the end of the text is one chunk.
//!
//! # Merges
//!
//! Each chunk starts as one symbol per byte of its UTF-8 encoding, and byte symbols are merged by
//! rank: the lowest-ranked adjacent pair is merged everywhere it occurs, and the scan repeats
//! until no recorded merge applies. A merge is only usable when both operands and the
//! concatenated result are in the vocabulary; unusable merges are ignored, and a pair recorded
//! twice is refused as [`TokenizerError::DuplicateMerge`] because its rank would be ambiguous.
//!
//! # Unknown symbols
//!
//! The byte-level alphabet makes one symbol per byte, so the constituent byte tokens of a symbol
//! are the symbol itself: a byte whose token string is absent from the vocabulary (a vocabulary
//! that is not byte-complete) is skipped instead of panicking, and a character of a vocabulary
//! entry that is not in the byte-level alphabet contributes no byte to decoding.

#![cfg_attr(not(test), no_std)]
// The crate contract requires a doc comment on every public item; enforce it mechanically.
#![deny(missing_docs)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::size_of;

use gguf_rs::{GgufError, GgufFile};

/// Number of symbols in the byte-level alphabet.
const BYTE_ALPHABET: usize = 256;

/// Per-merge slack added by [`Tokenizer::bytes`] for the merge map's node overhead.
const MAP_ENTRY_SLACK: usize = 16;

/// First code point assigned to bytes that are not printable ASCII or Latin-1.
const FIRST_EXTRA_CODE_POINT: u32 = 0x100;

/// Last code point assigned by the canonical table (68 extra code points).
const LAST_EXTRA_CODE_POINT: u32 = 0x143;

/// GGUF key holding the tokenizer family name.
const KEY_MODEL: &str = "tokenizer.ggml.model";

/// GGUF key holding the vocabulary (one string per token id).
const KEY_TOKENS: &str = "tokenizer.ggml.tokens";

/// GGUF key holding the `"left right"` merge list, lowest rank first.
const KEY_MERGES: &str = "tokenizer.ggml.merges";

/// GGUF key holding the beginning-of-sequence token id.
const KEY_BOS: &str = "tokenizer.ggml.bos_token_id";

/// GGUF key holding the end-of-sequence token id.
const KEY_EOS: &str = "tokenizer.ggml.eos_token_id";

/// GGUF key holding the model's BOS insertion policy.
const KEY_ADD_BOS: &str = "tokenizer.ggml.add_bos_token";

/// GGUF key holding the model's EOS insertion policy.
const KEY_ADD_EOS: &str = "tokenizer.ggml.add_eos_token";

/// The only `tokenizer.ggml.model` value this crate implements.
const MODEL_GPT2: &str = "gpt2";

/// Replacement code point for the (unreachable) case of an invalid code point in the table.
const REPLACEMENT: char = '\u{FFFD}';

/// Errors a [`Tokenizer`] can be built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenizerError {
    /// `tokenizer.ggml.model` is absent or is not a byte-level BPE model (`"gpt2"`).
    UnsupportedModel,
    /// `tokenizer.ggml.tokens` is absent.
    MissingTokens,
    /// `tokenizer.ggml.merges` is absent.
    MissingMerges,
    /// The same merge pair is listed twice, which would give it two ranks.
    DuplicateMerge,
    /// A token id in the metadata points outside the token array.
    BadSpecialToken,
    /// The GGUF file could not be read.
    Gguf(GgufError),
}

impl From<GgufError> for TokenizerError {
    fn from(error: GgufError) -> Self {
        TokenizerError::Gguf(error)
    }
}

/// Byte-level BPE tokenizer with owned tables.
pub struct Tokenizer {
    /// Token strings indexed by token id.
    tokens: Vec<String>,
    /// Token indices sorted by `(token string, id)`, the reverse index used for symbol lookup.
    /// Keeping one `u32` per token instead of a second copy of every token string is what makes
    /// the owned tables affordable for a 50k-token vocabulary.
    order: Vec<u32>,
    /// Adjacent symbol pair to `(rank, merged token id)`, where `None` marks a merge whose
    /// concatenated result is not a vocabulary token (recorded so that a duplicate pair is still
    /// detected, but never applied).
    merges: BTreeMap<(u32, u32), (u32, Option<u32>)>,
    /// Canonical byte to code point table.
    byte_to_char: [char; BYTE_ALPHABET],
    /// Beginning-of-sequence token id, when the model declares one.
    bos: Option<u32>,
    /// End-of-sequence token id, when the model declares one.
    eos: Option<u32>,
    /// Whether the model declares that BOS should be inserted before the prompt.
    add_bos: bool,
    /// Whether the model declares that EOS should be appended after the completion.
    add_eos: bool,
}

impl Tokenizer {
    /// Build a tokenizer from a parsed GGUF file.
    ///
    /// `tokenizer.ggml.model` must be `"gpt2"`; anything else (notably `"llama"` SentencePiece)
    /// returns [`TokenizerError::UnsupportedModel`] because this engine only supports byte-level
    /// BPE in this slice. All tables are copied out of the file, which is not borrowed afterwards.
    pub fn from_gguf(file: &GgufFile<'_>) -> Result<Self, TokenizerError> {
        match file.metadata_str(KEY_MODEL) {
            Some(MODEL_GPT2) => {}
            _ => return Err(TokenizerError::UnsupportedModel),
        }
        let tokens = file
            .metadata_str_array(KEY_TOKENS)
            .ok_or(TokenizerError::MissingTokens)?;
        let merges = file
            .metadata_str_array(KEY_MERGES)
            .ok_or(TokenizerError::MissingMerges)?;

        let mut owned = Vec::with_capacity(tokens.len());
        for token in tokens.iter() {
            // Token ids are u32 on the wire; entries beyond that are unaddressable and dropped.
            if owned.len() >= u32::MAX as usize {
                break;
            }
            owned.push(String::from(*token));
        }
        let vocab_len = owned.len();
        let mut order: Vec<u32> = (0..vocab_len as u32).collect();
        // Sorted by `(token, id)`, so a token string repeated in the vocabulary resolves to its
        // lowest id.
        order.sort_unstable_by(|&left, &right| {
            let left_token = owned.get(left as usize).map(String::as_str).unwrap_or("");
            let right_token = owned.get(right as usize).map(String::as_str).unwrap_or("");
            left_token.cmp(right_token).then(left.cmp(&right))
        });
        let mut table: BTreeMap<(u32, u32), (u32, Option<u32>)> = BTreeMap::new();
        for (rank, merge) in merges.iter().enumerate() {
            // A merge is recorded as `"left right"`; mapped symbols never contain a literal
            // space (U+0020 maps to `Ġ`), so the split is unambiguous. Entries without a
            // separator are malformed and cannot apply to any symbol.
            let Some((left, right)) = merge.split_once(' ') else {
                continue;
            };
            let (Some(first), Some(second)) = (
                lookup(&order, &owned, left),
                lookup(&order, &owned, right),
            ) else {
                // A symbol the vocabulary does not define can never be produced.
                continue;
            };
            let rank = u32::try_from(rank).unwrap_or(u32::MAX);
            if table.contains_key(&(first, second)) {
                return Err(TokenizerError::DuplicateMerge);
            }
            let mut merged = String::with_capacity(left.len() + right.len());
            merged.push_str(left);
            merged.push_str(right);
            // A merge whose result is not in the vocabulary can never be emitted.
            let result = lookup(&order, &owned, merged.as_str());
            table.insert((first, second), (rank, result));
        }

        let bos = match file.metadata_u32(KEY_BOS) {
            Some(id) if (id as usize) < vocab_len => Some(id),
            Some(_) => return Err(TokenizerError::BadSpecialToken),
            None => None,
        };
        let eos = match file.metadata_u32(KEY_EOS) {
            Some(id) if (id as usize) < vocab_len => Some(id),
            Some(_) => return Err(TokenizerError::BadSpecialToken),
            None => None,
        };

        Ok(Tokenizer {
            tokens: owned,
            order,
            merges: table,
            byte_to_char: canonical_byte_to_unicode(),
            bos,
            eos,
            add_bos: file.metadata_bool(KEY_ADD_BOS).unwrap_or(false),
            add_eos: file.metadata_bool(KEY_ADD_EOS).unwrap_or(false),
        })
    }

    /// Number of tokens in the vocabulary.
    pub fn vocab_size(&self) -> usize {
        self.tokens.len()
    }

    /// Number of merges recorded in the merge table, which is what the engine accounts for.
    ///
    /// Entries the merge list cannot use are not recorded: a malformed entry without a separating
    /// space, and a merge naming a symbol or producing a result the vocabulary does not define.
    /// A merge whose result is missing is recorded (so a duplicate pair is still detected) and
    /// therefore counted.
    pub fn merge_count(&self) -> usize {
        self.merges.len()
    }

    /// Estimated heap memory held by the owned tables, in bytes.
    ///
    /// The engine adds this to its `resident_bytes()`. It is an estimate: it counts the vocabulary
    /// storage (string headers and string bytes), the reverse index and the merge table plus a
    /// fixed slack per merge for the map's node overhead, and deliberately excludes allocator
    /// bookkeeping and the `Tokenizer` struct itself.
    pub fn bytes(&self) -> usize {
        let strings = self.tokens.capacity() * size_of::<String>()
            + self.tokens.iter().map(String::capacity).sum::<usize>();
        let index = self.order.capacity() * size_of::<u32>();
        let merges = self.merges.len()
            * (size_of::<((u32, u32), (u32, Option<u32>))>() + MAP_ENTRY_SLACK);
        strings + index + merges
    }

    /// Beginning-of-sequence token id, when the model declares one.
    pub fn bos_id(&self) -> Option<u32> {
        self.bos
    }

    /// End-of-sequence token id, when the model declares one.
    pub fn eos_id(&self) -> Option<u32> {
        self.eos
    }

    /// Whether the model declares that BOS belongs before a prompt
    /// (`tokenizer.ggml.add_bos_token`, `false` when absent).
    pub fn add_bos(&self) -> bool {
        self.add_bos
    }

    /// Whether the model declares that EOS belongs after a completion
    /// (`tokenizer.ggml.add_eos_token`, `false` when absent).
    pub fn add_eos(&self) -> bool {
        self.add_eos
    }

    /// Raw token text for one id, or `None` when the id is out of range.
    pub fn token_str(&self, id: u32) -> Option<&str> {
        self.tokens.get(id as usize).map(String::as_str)
    }

    /// Decoded bytes of one token (inverse byte-level mapping applied), `None` when the id is out
    /// of range.
    ///
    /// Characters of the token that are not in the byte-level alphabet (which byte-level
    /// vocabularies never contain) contribute no byte.
    pub fn token_bytes(&self, id: u32) -> Option<Vec<u8>> {
        let text = self.token_str(id)?;
        let mut out = Vec::with_capacity(text.len());
        for ch in text.chars() {
            if let Some(byte) = unicode_to_byte(ch) {
                out.push(byte);
            }
        }
        Some(out)
    }

    /// Encode text into token ids (no BOS/EOS inserted).
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        let mut symbols = Vec::new();
        let mut start = 0;
        while start < chars.len() {
            let end = next_pre_token(&chars, start);
            // `next_pre_token` never returns a non-advancing end; guard anyway so that no input
            // can turn into an infinite loop.
            let end = if end > start { end } else { start + 1 };
            if let Some(chunk) = chars.get(start..end) {
                symbols.clear();
                for &ch in chunk {
                    self.push_symbols(&mut symbols, ch);
                }
                self.merge_symbols(&mut symbols);
                out.append(&mut symbols);
            }
            start = end;
        }
        out
    }

    /// Encode and optionally wrap with BOS/EOS where the model declares them.
    pub fn encode_with_specials(&self, text: &str, add_bos: bool, add_eos: bool) -> Vec<u32> {
        let mut out = Vec::new();
        if add_bos {
            if let Some(bos) = self.bos {
                out.push(bos);
            }
        }
        out.extend(self.encode(text));
        if add_eos {
            if let Some(eos) = self.eos {
                out.push(eos);
            }
        }
        out
    }

    /// Decode ids to bytes; ids out of range are skipped.
    pub fn decode_bytes(&self, ids: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        for &id in ids {
            if let Some(bytes) = self.token_bytes(id) {
                out.extend_from_slice(&bytes);
            }
        }
        out
    }

    /// Decode ids to a `String`, lossily joining partial UTF-8 sequences.
    pub fn decode(&self, ids: &[u32]) -> String {
        String::from_utf8_lossy(&self.decode_bytes(ids)).into_owned()
    }

    /// Append the symbol ids for one input character.
    ///
    /// The symbol for a byte is the byte-level token for that byte; a byte whose token string is
    /// absent from the vocabulary is skipped (its constituent byte token is the symbol itself,
    /// which is precisely what is missing).
    fn push_symbols(&self, out: &mut Vec<u32>, ch: char) {
        let mut utf8 = [0u8; 4];
        for &byte in ch.encode_utf8(&mut utf8).as_bytes() {
            let symbol = self.byte_to_char[byte as usize];
            let mut buf = [0u8; 4];
            let text: &str = symbol.encode_utf8(&mut buf);
            if let Some(id) = lookup(&self.order, &self.tokens, text) {
                out.push(id);
            }
        }
    }

    /// Merge adjacent symbols by rank until no recorded merge applies.
    fn merge_symbols(&self, symbols: &mut Vec<u32>) {
        loop {
            let mut best: Option<((u32, u32), u32, u32)> = None;
            for pair in symbols.windows(2) {
                if let Some(&(rank, Some(merged))) = self.merges.get(&(pair[0], pair[1])) {
                    if best.is_none_or(|(_, best_rank, _)| rank < best_rank) {
                        best = Some(((pair[0], pair[1]), rank, merged));
                    }
                }
            }
            let Some(((first, second), _, merged)) = best else {
                return;
            };
            let len = symbols.len();
            let mut read = 0;
            let mut write = 0;
            while read < len {
                // `write <= read` throughout, so compaction never loses an unread symbol.
                if read + 1 < len && symbols[read] == first && symbols[read + 1] == second {
                    symbols[write] = merged;
                    read += 2;
                } else {
                    symbols[write] = symbols[read];
                    read += 1;
                }
                write += 1;
            }
            symbols.truncate(write);
        }
    }
}

/// Lowest id of `token` in `tokens`, using the `(token, id)`-sorted `order` index.
///
/// `order` must be sorted by `(tokens[index], index)`; a token string that appears more than once
/// in the vocabulary therefore resolves to its lowest id.
fn lookup(order: &[u32], tokens: &[String], token: &str) -> Option<u32> {
    let position = order.partition_point(|&index| {
        tokens
            .get(index as usize)
            .map(String::as_str)
            .unwrap_or("")
            < token
    });
    let index = *order.get(position)?;
    if tokens.get(index as usize).map(String::as_str)? == token {
        Some(index)
    } else {
        None
    }
}

/// Character class of the pre-tokenizer's three run alternatives.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    /// `\p{L}` (approximated by `char::is_alphabetic`).
    Letter,
    /// `\p{N}`.
    Numeric,
    /// `[^\s\p{L}\p{N}]`.
    Other,
    /// `\s`.
    Whitespace,
}

/// Classify one character for the pre-tokenizer.
fn classify(ch: char) -> Class {
    if ch.is_whitespace() {
        Class::Whitespace
    } else if ch.is_alphabetic() {
        Class::Letter
    } else if ch.is_numeric() {
        Class::Numeric
    } else {
        Class::Other
    }
}

/// Length in characters of the GPT-2 contraction starting at `start`, or `None` when there is no
/// `'s`, `'t`, `'re`, `'ve`, `'m`, `'ll` or `'d` there.
fn contraction_len(chars: &[char], start: usize) -> Option<usize> {
    if chars.get(start) != Some(&'\'') {
        return None;
    }
    match *chars.get(start + 1)? {
        's' | 't' | 'm' | 'd' => Some(2),
        'l' => (chars.get(start + 2) == Some(&'l')).then_some(3),
        'r' | 'v' => (chars.get(start + 2) == Some(&'e')).then_some(3),
        _ => None,
    }
}

/// End (exclusive) of the pre-token starting at `start`.
///
/// `start` must be a valid character index. The alternatives are tried in the reference order, and
/// the returned end is always greater than `start` when `start < chars.len()`.
fn next_pre_token(chars: &[char], start: usize) -> usize {
    let len = chars.len();
    let Some(&first) = chars.get(start) else {
        return len;
    };
    if first.is_whitespace() {
        let mut run_end = start;
        while let Some(&ch) = chars.get(run_end) {
            if !ch.is_whitespace() {
                break;
            }
            run_end += 1;
        }
        if run_end == len {
            // `\s+(?!\S)` at the end of the text consumes the whole run.
            return run_end;
        }
        if run_end > start + 1 {
            // `\s+(?!\S)`: everything but the final whitespace character.
            return run_end - 1;
        }
        if first != ' ' {
            // `\s+`: whitespace that cannot be the optional leading space of a group.
            return start + 1;
        }
        // A single literal space followed by a non-whitespace character is absorbed below.
    } else if first == '\'' {
        if let Some(length) = contraction_len(chars, start) {
            return start + length;
        }
    }
    // ` ?\p{L}+` / ` ?\p{N}+` / ` ?[^\s\p{L}\p{N}]+`; a space is only absorbed when a
    // non-whitespace character follows it, so `group_start` is a valid index.
    let group_start = if first == ' ' { start + 1 } else { start };
    let Some(&head) = chars.get(group_start) else {
        return start + 1;
    };
    let class = classify(head);
    let mut end = group_start + 1;
    while let Some(&ch) = chars.get(end) {
        if classify(ch) != class {
            break;
        }
        end += 1;
    }
    end
}

/// Character ranges of the pre-tokens of `chars`, used by `encode` and by the pre-tokenizer tests.
#[cfg(test)]
fn pre_token_ranges(chars: &[char]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = next_pre_token(chars, start);
        let end = if end > start { end } else { start + 1 };
        ranges.push((start, end));
        start = end;
    }
    ranges
}

/// The canonical GPT-2 `bytes_to_unicode` table: byte to code point.
fn canonical_byte_to_unicode() -> [char; BYTE_ALPHABET] {
    let mut table = [REPLACEMENT; BYTE_ALPHABET];
    let mut extra = 0u32;
    for (byte, slot) in table.iter_mut().enumerate() {
        let raw = byte as u32;
        let printable = (0x21..=0x7E).contains(&raw)
            || (0xA1..=0xAC).contains(&raw)
            || (0xAE..=0xFF).contains(&raw);
        if printable {
            *slot = char::from_u32(raw).unwrap_or(REPLACEMENT);
        } else {
            *slot = char::from_u32(FIRST_EXTRA_CODE_POINT + extra).unwrap_or(REPLACEMENT);
            extra += 1;
        }
    }
    table
}

/// Inverse of [`canonical_byte_to_unicode`]: code point to byte, `None` when the code point is not
/// part of the byte-level alphabet.
fn unicode_to_byte(ch: char) -> Option<u8> {
    let code_point = ch as u32;
    match code_point {
        // Printable ASCII and Latin-1 map to themselves.
        0x21..=0x7E | 0xA1..=0xAC | 0xAE..=0xFF => u8::try_from(code_point).ok(),
        // The bytes without a printable representative, in byte order:
        // 0x00..=0x20, then 0x7F..=0xA0, then 0xAD.
        FIRST_EXTRA_CODE_POINT..=LAST_EXTRA_CODE_POINT => {
            let index = code_point - FIRST_EXTRA_CODE_POINT;
            let byte = if index < 0x21 {
                index
            } else if index < 0x21 + 0x22 {
                index - 0x21 + 0x7F
            } else {
                0xAD
            };
            u8::try_from(byte).ok()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// GGUF value type ids on the wire.
    const TYPE_U32: u32 = 4;
    const TYPE_BOOL: u32 = 7;
    const TYPE_STRING: u32 = 8;
    const TYPE_ARRAY: u32 = 9;
    /// `"GGUF"` as a little-endian u32.
    const MAGIC: u32 = 0x4655_4747;

    fn push_string(buf: &mut Vec<u8>, value: &str) {
        buf.extend_from_slice(&(value.len() as u64).to_le_bytes());
        buf.extend_from_slice(value.as_bytes());
    }

    /// Minimal GGUF v3 writer: header plus metadata, no tensors.
    struct GgufImage {
        metadata: Vec<u8>,
        entries: u64,
    }

    impl GgufImage {
        fn new() -> Self {
            Self {
                metadata: Vec::new(),
                entries: 0,
            }
        }

        fn str(&mut self, key: &str, value: &str) -> &mut Self {
            push_string(&mut self.metadata, key);
            self.metadata.extend_from_slice(&TYPE_STRING.to_le_bytes());
            push_string(&mut self.metadata, value);
            self.entries += 1;
            self
        }

        fn u32(&mut self, key: &str, value: u32) -> &mut Self {
            push_string(&mut self.metadata, key);
            self.metadata.extend_from_slice(&TYPE_U32.to_le_bytes());
            self.metadata.extend_from_slice(&value.to_le_bytes());
            self.entries += 1;
            self
        }

        fn bool(&mut self, key: &str, value: bool) -> &mut Self {
            push_string(&mut self.metadata, key);
            self.metadata.extend_from_slice(&TYPE_BOOL.to_le_bytes());
            self.metadata.push(u8::from(value));
            self.entries += 1;
            self
        }

        fn str_array(&mut self, key: &str, values: &[&str]) -> &mut Self {
            push_string(&mut self.metadata, key);
            self.metadata.extend_from_slice(&TYPE_ARRAY.to_le_bytes());
            self.metadata.extend_from_slice(&TYPE_STRING.to_le_bytes());
            self.metadata
                .extend_from_slice(&(values.len() as u64).to_le_bytes());
            for value in values {
                push_string(&mut self.metadata, value);
            }
            self.entries += 1;
            self
        }

        fn finish(&self) -> Vec<u8> {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&MAGIC.to_le_bytes());
            bytes.extend_from_slice(&3u32.to_le_bytes());
            bytes.extend_from_slice(&0u64.to_le_bytes()); // tensor count
            bytes.extend_from_slice(&self.entries.to_le_bytes());
            bytes.extend_from_slice(&self.metadata);
            while bytes.len() % 32 != 0 {
                bytes.push(0);
            }
            bytes
        }
    }

    /// Byte-level encoded form of a byte sequence, e.g. `mapped(b" w") == "Ġw"`.
    fn mapped(bytes: &[u8]) -> String {
        let table = canonical_byte_to_unicode();
        bytes.iter().map(|&b| table[b as usize]).collect()
    }

    /// Hand-written vocabulary: the 256 byte tokens first (ids 0..=255), then merge results.
    struct TestVocab {
        tokens: Vec<String>,
        merges: Vec<String>,
    }

    impl TestVocab {
        /// All byte tokens plus one harmless merge, so that a test vocabulary is never empty.
        fn new() -> Self {
            let table = canonical_byte_to_unicode();
            let mut vocab = Self {
                tokens: table.iter().map(char::to_string).collect(),
                merges: Vec::new(),
            };
            vocab.merge(&mapped(b"\x00"), &mapped(b"\x00"));
            vocab
        }

        /// Append `left` + `right` as the next-ranked merge and return the merged token id.
        fn merge(&mut self, left: &str, right: &str) -> u32 {
            let merged = format!("{left}{right}");
            assert!(
                self.tokens.iter().all(|token| token != &merged),
                "token {merged:?} already present"
            );
            self.tokens.push(merged);
            self.merges.push(format!("{left} {right}"));
            (self.tokens.len() - 1) as u32
        }

        /// Id of a token string.
        fn id_of(&self, token: &str) -> u32 {
            self.tokens
                .iter()
                .position(|candidate| candidate == token)
                .map(|index| index as u32)
                .expect("token is in the vocabulary")
        }

        fn image(&self, model: Option<&str>, bos: Option<u32>, eos: Option<u32>) -> Vec<u8> {
            let token_refs: Vec<&str> = self.tokens.iter().map(String::as_str).collect();
            let merge_refs: Vec<&str> = self.merges.iter().map(String::as_str).collect();
            let mut image = GgufImage::new();
            if let Some(model) = model {
                image.str(KEY_MODEL, model);
            }
            image.str_array(KEY_TOKENS, &token_refs);
            image.str_array(KEY_MERGES, &merge_refs);
            if let Some(bos) = bos {
                image.u32(KEY_BOS, bos);
            }
            if let Some(eos) = eos {
                image.u32(KEY_EOS, eos);
            }
            image.finish()
        }
    }

    /// The vocabulary shared by the round-trip and merge tests, built the way a GPT-2 merge list
    /// builds tokens: byte symbols first, then rank-ordered merges.
    fn vocabulary() -> TestVocab {
        let mut vocab = TestVocab::new();
        vocab.merge(&mapped(b"h"), &mapped(b"e")); // he
        vocab.merge(&mapped(b"l"), &mapped(b"l")); // ll
        vocab.merge(&mapped(b"he"), &mapped(b"ll")); // hell
        vocab.merge(&mapped(b"hell"), &mapped(b"o")); // hello
        vocab.merge(&mapped(b" "), &mapped(b"w")); // Ġw
        vocab.merge(&mapped(b" w"), &mapped(b"o")); // Ġwo
        vocab.merge(&mapped(b" wo"), &mapped(b"r")); // Ġwor
        vocab.merge(&mapped(b" wor"), &mapped(b"l")); // Ġworl
        vocab.merge(&mapped(b" worl"), &mapped(b"d")); // Ġworld
        vocab.merge(&mapped(b"\xC3"), &mapped(b"\xA9")); // Ãŋ, the two bytes of "é"
        vocab
    }

    /// Build a tokenizer from a hand-built image and run `body` against it.
    fn with_tokenizer<R>(image: &[u8], body: impl FnOnce(&Tokenizer) -> R) -> R {
        let file = GgufFile::parse(image).expect("hand-built GGUF image parses");
        let tokenizer = Tokenizer::from_gguf(&file).expect("tokenizer builds");
        body(&tokenizer)
    }

    /// Build a tokenizer from a hand-built image, expecting failure.
    fn build_error(image: &[u8]) -> TokenizerError {
        let file = GgufFile::parse(image).expect("hand-built GGUF image parses");
        match Tokenizer::from_gguf(&file) {
            Ok(_) => panic!("expected the tokenizer to be refused"),
            Err(error) => error,
        }
    }

    #[test]
    fn byte_level_table_is_a_bijection() {
        let table = canonical_byte_to_unicode();
        // Anchors: printable ASCII and Latin-1 map to themselves, the rest is handed out in
        // byte order after U+0100.
        assert_eq!(table[0x00], '\u{0100}');
        assert_eq!(table[0x09], '\u{0109}');
        assert_eq!(table[0x0A], '\u{010A}');
        assert_eq!(table[0x20], '\u{0120}'); // Ġ, the byte-level space
        assert_eq!(table[0x21], '!');
        assert_eq!(table[0x7E], '~');
        assert_eq!(table[0x7F], '\u{0121}'); // ġ
        assert_eq!(table[0xA0], '\u{0142}');
        assert_eq!(table[0xA1], '¡');
        assert_eq!(table[0xAC], '¬');
        assert_eq!(table[0xAD], '\u{0143}');
        assert_eq!(table[0xAE], '®');
        assert_eq!(table[0xC3], 'Ã');
        assert_eq!(table[0xFF], 'ÿ');

        let mut seen = BTreeSet::new();
        for byte in 0..BYTE_ALPHABET {
            assert!(seen.insert(table[byte]), "duplicate mapping for {byte}");
            assert_eq!(unicode_to_byte(table[byte]), Some(byte as u8));
        }
        assert_eq!(seen.len(), BYTE_ALPHABET);
        // Code points outside the alphabet have no byte.
        assert_eq!(unicode_to_byte(' '), None);
        assert_eq!(unicode_to_byte('中'), None);
        assert_eq!(unicode_to_byte('\u{0144}'), None);
        assert_eq!(unicode_to_byte('\u{00}'), None);
    }

    #[test]
    fn pre_tokenizer_matches_the_gpt2_grouping() {
        let chunks = |text: &str| -> Vec<String> {
            let chars: Vec<char> = text.chars().collect();
            pre_token_ranges(&chars)
                .into_iter()
                .map(|(start, end)| chars[start..end].iter().collect())
                .collect()
        };
        assert_eq!(chunks("hello world"), ["hello", " world"]);
        assert_eq!(chunks("don't"), ["don", "'t"]);
        assert_eq!(chunks("we're"), ["we", "'re"]);
        assert_eq!(chunks("I'd"), ["I", "'d"]);
        assert_eq!(chunks("y'all"), ["y", "'", "all"]);
        assert_eq!(chunks("a  b"), ["a", " ", " b"]);
        assert_eq!(chunks("  leading"), [" ", " leading"]);
        assert_eq!(chunks("trailing   "), ["trailing", "   "]);
        assert_eq!(chunks("\t\ta"), ["\t", "\t", "a"]);
        assert_eq!(chunks(" 's"), [" '", "s"]);
        assert_eq!(chunks("1234 x"), ["1234", " x"]);
        assert_eq!(chunks("hi!!"), ["hi", "!!"]);
        assert_eq!(chunks(""), Vec::<String>::new());
    }

    #[test]
    fn round_trips_ascii() {
        let vocab = vocabulary();
        let image = vocab.image(Some(MODEL_GPT2), Some(1), Some(2));
        with_tokenizer(&image, |tokenizer| {
            for text in [
                "",
                "hello",
                "hello world",
                "Hello, World!",
                "GPT-2 tokenizer",
                "it's 100% fine",
                "\u{0}",
            ] {
                assert_eq!(tokenizer.decode(&tokenizer.encode(text)), text, "{text:?}");
            }
        });
    }

    #[test]
    fn round_trips_accented_latin1() {
        let vocab = vocabulary();
        let image = vocab.image(Some(MODEL_GPT2), None, None);
        with_tokenizer(&image, |tokenizer| {
            for text in [
                "café crème",
                "naïve façade",
                "ÀÉÎÕÜ àéîõü",
                "£5 ©2024 ± ¼",
                "¡Hola! ¿Qué tal?",
            ] {
                assert_eq!(tokenizer.decode(&tokenizer.encode(text)), text, "{text:?}");
            }
        });
    }

    #[test]
    fn round_trips_multibyte_utf8() {
        let vocab = vocabulary();
        let image = vocab.image(Some(MODEL_GPT2), None, None);
        with_tokenizer(&image, |tokenizer| {
            for text in [
                "日本語のテキスト",
                "Здравствуйте",
                "Ω≈ç√∫˜µ",
                "héllo 世界 🎉",
                "🦀🦀🦀",
            ] {
                assert_eq!(tokenizer.decode(&tokenizer.encode(text)), text, "{text:?}");
            }
            // A multi-byte character is encoded as one symbol per byte, and the merge test
            // vocabulary merges exactly those two bytes.
            assert_eq!(tokenizer.decode(&tokenizer.encode("é")), "é");
        });
    }

    #[test]
    fn round_trips_whitespace() {
        let vocab = vocabulary();
        let image = vocab.image(Some(MODEL_GPT2), None, None);
        with_tokenizer(&image, |tokenizer| {
            for text in [
                "  leading and   duplicate  ",
                "a\tb\n\nc\rd",
                "trailing   ",
                "   ",
                " \t ",
                "\n",
            ] {
                assert_eq!(tokenizer.decode(&tokenizer.encode(text)), text, "{text:?}");
            }
        });
    }

    #[test]
    fn known_merges_produce_the_merged_token() {
        let vocab = vocabulary();
        let image = vocab.image(Some(MODEL_GPT2), None, None);
        with_tokenizer(&image, |tokenizer| {
            // "l l" is a recorded merge, so the pair collapses to the `ll` token.
            assert_eq!(tokenizer.encode("ll"), [vocab.id_of(&mapped(b"ll"))]);
            // Rank order drives repeated merging: h+e, l+l, he+ll, hell+o.
            assert_eq!(tokenizer.encode("hello"), [vocab.id_of(&mapped(b"hello"))]);
            assert_eq!(
                tokenizer.encode("hello world"),
                [vocab.id_of(&mapped(b"hello")), vocab.id_of(&mapped(b" world"))]
            );
            // The merge across the bytes of a multi-byte character applies too.
            assert_eq!(tokenizer.encode("é"), [vocab.id_of(&mapped("é".as_bytes()))]);
            assert_eq!(tokenizer.token_str(vocab.id_of("hello")), Some("hello"));
            // Unmerged text falls back to byte tokens.
            assert_eq!(
                tokenizer.encode("hi"),
                [mapped(b"h"), mapped(b"i")].iter().map(|t| vocab.id_of(t)).collect::<Vec<_>>()
            );
        });
    }

    #[test]
    fn merge_count_reports_loaded_merges() {
        let vocab = vocabulary();
        let image = vocab.image(Some(MODEL_GPT2), None, None);
        with_tokenizer(&image, |tokenizer| {
            assert_eq!(tokenizer.merge_count(), vocab.merges.len());
            assert_eq!(tokenizer.vocab_size(), vocab.tokens.len());
        });
    }

    #[test]
    fn bytes_estimates_the_owned_tables() {
        let vocab = vocabulary();
        let image = vocab.image(Some(MODEL_GPT2), None, None);
        with_tokenizer(&image, |tokenizer| {
            let floor = vocab.tokens.len() * size_of::<String>()
                + vocab.tokens.iter().map(String::len).sum::<usize>();
            assert!(
                tokenizer.bytes() >= floor,
                "estimate {} must cover the vocabulary floor {floor}",
                tokenizer.bytes()
            );
            assert!(tokenizer.bytes() < 1 << 20, "estimate is implausibly large");
        });

        // A bigger vocabulary must not estimate less than a smaller one.
        let mut bigger = vocab;
        for index in 0..64 {
            bigger.tokens.push(format!("filler{index}"));
        }
        let image = bigger.image(Some(MODEL_GPT2), None, None);
        with_tokenizer(&image, |tokenizer| {
            assert!(tokenizer.bytes() > 64 * size_of::<String>());
            assert_eq!(tokenizer.vocab_size(), bigger.tokens.len());
        });
    }

    #[test]
    fn bos_eos_policy_is_read_from_metadata() {
        let vocab = vocabulary();
        let token_refs: Vec<&str> = vocab.tokens.iter().map(String::as_str).collect();
        let merge_refs: Vec<&str> = vocab.merges.iter().map(String::as_str).collect();
        let mut image = GgufImage::new();
        image.str(KEY_MODEL, MODEL_GPT2);
        image.str_array(KEY_TOKENS, &token_refs);
        image.str_array(KEY_MERGES, &merge_refs);
        image.u32(KEY_BOS, 1);
        image.u32(KEY_EOS, 2);
        image.bool(KEY_ADD_BOS, true);
        image.bool(KEY_ADD_EOS, false);
        with_tokenizer(&image.finish(), |tokenizer| {
            assert_eq!(tokenizer.bos_id(), Some(1));
            assert_eq!(tokenizer.eos_id(), Some(2));
            assert!(tokenizer.add_bos());
            assert!(!tokenizer.add_eos());
        });

        // Absent policy keys mean "insert nothing", independently of the ids being present.
        let image = vocab.image(Some(MODEL_GPT2), Some(1), Some(2));
        with_tokenizer(&image, |tokenizer| {
            assert_eq!(tokenizer.bos_id(), Some(1));
            assert_eq!(tokenizer.eos_id(), Some(2));
            assert!(!tokenizer.add_bos());
            assert!(!tokenizer.add_eos());
        });
    }

    #[test]
    fn unsupported_model_is_rejected() {
        let vocab = vocabulary();
        // SentencePiece models must be refused, not misread as byte-level BPE.
        let image = vocab.image(Some("llama"), None, None);
        assert_eq!(build_error(&image), TokenizerError::UnsupportedModel);
        // A missing model key is equally unsupported.
        let image = vocab.image(None, None, None);
        assert_eq!(build_error(&image), TokenizerError::UnsupportedModel);
    }

    #[test]
    fn missing_metadata_is_rejected() {
        let mut image = GgufImage::new();
        image.str(KEY_MODEL, MODEL_GPT2);
        image.str_array(KEY_MERGES, &[]);
        assert_eq!(build_error(&image.finish()), TokenizerError::MissingTokens);

        let mut image = GgufImage::new();
        image.str(KEY_MODEL, MODEL_GPT2);
        image.str_array(KEY_TOKENS, &[&mapped(b"a")]);
        assert_eq!(build_error(&image.finish()), TokenizerError::MissingMerges);
    }

    #[test]
    fn unusable_merge_entries_are_ignored() {
        // Three kinds of useless merge entries: one without a separator, one naming a symbol the
        // vocabulary does not define, and one whose concatenated result is not a token. None may
        // abort construction, and none may be applied while encoding.
        let mut image = GgufImage::new();
        image.str(KEY_MODEL, MODEL_GPT2);
        image.str_array(KEY_TOKENS, &[&mapped(b"l"), &mapped(b"ll")]);
        image.str_array(KEY_MERGES, &["ll", "l z", "ll ll"]);
        with_tokenizer(&image.finish(), |tokenizer| {
            // Only "ll ll" is recorded: the other two cannot ever apply.
            assert_eq!(tokenizer.merge_count(), 1);
            assert_eq!(tokenizer.token_str(1), Some("ll"));
            // The `ll` token exists but is unreachable, because the only merge that could build it
            // produces a token the vocabulary does not have.
            assert_eq!(tokenizer.encode("ll"), [0, 0]);
            assert_eq!(tokenizer.decode(&[1]), "ll");
        });
    }

    #[test]
    fn duplicate_merge_is_rejected() {
        let mut image = GgufImage::new();
        image.str(KEY_MODEL, MODEL_GPT2);
        image.str_array(KEY_TOKENS, &[&mapped(b"l"), &mapped(b"ll")]);
        image.str_array(KEY_MERGES, &["l l", "l l"]);
        assert_eq!(build_error(&image.finish()), TokenizerError::DuplicateMerge);
    }

    #[test]
    fn bad_special_token_is_rejected() {
        let vocab = TestVocab::new();
        let image = vocab.image(Some(MODEL_GPT2), Some(9_999), None);
        assert_eq!(build_error(&image), TokenizerError::BadSpecialToken);
        let image = vocab.image(Some(MODEL_GPT2), None, Some(9_999));
        assert_eq!(build_error(&image), TokenizerError::BadSpecialToken);
        // The last valid id is accepted.
        let last = (vocab.tokens.len() - 1) as u32;
        let image = vocab.image(Some(MODEL_GPT2), Some(last), Some(last));
        with_tokenizer(&image, |tokenizer| {
            assert_eq!(tokenizer.bos_id(), Some(last));
            assert_eq!(tokenizer.eos_id(), Some(last));
        });
    }

    #[test]
    fn specials_are_wrapped_only_when_declared_and_requested() {
        let vocab = vocabulary();
        let image = vocab.image(Some(MODEL_GPT2), Some(1), Some(2));
        with_tokenizer(&image, |tokenizer| {
            let body = tokenizer.encode("hi");
            assert_eq!(body.len(), 2);
            assert_eq!(
                tokenizer.encode_with_specials("hi", true, true),
                [1, body[0], body[1], 2]
            );
            assert_eq!(tokenizer.encode_with_specials("hi", false, false), body);
            assert_eq!(tokenizer.encode_with_specials("hi", true, false), [1, body[0], body[1]]);
        });

        // A model without BOS/EOS metadata inserts nothing even when asked.
        let image = vocab.image(Some(MODEL_GPT2), None, None);
        with_tokenizer(&image, |tokenizer| {
            assert_eq!(tokenizer.bos_id(), None);
            assert_eq!(tokenizer.eos_id(), None);
            let body = tokenizer.encode("hi");
            assert_eq!(tokenizer.encode_with_specials("hi", true, true), body);
        });
    }

    #[test]
    fn out_of_range_ids_are_skipped() {
        let vocab = vocabulary();
        let image = vocab.image(Some(MODEL_GPT2), Some(1), Some(2));
        with_tokenizer(&image, |tokenizer| {
            let ids = [u32::from(b'h'), 9_999, u32::from(b'i')];
            assert_eq!(tokenizer.decode_bytes(&ids), b"hi".to_vec());
            assert_eq!(tokenizer.decode(&ids), "hi");
            assert_eq!(tokenizer.decode_bytes(&[9_999, u32::MAX]), Vec::<u8>::new());
            assert_eq!(tokenizer.decode(&[]), "");
            assert_eq!(tokenizer.token_str(9_999), None);
            assert_eq!(tokenizer.token_bytes(9_999), None);
            // A space in text is the single byte-level token for U+0020.
            assert_eq!(tokenizer.token_bytes(u32::from(b' ')), Some(vec![b' ']));
            assert_eq!(tokenizer.token_str(u32::from(b' ')), Some("Ġ"));
        });
    }

    #[test]
    fn partial_utf8_is_rejoined_lossily() {
        let vocab = vocabulary();
        let image = vocab.image(Some(MODEL_GPT2), None, None);
        with_tokenizer(&image, |tokenizer| {
            // "é" is the two byte tokens 0xC3 and 0xA9, in ids 0xC3/0xA9 here.
            assert_eq!(tokenizer.decode(&[0xC3]), "\u{FFFD}");
            assert_eq!(tokenizer.decode(&[0xC3, 0xA9]), "é");
            assert_eq!(tokenizer.decode_bytes(&[0xC3, 0xA9]), "é".as_bytes().to_vec());
        });
    }

    #[test]
    fn unknown_symbols_are_skipped_without_panicking() {
        // A vocabulary that stops at byte 0x61 keeps 'a' but drops 'b', 'c', ...: no panic, the
        // missing bytes simply produce no token.
        let mut vocab = TestVocab::new();
        vocab.tokens.truncate(0x62);
        let image = vocab.image(Some(MODEL_GPT2), None, None);
        with_tokenizer(&image, |tokenizer| {
            assert_eq!(tokenizer.encode("a"), [0x61]);
            assert_eq!(tokenizer.decode(&tokenizer.encode("abc")), "a");
        });

        // A vocabulary entry whose characters are outside the byte-level alphabet decodes to no
        // bytes instead of panicking.
        let mut vocab = TestVocab::new();
        vocab.tokens.push("中".to_string());
        let ideograph = (vocab.tokens.len() - 1) as u32;
        let image = vocab.image(Some(MODEL_GPT2), None, None);
        with_tokenizer(&image, |tokenizer| {
            assert_eq!(tokenizer.token_str(ideograph), Some("中"));
            assert_eq!(tokenizer.token_bytes(ideograph), Some(Vec::new()));
        });

        // A vocabulary with no tokens at all is usable (and encodes nothing).
        let mut image = GgufImage::new();
        image.str(KEY_MODEL, MODEL_GPT2);
        image.str_array(KEY_TOKENS, &[]);
        image.str_array(KEY_MERGES, &[]);
        with_tokenizer(&image.finish(), |tokenizer| {
            assert_eq!(tokenizer.vocab_size(), 0);
            assert_eq!(tokenizer.merge_count(), 0);
            assert_eq!(tokenizer.encode("hi"), Vec::<u32>::new());
        });
    }
}
