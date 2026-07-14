// SPDX-License-Identifier: MIT
extern crate alloc;
use alloc::{boxed::Box, vec::Vec};

use crate::node::ViNode;

/// Generic router mapping page keys to builder functions.
///
/// `K` must be `Clone + PartialEq + 'static` (e.g., `&'static str` or a custom enum).
/// Pages are built lazily — each `push` / `pop` / `replace` invokes the builder fresh.
pub struct Router<K: Clone + PartialEq + 'static> {
    routes: Vec<(K, Box<dyn Fn() -> Box<dyn ViNode>>)>,
    history: Vec<K>,
}

impl<K: Clone + PartialEq + 'static> Router<K> {
    pub fn new(initial: K) -> Self {
        Self {
            routes: Vec::new(),
            history: alloc::vec![initial],
        }
    }

    /// Register a page builder for a key.
    pub fn register(&mut self, key: K, builder: impl Fn() -> Box<dyn ViNode> + 'static) {
        self.routes.push((key, Box::new(builder)));
    }

    /// Push a new page key onto the history stack.
    ///
    /// Returns the built widget, or `None` if the key is not registered.
    pub fn push(&mut self, key: K) -> Option<Box<dyn ViNode>> {
        let widget = self.build(&key)?;
        self.history.push(key);
        Some(widget)
    }

    /// Pop the current page.
    ///
    /// Returns `(new_current_key, widget)`, or `None` if already at root or the
    /// target key has no registered builder.
    ///
    /// # Invariant
    /// History is only mutated when the build succeeds, so a missing-builder
    /// failure leaves history unchanged (no silent corruption).
    pub fn pop(&mut self) -> Option<(K, Box<dyn ViNode>)> {
        if self.history.len() <= 1 {
            return None;
        }
        // Peek at the destination key before touching history.
        let key = self.history[self.history.len() - 2].clone();
        let widget = self.build(&key)?;
        // Build succeeded — now commit the history change.
        self.history.pop();
        Some((key, widget))
    }

    /// Replace the current page without touching history.
    ///
    /// Returns the built widget, or `None` if the key is not registered.
    pub fn replace(&mut self, key: K) -> Option<Box<dyn ViNode>> {
        let widget = self.build(&key)?;
        if let Some(last) = self.history.last_mut() {
            *last = key;
        }
        Some(widget)
    }

    pub fn can_pop(&self) -> bool {
        self.history.len() > 1
    }

    pub fn current_key(&self) -> &K {
        self.history.last().unwrap()
    }

    fn build(&self, key: &K) -> Option<Box<dyn ViNode>> {
        self.routes
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, builder)| builder())
    }
}
