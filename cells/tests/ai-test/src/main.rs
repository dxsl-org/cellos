//! QEMU oracle for the unified AI inference service (`/bin/ai`) — Spec 24 G2 Level A.
//!
//! The oracle drives the *public* SDK path a real application uses (`AiClient` over typed IPC) and
//! compares the result against the golden values produced by `scripts/gen-ai-test-model.py`, whose
//! reference forward pass shares no code with the engine:
//!
//! 1. `Describe` must report the deployed model, the CPU backend, and this build's limits.
//! 2. A greedy generation of the golden prompt must return the golden token ids, exactly.
//! 3. `embed` must return the golden vector within `1e-3` per component.
//! 4. A session abandoned mid-stream must not wedge the service: the next caller still gets a
//!    session and a full generation.
//!
//! Prints exactly one `[ai-test] PASS` line on success; any deviation prints `[ai-test] FAIL …`
//! and exits non-zero so the QEMU harness cannot mistake a partial run for a pass.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;

use ai_proto::backend;
use ai_sdk::{AiClient, AiClientError, InferParams};
use ostd::io::{print, print_usize, println};
use ostd::syscall::sys_exit;

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![Send, Recv, TryRecv, Log, LookupService, Yield];

ostd::declare_custom_heap!(2 * 1024 * 1024);

/// Golden values from the fixture generator (same file the deployed model was built from).
const GOLDEN: &str = include_str!("../../../../models/tiny-llama-64.golden.txt");

/// Poll round trips allowed for one 8-token generation. The service advances four model steps per
/// poll, so a 7-token prompt plus 8 generated tokens needs about four.
const MAX_POLLS: usize = 32;

/// Embedding tolerance per component (f32 engine vs the f64 reference).
const EMBED_TOLERANCE: f32 = 1e-3;

ostd::cell_main!(cell_main);

fn cell_main() {
    println("[ai-test] AI inference oracle starting");
    let golden = match Golden::parse(GOLDEN) {
        Some(golden) => golden,
        None => fail("golden fixture is unreadable"),
    };

    let mut client = AiClient::new(ai_sdk::ostd_transport::OstdTransport::new());
    let info = match client.describe() {
        Ok(info) => info,
        Err(AiClientError::NoService) => fail("service::AI is not registered"),
        Err(error) => fail_with("describe", error),
    };
    print("[ai-test] model=");
    println(info.model.as_str());
    print("[ai-test] vocab=");
    print_usize(info.vocab_size as usize);
    print(" context=");
    print_usize(info.context_tokens as usize);
    print(" sessions=");
    print_usize(info.max_sessions as usize);
    println("");
    if info.active_backend != backend::CPU {
        fail("service did not report the CPU backend");
    }
    if info.vocab_size == 0 {
        fail("service has no model resident");
    }

    // 1. Greedy generation must reproduce the reference token ids exactly.
    let params = InferParams::greedy(golden.prompt.as_str(), golden.greedy_ids.len() as u16);
    let generation = match client.generate(&params, MAX_POLLS) {
        Ok(generation) => generation,
        Err(error) => fail_with("generate", error),
    };
    if generation.ids != golden.greedy_ids {
        print("[ai-test] expected tokens ");
        print_usize(golden.greedy_ids.len());
        print(" got ");
        print_usize(generation.ids.len());
        println("");
        fail("greedy token ids differ from the reference");
    }
    print("[ai-test] greedy ids matched: ");
    print_usize(generation.ids.len());
    print(" tokens over ");
    print_usize(generation.polls);
    println(" polls");

    // 2. The embedding path must match the reference vector.
    let values = match client.embed(golden.embed_text.as_str()) {
        Ok(values) => values,
        Err(error) => fail_with("embed", error),
    };
    if values.len() != golden.embed_values.len() {
        fail("embedding dimension differs from the reference");
    }
    let mut worst = 0.0f32;
    for (actual, expected) in values.iter().zip(&golden.embed_values) {
        let delta = (actual - expected).abs();
        if delta > worst {
            worst = delta;
        }
    }
    if worst > EMBED_TOLERANCE {
        fail("embedding vector differs from the reference");
    }
    print("[ai-test] embedding matched: ");
    print_usize(values.len());
    println(" dims");

    // 3. An abandoned session must not wedge the service.
    let abandoned = match client.submit(&params) {
        Ok(request_id) => request_id,
        Err(error) => fail_with("submit", error),
    };
    match client.poll(abandoned, ai_proto::MAX_TOKENS_PER_POLL) {
        Ok(_) => {}
        Err(error) => fail_with("poll", error),
    }
    if client.cancel(abandoned).is_err() {
        fail("cancelling an abandoned session failed");
    }
    let after = match client.generate(&params, MAX_POLLS) {
        Ok(generation) => generation,
        Err(error) => fail_with("generate", error),
    };
    if after.ids != golden.greedy_ids {
        fail("the service did not recover after an abandoned session");
    }
    println("[ai-test] abandoned session released; service still serving");

    // 4. The ratified streaming surface: `AiClient::prompt` returns a token stream. Only the
    //    mock-transport host tests exercise it today, so drive it against the real service here.
    let future = client.prompt(&params);
    let mut future = core::pin::pin!(future);
    let waker = core::task::Waker::noop();
    let mut context = core::task::Context::from_waker(waker);
    let mut stream = match future.as_mut().poll(&mut context) {
        core::task::Poll::Ready(Ok(stream)) => stream,
        core::task::Poll::Ready(Err(error)) => fail_with("prompt", error),
        core::task::Poll::Pending => fail("prompt did not resolve on its first poll"),
    };
    let mut streamed = Vec::new();
    for item in &mut stream {
        match item {
            Ok(token) => streamed.push(token.id),
            Err(error) => fail_with("prompt stream", error),
        }
    }
    if streamed != golden.greedy_ids {
        fail("the token stream did not reproduce the reference ids");
    }
    if stream.text() != after.text {
        fail("the token stream text differs from the drained generation text");
    }
    print("[ai-test] prompt stream matched: ");
    print_usize(streamed.len());
    println(" tokens");

    println("[ai-test] PASS");
    sys_exit(0);
}

