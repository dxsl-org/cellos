#!/usr/bin/env python3
"""Generate the deterministic tiny Llama GGUF used by the AI-engine tests and the QEMU oracle.

Two outputs, both deterministic and reproducible:

  models/tiny-llama-64.gguf         the checkpoint (Q8_0 weights, tied output embedding)
  models/tiny-llama-64.golden.txt   golden values from an *independent* forward pass written here

The golden file is what makes the Rust engine test meaningful: this script shares no code with the
engine, so a match on token ids and logits is real evidence, not a self-comparison.

Design choices that keep the comparison meaningful:

* Weights are random (this is a machinery/numerics fixture, not a language model), but the exact
  f32 values are fixed by a seeded LCG, then quantized to Q8_0 — and the reference pass uses the
  *dequantized* values, i.e. exactly what the engine will read back.
* The BPE merge list deliberately avoids every character of the golden prompt, so any conforming
  byte-level BPE tokenizer must produce the same prompt ids (they are base byte tokens).
* Greedy decoding (temperature 0) makes the generated ids independent of the sampling RNG.

Usage:
    python3 scripts/gen-ai-test-model.py            # writes both files
    python3 scripts/gen-ai-test-model.py --check    # verifies existing files match
"""

from __future__ import annotations

import math
import struct
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MODEL_PATH = REPO / "models" / "tiny-llama-64.gguf"
GOLDEN_PATH = REPO / "models" / "tiny-llama-64.golden.txt"

# ── Model geometry ───────────────────────────────────────────────────────────────────────────────
N_EMBD = 64
N_LAYER = 2
N_HEAD = 4
N_HEAD_KV = 2
N_FF = 128
N_CTX = 128
RMS_EPS = 1e-5
ROPE_BASE = 10_000.0
HEAD_DIM = N_EMBD // N_HEAD
KV_DIM = N_HEAD_KV * HEAD_DIM
ALIGNMENT = 32
SEED = 0x67BE_2DFD

BASE_VOCAB = 256
MERGES = [
    ("h", "e"),
    ("l", "l"),
    ("he", "l"),
    ("hel", "l"),
    ("hell", "o"),
    ("t", "h"),
    ("th", "e"),
    ("the", "r"),
    ("w", "o"),
    ("wo", "r"),
    ("wor", "l"),
    ("worl", "d"),
]
BOS_TEXT = "<s>"
EOS_TEXT = "</s>"

GOLDEN_PROMPT = "xyz xyz"
GOLDEN_EMBED_TEXT = "xyz zy"
GOLDEN_MAX_TOKENS = 8
GOLDEN_LOGITS = 8


# ── GPT-2 byte-level mapping ─────────────────────────────────────────────────────────────────────
def bytes_to_unicode() -> dict[int, str]:
    """The canonical GPT-2 byte→unicode map (also used to name the base vocabulary tokens)."""
    printable = list(range(ord("!"), ord("~") + 1)) + list(range(ord("¡"), ord("¬") + 1)) + list(
        range(ord("®"), ord("ÿ") + 1)
    )
    mapped = list(printable)
    extra = 0
    for byte in range(256):
        if byte not in printable:
            printable.append(byte)
            mapped.append(256 + extra)
            extra += 1
    return {byte: chr(codepoint) for byte, codepoint in zip(printable, mapped)}


BYTE_TO_UNICODE = bytes_to_unicode()
UNICODE_TO_BYTE = {char: byte for byte, char in BYTE_TO_UNICODE.items()}


def build_vocab() -> list[str]:
    tokens = [BYTE_TO_UNICODE[byte] for byte in range(BASE_VOCAB)]
    for left, right in MERGES:
        tokens.append(left + right)
    tokens.append(BOS_TEXT)
    tokens.append(EOS_TEXT)
    return tokens


def encode(text: str) -> list[int]:
    """Reference encoder: BPE by merge rank over byte-level symbols.

    The golden prompt is merge-free, so its ids are base byte tokens and must match any conforming
    implementation. The merge path here exists to make the reference tokenizer non-trivial.
    """
    ranks = {(left, right): rank for rank, (left, right) in enumerate(MERGES)}
    token_to_id = {}
    for token_id, token in enumerate(build_vocab()):
        token_to_id.setdefault(token, token_id)

    symbols = [BYTE_TO_UNICODE[ord(char)] for char in text]
    while True:
        best_rank = None
        best_index = None
        for index in range(len(symbols) - 1):
            rank = ranks.get((symbols[index], symbols[index + 1]))
            if rank is not None and (best_rank is None or rank < best_rank):
                best_rank = rank
                best_index = index
        if best_index is None:
            break
        symbols[best_index : best_index + 2] = [symbols[best_index] + symbols[best_index + 1]]
    return [token_to_id[symbol] for symbol in symbols]


