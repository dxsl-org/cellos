// SPDX-License-Identifier: MIT
use super::*;

#[test]
fn pdf_extensions_and_one_based_fragments() {
    assert!(is_pdf_path("file:///data/Guide.PdF#page=2"));
    assert!(is_pdf_path("/data/guide.pdf"));
    assert!(!is_pdf_path("/data/guide.pdf.txt"));
    assert!(!is_pdf_path("pdf"));
    assert_eq!(page_number("file:///data/guide.pdf").unwrap(), 1);
    assert_eq!(page_number("file:///data/guide.pdf#page=2").unwrap(), 2);
    for fragment in ["page=0", "page=-1", "page=+2", "page=", "page=1#page=2", "page=4294967296", "other=2"] {
        assert!(page_number(&format!("file:///guide.pdf#{}", fragment)).is_err());
    }
    assert_eq!(page_url("file:///data/Guide.PDF#page=4", 3), "file:///data/Guide.PDF#page=3");
    assert_eq!(page_url("/data/guide.pdf", 2), "/data/guide.pdf#page=2");
}

#[test]
fn raster_metadata_is_bounded_and_consistent() {
    assert_eq!(validated_size(1024, 1024, 4 * 1024 * 1024).unwrap(), 4 * 1024 * 1024);
    for (width, height, bytes) in [(0, 1, 0), (1, 0, 0), (1025, 1, 4100), (1, 1025, 4100), (1, 1, 3), (u32::MAX, u32::MAX, 4)] {
        assert!(validated_size(width, height, bytes).is_err());
    }
}


#[test]
fn first_last_and_single_page_links_do_not_escape_page_range() {
    for (page, pages, expected_links) in [
        (1, 3, alloc::vec!["/guide.PDF#page=2"]),
        (3, 3, alloc::vec!["/guide.PDF#page=2"]),
        (1, 1, alloc::vec![]),
    ] {
        let nodes = PdfPage { page, pages, width: 1, height: 1, pixels: alloc::vec![0, 0, 0, 255] }.into_nodes("/guide.PDF");
        let DocNode::Paragraph { spans } = &nodes[0] else { panic!("Missing navigation") };
        let links: Vec<&str> = spans.iter().filter_map(|span| span.link.as_deref()).collect();
        assert_eq!(links, expected_links);
    }
}

#[test]
fn every_failure_after_open_attempts_close() {
    // Bad page metadata, invalid page numbers, render errors, wrong response
    // types, malformed chunks and transport failures all share the close path.
    for failure in 0..11 {
        let mut closed = false;
        let result = load_with("/guide.pdf", if failure == 1 { 3 } else { 1 }, |request| {
            match request {
                Request::Open { .. } => Ok(Response::Opened { document: 9, pages: if failure == 0 { 0 } else { 2 } }),
                Request::RenderPage { .. } => match failure {
                    2 => Ok(Response::Error { message: String::from("encrypted PDF requires a password") }),
                    3 => Err(String::from("service disconnected")),
                    4 => Ok(Response::Rendered { width: 1, height: 1, bytes: 3 }),
                    5 => Ok(Response::Closed),
                    _ => Ok(Response::Rendered { width: 1, height: 1, bytes: 4 }),
                },
                Request::ReadPixels { .. } => match failure {
                    6 => Ok(Response::Pixels { offset: 4, bytes: alloc::vec![0, 0, 0, 255] }),
                    7 => Ok(Response::Pixels { offset: 0, bytes: alloc::vec![] }),
                    8 => Ok(Response::Pixels { offset: 0, bytes: alloc::vec![0, 0, 0, 255, 0, 0, 0, 255] }),
                    9 => Ok(Response::Pixels { offset: 0, bytes: alloc::vec![0, 0, 0, 0] }),
                    _ => Err(String::from("pixel read interrupted")),
                },
                Request::Close { document } => {
                    assert_eq!(*document, 9);
                    closed = true;
                    Ok(Response::Closed)
                }
                Request::Ping => panic!("load does not issue readiness probes"),
            }
        });
        assert!(result.is_err(), "failure case {} unexpectedly succeeded", failure);
        assert!(closed, "failure case {} leaked its document", failure);
    }
}

