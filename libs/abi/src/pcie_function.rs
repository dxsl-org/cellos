// SPDX-License-Identifier: Apache-2.0
//! PCIe function record (`QueryPcieFunction = 434`) and MSI/MSI-X allocation
//! result (`AllocPcieIrq = 435`); ADR-0031, CP1 §3–§4.
//!
//! [`ViPcieFunctionV1`] is a class K record (`docs/abi-boundary-records.md`):
//! syscall 434 with an exact out length of [`PCIE_FUNCTION_RECORD_LEN`], version
//! exactly 1, little-endian in declared field order, every reserved byte and
//! every unknown flag bit zero. Producer: the kernel. Consumer: the Driver Cell
//! that owns the function (ownership comes from 418/424).
//!
//! Layout (`off`, size): 0 `version` u16 · 2 `cap_count` u16 · 4 `flags` u32 ·
//! 8 `bdf` u32 · 12 `vendor_id` u16 · 14 `device_id` u16 ·
//! 16 `subsystem_vendor_id` u16 · 18 `subsystem_id` u16 · 20 `class` ·
//! 21 `subclass` · 22 `prog_if` · 23 `revision` · 24 reserved [8] ·
//! 32 `bars[6]` 24 bytes each (`base` u64, `len` u64, `kind` u32, `bar_flags` u32) ·
//! 176 `msix` (`table_size` u16, `table_bar` u8, `pba_bar` u8, `table_offset` u32,
//! `pba_offset` u32, reserved u32) · 192 `msi` (`control` u16, reserved u16,
//! reserved u32) · 200 `caps[16]` 24 bytes each (`id`, `offset`, `len`,
//! reserved, `bytes` [20]).

/// The only record version 434 writes and a v1 consumer accepts.
pub const PCIE_FUNCTION_VERSION: u16 = 1;
/// Exact `out_len` syscall 434 accepts and bytes it writes.
pub const PCIE_FUNCTION_RECORD_LEN: usize = 584;
/// BAR slots in a type-0 header.
pub const PCIE_FUNCTION_BARS: usize = 6;
/// Most standard capabilities one record carries.
pub const PCIE_FUNCTION_CAPS_MAX: usize = 16;
/// Capability bytes copied per entry (covers the 20-byte virtio notify cap).
pub const PCIE_CAP_BYTES: usize = 20;

/// `flags` bit: the function has an MSI-X capability.
pub const FLAG_MSIX_PRESENT: u32 = 1 << 0;
/// `flags` bit: the function has an MSI capability.
pub const FLAG_MSI_PRESENT: u32 = 1 << 1;
/// `flags` bit: FADT IAPC_BOOT_ARCH bit 3 says MSI must not be used.
pub const FLAG_MSI_FORBIDDEN_BY_FIRMWARE: u32 = 1 << 2;
/// `flags` bit: the kernel's VT-d DMA translation is active. A virtio device
/// that does not negotiate `VIRTIO_F_ACCESS_PLATFORM` bypasses translation, so
/// a virtio driver must refuse such a device when this bit is set (CP2-04a
/// amendment, 2026-11-04). It is not proof that this function is confined: on
/// a host with several DMA remapping units it can be set for a function under
/// a unit the kernel has not programmed yet (ADR-0031 phase 12).
pub const FLAG_DMA_REMAPPED: u32 = 1 << 3;
/// Every `flags` bit v1 defines.
pub const FLAGS_KNOWN: u32 =
    FLAG_MSIX_PRESENT | FLAG_MSI_PRESENT | FLAG_MSI_FORBIDDEN_BY_FIRMWARE | FLAG_DMA_REMAPPED;

/// BAR `kind`: absent, or the upper half of a 64-bit BAR.
pub const BAR_KIND_ABSENT: u32 = 0;
/// BAR `kind`: 32-bit memory.
pub const BAR_KIND_MEM32: u32 = 1;
/// BAR `kind`: 64-bit memory.
pub const BAR_KIND_MEM64: u32 = 2;
/// BAR `kind`: I/O ports.
pub const BAR_KIND_IO: u32 = 3;
/// `bar_flags` bit: prefetchable memory.
pub const BAR_FLAG_PREFETCHABLE: u32 = 1 << 0;
/// `bar_flags` bit: `RequestMmio` maps this BAR without the MSI-X table/PBA pages.
pub const BAR_FLAG_MSIX_CARVED: u32 = 1 << 1;
const BAR_FLAGS_KNOWN: u32 = BAR_FLAG_PREFETCHABLE | BAR_FLAG_MSIX_CARVED;

/// `AllocPcieIrq` mode bit: MSI-X may be used.
pub const IRQ_MODE_ALLOW_MSIX: usize = 1 << 0;
/// `AllocPcieIrq` mode bit: MSI may be used.
pub const IRQ_MODE_ALLOW_MSI: usize = 1 << 1;
/// Largest `want` 435 accepts.
pub const IRQ_WANT_MAX: usize = 32;
/// `AllocPcieIrq` result kind: MSI-X.
pub const IRQ_KIND_MSIX: u8 = 1;
/// `AllocPcieIrq` result kind: MSI.
pub const IRQ_KIND_MSI: u8 = 2;