# ── Weights ──────────────────────────────────────────────────────────────────────────────────────
class Lcg:
    """Numerically explicit PRNG so both this script and any future reimplementation agree."""

    def __init__(self, seed: int) -> None:
        self.state = seed & 0xFFFF_FFFF_FFFF_FFFF

    def advance(self) -> None:
        self.state = (self.state * 6364136223846793005 + 1442695040888963407) & 0xFFFF_FFFF_FFFF_FFFF

    def next_u32(self) -> int:
        self.advance()
        return (self.state >> 33) & 0xFFFF_FFFF

    def next_f32(self) -> float:
        # Uniform in [-0.5, 0.5) with 24-bit resolution.
        self.advance()
        return ((self.state >> 40) & 0xFF_FFFF) / float(1 << 24) - 0.5


def f16_round(value: float) -> float:
    """Round a float through IEEE-754 binary16, as the Q8_0 scale on disk is an f16."""
    return struct.unpack("<e", struct.pack("<e", value))[0]


def generate_weights() -> dict[str, tuple[list[int], list[float]]]:
    """Return name → (dims, row-major f32 values) for every tensor, in a fixed order."""
    rng = Lcg(SEED)
    tensors: dict[str, tuple[list[int], list[float]]] = {}

    def matrix(name: str, rows: int, cols: int) -> None:
        values = [rng.next_f32() for _ in range(rows * cols)]
        tensors[name] = ([cols, rows], values)

    def vector(name: str, length: int, value: float) -> None:
        tensors[name] = ([length], [value] * length)

    matrix("token_embd.weight", len(build_vocab()), N_EMBD)
    vector("output_norm.weight", N_EMBD, 1.0)
    for layer in range(N_LAYER):
        prefix = f"blk.{layer}."
        vector(prefix + "attn_norm.weight", N_EMBD, 1.0)
        matrix(prefix + "attn_q.weight", N_EMBD, N_EMBD)
        matrix(prefix + "attn_k.weight", KV_DIM, N_EMBD)
        matrix(prefix + "attn_v.weight", KV_DIM, N_EMBD)
        matrix(prefix + "attn_output.weight", N_EMBD, N_EMBD)
        vector(prefix + "ffn_norm.weight", N_EMBD, 1.0)
        matrix(prefix + "ffn_gate.weight", N_FF, N_EMBD)
        matrix(prefix + "ffn_up.weight", N_FF, N_EMBD)
        matrix(prefix + "ffn_down.weight", N_EMBD, N_FF)
    return tensors


def quantize_q8_0(values: list[float]) -> bytes:
    """GGML Q8_0: one f16 scale + 32 int8 per 34-byte block."""
    assert len(values) % 32 == 0
    out = bytearray()
    for offset in range(0, len(values), 32):
        block = values[offset : offset + 32]
        peak = max(abs(value) for value in block)
        scale = f16_round(peak / 127.0) if peak > 0 else 0.0
        out += struct.pack("<e", scale)
        for value in block:
            quantized = int(round(value / scale)) if scale > 0 else 0
            quantized = max(-128, min(127, quantized))
            out += struct.pack("<b", quantized)
    return bytes(out)


def dequantize_q8_0(data: bytes, count: int) -> list[float]:
    out: list[float] = []
    offset = 0
    while len(out) < count:
        scale = struct.unpack_from("<e", data, offset)[0]
        offset += 2
        for index in range(32):
            value = struct.unpack_from("<b", data, offset + index)[0]
            out.append(value * scale)
        offset += 32
    return out


# ── Reference forward pass ───────────────────────────────────────────────────────────────────────
def rms_norm(x: list[float], weight: list[float], eps: float) -> list[float]:
    mean_square = sum(value * value for value in x) / len(x)
    scale = 1.0 / math.sqrt(mean_square + eps)
    return [value * scale * weight[index] for index, value in enumerate(x)]


def matvec(weights: list[float], rows: int, cols: int, x: list[float]) -> list[float]:
    return [
        sum(weights[row * cols + col] * x[col] for col in range(cols)) for row in range(rows)
    ]


