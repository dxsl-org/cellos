// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Ratatui on Cellos terminal sessions.
//!
//! [`backend::CellosBackend`] implements `ratatui_core::backend::Backend` over a
//! [`backend::ByteSink`] (a session's stdout endpoint); [`event::EventAdapter`]
//! turns the SDK's typed input and control frames into [`event::Event`]s;
//! [`session::TuiSession`] scopes the raw-input lease and the TUI screen mode.
//! [`theme`] is style data on Ratatui types (truecolor palette with 256/16-colour
//! fallbacks, border sets, styled bars, list highlight, key hints, block-element chart
//! markers). [`ratatui`] re-exports the pinned Ratatui crates under one path.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod backend;
pub mod event;
pub mod session;
pub mod theme;

/// The pinned Ratatui crates this backend is built on, under one path.
pub mod ratatui {
    pub use ratatui_core as core;
    pub use ratatui_widgets as widgets;
}

#[cfg(test)]
mod tests;