/// 434/435 error sentinels. Generic failure and `PermissionDenied` are
/// `usize::MAX`; the rest are typed like 432/433.
pub const ERR_PERMISSION_DENIED: usize = usize::MAX;
/// 434: no such function.
pub const ERR_NOT_FOUND: usize = usize::MAX - 2;
/// Malformed arguments or buffer.
pub const ERR_INVALID_INPUT: usize = usize::MAX - 3;
/// 435: no allowed capability, firmware forbids MSI, or not x86.
pub const ERR_NOT_SUPPORTED: usize = usize::MAX - 5;
/// 435: vector space exhausted.
pub const ERR_NO_VECTORS: usize = usize::MAX - 6;

/// One BAR slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PcieBar {
    pub base: u64,
    pub len: u64,
    pub kind: u32,
    pub bar_flags: u32,
}

/// MSI-X capability summary; zero unless [`FLAG_MSIX_PRESENT`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PcieMsix {
    /// Table entries (N, not N-1).
    pub table_size: u16,
    pub table_bar: u8,
    pub pba_bar: u8,
    pub table_offset: u32,
    pub pba_offset: u32,
}

/// One standard capability (config offsets 0x40..0xFF), in walk order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PcieCap {
    pub id: u8,
    pub offset: u8,
    /// Valid bytes in `bytes`; the rest are zero.
    pub len: u8,
    pub bytes: [u8; PCIE_CAP_BYTES],
}

/// Decoded v1 record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViPcieFunctionV1 {
    pub cap_count: u16,
    pub flags: u32,
    pub bdf: u32,
    pub vendor_id: u16,
    pub device_id: u16,
    pub subsystem_vendor_id: u16,
    pub subsystem_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub revision: u8,
    pub bars: [PcieBar; PCIE_FUNCTION_BARS],
    pub msix: PcieMsix,
    /// MSI Message Control; zero unless [`FLAG_MSI_PRESENT`].
    pub msi_control: u16,
    pub caps: [PcieCap; PCIE_FUNCTION_CAPS_MAX],
}

const BARS_OFF: usize = 32;
const BAR_LEN: usize = 24;
const MSIX_OFF: usize = 176;
const MSI_OFF: usize = 192;
const CAPS_OFF: usize = 200;
const CAP_LEN: usize = 24;
const _: () = assert!(CAPS_OFF + PCIE_FUNCTION_CAPS_MAX * CAP_LEN == PCIE_FUNCTION_RECORD_LEN);
const _: () = assert!(BARS_OFF + PCIE_FUNCTION_BARS * BAR_LEN == MSIX_OFF);

fn put(out: &mut [u8], off: usize, bytes: &[u8]) {
    out[off..off + bytes.len()].copy_from_slice(bytes);
}

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    (u32_at(b, off) as u64) | ((u32_at(b, off + 4) as u64) << 32)
}

impl ViPcieFunctionV1 {
    /// Wire bytes. Entries past `cap_count` and cap bytes past `len` are written
    /// as zero whatever the struct holds.
    pub fn encode(&self) -> [u8; PCIE_FUNCTION_RECORD_LEN] {
        let mut out = [0u8; PCIE_FUNCTION_RECORD_LEN];
        put(&mut out, 0, &PCIE_FUNCTION_VERSION.to_le_bytes());
        put(&mut out, 2, &self.cap_count.to_le_bytes());
        put(&mut out, 4, &self.flags.to_le_bytes());
        put(&mut out, 8, &self.bdf.to_le_bytes());
        put(&mut out, 12, &self.vendor_id.to_le_bytes());
        put(&mut out, 14, &self.device_id.to_le_bytes());
        put(&mut out, 16, &self.subsystem_vendor_id.to_le_bytes());
        put(&mut out, 18, &self.subsystem_id.to_le_bytes());
        put(
            &mut out,
            20,
            &[self.class, self.subclass, self.prog_if, self.revision],
        );
        for (i, bar) in self.bars.iter().enumerate() {
            let at = BARS_OFF + i * BAR_LEN;
            put(&mut out, at, &bar.base.to_le_bytes());
            put(&mut out, at + 8, &bar.len.to_le_bytes());
            put(&mut out, at + 16, &bar.kind.to_le_bytes());
            put(&mut out, at + 20, &bar.bar_flags.to_le_bytes());
        }
        let m = &self.msix;
        put(&mut out, MSIX_OFF, &m.table_size.to_le_bytes());
        put(&mut out, MSIX_OFF + 2, &[m.table_bar, m.pba_bar]);
        put(&mut out, MSIX_OFF + 4, &m.table_offset.to_le_bytes());
        put(&mut out, MSIX_OFF + 8, &m.pba_offset.to_le_bytes());
        put(&mut out, MSI_OFF, &self.msi_control.to_le_bytes());
        let count = (self.cap_count as usize).min(PCIE_FUNCTION_CAPS_MAX);
        for (i, cap) in self.caps[..count].iter().enumerate() {
            let at = CAPS_OFF + i * CAP_LEN;
            let len = (cap.len as usize).min(PCIE_CAP_BYTES);
            put(&mut out, at, &[cap.id, cap.offset, len as u8]);
            put(&mut out, at + 4, &cap.bytes[..len]);
        }
        out
    }