def rope(vector: list[float], position: int, base: float) -> list[float]:
    out = list(vector)
    half = len(vector) // 2
    for index in range(half):
        theta = position * (base ** (-2.0 * index / len(vector)))
        cos, sin = math.cos(theta), math.sin(theta)
        left, right = vector[index], vector[index + half]
        out[index] = left * cos - right * sin
        out[index + half] = left * sin + right * cos
    return out


def softmax(values: list[float]) -> list[float]:
    peak = max(values)
    exps = [math.exp(value - peak) for value in values]
    total = sum(exps)
    return [value / total for value in exps]


def silu(value: float) -> float:
    return value / (1.0 + math.exp(-value))


def forward_hidden(tokens: list[int], weights: dict[str, list[float]], dims: dict[str, list[int]]) -> list[list[float]]:
    """Reference Llama forward pass; returns the hidden state (before output norm) per position."""
    del dims
    kv_cache_k = [[0.0] * KV_DIM for _ in range(N_LAYER * N_CTX)]
    kv_cache_v = [[0.0] * KV_DIM for _ in range(N_LAYER * N_CTX)]
    hidden_states: list[list[float]] = []

    for position, token in enumerate(tokens):
        x = weights["token_embd.weight"][token * N_EMBD : (token + 1) * N_EMBD]
        for layer in range(N_LAYER):
            prefix = f"blk.{layer}."
            xb = rms_norm(x, weights[prefix + "attn_norm.weight"], RMS_EPS)
            q = matvec(weights[prefix + "attn_q.weight"], N_EMBD, N_EMBD, xb)
            k = matvec(weights[prefix + "attn_k.weight"], KV_DIM, N_EMBD, xb)
            v = matvec(weights[prefix + "attn_v.weight"], KV_DIM, N_EMBD, xb)
            q = [
                rotated
                for head in range(N_HEAD)
                for rotated in rope(q[head * HEAD_DIM : (head + 1) * HEAD_DIM], position, ROPE_BASE)
            ]
            k = [
                rotated
                for head in range(N_HEAD_KV)
                for rotated in rope(k[head * HEAD_DIM : (head + 1) * HEAD_DIM], position, ROPE_BASE)
            ]
            kv_cache_k[layer * N_CTX + position] = k
            kv_cache_v[layer * N_CTX + position] = v

            attn = [0.0] * N_EMBD
            # Scaled dot-product attention: 1/sqrt(head_dim), as every Llama-family implementation
            # applies it (llama.cpp folds it into q, llama2.c multiplies the accumulated score).
            scale = 1.0 / math.sqrt(HEAD_DIM)
            for head in range(N_HEAD):
                kv_head = head // (N_HEAD // N_HEAD_KV)
                q_head = q[head * HEAD_DIM : (head + 1) * HEAD_DIM]
                scores = []
                for key_position in range(position + 1):
                    k_head = kv_cache_k[layer * N_CTX + key_position][
                        kv_head * HEAD_DIM : (kv_head + 1) * HEAD_DIM
                    ]
                    scores.append(sum(a * b for a, b in zip(q_head, k_head)) * scale)
                weights_scores = softmax(scores)
                for key_position, score in enumerate(weights_scores):
                    v_head = kv_cache_v[layer * N_CTX + key_position][
                        kv_head * HEAD_DIM : (kv_head + 1) * HEAD_DIM
                    ]
                    for index in range(HEAD_DIM):
                        attn[head * HEAD_DIM + index] += score * v_head[index]

            projected = matvec(weights[prefix + "attn_output.weight"], N_EMBD, N_EMBD, attn)
            x = [a + b for a, b in zip(x, projected)]

            xb = rms_norm(x, weights[prefix + "ffn_norm.weight"], RMS_EPS)
            gate = matvec(weights[prefix + "ffn_gate.weight"], N_FF, N_EMBD, xb)
            up = matvec(weights[prefix + "ffn_up.weight"], N_FF, N_EMBD, xb)
            hidden = [
                silu(gate[index]) * up[index] for index in range(N_FF)
            ]
            down = matvec(weights[prefix + "ffn_down.weight"], N_EMBD, N_FF, hidden)
            x = [a + b for a, b in zip(x, down)]

        hidden_states.append(list(x))

    return hidden_states


def logits_of(hidden: list[float], weights: dict[str, list[float]], dims: dict[str, list[int]]) -> list[float]:
    """Output norm + tied output embedding: logits[v] = dot(token_embd[v], norm(hidden))."""
    n_vocab = dims["token_embd.weight"][1]
    final = rms_norm(hidden, weights["output_norm.weight"], RMS_EPS)
    return [
        sum(
            weights["token_embd.weight"][v * N_EMBD + index] * final[index]
            for index in range(N_EMBD)
        )
        for v in range(n_vocab)
    ]