#[test]
fn close_failure_is_not_reported_as_a_successful_load() {
    let result = load_with("/guide.pdf", 1, |request| Ok(match request {
        Request::Open { .. } => Response::Opened { document: 1, pages: 1 },
        Request::RenderPage { .. } => Response::Rendered { width: 1, height: 1, bytes: 4 },
        Request::ReadPixels { .. } => Response::Pixels { offset: 0, bytes: alloc::vec![1, 2, 3, 255] },
        Request::Close { .. } => Response::Error { message: String::from("close failed") },
        Request::Ping => panic!("load does not issue readiness probes"),
    }));
    assert!(result.err().unwrap().contains("close failed"));
}

#[test]
fn maximum_pixel_chunk_fits_copied_ipc_buffer() {
    let response = Response::Pixels { offset: 4 * 1024 * 1024 - 3072, bytes: alloc::vec![255; PIXEL_CHUNK_BYTES] };
    let mut buffer = [0u8; IPC_BYTES];
    let encoded_len = postcard::to_slice(&response, &mut buffer).unwrap().len();
    assert!(encoded_len < IPC_BYTES);
}

#[derive(Default)]
struct LeaseTrace {
    operations: Vec<&'static str>,
    render_failure: bool,
}
struct PdfTransport(alloc::rc::Rc<core::cell::RefCell<LeaseTrace>>);
impl Transport for PdfTransport {
    fn lookup(&mut self, service: u16) -> Option<usize> {
        Some(if service == api::syscall::service::OCEL_ACTIVATOR { 10 } else { 20 })
    }
    fn call(&mut self, peer: usize, request: &[u8], reply: &mut [u8], ticks: u64) -> Result<usize, String> {
        assert!(ticks > 0);
        let mut trace = self.0.borrow_mut();
        let bytes = if peer == 10 {
            use ocel_service_proto::{Request as Activation, Response as Activated};
            match Activation::decode(request).unwrap() {
                Activation::Acquire { engine } => {
                    assert_eq!(engine, Engine::Pdf);
                    trace.operations.push("acquire");
                    Activated::Ready { engine, lease: 9, tid: 20 }.encode().to_vec()
                }
                Activation::Release { engine, lease } => {
                    assert_eq!((engine, lease), (Engine::Pdf, 9));
                    trace.operations.push("release");
                    Activated::Released { engine }.encode().to_vec()
                }
            }
        } else {
            assert_eq!(peer, 20);
            let response = match postcard::from_bytes::<Request>(request).unwrap() {
                Request::Open { path } => {
                    assert_eq!(path, "/guide.pdf");
                    trace.operations.push("open");
                    Response::Opened { document: 1, pages: 2 }
                }
                Request::RenderPage { page, .. } => {
                    assert!(page < 2);
                    trace.operations.push("render");
                    if trace.render_failure { Response::Error { message: String::from("render failed") } }
                    else { Response::Rendered { width: 1, height: 1, bytes: 4 } }
                }
                Request::ReadPixels { offset, length, .. } => {
                    assert_eq!((offset, length), (0, 4));
                    trace.operations.push("pixels");
                    Response::Pixels { offset: 0, bytes: alloc::vec![1, 2, 3, 255] }
                }
                Request::Close { .. } => { trace.operations.push("close"); Response::Closed }
                Request::Ping => panic!("viewer does not issue init's readiness probes"),
            };
            postcard::to_allocvec(&response).unwrap()
        };
        reply[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}

#[test]
fn raster_is_copied_before_release_and_next_page_acquires_again() {
    let trace = alloc::rc::Rc::new(core::cell::RefCell::new(LeaseTrace::default()));
    let first = load_page_with("/guide.pdf", 1, PdfTransport(trace.clone())).unwrap();
    assert_eq!(trace.borrow().operations, alloc::vec!["acquire", "open", "render", "pixels", "close", "release"]);
    assert_eq!(first.pixels, alloc::vec![1, 2, 3, 255]);
    let second = load_page_with("/guide.pdf", 2, PdfTransport(trace.clone())).unwrap();
    assert_eq!(second.page, 2);
    assert_eq!(&trace.borrow().operations[6..], &["acquire", "open", "render", "pixels", "close", "release"]);
    // Both owned rasters survive after all service/lease operations are gone.
    assert_eq!(first.pixels, second.pixels);
}

#[test]
fn render_failure_closes_document_then_releases_demand() {
    let trace = alloc::rc::Rc::new(core::cell::RefCell::new(LeaseTrace { render_failure: true, ..LeaseTrace::default() }));
    assert!(load_page_with("/guide.pdf", 1, PdfTransport(trace.clone())).is_err());
    assert_eq!(trace.borrow().operations, alloc::vec!["acquire", "open", "render", "close", "release"]);
}
