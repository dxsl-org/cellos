//! Pure VirtIO-GPU wire codecs shared by the device and host-side tests.

extern crate alloc;
use alloc::vec::Vec;
pub const GET_DISPLAY_INFO: u32 = 0x0100;
pub const RESOURCE_CREATE_2D: u32 = 0x0101;
pub const RESOURCE_UNREF: u32 = 0x0102;
pub const SET_SCANOUT: u32 = 0x0103;
pub const RESOURCE_FLUSH: u32 = 0x0104;
pub const TRANSFER_TO_HOST_2D: u32 = 0x0105;
pub const RESOURCE_ATTACH_BACKING: u32 = 0x0106;
pub const UPDATE_CURSOR: u32 = 0x0300;
pub const MOVE_CURSOR: u32 = 0x0301;
pub const OK_NODATA: u32 = 0x1100;
pub const OK_DISPLAY_INFO: u32 = 0x1101;
pub const ERR_OUT_OF_MEMORY: u32 = 0x1201;
pub const ERR_INVALID_RESOURCE_ID: u32 = 0x1203;
pub const ERR_INVALID_PARAMETER: u32 = 0x1205;
pub const ERR_UNSPEC: u32 = 0x1200;
pub const FORMAT_B8G8R8A8_UNORM: u32 = 1;
pub const FORMAT_B8G8R8X8_UNORM: u32 = 2;

pub const CTRL_HDR_LEN: usize = 24;
const RECT_LEN: usize = 16;
const DISPLAY_ONE_LEN: usize = 24;
const MAX_SCANOUTS: usize = 16;
const MAX_ATTACH_ENTRIES: usize = 128;
pub const DISPLAY_INFO_LEN: usize = CTRL_HDR_LEN + DISPLAY_ONE_LEN * MAX_SCANOUTS;
#[derive(Clone, Copy)]
pub struct CtrlHeader {
    pub command: u32,
    pub flags: u32,
    pub fence_id: u64,
}
#[derive(Clone, Copy)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy)]
pub struct ResourceCreate2d {
    pub resource_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy)]
pub struct ResourceIdCmd {
    pub resource_id: u32,
}
#[derive(Clone, Copy)]
pub struct SetScanoutCmd {
    pub rect: Rect,
    pub scanout_id: u32,
    pub resource_id: u32,
}

#[derive(Clone, Copy)]
pub struct TransferToHost2dCmd {
    pub rect: Rect,
    pub offset: u64,
    pub resource_id: u32,
}

#[derive(Clone, Copy)]
pub struct FlushCmd {
    pub rect: Rect,
    pub resource_id: u32,
}

#[derive(Clone, Copy)]
pub struct CursorCmd {
    pub resource_id: u32,
    pub scanout_id: u32,
    pub x: u32,
    pub y: u32,
    pub hot_x: u32,
    pub hot_y: u32,
}

#[derive(Clone, Copy)]
pub struct MemEntry {
    pub addr: u64,
    pub length: u32,
}

pub fn parse_header(raw: &[u8]) -> Option<CtrlHeader> {
    Some(CtrlHeader {
        command: le32(raw, 0)?,
        flags: le32(raw, 4)?,
        fence_id: le64(raw, 8)?,
    })
}

pub fn parse_create_2d(raw: &[u8]) -> Option<ResourceCreate2d> {
    Some(ResourceCreate2d {
        resource_id: le32(raw, CTRL_HDR_LEN)?,
        format: le32(raw, CTRL_HDR_LEN + 4)?,
        width: le32(raw, CTRL_HDR_LEN + 8)?,
        height: le32(raw, CTRL_HDR_LEN + 12)?,
    })
}

pub fn parse_resource_id(raw: &[u8]) -> Option<ResourceIdCmd> {
    Some(ResourceIdCmd {
        resource_id: le32(raw, CTRL_HDR_LEN)?,
    })
}

pub fn parse_set_scanout(raw: &[u8]) -> Option<SetScanoutCmd> {
    Some(SetScanoutCmd {
        rect: rect(raw, CTRL_HDR_LEN)?,
        scanout_id: le32(raw, CTRL_HDR_LEN + RECT_LEN)?,
        resource_id: le32(raw, CTRL_HDR_LEN + RECT_LEN + 4)?,
    })
}

pub fn parse_transfer(raw: &[u8]) -> Option<TransferToHost2dCmd> {
    Some(TransferToHost2dCmd {
        rect: rect(raw, CTRL_HDR_LEN)?,
        offset: le64(raw, CTRL_HDR_LEN + RECT_LEN)?,
        resource_id: le32(raw, CTRL_HDR_LEN + RECT_LEN + 8)?,
    })
}

pub fn parse_flush(raw: &[u8]) -> Option<FlushCmd> {
    Some(FlushCmd {
        rect: rect(raw, CTRL_HDR_LEN)?,
        resource_id: le32(raw, CTRL_HDR_LEN + RECT_LEN)?,
    })
}

pub fn parse_cursor(raw: &[u8], with_resource: bool) -> Option<CursorCmd> {
    let pos = CTRL_HDR_LEN;
    Some(CursorCmd {
        scanout_id: le32(raw, pos)?,
        x: le32(raw, pos + 4)?,
        y: le32(raw, pos + 8)?,
        resource_id: if with_resource { le32(raw, pos + 16)? } else { 0 },
        hot_x: if with_resource { le32(raw, pos + 20)? } else { 0 },
        hot_y: if with_resource { le32(raw, pos + 24)? } else { 0 },
    })
}

pub fn parse_attach_backing(raw: &[u8]) -> Option<(u32, Vec<MemEntry>)> {
    let resource_id = le32(raw, CTRL_HDR_LEN)?;
    let count = le32(raw, CTRL_HDR_LEN + 4)? as usize;
    let required = CTRL_HDR_LEN
        .checked_add(8)?
        .checked_add(count.checked_mul(16)?)?;
    if count > MAX_ATTACH_ENTRIES || required > raw.len() {
        return None;
    }
    let mut entries = Vec::new();
    entries.try_reserve_exact(count).ok()?;
    let mut off = CTRL_HDR_LEN + 8;
    for _ in 0..count {
        entries.push(MemEntry {
            addr: le64(raw, off)?,
            length: le32(raw, off + 8)?,
        });
        off += 16;
    }
    Some((resource_id, entries))
}

pub fn display_info(header: CtrlHeader, width: u32, height: u32) -> [u8; DISPLAY_INFO_LEN] {
    let mut response = [0u8; DISPLAY_INFO_LEN];
    encode_header(&mut response[..CTRL_HDR_LEN], OK_DISPLAY_INFO, header);
    response[32..36].copy_from_slice(&width.to_le_bytes());
    response[36..40].copy_from_slice(&height.to_le_bytes());
    response[40..44].copy_from_slice(&1u32.to_le_bytes());
    response
}

pub fn encode_header(output: &mut [u8], response_type: u32, request: CtrlHeader) {
    output[0..4].copy_from_slice(&response_type.to_le_bytes());
    output[4..8].copy_from_slice(&request.flags.to_le_bytes());
    output[8..16].copy_from_slice(&request.fence_id.to_le_bytes());
}

fn rect(raw: &[u8], off: usize) -> Option<Rect> {
    Some(Rect {
        x: le32(raw, off)?,
        y: le32(raw, off + 4)?,
        width: le32(raw, off + 8)?,
        height: le32(raw, off + 12)?,
    })
}

fn le32(raw: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(raw.get(off..off + 4)?.try_into().ok()?))
}

fn le64(raw: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(raw.get(off..off + 8)?.try_into().ok()?))
}