def forward(tokens: list[int], weights, dims) -> list[float]:
    """Logits of the last position."""
    hidden = forward_hidden(tokens, weights, dims)
    return logits_of(hidden[-1], weights, dims)


def embed(text: str, weights, dims) -> list[float]:
    """Mean-pooled, L2-normalized hidden state — the reference for `Engine::embed`."""
    hidden = forward_hidden(encode(text), weights, dims)
    pooled = [
        sum(state[index] for state in hidden) / len(hidden) for index in range(N_EMBD)
    ]
    norm = math.sqrt(sum(value * value for value in pooled))
    return [value / norm for value in pooled] if norm > 0 else pooled


# ── GGUF writer ──────────────────────────────────────────────────────────────────────────────────
GGUF_TYPE_U32 = 4
GGUF_TYPE_F32 = 6
GGUF_TYPE_BOOL = 7
GGUF_TYPE_STRING = 8
GGUF_TYPE_ARRAY = 9
GGML_TYPE_F32 = 0
GGML_TYPE_Q8_0 = 8


def pack_string(text: str) -> bytes:
    raw = text.encode("utf-8")
    return struct.pack("<Q", len(raw)) + raw


def pack_metadata(key: str, value) -> bytes:
    out = bytearray(pack_string(key))
    if isinstance(value, bool):
        out += struct.pack("<I", GGUF_TYPE_BOOL) + struct.pack("<B", 1 if value else 0)
    elif isinstance(value, int):
        out += struct.pack("<I", GGUF_TYPE_U32) + struct.pack("<I", value)
    elif isinstance(value, float):
        out += struct.pack("<I", GGUF_TYPE_F32) + struct.pack("<f", value)
    elif isinstance(value, str):
        out += struct.pack("<I", GGUF_TYPE_STRING) + pack_string(value)
    elif isinstance(value, list) and value and isinstance(value[0], str):
        out += struct.pack("<I", GGUF_TYPE_ARRAY) + struct.pack("<I", GGUF_TYPE_STRING)
        out += struct.pack("<Q", len(value))
        for item in value:
            out += pack_string(item)
    else:
        raise TypeError(f"unsupported metadata value for {key}: {value!r}")
    return bytes(out)


def build_gguf() -> bytes:
    tokens = build_vocab()
    tensors = generate_weights()

    metadata = [
        ("general.architecture", "llama"),
        ("general.name", "tiny-llama-64"),
        ("general.alignment", ALIGNMENT),
        ("llama.block_count", N_LAYER),
        ("llama.context_length", N_CTX),
        ("llama.embedding_length", N_EMBD),
        ("llama.feed_forward_length", N_FF),
        ("llama.attention.head_count", N_HEAD),
        ("llama.attention.head_count_kv", N_HEAD_KV),
        ("llama.attention.layer_norm_rms_epsilon", RMS_EPS),
        ("llama.rope.freq_base", ROPE_BASE),
        ("llama.rope.dimension_count", HEAD_DIM),
        ("tokenizer.ggml.model", "gpt2"),
        ("tokenizer.ggml.tokens", tokens),
        ("tokenizer.ggml.merges", [f"{left} {right}" for left, right in MERGES]),
        ("tokenizer.ggml.bos_token_id", BASE_VOCAB + len(MERGES)),
        ("tokenizer.ggml.eos_token_id", BASE_VOCAB + len(MERGES) + 1),
        ("tokenizer.ggml.add_bos_token", False),
        ("tokenizer.ggml.add_eos_token", False),
    ]

    header = bytearray(b"GGUF")
    header += struct.pack("<I", 3)
    header += struct.pack("<Q", len(tensors))
    header += struct.pack("<Q", len(metadata))
    for key, value in metadata:
        header += pack_metadata(key, value)

    # Tensor data, each row quantized independently so 32-weight blocks never straddle rows.
    data = bytearray()
    directory = bytearray()
    for name, (dims, values) in tensors.items():
        if name.endswith("_norm.weight"):
            kind = GGML_TYPE_F32
            payload = struct.pack(f"<{len(values)}f", *values)
        else:
            kind = GGML_TYPE_Q8_0
            if dims[0] % 32 != 0:
                # Pad the row with zeros so a block never spans two rows.
                raise SystemExit(f"{name}: row length {dims[0]} is not a Q8_0 multiple")
            payload = quantize_q8_0(values)

        directory += pack_string(name)
        directory += struct.pack("<I", len(dims))
        for dim in dims:
            directory += struct.pack("<Q", dim)
        directory += struct.pack("<I", kind)
        directory += struct.pack("<Q", len(data))
        data += payload

    blob = bytes(header) + bytes(directory)
    padding = (-len(blob)) % ALIGNMENT
    return blob + b"\0" * padding + bytes(data)


