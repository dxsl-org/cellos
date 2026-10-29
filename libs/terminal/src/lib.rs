// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Public Terminal SDK for the Cellos operational tooling workstream.
//!
//! Provides public headless session/I/O facilities, generational stream endpoints,
//! VT escape controls, mode guards, and the `cellos-vt1` input/output codec.
//! Independent of viUI and Ratatui.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod codec;
pub mod controls;
pub mod error;
pub mod mode;
pub mod session;
pub mod pipe_child;
pub mod stream;
pub mod input;
pub mod raw_input;
pub mod context;
pub mod host;

pub use codec::{
    encode_key_action, Decoder, KeyAction, KeyState, KeySymbol, Modifiers,
};
pub use controls::{Color, ScreenControls, TextAttributes, ViewportSize};
pub use error::{StreamError, TerminalError};
pub use mode::{ModeGuard, SessionModeGuard, TerminalMode};
pub use session::{SessionRef, TerminalSession, ForegroundChild, Capabilities, FrontendOutput, completion_writer};
pub use host::{ChildPoll, FrontendHost, FrontendState};
pub use pipe_child::{PipeSpec, PipeChild, Pipeline};
pub use stream::{Endpoint, EndpointDirection, EndpointRef, EndpointRights, Operation, OperationRef, PreparedChild, PreparedRef, ChildRef, ReceiptRef, PrincipalRef};
pub use api::cell_session::Status;

#[cfg(test)]
mod tests;
