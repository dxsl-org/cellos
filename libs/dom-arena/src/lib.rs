// SPDX-License-Identifier: MIT
//! Gosub-style Arena-backed DOM tree, NodeId model, and JS Engine Abstraction.
//!
//! Provides a safe, pointer-free DOM representation using continuous `NodeId` indices,
//! batched DOM mutations for cross-tier IPC, and an engine-agnostic `JsEngine` trait.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Lightweight, copyable identifier referencing a node in the `DocumentArena`.
/// Directly mirrors the Gosub NodeId pattern to prevent circular `Rc`/`RefCell` references.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

impl NodeId {
    pub const ROOT: Self = Self(0);
    pub const INVALID: Self = Self(u32::MAX);

    #[inline]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Inner payload of a DOM Node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NodeData {
    DocumentRoot,
    Element {
        tag: String,
        attributes: Vec<(String, String)>,
    },
    Text(String),
    Comment(String),
}

/// A node within the DOM Arena.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub next_sibling: Option<NodeId>,
    pub prev_sibling: Option<NodeId>,
    pub data: NodeData,
}

impl DomNode {
    pub fn new(id: NodeId, data: NodeData) -> Self {
        Self {
            id,
            parent: None,
            first_child: None,
            last_child: None,
            next_sibling: None,
            prev_sibling: None,
            data,
        }
    }

    pub fn is_element(&self) -> bool {
        matches!(self.data, NodeData::Element { .. })
    }

    pub fn is_text(&self) -> bool {
        matches!(self.data, NodeData::Text(_))
    }

    pub fn tag(&self) -> Option<&str> {
        match &self.data {
            NodeData::Element { tag, .. } => Some(tag.as_str()),
            _ => None,
        }
    }

    pub fn get_attribute(&self, name: &str) -> Option<&str> {
        match &self.data {
            NodeData::Element { attributes, .. } => {
                for (k, v) in attributes {
                    if k == name {
                        return Some(v.as_str());
                    }
                }
                None
            }
            _ => None,
        }
    }
}

/// Flat arena storing all DOM nodes for a document.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentArena {
    pub nodes: Vec<DomNode>,
    pub root: NodeId,
}

impl Default for DocumentArena {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentArena {
    pub fn new() -> Self {
        let nodes = alloc::vec![DomNode::new(NodeId::ROOT, NodeData::DocumentRoot)];
        Self {
            nodes,
            root: NodeId::ROOT,
        }
    }

    pub fn alloc_node(&mut self, data: NodeData) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(DomNode::new(id, data));
        id
    }

    pub fn get(&self, id: NodeId) -> Option<&DomNode> {
        self.nodes.get(id.index())
    }

    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut DomNode> {
        self.nodes.get_mut(id.index())
    }

    /// Appends `child` as the last child of `parent`.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> bool {
        if parent.index() >= self.nodes.len() || child.index() >= self.nodes.len() {
            return false;
        }

        let old_last = self.nodes[parent.index()].last_child;
        self.nodes[child.index()].parent = Some(parent);
        self.nodes[child.index()].prev_sibling = old_last;
        self.nodes[child.index()].next_sibling = None;

