// SPDX-License-Identifier: MIT
//! Multi-screen navigation for ViUI apps.
//!
//! # Stack navigation
//! Use [`StackNavigator`] when screens form a linear history (push/pop).
//! Wrap it as the root widget of your [`crate::app_runner::ViApp`].
//!
//! # Tab navigation
//! Use [`TabNavigator`] for top-level sections with instant switching.
//! Pages are built lazily on first activation and kept alive until the
//! navigator is dropped.

pub mod router;
pub mod stack_nav;
pub mod tab_nav;

pub use router::Router;
pub use stack_nav::{SlideDir, StackNavigator};
pub use tab_nav::{TabEntry, TabNavigator};
