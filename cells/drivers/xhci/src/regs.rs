//! xHCI register offsets, PORTSC bits, TRB layout, and context field encodings.
//!
//! Layouts follow the xHCI specification (Rev 1.1/1.2) as cross-checked against
//! the iPXE `include/ipxe/xhci.h` definitions. The driver only supports 32-byte
//! contexts (HCCPARAMS1.CSZ = 0), which is what `qemu-xhci` and every USB2/3
//! xHCI implementation this milestone targets advertise; CSZ = 1 fails closed.

// ── Capability registers (offsets from the BAR base) ──────────────────────────

pub const CAP_CAPLENGTH: usize = 0x00;
pub const CAP_HCSPARAMS1: usize = 0x04;
pub const CAP_HCSPARAMS2: usize = 0x08;
pub const CAP_HCCPARAMS1: usize = 0x10;
pub const CAP_DBOFF: usize = 0x14;
pub const CAP_RTSOFF: usize = 0x18;

// ── Operational registers (offsets from BAR + CAPLENGTH) ─────────────────────

pub const OP_USBCMD: usize = 0x00;
pub const OP_USBSTS: usize = 0x04;
pub const OP_CRCR: usize = 0x18;
pub const OP_DCBAAP: usize = 0x30;
pub const OP_CONFIG: usize = 0x38;
/// First port register set; each subsequent port is +0x10.
pub const OP_PORT_BASE: usize = 0x400;
pub const OP_PORT_STRIDE: usize = 0x10;

pub const USBCMD_RS: u32 = 1 << 0;
pub const USBCMD_HCRST: u32 = 1 << 1;
pub const USBSTS_HCH: u32 = 1 << 0;
pub const USBSTS_CNR: u32 = 1 << 11;

// ── Interrupter 0 registers (offsets from BAR + RTSOFF) ──────────────────────

pub const RT_IMAN: usize = 0x20;
pub const RT_ERSTSZ: usize = 0x28;
pub const RT_ERSTBA: usize = 0x30;
pub const RT_ERDP: usize = 0x38;

// ── PORTSC bits ──────────────────────────────────────────────────────────────

pub const PORTSC_CCS: u32 = 1 << 0;
pub const PORTSC_PED: u32 = 1 << 1;
pub const PORTSC_PR: u32 = 1 << 4;
pub const PORTSC_PP: u32 = 1 << 9;
pub const PORTSC_PIC_MASK: u32 = 3 << 14;
pub const PORTSC_CSC: u32 = 1 << 17;
pub const PORTSC_PEC: u32 = 1 << 18;
pub const PORTSC_WRC: u32 = 1 << 19;
pub const PORTSC_OCC: u32 = 1 << 20;
pub const PORTSC_PRC: u32 = 1 << 21;
pub const PORTSC_PLC: u32 = 1 << 22;
pub const PORTSC_CEC: u32 = 1 << 23;
/// Every RW1C change bit; writing a 1 acknowledges (clears) it.
pub const PORTSC_CHANGE: u32 =
    PORTSC_CSC | PORTSC_PEC | PORTSC_WRC | PORTSC_OCC | PORTSC_PRC | PORTSC_PLC | PORTSC_CEC;
/// Bits that must be written back unchanged when modifying PORTSC. PED is
/// deliberately absent: writing a 1 to PED disables the port.
pub const PORTSC_PRESERVE: u32 = PORTSC_PP | PORTSC_PIC_MASK;

/// USB port speed IDs (PORTSC bits 13:10).
pub const SPEED_HIGH: u32 = 3;

// ── TRBs ─────────────────────────────────────────────────────────────────────

pub const TRB_SIZE: usize = 16;

/// One transfer request block (16 bytes, little-endian).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Trb {
    pub parameter: u64,
    pub status: u32,
    pub control: u32,
}

pub const TRB_CYCLE: u32 = 1 << 0;
pub const TRB_ENT: u32 = 1 << 1;
pub const TRB_CHAIN: u32 = 1 << 4;
pub const TRB_IOC: u32 = 1 << 5;
pub const TRB_IDT: u32 = 1 << 6;
/// Data/Status stage direction: 1 = IN (device to host).
pub const TRB_DIR_IN: u32 = 1 << 16;

/// TRB type occupies control bits 15:10.
pub const fn trb_type(kind: u32) -> u32 {
    kind << 10
}

pub const TYPE_NORMAL: u32 = 1;
pub const TYPE_SETUP: u32 = 2;
pub const TYPE_DATA: u32 = 3;
pub const TYPE_STATUS: u32 = 4;
pub const TYPE_LINK: u32 = 6;
pub const TYPE_ENABLE_SLOT: u32 = 9;
pub const TYPE_ADDRESS_DEVICE: u32 = 11;
pub const TYPE_CONFIGURE_ENDPOINT: u32 = 12;
pub const TYPE_EVALUATE_CONTEXT: u32 = 13;
pub const TYPE_TRANSFER_EVENT: u32 = 32;
pub const TYPE_COMMAND_COMPLETE: u32 = 33;

/// Setup-stage transfer type (control bits 17:16).
pub const TRT_NO_DATA: u32 = 0;
pub const TRT_IN: u32 = 3;

