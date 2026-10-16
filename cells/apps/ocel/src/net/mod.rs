// SPDX-License-Identifier: MIT
//! Networking modules for Ocel.

pub mod http;
pub mod gemini;
#[allow(unused_imports)]
pub use gemini::{fetch_gemini, parse_gemtext, GeminiResponse};
#[allow(unused_imports)]
pub use http::{fetch_binary, fetch_http, fetch_http_binary, fetch_https, fetch_https_binary};