        if let Some(prev) = old_last {
            self.nodes[prev.index()].next_sibling = Some(child);
        } else {
            self.nodes[parent.index()].first_child = Some(child);
        }
        self.nodes[parent.index()].last_child = Some(child);
        true
    }

    /// Recursively extracts concatenated text content under `node_id`.
    pub fn get_text_content(&self, node_id: NodeId) -> String {
        let mut out = String::new();
        self.collect_text(node_id, &mut out);
        out
    }

    fn collect_text(&self, node_id: NodeId, out: &mut String) {
        let Some(node) = self.get(node_id) else {
            return;
        };
        if let NodeData::Text(ref text) = node.data {
            out.push_str(text);
        }
        let mut cur = node.first_child;
        while let Some(child_id) = cur {
            self.collect_text(child_id, out);
            cur = self.get(child_id).and_then(|n| n.next_sibling);
        }
    }

    /// Applies a single DOM mutation directly to the arena.
    pub fn apply_mutation(&mut self, mutation: &DomMutation) -> bool {
        match mutation {
            DomMutation::SetText { node, text } => {
                if let Some(n) = self.get_mut(*node) {
                    if let NodeData::Text(ref mut s) = n.data {
                        *s = text.clone();
                        return true;
                    }
                    n.data = NodeData::Text(text.clone());
                    return true;
                }
                false
            }
            DomMutation::SetAttribute { node, key, val } => {
                if let Some(n) = self.get_mut(*node) {
                    if let NodeData::Element {
                        ref mut attributes, ..
                    } = n.data
                    {
                        for (k, v) in attributes.iter_mut() {
                            if k == key {
                                *v = val.clone();
                                return true;
                            }
                        }
                        attributes.push((key.clone(), val.clone()));
                        return true;
                    }
                }
                false
            }
            DomMutation::RemoveAttribute { node, key } => {
                if let Some(n) = self.get_mut(*node) {
                    if let NodeData::Element {
                        ref mut attributes, ..
                    } = n.data
                    {
                        attributes.retain(|(k, _)| k != key);
                        return true;
                    }
                }
                false
            }
            DomMutation::AppendChild { parent, child } => self.append_child(*parent, *child),
            DomMutation::RemoveChild { .. } => {
                // Future extension for node detach
                true
            }
            DomMutation::SetDocumentTitle { .. } => true,
        }
    }
}

// ─── DOM Mutations & Events for Cross-Tier IPC ─────────────────────────────────

/// Fine-grained DOM mutation operation emitted by the JS engine to Tier 1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DomMutation {
    SetText {
        node: NodeId,
        text: String,
    },
    SetAttribute {
        node: NodeId,
        key: String,
        val: String,
    },
    RemoveAttribute {
        node: NodeId,
        key: String,
    },
    AppendChild {
        parent: NodeId,
        child: NodeId,
    },
    RemoveChild {
        parent: NodeId,
        child: NodeId,
    },
    SetDocumentTitle {
        title: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Click,
    KeyDown,
    Input,
    Submit,
    Change,
}

/// Event dispatched from Tier 1 (user action) to Tier 2 (JS listener execution).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomEvent {
    pub target: NodeId,
    pub kind: EventKind,
    pub client_x: i32,
    pub client_y: i32,
    pub key: Option<char>,
}

// ─── Wire Contract (Postcard IPC) ──────────────────────────────────────────────

pub const OCEL_JS_IPC_BUF_SIZE: usize = 4096;

/// Requests sent from Ocel (Tier 1) to `ocel-js` (Tier 2).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OcelJsRequest {
    /// Execute an inline or external JavaScript script.
    Eval { script: String },
    /// Dispatch a UI event to the JS environment.
    DispatchEvent { event: DomEvent },
    /// Reset the JS context (e.g. on new page navigation).
    ResetContext,
}

/// Responses sent from `ocel-js` (Tier 2) to Ocel (Tier 1).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OcelJsResponse {
    /// Script or event execution succeeded, returning batched mutations.
    Success {
        mutations: Vec<DomMutation>,
        result_repr: String,
    },
    /// Script execution resulted in a JavaScript error.
    Error { message: String, line: u32 },
}

// ─── Gosub-Inspired JS Engine Trait Abstraction ────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct JsError {
    pub message: String,
    pub line: u32,
}

/// Engine-agnostic JavaScript Engine trait (Gosub-inspired).
///
/// Enables swapping between SimpleJsRuntime, QuickJS, or future V8 backends
/// without modifying the core document or IPC layers.
pub trait JsEngine {
    type Context: JsContext;
    fn create_context(&mut self) -> Self::Context;
}

/// Context in which JavaScript code executes and interacts with DOM Proxies.
pub trait JsContext {
    /// Evaluate a JavaScript code string.
    fn eval(&mut self, script: &str) -> Result<String, JsError>;
    /// Dispatch an event to JavaScript event listeners.
    fn dispatch_event(&mut self, event: &DomEvent) -> Result<(), JsError>;
    /// Drain all collected DOM mutations produced by recent script executions.
    fn take_mutations(&mut self) -> Vec<DomMutation>;
    /// Reset execution state and global variables.
    fn reset(&mut self);
}
