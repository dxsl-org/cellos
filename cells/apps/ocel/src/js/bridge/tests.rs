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
    assert_eq!(state.borrow().closes, 1);
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
    assert_eq!(state.borrow().closes, 1);
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

#[test]
fn busy_bridge_does_not_evict_and_navigation_can_reacquire() {
    let state = fresh_state();
    let arena = DocumentArena::new();
    let mut owner = Bridge::new();
    let mut contender = Bridge::new();
    owner.sync_script_document(Some(&arena), true, "owner").unwrap();
    let token = owner.context_id();
    let error = contender.sync_script_document(Some(&arena), true, "other").unwrap_err();
    assert!(error.message.contains("busy"));
    assert!(owner.is_active());
    assert!(!contender.is_active());
    assert_eq!(state.borrow().active_context, Some(token));
    owner.eval("still active").unwrap();
    owner.reset();
    assert!(state.borrow().active_context.is_none());
    contender.sync_script_document(Some(&arena), true, "now available").unwrap();
    assert!(contender.is_active());
    contender.reset();
    owner.sync_script_document(Some(&arena), true, "next navigation").unwrap();
    assert!(owner.is_active());
}

fn browser_environment() -> BrowserEnvironment {
    BrowserEnvironment { url: String::from("https://tuoitre.vn/"), user_agent: String::from("Ocel/1"),
        width: 800, height: 600, now_ms: 10, cookie: String::new() }
}

#[test]
fn browser_commands_require_active_document_and_reset_cancels_queued_outputs() {
    let state = fresh_state();
    let mut bridge = Bridge::new();
    assert!(bridge.configure_browser(&browser_environment()).is_err());
    assert!(state.borrow().requests.is_empty());
    bridge.sync_document(&DocumentArena::new(), "browser").unwrap();
    bridge.configure_browser(&browser_environment()).unwrap();
    bridge.eval_script("nativeSource()", None).unwrap();
    state.borrow_mut().next_js_response = Some(OcelJsResponse::Success {
        mutations: Vec::new(), result_repr: String::new(),
        requests: alloc::vec![BrowserRequest::Abort { id: 12 }], wake_at: Some(123),
    });
    bridge.eval("issue request").unwrap();
    assert_eq!(bridge.next_browser_wakeup(), Some(123));
    assert!(matches!(state.borrow().commands[1], OcelJsCommand::ConfigureBrowser { .. }));
    assert!(matches!(state.borrow().commands[2], OcelJsCommand::EvalScript { node: None, .. }));
    bridge.reset();
    assert!(bridge.take_browser_requests().is_empty());
    assert_eq!(bridge.next_browser_wakeup(), None);
    let reply = BrowserReply { id: 12, url: String::from("https://tuoitre.vn/"), status: 200,
        headers: Vec::new(), body: String::new(), error: None };
    assert!(bridge.complete_browser(&reply).is_err());
}

#[test]
fn script_exception_preserves_browser_output_and_mutations_without_invalidating() {
    let state = fresh_state();
    let mut bridge = Bridge::new();
    bridge.sync_document(&DocumentArena::new(), "browser").unwrap();
    state.borrow_mut().next_js_response = Some(OcelJsResponse::Error {
        message: String::from("script threw"), line: 0,
        mutations: alloc::vec![DomMutation::SetDocumentTitle { title: String::from("before throw") }],
        requests: alloc::vec![BrowserRequest::SetCookie { value: String::from("real=request") }], wake_at: Some(77),
    });
    assert!(bridge.eval("throw").is_err());
    assert!(bridge.is_active());
    assert_eq!(bridge.take_mutations().len(), 1);
    assert_eq!(bridge.take_browser_requests().len(), 1);
    assert_eq!(bridge.next_browser_wakeup(), Some(77));
}

#[test]
fn browser_output_limit_closes_context_instead_of_delivering_partial_requests() {
    let state = fresh_state();
    let mut bridge = Bridge::new();
    bridge.sync_document(&DocumentArena::new(), "browser").unwrap();
    state.borrow_mut().next_js_response = Some(OcelJsResponse::Success {
        mutations: Vec::new(), result_repr: String::new(),
        requests: (0..=dom_arena::OCEL_JS_MAX_BROWSER_REQUESTS).map(|id| BrowserRequest::Abort { id: id as u32 }).collect(),
        wake_at: None,
    });
    assert!(bridge.eval("too many").is_err());
    assert!(!bridge.is_active());
    assert!(bridge.take_browser_requests().is_empty());
}

#[test]
fn synchronous_layout_query_applies_mutations_once_and_resumes_same_script() {
    let state = fresh_state();
    let mut bridge = Bridge::new();
    bridge.sync_document(&DocumentArena::new(), "browser").unwrap();
    state.borrow_mut().next_layout_query = Some(dom_arena::BrowserLayoutQuery {
        node: NodeId::ROOT,
        mutations: alloc::vec![DomMutation::SetDocumentTitle { title: String::from("query-side-effect") }],
    });
    let mut calls = 0;
    let result = bridge.eval_script_with_layout("synchronous geometry", None, &mut |mutations, node| {
        calls += 1; assert_eq!(node, NodeId::ROOT); assert_eq!(mutations.len(), 1);
        Ok(BrowserFrame { now_ms: 10, width: 800, height: 600, scroll_y: 0,
            layout: Vec::new(), cookie: String::new(), parsing_complete: false })
    }).unwrap();
    assert_eq!(result, "resumed"); assert_eq!(calls, 1);
    assert!(bridge.take_mutations().is_empty());
    assert!(matches!(state.borrow().layout_resume, Some(dom_arena::BrowserLayoutReply::Frame(_))));
}

#[test]
fn native_layout_host_error_resumes_javascript_with_explicit_error_reply() {
    let state = fresh_state();
    let mut bridge = Bridge::new();
    bridge.sync_document(&DocumentArena::new(), "browser").unwrap();
    state.borrow_mut().next_layout_query = Some(dom_arena::BrowserLayoutQuery { node: NodeId::ROOT, mutations: Vec::new() });
    bridge.eval_script_with_layout("catch native error", None, &mut |_, _| Err(JsError {
        message: String::from("native layout failed"), line: 0,
    })).unwrap();
    assert!(matches!(&state.borrow().layout_resume, Some(dom_arena::BrowserLayoutReply::Error(message)) if message == "native layout failed"));
}
