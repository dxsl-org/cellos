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

    /// Moves a node to the last-child position, rejecting cycles and leaf parents.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> bool {
        if self.get(child).is_none() || child == self.root
            || !matches!(self.get(parent).map(|n| &n.data),
                Some(NodeData::DocumentRoot | NodeData::Element { .. }))
        {
            return false;
        }
        let mut ancestor = Some(parent);
        while let Some(id) = ancestor {
            if id == child { return false; }
            ancestor = self.nodes[id.index()].parent;
        }
        if let Some(old_parent) = self.nodes[child.index()].parent {
            self.remove_child(old_parent, child);
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

    /// Detaches only a direct child. IDs and detached subtrees remain alive.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> bool {
        let Some(node) = self.get(child) else { return false; };
        if node.parent != Some(parent) { return false; }
        let (prev, next) = (node.prev_sibling, node.next_sibling);
        if let Some(id) = prev { self.nodes[id.index()].next_sibling = next; }
        else { self.nodes[parent.index()].first_child = next; }
        if let Some(id) = next { self.nodes[id.index()].prev_sibling = prev; }
        else { self.nodes[parent.index()].last_child = prev; }
        let node = &mut self.nodes[child.index()];
        node.parent = None;
        node.prev_sibling = None;
        node.next_sibling = None;
        true
    }

    /// Element textContent excludes comments; character nodes return their data.
    pub fn get_text_content(&self, node_id: NodeId) -> String {
        let Some(node) = self.get(node_id) else { return String::new(); };
        match &node.data {
            NodeData::DocumentRoot => return String::new(),
            NodeData::Text(s) | NodeData::Comment(s) => return s.clone(),
            _ => {}
        }
        let mut out = String::new();
        let mut cur = node.first_child;
        while let Some(id) = cur {
            let node = &self.nodes[id.index()];
            if let NodeData::Text(s) = &node.data { out.push_str(s); }
            if let Some(child) = node.first_child { cur = Some(child); continue; }
            let mut cursor = id;
            loop {
                let node = &self.nodes[cursor.index()];
                if let Some(next) = node.next_sibling { cur = Some(next); break; }
                match node.parent {
                    Some(parent) if parent != node_id => cursor = parent,
                    _ => { cur = None; break; }
                }
            }
        }
        out
    }

    pub fn set_text_content(&mut self, node: NodeId, text: &str) -> bool {
        let Some(n) = self.get_mut(node) else { return false; };
        match &mut n.data {
            NodeData::Text(s) | NodeData::Comment(s) => { *s = String::from(text); return true; }
            NodeData::DocumentRoot => return true,
            NodeData::Element { .. } => {}
        }
        while let Some(child) = self.nodes[node.index()].first_child {
            self.remove_child(node, child);
        }
        if !text.is_empty() {
            let child = self.alloc_node(NodeData::Text(String::from(text)));
            self.append_child(node, child);
        }
        true
    }

    /// Checks IDs, reciprocal sibling links, parent types and acyclic ancestry.
    pub fn validate(&self) -> bool {
        if self.root != NodeId::ROOT || self.nodes.is_empty()
            || !matches!(self.nodes[0].data, NodeData::DocumentRoot)
            || self.nodes[0].parent.is_some() { return false; }
        let mut colors = alloc::vec![0u8; self.nodes.len()];
        for (index, node) in self.nodes.iter().enumerate() {
            if node.id.index() != index { return false; }
            if !matches!(node.data, NodeData::DocumentRoot | NodeData::Element { .. })
                && (node.first_child.is_some() || node.last_child.is_some()) { return false; }
            if index != 0 && matches!(node.data, NodeData::DocumentRoot) { return false; }
            if let Some(parent) = node.parent {
                let Some(parent) = self.get(parent) else { return false; };
                if !matches!(parent.data, NodeData::DocumentRoot | NodeData::Element { .. }) { return false; }
                if node.prev_sibling.is_none() && parent.first_child != Some(node.id) { return false; }
                if node.next_sibling.is_none() && parent.last_child != Some(node.id) { return false; }
            } else if node.prev_sibling.is_some() || node.next_sibling.is_some() { return false; }
            for (link, forward) in [(node.prev_sibling, false), (node.next_sibling, true)] {
                if let Some(id) = link {
                    let Some(other) = self.get(id) else { return false; };
                    if other.parent != node.parent || id == node.id
                        || (if forward { other.prev_sibling } else { other.next_sibling }) != Some(node.id)
                    { return false; }
                }
            }
            let mut cursor = node.first_child;
            let mut prev = None;
            let mut count = 0;
            while let Some(id) = cursor {
                let Some(child) = self.get(id) else { return false; };
                if child.parent != Some(node.id) || child.prev_sibling != prev
                    || count >= self.nodes.len() { return false; }
                if colors[id.index()] != 0 { return false; }
                colors[id.index()] = 1;
                prev = Some(id);
                cursor = child.next_sibling;
                count += 1;
            }
            if prev != node.last_child { return false; }
        }
        // Three-color parent walk validates deep trees in linear time, without recursion.
        if self.nodes.iter().enumerate().any(|(index, node)| node.parent.is_some() != (colors[index] == 1)) {
            return false;
        }
        colors.fill(0);
        for index in 0..self.nodes.len() {
            let mut cursor = Some(NodeId(index as u32));
            while let Some(id) = cursor {
                match colors[id.index()] {
                    1 => return false,
                    2 => break,
                    _ => colors[id.index()] = 1,
                }
                cursor = self.nodes[id.index()].parent;
            }
            let mut cursor = Some(NodeId(index as u32));
            while let Some(id) = cursor {
                if colors[id.index()] != 1 { break; }
                colors[id.index()] = 2;
                cursor = self.nodes[id.index()].parent;
            }
        }
        true
    }

    /// Applies a single DOM mutation directly to the arena.
    pub fn apply_mutation(&mut self, mutation: &DomMutation) -> bool {
        match mutation {
            DomMutation::CreateNode { node, data } => {
                if node.index() != self.nodes.len() || matches!(data, NodeData::DocumentRoot) {
                    return false;
                }
                self.alloc_node(data.clone());
                true
            }
            DomMutation::SetText { node, text } => self.set_text_content(*node, text),
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
            DomMutation::RemoveChild { parent, child } => self.remove_child(*parent, *child),
            DomMutation::SetDocumentTitle { .. } => true,
        }
    }
}

