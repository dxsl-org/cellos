// SPDX-License-Identifier: MIT
use super::*;
use crate::lease::tests::{fresh_state, MockTransport};
use ocel_service_proto::Request as LeaseRequest;

type Bridge = Tier2JsBridge<MockTransport>;

#[test]
fn static_formats_and_non_executable_html_never_acquire() {
    let state = fresh_state();
    let mut bridge = Bridge::new();
    for (format, source) in [
        (crate::parser::DocFormat::Html, "<p>Static HTML</p>"),
        (crate::parser::DocFormat::Html, "<script type='application/json'>{}</script><script type='module'>module()</script>"),
        (crate::parser::DocFormat::Markdown, "# Markdown"),
        (crate::parser::DocFormat::PlainText, "plain text"),
    ] {
        let (_, scripts, arena) = crate::parser::parse_content(format, source);
        assert!(!bridge.sync_script_document(arena.as_ref(), !scripts.is_empty(), "static").unwrap());
        assert!(bridge.eval("1 + 1").is_err());
    }
    // Image and PDF loads do not carry an HTML arena or scripts.
    assert!(!bridge.sync_script_document(None, false, "image").unwrap());
    assert!(state.borrow().requests.is_empty());
    assert_eq!(state.borrow().opens, 0);
}

#[test]
fn script_document_holds_one_lease_and_navigation_releases_it() {
    let state = fresh_state();
    let mut bridge = Bridge::new();
    let html = crate::parser::html::parse_html("<p>active</p><script>1 + 1</script>");
    assert!(bridge.sync_script_document(Some(&html.arena), !html.scripts.is_empty(), "active").unwrap());
    let old_context = bridge.context_id();
    bridge.eval("1 + 1").unwrap();
    assert_eq!(state.borrow().requests, alloc::vec![LeaseRequest::Acquire { engine: Engine::JavaScript }]);
    assert_eq!(state.borrow().opens, 1);
    assert_eq!(state.borrow().commands.len(), 2);
    assert!(!bridge.sync_script_document(Some(&html.arena), false, "static").unwrap());
    assert_ne!(bridge.context_id(), old_context);
    assert_eq!(state.borrow().requests[1], LeaseRequest::Release { engine: Engine::JavaScript, lease: 7 });
    assert!(bridge.dispatch_event(&DomEvent { target: html.arena.root, kind: dom_arena::EventKind::Click, client_x: 0, client_y: 0, key: None }).is_err());
    assert_eq!(state.borrow().opens, 1);
    assert_eq!(state.borrow().requests.len(), 2);
}

#[test]
fn acquired_but_failed_sync_releases_and_events_cannot_reopen() {
    let state = fresh_state();
    state.borrow_mut().fail_sync = true;
    let mut bridge = Bridge::new();
    let arena = DocumentArena::new();
    assert!(bridge.sync_script_document(Some(&arena), true, "broken").is_err());
    assert_eq!(state.borrow().requests, alloc::vec![LeaseRequest::Acquire { engine: Engine::JavaScript }, LeaseRequest::Release { engine: Engine::JavaScript, lease: 7 }]);
    assert!(bridge.eval("1").is_err());
    assert!(bridge.dispatch_event(&DomEvent { target: arena.root, kind: dom_arena::EventKind::Click, client_x: 0, client_y: 0, key: None }).is_err());
    assert_eq!(state.borrow().requests.len(), 2);
}

#[test]
fn dropped_context_releases_and_stale_provider_does_not_reactivate() {
    let state = fresh_state();
    let mut bridge = Bridge::new();
    bridge.sync_script_document(Some(&DocumentArena::new()), true, "script").unwrap();
    state.borrow_mut().stale_provider = true;
    assert!(bridge.eval("1").is_err());
    assert_eq!(state.borrow().requests.len(), 2);
    assert!(bridge.eval("2").is_err());
    drop(bridge);
    assert_eq!(state.borrow().requests.len(), 2);
    let state = fresh_state();
    let mut bridge = Bridge::new();
    bridge.sync_script_document(Some(&DocumentArena::new()), true, "script").unwrap();
    drop(bridge);
    assert_eq!(state.borrow().requests.len(), 2);
}

#[test]
fn missing_optional_engine_is_an_error_not_matcher_execution() {
    let state = fresh_state();
    state.borrow_mut().failure = Some(ocel_service_proto::Failure::Unavailable);
    let mut bridge = Bridge::new();
    let error = bridge.sync_script_document(Some(&DocumentArena::new()), true, "script").unwrap_err();
    assert!(error.message.contains("JavaScript engine is unavailable"));
    assert!(bridge.eval("document.title = 'must not match'").is_err());
    assert_eq!(state.borrow().opens, 0);
    assert!(state.borrow().commands.is_empty());
}
