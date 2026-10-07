// SPDX-License-Identifier: AGPL-3.0-or-later
#![no_std]
#![no_main]

extern crate alloc;
extern crate api;
extern crate ostd;

use alloc::{string::String, vec::Vec};
use ocel_pdf_proto::{Request, Response, IPC_BYTES, MAX_DOCUMENT_BYTES};
use ostd::syscall::{sys_recv, sys_send, sys_yield, SyscallResult};

mod engine;

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_UNTRUSTED
);
api::declare_syscalls![Log, Recv, Send, Yield, Exit, LookupService, OpenCap, ReadCap, CloseCap];
ostd::declare_custom_heap!(16 * 1024 * 1024);
ostd::cell_main!(cell_main);

struct OwnedDocument {
    owner: usize,
    handle: u32,
    document: engine::Document,
}

fn read_document(path: &str) -> Result<Vec<u8>, String> {
    if path.is_empty() || path.len() > 512 || path.as_bytes().contains(&0) {
        return Err(String::from("Invalid PDF file path"));
    }
    let mut file = ostd::fs::File::open(path).map_err(|_| String::from("Cannot open PDF file"))?;
    let mut bytes = Vec::new();
    // Reserve once: no repeated copying of the bounded document. The extra
    // byte is read separately to reject oversized inputs rather than truncate.
    bytes.try_reserve_exact(MAX_DOCUMENT_BYTES).map_err(|_| String::from("PDF input allocation failed"))?;
    let result = (|| {
        let mut chunk = [0u8; 4096];
        loop {
            let remaining = MAX_DOCUMENT_BYTES - bytes.len();
            let count = file.read(&mut chunk[..remaining.min(4096).max(1)])
                .map_err(|_| String::from("Cannot read PDF file"))?;
            if count == 0 { break; }
            if count > remaining { return Err(String::from("PDF exceeds 2 MiB input limit")); }
            bytes.extend_from_slice(&chunk[..count]);
        }
        if bytes.is_empty() { return Err(String::from("PDF file is empty")); }
        Ok(bytes)
    })();
    let _ = file.close();
    result
}

fn handle(request: Request, owner: usize, active: &mut Option<OwnedDocument>, next: &mut u32) -> Result<Response, String> {
    match request {
        Request::Ping => Ok(Response::Ready),
        Request::Open { path } => {
            if active.is_some() { return Err(String::from("PDF service busy: close the active document first")); }
            if *next == 0 { return Err(String::from("PDF handle space exhausted")); }
            let document = engine::Document::open(read_document(&path)?)?;
            let pages = document.pages;
            let handle = *next;
            *next = next.checked_add(1).unwrap_or(0);
            *active = Some(OwnedDocument { owner, handle, document });
            Ok(Response::Opened { document: handle, pages })
        }
        Request::RenderPage { document, page, max_width, max_height } => {
            let live = active.as_mut().filter(|d| d.owner == owner && d.handle == document)
                .ok_or_else(|| String::from("Unknown PDF document handle"))?;
            let (width, height, bytes) = live.document.render(page, max_width, max_height)?;
            Ok(Response::Rendered { width, height, bytes })
        }
        Request::ReadPixels { document, offset, length } => {
            let live = active.as_ref().filter(|d| d.owner == owner && d.handle == document)
                .ok_or_else(|| String::from("Unknown PDF document handle"))?;
            let bytes = live.document.read_pixels(offset as usize, length as usize)?;
            Ok(Response::Pixels { offset, bytes })
        }
        Request::Close { document } => {
            if !active.as_ref().is_some_and(|d| d.owner == owner && d.handle == document) {
                return Err(String::from("Unknown PDF document handle"));
            }
            *active = None;
            Ok(Response::Closed)
        }
    }
}

fn cell_main() {
    init_custom_heap();
    ostd::io::println("[ocel-pdf] MuPDF 1.26.1 native PDF renderer ready (AGPL-3.0-or-later)");
    // Registered by init. This service never spawns cells or self-registers.
    let mut active = None;
    let mut next_handle = 1;
    let mut receive = [0u8; IPC_BYTES];
    let mut send = [0u8; IPC_BYTES];
    loop {
        receive.fill(0);
        match sys_recv(0, &mut receive) {
            SyscallResult::Ok(owner) if owner > 0 => {
                let operation = ostd::ipc::current();
                let response = match postcard::from_bytes::<Request>(&receive) {
                    Ok(request) => handle(request, owner, &mut active, &mut next_handle)
                        .unwrap_or_else(|message| Response::Error { message }),
                    Err(_) => Response::Error { message: String::from("Malformed PDF request") },
                };
                if let Ok(encoded) = postcard::to_slice(&response, &mut send) {
                    if let Some(operation) = operation {
                        let _ = ostd::ipc::reply(operation, encoded);
                    } else {
                        let _ = sys_send(owner, encoded);
                    }
                }
            }
            _ => sys_yield(),
        }
    }
}