def is_norm(name: str) -> bool:
    """Norm tensors are stored as plain f32; everything else goes to disk as Q8_0."""
    return name.endswith("_norm.weight")


def reference_weights() -> tuple[dict[str, list[float]], dict[str, list[int]]]:
    """Model as the engine will see it: Q8_0 tensors dequantized, norms as written."""
    weights: dict[str, list[float]] = {}
    dims: dict[str, list[int]] = {}
    for name, (dims_value, values) in generate_weights().items():
        dims[name] = dims_value
        if is_norm(name):
            weights[name] = list(values)
        else:
            weights[name] = dequantize_q8_0(quantize_q8_0(values), len(values))
    return weights, dims


def greedy_with_margin(tokens: list[int], weights, dims, count: int) -> tuple[list[int], list[float]]:
    """Greedy decode and report the argmax margin per step.

    A margin test keeps the fixture honest: if two logits are within numerical noise, the golden ids
    would be flaky and the fixture would be worthless as an oracle.
    """
    sequence = list(tokens)
    produced: list[int] = []
    margins: list[float] = []
    for _ in range(count):
        logits = forward(sequence, weights, dims)
        ordered = sorted(range(len(logits)), key=lambda index: logits[index], reverse=True)
        best, runner_up = ordered[0], ordered[1]
        produced.append(best)
        margins.append(logits[best] - logits[runner_up])
        sequence.append(best)
    return produced, margins


def golden_text(weights, dims) -> str:
    prompt_ids = encode(GOLDEN_PROMPT)
    logits = forward(prompt_ids, weights, dims)
    produced, margins = greedy_with_margin(prompt_ids, weights, dims, GOLDEN_MAX_TOKENS)
    weakest = min(margins) if margins else 0.0
    if weakest < 0.05:
        raise SystemExit(
            f"golden fixture is numerically fragile: weakest greedy margin {weakest:.4f} < 0.05; "
            "change SEED so the fixture cannot be decided by floating-point noise"
        )
    lines = [
        "# Golden values for the Cellos tiny Llama fixture. Generated by scripts/gen-ai-test-model.py.",
        "# The reference forward pass in that script shares no code with libs/ai-engine.",
        f"prompt={GOLDEN_PROMPT}",
        "prompt_ids=" + ",".join(str(token) for token in prompt_ids),
        "greedy_ids=" + ",".join(str(token) for token in produced),
        "logits=" + ",".join(f"{value:.6f}" for value in logits[:GOLDEN_LOGITS]),
        f"weakest_margin={weakest:.6f}",
        "# Engine::embed reference: mean-pooled, L2-normalized hidden state of `embed_text`.",
        f"embed_text={GOLDEN_EMBED_TEXT}",
        "embed_ids=" + ",".join(str(token) for token in encode(GOLDEN_EMBED_TEXT)),
        "embed_values=" + ",".join(f"{value:.6f}" for value in embed(GOLDEN_EMBED_TEXT, weights, dims)),
        "",
    ]
    return "\n".join(lines)


def main() -> int:
    check = "--check" in sys.argv
    weights, dims = reference_weights()
    blob = build_gguf()
    golden = golden_text(weights, dims)

    if check:
        ok = True
        for path, expected in ((MODEL_PATH, blob), (GOLDEN_PATH, golden.encode())):
            if not path.exists():
                print(f"MISSING {path}")
                ok = False
            elif path.read_bytes() != expected:
                print(f"STALE {path} — re-run without --check")
                ok = False
        print("OK" if ok else "FAILED")
        return 0 if ok else 1

    MODEL_PATH.parent.mkdir(parents=True, exist_ok=True)
    MODEL_PATH.write_bytes(blob)
    GOLDEN_PATH.write_text(golden)
    print(f"wrote {MODEL_PATH.relative_to(REPO)} ({len(blob)} bytes)")
    print(f"wrote {GOLDEN_PATH.relative_to(REPO)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
