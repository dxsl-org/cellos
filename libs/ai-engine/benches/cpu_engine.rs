//! Host benchmark for the native CPU inference engine (Spec 24 §4).
//!
//! This is a measurement instrument, not a test: nothing in CI asserts its numbers, because
//! throughput depends on the host, the checkpoint, and the build profile. Run it with a real
//! checkpoint and compare the `[bench]` lines between two builds to see what a kernel change did.
//!
//! ```text
//! cargo bench -p ai-engine --target x86_64-unknown-linux-gnu --bench cpu_engine -- \
//!     --model .ai-models/stories15M-q8_0.gguf --tokens 32
//! ```
//!
//! The explicit `--target` is required: the workspace's default target is bare-metal RISC-V
//! (`.cargo/config.toml`), and this instrument needs a host to time on.
//!
//! `--model` defaults to `$CELLOS_AI_BENCH_MODEL`, then to `.ai-models/stories15M-q8_0.gguf`.
//! Kernel rows measure `tensor-math` directly at the resident model's own shapes, so a kernel
//! number here and the end-to-end token rate move together.
//!
//! Reported per measurement: the fastest and the median of `iters` runs. The minimum is the
//! kernel's own cost; the median is what a caller sees when the host is busy.

use std::process::ExitCode;
use std::time::Instant;

use ai_engine::{Engine, SamplingParams};
use tensor_math::quant::{Q8_0_BLOCK_BYTES, Q8_0_BLOCK_WEIGHTS};

/// Prompt used for the throughput run: plain completion, which every fixture family accepts.
const PROMPT: &str = "Once upon a time";

struct Args {
    model: String,
    tokens: usize,
    prompt_repeats: usize,
    iters: u32,
    kernels: bool,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut args = Args {
            model: std::env::var("CELLOS_AI_BENCH_MODEL").unwrap_or_else(|_| {
                // Cargo runs the bench with the package root as the working directory, so the
                // gitignored checkpoint cache is addressed relative to the manifest.
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.ai-models/stories15M-q8_0.gguf")
                    .to_string_lossy()
                    .into_owned()
            }),
            tokens: 32,
            prompt_repeats: 1,
            iters: 0,
            kernels: true,
        };
        let mut argv = std::env::args().skip(1);
        while let Some(flag) = argv.next() {
            match flag.as_str() {
                "--model" => args.model = next_value(&mut argv, &flag)?,
                "--tokens" => args.tokens = parse_usize(&next_value(&mut argv, &flag)?, &flag)?,
                "--iters" => {
                    args.iters = parse_usize(&next_value(&mut argv, &flag)?, &flag)? as u32
                }
                "--prompt-repeats" => {
                    args.prompt_repeats = parse_usize(&next_value(&mut argv, &flag)?, &flag)?
                }
                "--no-kernels" => args.kernels = false,
                // `cargo bench` passes this to the binary; the harness is already opt-in.
                "--bench" => {}
                "-h" | "--help" => {
                    println!(
                        "usage: cpu_engine [--model PATH] [--tokens N] [--prompt-repeats N] \
                         [--iters N] [--no-kernels]"
                    );
                    std::process::exit(0);
                }
                other => return Err(format!("unknown flag {other}")),
            }
        }
        Ok(args)
    }
}

fn next_value(argv: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    argv.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn parse_usize(text: &str, flag: &str) -> Result<usize, String> {
    text.parse()
        .map_err(|_| format!("{flag} expects a number, got {text}"))
}

/// Run `body` `iters` times and return (fastest, median) in nanoseconds.
fn time_runs(iters: u32, mut body: impl FnMut()) -> (f64, f64) {
    let mut samples = Vec::with_capacity(iters as usize);
    for _ in 0..iters {
        let started = Instant::now();
        body();
        samples.push(started.elapsed().as_secs_f64() * 1e9);
    }
    samples.sort_by(f64::total_cmp);
    (samples[0], samples[samples.len() / 2])
}

/// A deterministic Q8_0 tensor (`rows × cols`), used when the resident model has no such shape.
fn synthetic_q8(rows: usize, cols: usize) -> Vec<u8> {
    let row_bytes = cols / Q8_0_BLOCK_WEIGHTS * Q8_0_BLOCK_BYTES;
    let mut bytes = vec![0u8; rows * row_bytes];
    let mut state = 0x2545_f491_4f6c_dd1du64;
    for (index, byte) in bytes.iter_mut().enumerate() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        // A binary16 scale just below 1 keeps the row products inside f32 range; the weights are
        // uniform i8, so the kernel's arithmetic matches a real checkpoint's shape of work.
        *byte = if index % row_bytes < 2 {
            u16::to_le_bytes(0x3800 | (state as u16 & 0x03ff))[index % row_bytes]
        } else {
            (state >> 24) as u8
        };
    }
    bytes
}

