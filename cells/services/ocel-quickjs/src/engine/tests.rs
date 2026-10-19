// SPDX-License-Identifier: MIT
use super::*;
use alloc::vec;
use dom_arena::BrowserNodeLayout;

fn fixture() -> (QuickJsContext, DocumentArena, NodeId, NodeId, NodeId) {
    let mut arena = DocumentArena::new();
    let mut element = |tag: &str| arena.alloc_node(NodeData::Element { tag: String::from(tag), attributes: Vec::new() });
    let html = element("html"); let head = element("head"); let body = element("body"); let script = element("script");
    assert!(arena.append_child(NodeId::ROOT, html)); assert!(arena.append_child(html, head));
    assert!(arena.append_child(html, body)); assert!(arena.append_child(head, script));
    let mut context = QuickJsContext::new();
    context.sync_document(&arena, "native title").unwrap();
    context.configure_browser(&BrowserEnvironment {
        url: String::from("https://tuoitre.vn/news/index.html?q=yes#part"), user_agent: String::from("Ocel/1.0"),
        width: 800, height: 600, now_ms: 100, cookie: String::from("visible=one"),
    }).unwrap();
    (context, arena, body, head, script)
}
fn frame(now_ms: u64, complete: bool) -> BrowserFrame {
    BrowserFrame { now_ms, width: 800, height: 600, scroll_y: 0, layout: Vec::new(), cookie: String::from("visible=one"), parsing_complete: complete }
}
fn apply(context: &mut QuickJsContext, arena: &mut DocumentArena) {
    for mutation in context.take_mutations() { assert!(arena.apply_mutation(&mutation), "invalid mutation: {:?}", mutation); }
    assert!(arena.validate());
}
fn reply(id: u32, body: &str) -> BrowserReply {
    BrowserReply { id, url: String::from("https://tuoitre.vn/resource"), status: 200,
        headers: vec![(String::from("content-type"), String::from("application/json")), (String::from("x-origin"), String::from("native"))],
        body: String::from(body), error: None }
}
#[test]
fn native_environment_identity_location_and_static_current_script() {
    let (mut c, _, _, _, script) = fixture();
    assert_eq!(c.eval("window===self && self===top && top===globalThis && document.defaultView===window").unwrap(), "true");
    assert_eq!(c.eval("[navigator.userAgent,location.origin,location.pathname,location.search,location.hash,document.URL,document.readyState].join('|')").unwrap(),
        "Ocel/1.0|https://tuoitre.vn|/news/index.html|?q=yes|#part|https://tuoitre.vn/news/index.html?q=yes#part|loading");
    assert_eq!(c.eval_script("document.currentScript === node_4", Some(script)).unwrap(), "true");
    assert_eq!(c.eval("document.currentScript === null").unwrap(), "true");
    assert!(c.eval("location.href='/elsewhere'").is_err());
    assert_eq!(c.eval("'serviceWorker' in navigator || 'MutationObserver' in window || 'ResizeObserver' in window || 'localStorage' in window").unwrap(), "false");
    assert!(c.take_browser_requests().is_empty());
}
#[test]
fn fragments_insert_clone_inner_html_keep_native_ids_and_detached_wrappers() {
    let (mut c, mut arena, _, _, _) = fixture();
    c.eval(r#"var detached=document.createElement('aside'); var saved=detached;
      var fragment=document.createDocumentFragment();
      var section=document.createElement('section'); section.id='section';
      section.innerHTML='<p class="item first" data-n="one">A &amp; B</p><p class="item">C</p><!--tail-->';
      fragment.appendChild(section); document.body.appendChild(fragment);
      var first=section.firstChild, copy=first.cloneNode(true); section.insertBefore(copy,first);
      first.replaceWith(document.createTextNode('replacement')); section.lastChild.remove();
      var savedDocument=document; var eventCount=0; saved.addEventListener('keep',function(){eventCount++});
    "#).unwrap();
    apply(&mut c, &mut arena);
    assert_eq!(c.eval("document.querySelector('#section').innerHTML").unwrap(), "<p class=\"item first\" data-n=\"one\">A &amp; B</p>replacement<p class=\"item\">C</p>");
    assert_eq!(c.eval("fragment.childNodes.length").unwrap(), "0");
    c.sync_document(&arena, "same document").unwrap();
    assert_eq!(c.eval("saved===detached && savedDocument===document && !saved.isConnected").unwrap(), "true");
    c.eval("saved.dispatchEvent(new Event('keep'))").unwrap();
    assert_eq!(c.eval("eventCount").unwrap(), "1");
}
#[test]
fn compound_selectors_style_class_list_and_reflections_are_real_mutations() {
    let (mut c, mut arena, _, _, _) = fixture();
    c.eval(r#"document.body.innerHTML='<section id="s"><a class="a b" href="/news/" data-k="alpha beta">one</a><a class="b" href="/next">two</a></section>';
      var first=document.querySelector('section#s > a.a.b[data-k~="beta"]:first-child');
      first.classList.toggle('a'); first.classList.add('active'); first.style.setProperty('margin-top','12px','important');
      first.style.backgroundColor='red'; first.style.removeProperty('background-color');
      var link=document.createElement('link'); link.rel='stylesheet'; link.href='/site.css';
    "#).unwrap();
    assert_eq!(c.eval("document.querySelectorAll('#s > a.b:not(.active), #s > a.active').length").unwrap(), "2");
    assert_eq!(c.eval("first.matches('section > .active[data-k^=alpha]') && first.closest('#s').id==='s'").unwrap(), "true");
    assert_eq!(c.eval("first.style.getPropertyValue('margin-top')+'|'+first.style.getPropertyPriority('margin-top')+'|'+link.href").unwrap(), "12px|important|https://tuoitre.vn/site.css");
    assert!(c.take_browser_requests().is_empty());
    apply(&mut c, &mut arena);
}
#[test]
fn native_geometry_is_viewport_relative_and_empty_frames_retain_it() {
    let (mut c, _, body, _, _) = fixture();
    let mut f = frame(110, false); f.scroll_y = 50;
    f.layout.push(BrowserNodeLayout { node: body, x: 10.0, y: -20.0, width: 300.5, height: 80.0,
        styles: vec![(String::from("font-size"), String::from("17px")), (String::from("display"), String::from("block"))] });
    c.pump_browser(&f).unwrap();
    assert_eq!(c.eval("var rect=document.body.getBoundingClientRect(); [rect.top,rect.left,rect.right,rect.bottom,getComputedStyle(document.body).fontSize].join('|')").unwrap(), "-20|10|310.5|60|17px");
    c.pump_browser(&frame(120, false)).unwrap();
    assert_eq!(c.eval("document.body.getBoundingClientRect().width").unwrap(), "300.5");
    assert_eq!(c.eval("document.createElement('div').getBoundingClientRect().width").unwrap(), "0");
    assert!(c.eval("document.head.getBoundingClientRect()").is_err());
    f.now_ms = 130; f.layout[0].width = f32::NAN; assert!(c.pump_browser(&f).is_err());
}
#[test]
fn timers_cancel_bound_and_execute_only_from_native_monotonic_pumps() {
    let (mut c, _, _, _, _) = fixture();
    c.eval("var events=[]; var canceled=setTimeout(()=>events.push('bad'),1); clearTimeout(canceled); var interval=setInterval(()=>events.push('interval'),10); setTimeout(()=>{events.push('timeout');clearInterval(interval)},15); requestAnimationFrame(t=>events.push('raf:'+t));").unwrap();
    assert_eq!(c.next_browser_wakeup(), Some(110));
    c.pump_browser(&frame(110, false)).unwrap(); assert_eq!(c.eval("events.join(',')").unwrap(), "interval");
    c.pump_browser(&frame(120, false)).unwrap(); assert_eq!(c.eval("events.join(',')").unwrap(), "interval,timeout,raf:120");
    assert_eq!(c.next_browser_wakeup(), None); assert!(c.pump_browser(&frame(119, false)).is_err());
    c.eval("for(var i=0;i<256;i++)setTimeout(()=>{},1000)").unwrap();
    assert!(c.eval("setTimeout(()=>{},1)").is_err());
}
#[test]
fn document_and_window_events_bubble_once_with_listener_identity_preserved() {
    let (mut c, mut arena, body, _, _) = fixture();
    c.eval("var events=[]; document.addEventListener('DOMContentLoaded',()=>events.push('dom')); window.addEventListener('load',()=>events.push('load')); document.body.addEventListener('click',()=>events.push('body')); document.addEventListener('click',()=>events.push('capture'),true);").unwrap();
    c.sync_document(&arena, "again").unwrap();
    c.dispatch_event(&DomEvent { target: body, kind: dom_arena::EventKind::Click, client_x: 1, client_y: 2, key: None }).unwrap();
    c.pump_browser(&frame(110, true)).unwrap(); c.pump_browser(&frame(120, true)).unwrap();
    assert_eq!(c.eval("events.join(',')").unwrap(), "capture,body,dom,load");
    apply(&mut c, &mut arena);
}
#[test]
fn fetch_promises_use_actual_response_metadata_and_abort_cancels_native_request() {
    let (mut c, _, _, _, _) = fixture();
    c.eval("var answer; fetch('/api',{credentials:'include',headers:{'X-Test':'yes'}}).then(async r=>answer=[r.status,r.headers.get('X-Origin'),await r.json()]);").unwrap();
    let requests = c.take_browser_requests();
    let BrowserRequest::Fetch { id, url, credentials, headers, .. } = &requests[0] else { panic!("expected native fetch"); };
    assert_eq!(url, "https://tuoitre.vn/api"); assert_eq!(credentials, "include"); assert_eq!(headers[0], (String::from("x-test"), String::from("yes")));
    let mut response = reply(*id, "{\"real\":true}"); response.status = 201; c.complete_browser(&response).unwrap();
    assert_eq!(c.eval("JSON.stringify(answer)").unwrap(), "[201,\"native\",{\"real\":true}]");
    c.eval("var reason; var controller=new AbortController(); fetch('/cancel',{signal:controller.signal}).catch(e=>reason=e.name); controller.abort();").unwrap();
    let requests = c.take_browser_requests(); assert!(matches!(requests[0], BrowserRequest::Fetch { .. })); assert!(matches!(requests[1], BrowserRequest::Abort { .. }));
    assert_eq!(c.eval("reason").unwrap(), "AbortError");
}
#[test]
fn xhr_is_asynchronous_and_reports_headers_and_actual_http_failure_status() {
    let (mut c, _, _, _, _) = fixture();
    c.eval("var states=[]; var xhr=new XMLHttpRequest(); xhr.onreadystatechange=()=>states.push(xhr.readyState); xhr.onload=()=>states.push('load'); xhr.open('GET','/not-found'); xhr.send();").unwrap();
    let requests = c.take_browser_requests(); let BrowserRequest::Fetch { id, .. } = requests[0] else { panic!("expected fetch"); };
    assert_eq!(c.eval("xhr.readyState").unwrap(), "1");
    let mut r = reply(id, "not here"); r.status = 404; c.complete_browser(&r).unwrap();
    assert_eq!(c.eval("states.join(',')+'|'+xhr.status+'|'+xhr.responseText+'|'+xhr.getResponseHeader('X-Origin')").unwrap(), "1,2,3,4,load|404|not here|native");
    assert!(c.eval("new XMLHttpRequest().open('GET','/',false)").is_err());
}
#[test]
fn dynamic_script_executes_before_load_and_document_load_waits_for_resources() {
    let (mut c, mut arena, _, _, _) = fixture();
    c.eval("var events=[]; document.addEventListener('DOMContentLoaded',()=>events.push('dom')); window.addEventListener('load',()=>events.push('window')); var dynamic=document.createElement('script'); dynamic.src='/dynamic.js'; dynamic.onload=()=>events.push('script:'+window.fromNative); document.head.appendChild(dynamic);").unwrap();
    apply(&mut c, &mut arena);
    let requests = c.take_browser_requests(); let BrowserRequest::Load { id, kind, .. } = requests[0] else { panic!("expected load"); }; assert_eq!(kind, BrowserResourceKind::Script);
    c.pump_browser(&frame(110, true)).unwrap(); assert_eq!(c.eval("events.join(',')").unwrap(), "dom");
    c.complete_browser(&reply(id, "window.fromNative=document.currentScript===dynamic?'real':'wrong'; document.body.setAttribute('data-script','yes')")).unwrap();
    apply(&mut c, &mut arena);
    assert_eq!(c.eval("events.join(',')+'|'+document.readyState").unwrap(), "dom,script:real,window|complete");
}
#[test]
fn thrown_scripts_preserve_mutations_and_native_requests_and_dispatch_error() {
    let (mut c, mut arena, _, _, _) = fixture();
    assert!(c.eval("document.body.setAttribute('before','throw'); fetch('/still-send'); throw Error('script failed')").is_err());
    assert_eq!(c.take_browser_requests().len(), 1); apply(&mut c, &mut arena);
    c.eval("var errors=0;var bad=document.createElement('script');bad.src='/bad.js';bad.onerror=()=>errors++;document.head.appendChild(bad)").unwrap();
    apply(&mut c, &mut arena); let requests = c.take_browser_requests(); let BrowserRequest::Load { id, .. } = requests[0] else { panic!("load"); };
    assert!(c.complete_browser(&reply(id, "document.body.setAttribute('side-effect','retained'); throw Error('bad source')")).is_err());
    apply(&mut c, &mut arena); assert_eq!(c.eval("errors+'|'+document.body.getAttribute('side-effect')").unwrap(), "1|retained");
}
#[test]
fn removed_or_replaced_dynamic_resources_abort_and_stale_source_never_executes() {
    let (mut c, mut arena, _, _, _) = fixture();
    c.eval("var dynamic=document.createElement('script');dynamic.src='/old.js';document.head.appendChild(dynamic);dynamic.src='/new.js'").unwrap();
    apply(&mut c, &mut arena); let requests = c.take_browser_requests(); assert_eq!(requests.len(), 3);
    let BrowserRequest::Load { id: old, .. } = requests[0] else { panic!("load"); };
    assert!(matches!(requests[1], BrowserRequest::Abort { id } if id == old));
    assert!(c.complete_browser(&reply(old, "window.stale=true")).is_err());
    assert_eq!(c.eval("typeof stale").unwrap(), "undefined");
    c.eval("dynamic.remove()").unwrap(); apply(&mut c, &mut arena); assert!(matches!(c.take_browser_requests()[0], BrowserRequest::Abort { .. }));
}
#[test]
fn cookie_changes_are_requests_and_native_refresh_is_authoritative() {
    let (mut c, _, _, _, _) = fixture();
    c.eval("document.cookie='next=two; Path=/; Secure'").unwrap();
    assert_eq!(c.take_browser_requests(), vec![BrowserRequest::SetCookie { value: String::from("next=two; Path=/; Secure") }]);
    let mut f = frame(110, false); f.cookie = String::from("visible=one; next=two"); c.pump_browser(&f).unwrap();
    assert_eq!(c.eval("document.cookie").unwrap(), "visible=one; next=two");
    assert!(c.eval("document.cookie='bad=one\\nInjected: yes'").is_err());
}
#[test]
fn inner_html_scripts_are_inert_until_explicit_script_replacement() {
    let (mut c, mut arena, _, _, _) = fixture();
    c.eval("document.body.innerHTML='<script>window.executed=1</script>';var inert=document.body.firstChild;var active=document.createElement('script');active.textContent=inert.textContent;inert.replaceWith(active)").unwrap();
    apply(&mut c, &mut arena); assert_eq!(c.eval("executed").unwrap(), "1");
}
#[test]
fn reset_discards_browser_requests_timers_and_detached_document_state() {
    let (mut c, _, _, _, _) = fixture(); c.eval("fetch('/pending');setTimeout(()=>{},10);window.leaked=true").unwrap();
    c.reset(); assert!(c.take_browser_requests().is_empty()); assert_eq!(c.next_browser_wakeup(), None);
    assert_eq!(c.eval("typeof leaked + '|' + typeof document").unwrap(), "undefined|undefined");
}

std::thread_local! {
    static LAYOUT_ARENA: core::cell::RefCell<Option<DocumentArena>> = const { core::cell::RefCell::new(None) };
    static LAYOUT_QUERIES: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}
fn layout_host(query: &dom_arena::BrowserLayoutQuery) -> Result<BrowserFrame, String> {
    LAYOUT_QUERIES.with(|count| count.set(count.get() + 1));
    LAYOUT_ARENA.with(|slot| {
        let mut arena = slot.borrow_mut();
        let arena = arena.as_mut().unwrap();
        for mutation in &query.mutations {
            if !arena.apply_mutation(mutation) { return Err(String::from("Native rejected mutation")); }
        }
        assert!(arena.validate());
        let node = arena.get(query.node).ok_or_else(|| String::from("Missing requested node"))?;
        let width = node.get_attribute("style").and_then(|text| text.split("width:").nth(1))
            .and_then(|part| part.trim().split("px").next()).and_then(|text| text.parse::<f32>().ok()).unwrap_or(42.0);
        let mut frame = frame(110, false);
        frame.layout.push(BrowserNodeLayout { node: query.node, x: 12.0, y: 24.0, width, height: 10.0,
            styles: vec![(String::from("width"), alloc::format!("{width}px"))] });
        Ok(frame)
    })
}
#[test]
fn synchronous_c_host_layout_applies_drained_prefix_and_requeries_after_style_write() {
    let (mut c, arena, _, _, _) = fixture();
    LAYOUT_ARENA.with(|slot| *slot.borrow_mut() = Some(arena));
    LAYOUT_QUERIES.with(|count| count.set(0));
    c.native_layout.host = Some(layout_host);
    let result = c.eval("var probe=document.createElement('div');document.body.appendChild(probe);var firstWidth=probe.offsetWidth;probe.style.width='75px';firstWidth+'|'+probe.offsetWidth").unwrap();
    assert_eq!(result, "42|75");
    assert!(c.take_mutations().is_empty());
    assert_eq!(LAYOUT_QUERIES.with(|count| count.get()), 2);
    c.eval("probe.setAttribute('data-after','retained')").unwrap();
    assert_eq!(c.take_mutations().len(), 1);
}

#[test]
fn tree_walker_filters_text_and_native_font_readiness_and_media_changes_are_real() {
    let (mut c, mut arena, _, _, _) = fixture();
    c.eval("document.body.innerHTML='<div>one<span>two</span><!--skip--></div>';var walker=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT,null);var texts=[];for(var n;n=walker.nextNode();)texts.push(n.nodeValue);var fontsReady=false;document.fonts.ready.then(()=>fontsReady=true);var media=matchMedia('(min-width: 900px)');var changes=[];media.addEventListener('change',e=>changes.push(e.matches));").unwrap();
    apply(&mut c, &mut arena);
    assert_eq!(c.eval("texts.join('|')+'|'+fontsReady+'|'+document.fonts.size+'|'+media.matches").unwrap(), "one|two|false|0|false");
    let mut f = frame(110, false); f.width = 1000; c.pump_browser(&f).unwrap();
    assert_eq!(c.eval("fontsReady+'|'+media.matches+'|'+changes.join(',')").unwrap(), "true|true|true");
}