/// Print a failure line and exit non-zero: the harness must never read a partial run as a pass.
fn fail(reason: &str) -> ! {
    println("[ai-test] FAIL");
    println(reason);
    sys_exit(1);
}

/// Fail with the client error's typed cause, so the serial log names the exact refusal.
fn fail_with(context: &str, error: AiClientError) -> ! {
    ostd::io::print("[ai-test] FAIL ");
    println(context);
    ostd::io::print_fmt(format_args!("[ai-test] cause: {error:?}\n"));
    sys_exit(1);
}

/// The golden expectations the deployed model must reproduce.
struct Golden {
    prompt: String,
    greedy_ids: Vec<u32>,
    embed_text: String,
    embed_values: Vec<f32>,
}

impl Golden {
    /// Parse `key=value` lines, ignoring comments.
    fn parse(text: &str) -> Option<Self> {
        let mut golden = Self {
            prompt: String::new(),
            greedy_ids: Vec::new(),
            embed_text: String::new(),
            embed_values: Vec::new(),
        };
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=')?;
            match key {
                "prompt" => golden.prompt = String::from(value),
                "greedy_ids" => golden.greedy_ids = parse_ids(value)?,
                "embed_text" => golden.embed_text = String::from(value),
                "embed_values" => golden.embed_values = parse_values(value)?,
                _ => {}
            }
        }
        if golden.greedy_ids.is_empty() || golden.embed_values.is_empty() {
            return None;
        }
        Some(golden)
    }
}

fn parse_ids(value: &str) -> Option<Vec<u32>> {
    let mut ids = Vec::new();
    for item in value.split(',') {
        if item.is_empty() {
            continue;
        }
        ids.push(item.parse::<u32>().ok()?);
    }
    Some(ids)
}

fn parse_values(value: &str) -> Option<Vec<f32>> {
    let mut values = Vec::new();
    for item in value.split(',') {
        if item.is_empty() {
            continue;
        }
        values.push(item.parse::<f32>().ok()?);
    }
    Some(values)
}
