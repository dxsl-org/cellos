// SPDX-License-Identifier: AGPL-3.0-or-later
//! The only unsafe boundary: C owns MuPDF and catches all of its exceptions.
use alloc::{string::String, vec::Vec};
use core::{ffi::{c_char, c_int, c_void}, ptr::NonNull};
use ocel_pdf_proto::{MAX_DOCUMENT_BYTES, MAX_PAGE_PIXELS, PIXEL_CHUNK_BYTES};

extern "C" {
    fn ocel_pdf_open(input: *const u8, length: usize, pages: *mut u32,
        error: *mut c_char, capacity: usize) -> *mut c_void;
    fn ocel_pdf_close(document: *mut c_void);
    fn ocel_pdf_render(document: *mut c_void, page: u32, max_width: u32, max_height: u32,
        width: *mut u32, height: *mut u32, bytes: *mut u32,
        error: *mut c_char, capacity: usize) -> c_int;
    fn ocel_pdf_read_pixels(document: *mut c_void, offset: usize, output: *mut u8, length: usize) -> c_int;
}

pub struct Document {
    native: NonNull<c_void>,
    // fz_open_memory borrows these bytes. Drop closes native BEFORE Rust drops
    // the vector. Moving the Document never moves the vector's allocation.
    _input: Vec<u8>,
    pub pages: u32,
    raster_bytes: usize,
}

fn message(error: &[u8]) -> String {
    let length = error.iter().position(|&c| c == 0).unwrap_or(error.len());
    String::from_utf8_lossy(&error[..length]).into_owned()
}

impl Document {
    pub fn open(input: Vec<u8>) -> Result<Self, String> {
        if input.is_empty() || input.len() > MAX_DOCUMENT_BYTES {
            return Err(String::from("Invalid PDF document size"));
        }
        let mut pages = 0;
        let mut error = [0u8; 256];
        // SAFETY: inputs remain alive for the whole document lifetime; outputs
        // are writable and C returns normally after catching any exception.
        let pointer = unsafe { ocel_pdf_open(input.as_ptr(), input.len(), &mut pages,
            error.as_mut_ptr().cast(), error.len()) };
        let native = NonNull::new(pointer).ok_or_else(|| message(&error))?;
        Ok(Self { native, _input: input, pages, raster_bytes: 0 })
    }

    pub fn render(&mut self, page: u32, max_width: u32, max_height: u32) -> Result<(u32, u32, u32), String> {
        if page >= self.pages || max_width == 0 || max_height == 0 ||
            max_width as usize > MAX_PAGE_PIXELS || max_height as usize > MAX_PAGE_PIXELS {
            return Err(String::from("Invalid PDF page or raster dimensions"));
        }
        self.raster_bytes = 0;
        let (mut width, mut height, mut bytes) = (0, 0, 0);
        let mut error = [0u8; 256];
        // SAFETY: native is live, exclusively accessed, and C bounds output.
        let status = unsafe { ocel_pdf_render(self.native.as_ptr(), page, max_width, max_height,
            &mut width, &mut height, &mut bytes, error.as_mut_ptr().cast(), error.len()) };
        if status != 0 { return Err(message(&error)); }
        if width == 0 || height == 0 || width > max_width || height > max_height ||
            (width as usize).checked_mul(height as usize).filter(|&n| n <= MAX_PAGE_PIXELS)
                .and_then(|n| n.checked_mul(4)) != Some(bytes as usize) {
            return Err(String::from("Invalid MuPDF raster result"));
        }
        self.raster_bytes = bytes as usize;
        Ok((width, height, bytes))
    }

    pub fn read_pixels(&self, offset: usize, length: usize) -> Result<Vec<u8>, String> {
        if self.raster_bytes == 0 || length == 0 || length > PIXEL_CHUNK_BYTES ||
            offset.checked_add(length).filter(|&end| end <= self.raster_bytes).is_none() {
            return Err(String::from("Invalid PDF raster range"));
        }
        let mut output = Vec::new();
        output.try_reserve_exact(length).map_err(|_| String::from("Pixel allocation failed"))?;
        output.resize(length, 0);
        // SAFETY: C validates offset/length independently; output is initialized
        // writable storage of exactly length, and native remains owned by self.
        if unsafe { ocel_pdf_read_pixels(self.native.as_ptr(), offset, output.as_mut_ptr(), length) } != 0 {
            return Err(String::from("PDF raster is unavailable"));
        }
        Ok(output)
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        // SAFETY: this pointer is exclusively owned and closed exactly once.
        unsafe { ocel_pdf_close(self.native.as_ptr()); }
    }
}

#[no_mangle]
pub extern "C" fn ocel_pdf_exit(status: c_int) -> ! {
    ostd::syscall::sys_exit(status as usize)
}
