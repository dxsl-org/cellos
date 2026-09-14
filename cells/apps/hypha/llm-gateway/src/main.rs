//! Hypha `llm-gateway` — the LLM service cell.
//!
//! Receives [`LlmRequest`] over IPC and answers [`LlmReply`] from one of two backends, in order:
//!
//! - **local** (first): the on-device inference service Cell (`/bin/ai`, Spec 24 `service::AI`)
//!   through `ai-sdk`'s `AiClient`. Used when the service is registered, has a model, and the
//!   prompt fits one AI IPC message — the rules live in `hypha_llm_gateway::local` and are pinned
//!   by host tests there.
//! - **network** (second): an HTTP chat-completion round trip to an OpenAI-compatible endpoint.
//!   Two transports (see `transport.rs`):
//!   - **plaintext** (`USE_TLS = false`, default): plain TCP via `NetClient` to a plain-HTTP mock
//!     (`tools/hypha-mock-llm/mock_proxy.py --plain`, port 8080). Easiest to test — no TLS
//!     variable.
//!   - **TLS** (`USE_TLS = true`): TLS 1.3 via the net service (port 8443).
//!
//! Both backends run over IPC only: this cell holds **no** network capability. `core` spawns it
//! and talks to it by tid (no registry in P1). Which backend answered is printed per turn.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

mod transport;

use agent_proto::{LlmReply, LlmRequest};
use ai_sdk::{AiClient, AiClientError, InferParams};
use alloc::format;
use alloc::string::String;
use hypha_llm_gateway::{http, local};
use ostd::app::{AppContext, AppEvent};
use ostd::io::{print, print_usize, println};
use ostd::runtime::CellRuntime;

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![Send, Recv, Log, LookupService];

// ── Configuration ────────────────────────────────────────────────────────────
// Plaintext (default): the NetClient.tcp_send contract bug (os-gap G16) is fixed
// and the transport retries until the socket is Established. TLS is currently
// avoided — the net cell's embedded-tls handshake faults on a real server
// (os-gap G17, load page fault). Flip to true once G17 is resolved.
const USE_TLS: bool = false;
// 10.0.2.2 = QEMU user-net gateway = the host (pin the IP — os-gap G3).
const PROXY_IP: [u8; 4] = [10, 0, 2, 2];
const PROXY_PORT: u16 = if USE_TLS { 8443 } else { 8080 };
const PROXY_HOST: &str = "10.0.2.2";
const MODEL: &str = "claude-sonnet-4-6";
/// Reply text must fit one IPC message (os-gap G5: Grant streaming comes later).
const IPC_REPLY_MAX: usize = 3800;
/// Tokens the local model is asked for; bounded so one turn cannot monopolise the gateway.
const LOCAL_MAX_TOKENS: u16 = 64;
/// Poll round trips allowed for one local turn; the service advances four model steps per poll.
const LOCAL_MAX_POLLS: usize = 96;

ostd::cell_main!(cell_main);

fn cell_main() {
    println("[hypha/llm-gateway] service ready");
    // no_heartbeat: an LLM round-trip can exceed the default 5 s watchdog.
    CellRuntime::new().no_heartbeat().run(|ctx, ev| match ev {
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
            handle(ctx, sender_tid, &data);
        }
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => ostd::syscall::sys_exit(0),
        _ => {}
    });
}

/// Decode one request, run the completion, reply (truncated to one IPC message).
fn handle(ctx: &AppContext, sender: usize, data: &[u8]) {
    let reply = match postcard::from_bytes::<LlmRequest>(data) {
        Ok(LlmRequest::Complete { prompt }) => match complete(MODEL, prompt) {
            Ok(text) => classify_reply(text),
            Err(e) => LlmReply::Error(e),
        },
        Err(_) => LlmReply::Error(String::from("bad LlmRequest encoding")),
    };
    let mut buf = [0u8; 4096];
    if let Ok(bytes) = postcard::to_slice(&reply, &mut buf) {
        let _ = ctx.send(sender, bytes);
    }
}

/// One-shot completion: the on-device inference service first, the network endpoint second.
///
/// Which backend answered is printed, so a serial log alone distinguishes a local turn from a
/// networked one. The local path is skipped for exactly two reasons (`local` documents both): the
/// prompt does not fit one AI IPC message, or no local inference is registered or loaded.
fn complete(model: &str, prompt: &str) -> Result<String, String> {
    if local::prompt_fits_wire(prompt.len()) {
        match complete_locally(prompt) {
            Ok(text) => return Ok(text),
            Err(error) if local::network_fallback_allowed(&error) => {
                print("[gw] no local inference (");
                print(&format!("{error:?}"));
                println("); using the network backend");
            }
            Err(error) => return Err(format!("local inference failed: {error:?}")),
        }
    } else {
        print("[gw] prompt ");
        print_usize(prompt.len());
        println(" B exceeds the local AI wire budget; using the network backend");
    }

    complete_over_network(model, prompt)
}

/// Ask the on-device inference service (`/bin/ai`) for a completion.
///
/// The capability probe names the model in the log — the same reason the HTTP front end reports
/// it: a caller can tell which deployment answered a turn without a second endpoint.
fn complete_locally(prompt: &str) -> Result<String, AiClientError> {
    let mut client = AiClient::new(ai_sdk::ostd_transport::OstdTransport::new());
    let model = client.describe()?.model;
    let params = InferParams::greedy(prompt, LOCAL_MAX_TOKENS);
    let generation = client.generate(&params, LOCAL_MAX_POLLS)?;
    print("[gw] local AI backend: ");
    print(model.as_str());
    print(", ");
    print_usize(generation.ids.len());
    println(" tokens");
    Ok(generation.text)
}

/// One-shot chat completion over the network backend. Non-streaming, single inline prompt (P1).
fn complete_over_network(model: &str, prompt: &str) -> Result<String, String> {
    let body = http::build_chat_body(model, prompt);
    let request = http::build_post(PROXY_HOST, "/v1/chat/completions", &body);

    print("[gw] ");
    println(if USE_TLS {
        "TLS mode -> :8443"
    } else {
        "plaintext mode -> :8080"
    });

    let resp = transport::roundtrip(
        USE_TLS,
        PROXY_HOST,
        PROXY_IP,
        PROXY_PORT,
        request.as_bytes(),
    )?;

    print("[gw] response bytes: ");
    print_usize(resp.len());
    println("");

    let body = http::http_body(&resp).ok_or_else(|| String::from("no HTTP body in response"))?;
    let content =
        http::extract_content(body).ok_or_else(|| String::from("no content field in response"))?;
    Ok(content)
}

/// Inspect the raw completion text: if it starts with `TOOL_CALL:` return
/// `ToolCalls`; otherwise return `Text` (truncated to the IPC budget).
fn classify_reply(text: String) -> LlmReply {
    if let Some(call) = http::extract_tool_call(&text) {
        LlmReply::ToolCalls(alloc::vec![call])
    } else {
        LlmReply::Text(fit(text, IPC_REPLY_MAX))
    }
}

/// Truncate `s` to at most `max` bytes on a char boundary, marking truncation.
fn fit(mut s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push_str(" …[truncated: P1 4KB IPC cap — Grant streaming later]");
    s
}