    /// Strict v1 decode: exact length, version 1, known flag bits, a capability
    /// count within bounds, and every reserved or unused byte zero.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != PCIE_FUNCTION_RECORD_LEN || u16_at(bytes, 0) != PCIE_FUNCTION_VERSION {
            return None;
        }
        let mut rec = Self {
            cap_count: u16_at(bytes, 2),
            flags: u32_at(bytes, 4),
            bdf: u32_at(bytes, 8),
            vendor_id: u16_at(bytes, 12),
            device_id: u16_at(bytes, 14),
            subsystem_vendor_id: u16_at(bytes, 16),
            subsystem_id: u16_at(bytes, 18),
            class: bytes[20],
            subclass: bytes[21],
            prog_if: bytes[22],
            revision: bytes[23],
            msix: PcieMsix {
                table_size: u16_at(bytes, MSIX_OFF),
                table_bar: bytes[MSIX_OFF + 2],
                pba_bar: bytes[MSIX_OFF + 3],
                table_offset: u32_at(bytes, MSIX_OFF + 4),
                pba_offset: u32_at(bytes, MSIX_OFF + 8),
            },
            msi_control: u16_at(bytes, MSI_OFF),
            ..Self::default()
        };
        if rec.flags & !FLAGS_KNOWN != 0
            || rec.bdf > 0xFFFF
            || rec.cap_count as usize > PCIE_FUNCTION_CAPS_MAX
        {
            return None;
        }
        for (i, bar) in rec.bars.iter_mut().enumerate() {
            let at = BARS_OFF + i * BAR_LEN;
            *bar = PcieBar {
                base: u64_at(bytes, at),
                len: u64_at(bytes, at + 8),
                kind: u32_at(bytes, at + 16),
                bar_flags: u32_at(bytes, at + 20),
            };
            if bar.kind > BAR_KIND_IO || bar.bar_flags & !BAR_FLAGS_KNOWN != 0 {
                return None;
            }
        }
        for (i, cap) in rec.caps.iter_mut().enumerate() {
            let at = CAPS_OFF + i * CAP_LEN;
            let entry = &bytes[at..at + CAP_LEN];
            let len = entry[2] as usize;
            let in_use = i < rec.cap_count as usize;
            if entry[3] != 0
                || len > PCIE_CAP_BYTES
                || (!in_use && entry.iter().any(|&b| b != 0))
                || entry[4 + len..].iter().any(|&b| b != 0)
            {
                return None;
            }
            cap.id = entry[0];
            cap.offset = entry[1];
            cap.len = len as u8;
            cap.bytes[..len].copy_from_slice(&entry[4..4 + len]);
        }
        let reserved_zero = bytes[24..BARS_OFF].iter().all(|&b| b == 0)
            && bytes[MSIX_OFF + 12..MSI_OFF].iter().all(|&b| b == 0)
            && bytes[MSI_OFF + 2..CAPS_OFF].iter().all(|&b| b == 0);
        reserved_zero.then_some(rec)
    }

    /// The standard capability with `id`, first in walk order.
    pub fn cap(&self, id: u8) -> Option<&PcieCap> {
        self.caps[..(self.cap_count as usize).min(PCIE_FUNCTION_CAPS_MAX)]
            .iter()
            .find(|c| c.id == id)
    }
}

/// Decoded `AllocPcieIrq` success value: `first_irq | count << 8 | kind << 16`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcieIrqAlloc {
    /// First logical IRQ; wait with `WaitIrq(first_irq + i, 0)` or
    /// `WaitCompletion(IRQ)`, which covers every line of the allocation.
    pub first_irq: u8,
    pub count: u8,
    /// [`IRQ_KIND_MSIX`] or [`IRQ_KIND_MSI`].
    pub kind: u8,
}

impl PcieIrqAlloc {
    pub const fn pack(self) -> usize {
        self.first_irq as usize | (self.count as usize) << 8 | (self.kind as usize) << 16
    }

    pub const fn unpack(raw: usize) -> Option<Self> {
        let kind = ((raw >> 16) & 0xFF) as u8;
        let count = ((raw >> 8) & 0xFF) as u8;
        if raw >> 24 != 0 || count == 0 || (kind != IRQ_KIND_MSIX && kind != IRQ_KIND_MSI) {
            return None;
        }
        Some(Self {
            first_irq: raw as u8,
            count,
            kind,
        })
    }
}

#[cfg(test)]
#[path = "pcie_function_tests.rs"]
mod tests;