// ─── DOM Mutations & Events for Cross-Tier IPC ─────────────────────────────────

/// Fine-grained DOM mutation operation emitted by the JS engine to Tier 1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DomMutation {
    CreateNode {
        node: NodeId,
        data: NodeData,
    },
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

/// Logical commands transported through bounded, ordered IPC chunks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OcelJsCommand {
    Eval { script: String },
    DispatchEvent { event: DomEvent },
    SyncDocument { arena: DocumentArena, title: String },
}

pub const OCEL_JS_CHUNK_SIZE: usize = 3072;
pub const OCEL_JS_MAX_TRANSFER: usize = 512 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OcelJsRequest {
    /// Explicitly replaces the one active context; old tokens become invalid.
    OpenContext { context: u64 },
    Begin { context: u64, total: usize },
    Chunk { context: u64, offset: usize, data: Vec<u8> },
    Commit { context: u64 },
    ReadResponse { context: u64, offset: usize },
    /// Readiness probe; never replaces or mutates the active document context.
    Ping,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OcelJsReply {
    Ack,
    ResponseChunk { offset: usize, total: usize, data: Vec<u8> },
    Error { message: String },
    Ready,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OcelJsResponse {
    Success { mutations: Vec<DomMutation>, result_repr: String },
    /// Side effects preceding a thrown exception must still reach the document.
    Error { message: String, line: u32, mutations: Vec<DomMutation> },
}

pub mod ipc;

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
    /// Synchronize a validated arena without clearing same-document JS globals.
    fn sync_document(&mut self, _arena: &DocumentArena, _title: &str) -> Result<(), JsError> {
        Err(JsError { message: String::from("engine has no document-backed DOM"), line: 0 })
    }
    /// Evaluate a JavaScript code string.
    fn eval(&mut self, script: &str) -> Result<String, JsError>;
    /// Dispatch an event to JavaScript event listeners.
    fn dispatch_event(&mut self, event: &DomEvent) -> Result<(), JsError>;
    /// Drain all collected DOM mutations produced by recent script executions.
    fn take_mutations(&mut self) -> Vec<DomMutation>;
    /// Reset execution state and global variables.
    fn reset(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn element(arena: &mut DocumentArena, tag: &str) -> NodeId {
        arena.alloc_node(NodeData::Element { tag: String::from(tag), attributes: Vec::new() })
    }
    #[test]
    fn moving_and_removing_children_preserves_links_and_rejects_cycles() {
        let mut arena = DocumentArena::new();
        let a = element(&mut arena, "div");
        let b = element(&mut arena, "div");
        let c = element(&mut arena, "span");
        arena.append_child(NodeId::ROOT, a);
        arena.append_child(NodeId::ROOT, b);
        arena.append_child(a, c);
        assert!(!arena.append_child(c, a));
        assert!(!arena.append_child(c, NodeId::ROOT));
        assert!(arena.append_child(b, c));
        assert_eq!(arena.get(a).unwrap().first_child, None);
        assert!(arena.append_child(NodeId::ROOT, a));
        assert_eq!(arena.get(b).unwrap().next_sibling, Some(a));
        assert_eq!(arena.get(a).unwrap().prev_sibling, Some(b));
        assert!(!arena.remove_child(a, c));
        assert!(arena.remove_child(b, c));
        assert_eq!(arena.get(c).unwrap().parent, None);
        assert!(arena.validate());
    }
    #[test]
    fn element_text_content_keeps_type_attributes_and_detached_subtree() {
        let mut arena = DocumentArena::new();
        let element = arena.alloc_node(NodeData::Element {
            tag: String::from("p"), attributes: alloc::vec![(String::from("id"), String::from("greeting"))],
        });
        let text = arena.alloc_node(NodeData::Text(String::from("old")));
        let comment = arena.alloc_node(NodeData::Comment(String::from("hidden")));
        arena.append_child(NodeId::ROOT, element);
        arena.append_child(element, text);
        arena.append_child(element, comment);
        assert_eq!(arena.get_text_content(element), "old");
        assert_eq!(arena.get_text_content(comment), "hidden");
        let new_id = NodeId(arena.nodes.len() as u32);
        assert!(arena.set_text_content(element, "new\n\t\0"));
        assert_eq!(arena.get(element).unwrap().tag(), Some("p"));
        assert_eq!(arena.get(element).unwrap().get_attribute("id"), Some("greeting"));
        assert_eq!(arena.get(element).unwrap().first_child, Some(new_id));
        assert_eq!(arena.get(text).unwrap().parent, None);
        assert_eq!(arena.get(comment).unwrap().parent, None);
        assert_eq!(arena.get_text_content(element), "new\n\t\0");
        assert!(arena.set_text_content(element, ""));
        assert_eq!(arena.get(element).unwrap().first_child, None);
        assert!(arena.validate());
    }
    #[test]
    fn creation_ids_are_sequential_and_snapshots_reject_broken_links() {
        let mut arena = DocumentArena::new();
        let data = NodeData::Text(String::from("hello"));
        assert!(!arena.apply_mutation(&DomMutation::CreateNode { node: NodeId(3), data: data.clone() }));
        assert!(arena.apply_mutation(&DomMutation::CreateNode { node: NodeId(1), data }));
        assert!(!arena.append_child(NodeId(1), NodeId(1)));
        arena.nodes[1].next_sibling = Some(NodeId(99));
        assert!(!arena.validate());
    }
}
