//! VirtIO-GPU queue I/O around the pure wire codecs.

extern crate alloc;

use crate::virtqueue::DescBuf;
use alloc::vec::Vec;

pub use super::wire::*;

const MAX_REQUEST_LEN: usize = 64 * 1024;

pub fn read_header(bufs: &[DescBuf], vm_id: usize) -> Option<CtrlHeader> {
    parse_header(&read_request_bytes(bufs, vm_id)?)
}

pub fn parse_create_2d(bufs: &[DescBuf], vm_id: usize) -> Option<ResourceCreate2d> {
    super::wire::parse_create_2d(&read_request_bytes(bufs, vm_id)?)
}

pub fn parse_resource_id(bufs: &[DescBuf], vm_id: usize) -> Option<ResourceIdCmd> {
    super::wire::parse_resource_id(&read_request_bytes(bufs, vm_id)?)
}

pub fn parse_set_scanout(bufs: &[DescBuf], vm_id: usize) -> Option<SetScanoutCmd> {
    super::wire::parse_set_scanout(&read_request_bytes(bufs, vm_id)?)
}

pub fn parse_transfer_to_host_2d(bufs: &[DescBuf], vm_id: usize) -> Option<TransferToHost2dCmd> {
    super::wire::parse_transfer(&read_request_bytes(bufs, vm_id)?)
}

pub fn parse_flush(bufs: &[DescBuf], vm_id: usize) -> Option<FlushCmd> {
    super::wire::parse_flush(&read_request_bytes(bufs, vm_id)?)
}

pub fn parse_cursor(bufs: &[DescBuf], vm_id: usize, with_resource: bool) -> Option<CursorCmd> {
    super::wire::parse_cursor(&read_request_bytes(bufs, vm_id)?, with_resource)
}

pub fn parse_attach_backing(
    bufs: &[DescBuf],
    vm_id: usize,
) -> Option<(CtrlHeader, u32, Vec<MemEntry>)> {
    let raw = read_request_bytes(bufs, vm_id)?;
    let header = parse_header(&raw)?;
    let (resource_id, entries) = super::wire::parse_attach_backing(&raw)?;
    Some((header, resource_id, entries))
}

pub fn write_display_info(bufs: &[DescBuf], vm_id: usize, response: &[u8]) -> u32 {
    write_bytes(bufs, vm_id, response)
}

pub fn write_ok(bufs: &[DescBuf], vm_id: usize, header: CtrlHeader) -> u32 {
    write_response(bufs, vm_id, OK_NODATA, header)
}

pub fn write_error(bufs: &[DescBuf], vm_id: usize, header: CtrlHeader, code: u32) -> u32 {
    write_response(bufs, vm_id, code, header)
}

fn read_request_bytes(bufs: &[DescBuf], vm_id: usize) -> Option<Vec<u8>> {
    let need = bufs
        .iter()
        .filter(|buf| !buf.writable)
        .try_fold(0usize, |sum, buf| sum.checked_add(buf.len as usize))?;
    if need > MAX_REQUEST_LEN {
        return None;
    }
    let mut raw = Vec::new();
    raw.try_reserve_exact(need).ok()?;
    for buf in bufs.iter().filter(|buf| !buf.writable) {
        let start = raw.len();
        raw.resize(start + buf.len as usize, 0);
        if crate::vmm::read_guest_memory(vm_id, buf.gpa, &mut raw[start..]) != buf.len as usize {
            return None;
        }
    }
    Some(raw)
}

fn write_response(bufs: &[DescBuf], vm_id: usize, response_type: u32, request: CtrlHeader) -> u32 {
    let mut response = [0u8; CTRL_HDR_LEN];
    encode_header(&mut response, response_type, request);
    write_bytes(bufs, vm_id, &response)
}

fn write_bytes(bufs: &[DescBuf], vm_id: usize, response: &[u8]) -> u32 {
    let capacity = bufs
        .iter()
        .filter(|buf| buf.writable)
        .try_fold(0usize, |sum, buf| sum.checked_add(buf.len as usize));
    if capacity.unwrap_or(0) < response.len() {
        return 0;
    }
    let mut written = 0usize;
    for buf in bufs.iter().filter(|buf| buf.writable) {
        let count = (buf.len as usize).min(response.len() - written);
        if count == 0 {
            break;
        }
        if crate::vmm::write_guest_memory(vm_id, buf.gpa, &response[written..written + count])
            != count
        {
            return 0;
        }
        written += count;
    }
    written as u32
}
