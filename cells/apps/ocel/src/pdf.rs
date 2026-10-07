// SPDX-License-Identifier: MIT
//! Copied, sender-filtered IPC client for the isolated native PDF service.
//! No PDF parser or engine runs inside Ocel. Each load owns one document handle,
//! reads one bounded opaque BGRA page, and closes the handle before returning.

use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use ocel_pdf_proto::{Request, Response, IPC_BYTES, MAX_PAGE_PIXELS, PIXEL_CHUNK_BYTES};
use crate::lease::{KernelTransport, Lease, Transport};
use ocel_service_proto::Engine;

use crate::doc::{DocNode, StyledSpan};

const PAGE_BOUND: u32 = 1024;

pub fn is_pdf_path(url: &str) -> bool {
    let path = url.split('#').next().unwrap_or(url);
    path.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("pdf"))
}

/// A missing fragment selects page one; malformed or zero pages are errors,
/// never a request to feed binary PDF source into the text reader.
pub fn page_number(url: &str) -> Result<u32, String> {
    let Some((_, fragment)) = url.split_once('#') else {
        return Ok(1);
    };
    let invalid = || String::from("Invalid PDF page fragment. Use #page=N with a positive, one-based page number.");
    let value = fragment.strip_prefix("page=").ok_or_else(invalid)?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    value.parse::<u32>().ok().filter(|page| *page != 0).ok_or_else(invalid)
}

fn page_url(url: &str, page: u32) -> String {
    let base = url.split('#').next().unwrap_or(url);
    format!("{}#page={}", base, page)
}

pub struct PdfPage {
    page: u32,
    pages: u32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl PdfPage {
    pub fn into_nodes(self, original_url: &str) -> Vec<DocNode> {
        let mut spans = alloc::vec![StyledSpan::plain(&format!("Page {} of {}", self.page, self.pages))];
        if self.page > 1 {
            spans.push(StyledSpan::plain("   "));
            let mut previous = StyledSpan::plain("Previous page");
            previous.link = Some(page_url(original_url, self.page - 1));
            spans.push(previous);
        }
        if self.page < self.pages {
            spans.push(StyledSpan::plain("   "));
            let mut next = StyledSpan::plain("Next page");
            next.link = Some(page_url(original_url, self.page + 1));
            spans.push(next);
        }
        alloc::vec![
            DocNode::Paragraph { spans },
            DocNode::Image {
                width: self.width,
                height: self.height,
                pixels: Rc::new(self.pixels),
            },
        ]
    }
}

pub fn load_page(path: &str, page: u32) -> Result<PdfPage, String> {
    load_page_with(path, page, KernelTransport)
}

fn load_page_with(path: &str, page: u32, transport: impl Transport) -> Result<PdfPage, String> {
    let mut lease = Lease::acquire_with(Engine::Pdf, transport)?;
    let raster = load_with(path, page, |request| exchange(&mut lease, request));
    let released = lease.release();
    let raster = raster?;
    released?;
    Ok(raster)
}

fn exchange(lease: &mut Lease<impl Transport>, request: &Request) -> Result<Response, String> {
    let mut send_buf = [0u8; IPC_BYTES];
    let encoded = postcard::to_slice(request, &mut send_buf)
        .map_err(|_| String::from("PDF request exceeds the 4096-byte IPC limit."))?;
    let mut recv_buf = [0u8; IPC_BYTES];
    let len = lease.exchange(encoded, &mut recv_buf)?;
    postcard::from_bytes(&recv_buf[..len])
        .map_err(|_| String::from("The native PDF service returned a malformed IPC reply."))
}

fn load_with(
    path: &str,
    page: u32,
    mut exchange: impl FnMut(&Request) -> Result<Response, String>,
) -> Result<PdfPage, String> {
    let (document, pages) = match exchange(&Request::Open { path: String::from(path) })? {
        Response::Opened { document, pages } => (document, pages),
        Response::Error { message } => return Err(format!("Could not open PDF: {}", message)),
        _ => return Err(String::from("The PDF service returned an unexpected Open reply.")),
    };
    let rendered = read_page(document, pages, page, &mut exchange);
    // Every path after Opened attempts Close, including bad metadata, invalid
    // page numbers, render failures, and malformed or interrupted pixel chunks.
    let closed = match exchange(&Request::Close { document }) {
        Ok(Response::Closed) => Ok(()),
        Ok(Response::Error { message }) => Err(format!("Could not close PDF: {}", message)),
        Ok(_) => Err(String::from("The PDF service returned an unexpected Close reply.")),
        Err(error) => Err(error),
    };
    let raster = rendered?;
    closed?;
    Ok(raster)
}

fn read_page(
    document: u32,
    pages: u32,
    page: u32,
    exchange: &mut impl FnMut(&Request) -> Result<Response, String>,
) -> Result<PdfPage, String> {
    if pages == 0 {
        return Err(String::from("The PDF service reported a document with no pages."));
    }
    if page == 0 || page > pages {
        return Err(format!("PDF page {} is out of range. This document has {} pages.", page, pages));
    }
    let (width, height, total) = match exchange(&Request::RenderPage {
        document,
        page: page - 1,
        max_width: PAGE_BOUND,
        max_height: PAGE_BOUND,
    })? {
        Response::Rendered { width, height, bytes } => {
            (width, height, validated_size(width, height, bytes)?)
        }
        Response::Error { message } => return Err(format!("Could not render PDF page: {}", message)),
        _ => return Err(String::from("The PDF service returned an unexpected RenderPage reply.")),
    };
    let mut pixels = Vec::with_capacity(total);
    while pixels.len() < total {
        let offset = pixels.len();
        let length = (total - offset).min(PIXEL_CHUNK_BYTES);
        match exchange(&Request::ReadPixels {
            document,
            offset: offset as u32,
            length: length as u16,
        })? {
            Response::Pixels { offset: received_offset, bytes } => {
                if received_offset as usize != offset || bytes.len() != length {
                    return Err(String::from("The PDF service returned a non-contiguous or incorrectly sized pixel chunk."));
                }
                // Chunk size and all requested offsets are multiples of four.
                if bytes.chunks_exact(4).any(|pixel| pixel[3] != 255) {
                    return Err(String::from("The PDF service returned non-opaque pixels instead of opaque BGRA8888."));
                }
                pixels.extend_from_slice(&bytes);
            }
            Response::Error { message } => return Err(format!("Could not read PDF pixels: {}", message)),
            _ => return Err(String::from("The PDF service returned an unexpected ReadPixels reply.")),
        }
    }
    Ok(PdfPage { page, pages, width, height, pixels })
}

fn validated_size(width: u32, height: u32, bytes: u32) -> Result<usize, String> {
    let pixel_count = (width as usize).checked_mul(height as usize);
    let expected = pixel_count.and_then(|count| count.checked_mul(4));
    if width == 0 || height == 0 || width > PAGE_BOUND || height > PAGE_BOUND
        || !matches!(pixel_count, Some(count) if count <= MAX_PAGE_PIXELS)
        || expected != Some(bytes as usize)
    {
        return Err(String::from("The PDF service returned invalid raster dimensions or byte count (limit: 1024 x 1024, 1,048,576 pixels)."));
    }
    Ok(bytes as usize)
}

#[cfg(test)]
mod tests;