/// Completion code: success.
pub const CMPLT_SUCCESS: u8 = 1;
/// Completion code: short packet (a valid partial transfer).
pub const CMPLT_SHORT_PACKET: u8 = 13;

/// `control` field of a Transfer Event: endpoint ID bits 20:16, slot bits 31:24.
pub const fn event_ep_id(control: u32) -> u8 {
    ((control >> 16) & 0x1F) as u8
}
pub const fn event_slot(control: u32) -> u8 {
    (control >> 24) as u8
}
pub const fn event_type(control: u32) -> u32 {
    (control >> 10) & 0x3F
}
/// Completion code of a Transfer/Command Completion Event (status bits 31:24).
pub const fn event_code(status: u32) -> u8 {
    (status >> 24) as u8
}
/// Residual bytes of a Transfer Event (status bits 23:0).
pub const fn event_residual(status: u32) -> u32 {
    status & 0x00FF_FFFF
}

// ── Contexts (32-byte stride; CSZ = 0) ───────────────────────────────────────

pub const CTX_STRIDE: usize = 32;
/// Input Control Context: Add flags (dword 1). Drop flags (dword 0) stay zero
/// because this driver only ever adds contexts.
pub const ICC_ADD: usize = 4;
/// Slot Context starts after the 8-dword Input Control Context.
pub const SLOT_CTX: usize = 32;
/// Input-context offset of the Endpoint Context for Device Context Index `dci`.
///
/// The DCI is `2 * endpoint_number + direction` (IN = 1): DCI 1 = EP0,
/// DCI 2 = EP1 OUT, DCI 3 = EP1 IN. The Input Control Context occupies the
/// first context slot, so context `dci` lives at `32 * (dci + 1)`.
pub const fn ep_ctx(dci: u8) -> usize {
    32 + (dci as usize) * CTX_STRIDE
}

/// Input Control Context Add/Drop flag for a context index (0 = slot, 1 = EP0…).
pub const fn ctx_flag(index: u8) -> u32 {
    1 << index
}

/// Slot Context dword 0: Context Entries (31:27), Hub (26), Speed (23:20), Route.
pub const fn slot_info(context_entries: u32, speed: u32) -> u32 {
    (context_entries << 27) | (speed << 20)
}

/// Endpoint Context dword 1: MaxPacketSize (31:16), MaxBurst (15:8),
/// HID (7), EP Type (5:3), CErr (2:1).
pub const fn ep_dw1(mps: u16, ep_type: u32) -> u32 {
    ((mps as u32) << 16) | (3 << 1) | (ep_type << 3)
}

/// Endpoint types.
pub const EP_TYPE_CONTROL: u32 = 4;
pub const EP_TYPE_INTERRUPT_IN: u32 = 7;

// ── Device descriptor / HID interface constants ──────────────────────────────

pub const USB_DT_DEVICE: u16 = 1;
pub const USB_DT_CONFIG: u16 = 2;
pub const USB_REQ_GET_DESCRIPTOR: u8 = 6;
pub const USB_REQ_SET_CONFIGURATION: u8 = 9;
pub const USB_REQ_SET_PROTOCOL: u8 = 0x0B;

/// Standard device request, device-to-host (bmRequestType).
pub const BM_GET_DESCRIPTOR: u8 = 0x80;
/// Standard host-to-device request to the device.
pub const BM_SET_CONFIGURATION: u8 = 0x00;
/// Class host-to-device request to an interface (HID SET_PROTOCOL).
pub const BM_SET_PROTOCOL: u8 = 0x21;

pub const DESC_INTERFACE: u8 = 4;
pub const DESC_ENDPOINT: u8 = 5;
pub const HID_CLASS: u8 = 3;
pub const HID_SUBCLASS_BOOT: u8 = 1;
pub const HID_PROTOCOL_KEYBOARD: u8 = 1;
pub const XFER_INTERRUPT: u8 = 3;
/// HID boot protocol (SET_PROTOCOL wValue = 0).
pub const HID_PROTOCOL_BOOT: u16 = 0;

// ── Ring geometry ────────────────────────────────────────────────────────────

/// TRBs per command/transfer ring. The last slot holds the Link TRB, so 63 are
/// usable — far more than the handful of commands and control TDs enumeration
/// needs, and more than enough for a sustained interrupt-IN poll.
pub const RING_TRBS: usize = 64;
/// TRBs in the single event-ring segment (no Link TRB; it is a circular buffer).
pub const EVENT_TRBS: usize = 64;

// ── Poll budgets ─────────────────────────────────────────────────────────────

/// Bounded poll for register/event transitions that complete in microseconds.
/// Each iteration reads MMIO (a VM exit under QEMU), so the budget is small
/// enough that a failing stage reports instead of blowing the boot window.
pub const POLL_LIMIT: u64 = 2_000_000;
/// Port reset can take longer than a register transition.
pub const RESET_POLL_LIMIT: u64 = 500_000;
/// Yield every this many polls so a long wait does not starve the system.
pub const POLL_YIELD_EVERY: u64 = 4096;
