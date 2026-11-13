// SPDX-License-Identifier: Apache-2.0
//! Per-device block routes (`RegisterBlockDriver = 416`, `LookupBlockRoute = 432`).
//!
//! Each storage device the kernel has seen a block Driver Cell register for has one
//! route slot, bound to that device for the boot. Only the Cell that owns the device
//! (its PCI BDF lease or its exact MMIO window) may (re)publish the slot's provider,
//! so a restarted driver regains its own route and never another device's.
//!
//! [`ViBlockRouteV1`] is a class K record (`docs/abi-boundary-records.md`): syscall
//! 432 plus exact length, version exactly 1, `kind` in {1, 2}, flag bits outside
//! [`FLAGS_KNOWN`] refused, provider fields zero unless [`FLAG_PROVIDER_LIVE`],
//! little-endian in declared field order. Producer: kernel. Consumers: VFS, init.

/// Exact byte length of [`ViBlockRouteV1`].
pub const BLOCK_ROUTE_RECORD_LEN: usize = 40;
/// The only record version this layout writes and accepts.
pub const BLOCK_ROUTE_VERSION: u16 = 1;
/// Route slots the kernel keeps; 432 indices are `0..MAX_BLOCK_ROUTES`.
pub const MAX_BLOCK_ROUTES: usize = 8;

/// Device kind: a PCI function; `id` is the kernel BDF encoding `bus<<8 | dev<<3 | fn`.
pub const BLOCK_DEVICE_PCI: u16 = 1;
/// Device kind: a platform MMIO window; `id` is its exact physical base.
pub const BLOCK_DEVICE_MMIO: u16 = 2;

/// The route has a live provider; the provider fields name it.
pub const FLAG_PROVIDER_LIVE: u32 = 1 << 0;
/// Every defined flag bit.
pub const FLAGS_KNOWN: u32 = FLAG_PROVIDER_LIVE;

/// A storage device as named in 416 arguments and in the route record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeviceKey {
    /// [`BLOCK_DEVICE_PCI`] or [`BLOCK_DEVICE_MMIO`].
    pub kind: u16,
    /// BDF or MMIO base, per `kind`.
    pub id: u64,
}

impl DeviceKey {
    /// `Some` only for a key a 416 call or a route record may carry.
    pub fn new(kind: u16, id: u64) -> Option<Self> {
        let valid = match kind {
            BLOCK_DEVICE_PCI => id <= 0xFFFF,
            BLOCK_DEVICE_MMIO => id != 0,
            _ => false,
        };
        valid.then_some(Self { kind, id })
    }
}

/// One route slot, version 1. Field order is the wire order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViBlockRouteV1 {
    /// [`BLOCK_ROUTE_VERSION`].
    pub version: u16,
    /// Device kind of the slot's key.
    pub kind: u16,
    /// [`FLAG_PROVIDER_LIVE`] or zero.
    pub flags: u32,
    /// Device id of the slot's key.
    pub id: u64,
    /// Live provider task; zero when not live.
    pub provider_tid: u64,
    /// Live provider Cell; zero when not live.
    pub cell_id: u64,
    /// Live provider generation; zero when not live.
    pub generation: u64,
}

const _: () = assert!(core::mem::size_of::<ViBlockRouteV1>() == BLOCK_ROUTE_RECORD_LEN);

/// Why bytes are not a valid V1 record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteRecordError {
    /// Length is not exactly [`BLOCK_ROUTE_RECORD_LEN`].
    Length,
    /// `version` is not [`BLOCK_ROUTE_VERSION`].
    UnsupportedVersion,
    /// `kind`/`id` is not a valid [`DeviceKey`].
    Device,
    /// `flags` has a bit outside [`FLAGS_KNOWN`].
    UnknownFlag,
    /// Provider fields disagree with [`FLAG_PROVIDER_LIVE`].
    Liveness,
}

impl ViBlockRouteV1 {
    /// The slot's device key.
    pub fn device(&self) -> DeviceKey {
        DeviceKey {
            kind: self.kind,
            id: self.id,
        }
    }

    /// The live provider's `(tid, cell_id, generation)`, if any.
    pub fn provider(&self) -> Option<(u64, u64, u64)> {
        (self.flags & FLAG_PROVIDER_LIVE != 0).then_some((
            self.provider_tid,
            self.cell_id,
            self.generation,
        ))
    }

    /// Little-endian wire bytes.
    pub fn encode(&self) -> [u8; BLOCK_ROUTE_RECORD_LEN] {
        let mut out = [0u8; BLOCK_ROUTE_RECORD_LEN];
        out[0..2].copy_from_slice(&self.version.to_le_bytes());
        out[2..4].copy_from_slice(&self.kind.to_le_bytes());
        out[4..8].copy_from_slice(&self.flags.to_le_bytes());
        let words = [self.id, self.provider_tid, self.cell_id, self.generation];
        for (i, v) in words.iter().enumerate() {
            out[8 + i * 8..16 + i * 8].copy_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// Decode and fully validate wire bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, RouteRecordError> {
        if bytes.len() != BLOCK_ROUTE_RECORD_LEN {
            return Err(RouteRecordError::Length);
        }
        let le = |at: usize, n: usize| {
            bytes[at..at + n]
                .iter()
                .rev()
                .fold(0u64, |acc, b| acc << 8 | u64::from(*b))
        };
        let record = Self {
            version: le(0, 2) as u16,
            kind: le(2, 2) as u16,
            flags: le(4, 4) as u32,
            id: le(8, 8),
            provider_tid: le(16, 8),
            cell_id: le(24, 8),
            generation: le(32, 8),
        };
        if record.version != BLOCK_ROUTE_VERSION {
            return Err(RouteRecordError::UnsupportedVersion);
        }
        if DeviceKey::new(record.kind, record.id).is_none() {
            return Err(RouteRecordError::Device);
        }
        if record.flags & !FLAGS_KNOWN != 0 {
            return Err(RouteRecordError::UnknownFlag);
        }
        let consistent = if record.flags & FLAG_PROVIDER_LIVE != 0 {
            record.provider_tid != 0
        } else {
            record.provider_tid == 0 && record.cell_id == 0 && record.generation == 0
        };
        if !consistent {
            return Err(RouteRecordError::Liveness);
        }
        Ok(record)
    }
}

#[cfg(test)]
#[path = "block_route_tests.rs"]
mod tests;