fn report_kernel(label: &str, iters: u32, work: f64, body: impl FnMut()) {
    let (fastest, median) = time_runs(iters, body);
    println!(
        "[bench] kernel={label} iters={iters} ns_min={fastest:.0} ns_median={median:.0} gflops={:.2}",
        // `work` is 2×MACs for a matvec and element count for a per-element pass.
        work / fastest
    );
}

fn main() -> ExitCode {
    let args = match Args::parse() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("[bench] {error}");
            return ExitCode::FAILURE;
        }
    };

    let bytes = match std::fs::read(&args.model) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("[bench] cannot read {}: {error}", args.model);
            return ExitCode::FAILURE;
        }
    };
    let file_len = bytes.len();

    let load_started = Instant::now();
    let mut engine = match Engine::load(bytes, 1 << 30) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("[bench] load failed: {error:?}");
            return ExitCode::FAILURE;
        }
    };
    let load_ms = load_started.elapsed().as_secs_f64() * 1e3;
    let cfg = engine.config().clone();
    println!(
        "[bench] model={} file={} layers={} embd={} ffn={} heads={} kv_heads={} vocab={} ctx={} \
         resident_mib={} load_ms={load_ms:.1}",
        cfg.name,
        file_len,
        cfg.n_layer,
        cfg.n_embd,
        cfg.n_ff,
        cfg.n_head,
        cfg.n_head_kv,
        cfg.vocab_size,
        cfg.n_ctx,
        engine.resident_bytes() / (1024 * 1024),
    );

    let prompt = PROMPT.repeat(args.prompt_repeats);
    let params = SamplingParams {
        max_tokens: args.tokens.min(u16::MAX as usize) as u16,
        temperature_milli: 0,
        top_k: 0,
        seed: 7,
    };
    let request = match engine.submit(&prompt, params) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("[bench] submit failed: {error:?}");
            return ExitCode::FAILURE;
        }
    };

    // Prefill: exactly one forward pass per prompt token, so this is prompt processing cost.
    let prompt_tokens = engine
        .tokenizer()
        .encode_with_specials(&prompt, engine.tokenizer().add_bos(), false)
        .len();
    let prefill_started = Instant::now();
    if let Err(error) = engine.generate(request, prompt_tokens) {
        eprintln!("[bench] prefill failed: {error:?}");
        return ExitCode::FAILURE;
    }
    let prefill_ms = prefill_started.elapsed().as_secs_f64() * 1e3;
    println!(
        "[bench] prefill prompt_tokens={prompt_tokens} ms={prefill_ms:.1} ms_per_token={:.2}",
        prefill_ms / prompt_tokens.max(1) as f64
    );

    // Decode: a budget of two steps produces exactly one token (sample, then forward the sampled
    // token), so one call's duration is the steady-state cost of one generated token. A call that
    // reports `done` stopped after its sample and never ran the forward, so it is not a token
    // cost and is left out of the samples.
    let mut steps = Vec::with_capacity(args.tokens);
    let mut produced = 0usize;
    while produced < args.tokens {
        let started = Instant::now();
        let progress = match engine.generate(request, 2) {
            Ok(progress) => progress,
            Err(error) => {
                eprintln!("[bench] decode failed: {error:?}");
                return ExitCode::FAILURE;
            }
        };
        let elapsed = started.elapsed().as_secs_f64() * 1e3;
        match engine.drain(request, usize::MAX) {
            Ok(drained) => produced += drained.ids.len(),
            Err(error) => {
                eprintln!("[bench] drain failed: {error:?}");
                return ExitCode::FAILURE;
            }
        }
        if !progress.done {
            steps.push(elapsed);
        }
        if progress.done {
            break;
        }
    }
    let mut sorted = steps.clone();
    sorted.sort_by(f64::total_cmp);
    let decode_min = sorted.first().copied().unwrap_or(f64::NAN);
    let decode_median = sorted.get(sorted.len() / 2).copied().unwrap_or(f64::NAN);
    println!(
        "[bench] decode tokens={} ms_min={decode_min:.2} ms_median={decode_median:.2} \
         tps_min={:.2} tps_median={:.2}",
        produced,
        1000.0 / decode_min.max(f64::MIN_POSITIVE),
        1000.0 / decode_median.max(f64::MIN_POSITIVE),
    );
    if let Err(error) = engine.release(request) {
        eprintln!("[bench] release failed: {error:?}");
        return ExitCode::FAILURE;
    }

    if !args.kernels {
        return ExitCode::SUCCESS;
    }

    // Kernel rows. The default iteration count keeps each measurement in the tens of
    // milliseconds on a modern host; `--iters` overrides it for every row.
    let scale = if args.iters > 0 { args.iters } else { 64 };
    let iters = |default: u32| if args.iters > 0 { args.iters } else { default };
    let embd = cfg.n_embd;
    let ffn = cfg.n_ff;
    let vocab = cfg.vocab_size;

    let x = vec![0.5f32; embd.max(ffn)];
    let mut row_out = vec![0.0f32; vocab];

    // Shape list: (label, rows, cols) — the projections one layer runs, plus the tied output.
    let shapes = [
        ("matvec_q8_0_attn_q", embd, embd),
        ("matvec_q8_0_attn_o", embd, embd),
        ("matvec_q8_0_ffn_gate", ffn, embd),
        ("matvec_q8_0_ffn_down", embd, ffn),
        ("matvec_q8_0_logits", vocab, embd),
    ];
    for (label, rows, cols) in shapes {
        let packed = synthetic_q8(rows, cols);
        let mut slots = vec![0.0f32; rows];
        report_kernel(label, scale, 2.0 * (rows * cols) as f64, || {
            tensor_math::matvec_q8_0(&mut slots, &packed, &x[..cols], rows, cols).unwrap();
            row_out[0] = slots[0];
        });
    }

    let dense = vec![0.25f32; embd * embd];
    let mut dense_out = vec![0.0f32; embd];
    report_kernel("matvec_f32", scale, 2.0 * (embd * embd) as f64, || {
        tensor_math::matvec(&mut dense_out, &dense, &x[..embd], embd, embd).unwrap();
        row_out[0] = dense_out[0];
    });

    let norm_weight = vec![1.0f32; embd];
    let mut norm_out = vec![0.0f32; embd];
    report_kernel("rms_norm", scale * 16, embd as f64, || {
        tensor_math::rms_norm(&mut norm_out, &x[..embd], &norm_weight, cfg.rms_eps).unwrap();
        row_out[0] = norm_out[0];
    });

    let mut logits = vec![0.5f32; vocab];
    report_kernel("softmax", iters(8), vocab as f64, || {
        logits.fill(0.5);
        logits[0] = core::hint::black_box(logits[0]);
        tensor_math::softmax_in_place(&mut logits);
        row_out[0] = logits[0];
    });

    let mut rng = tensor_math::Rng::new(9);
    let mut top_k_logits = vec![0.0f32; vocab];
    report_kernel("sample_top_k", iters(8), vocab as f64, || {
        for (index, value) in top_k_logits.iter_mut().enumerate() {
            *value = ((index.wrapping_mul(2_654_435_761)) % 100_000) as f32;
        }
        row_out[0] = tensor_math::sample_top_k(&mut top_k_logits, 40, 0.8, &mut rng)
            .map(|index| index as f32)
            .unwrap_or_default();
    });

    ExitCode::SUCCESS
}
