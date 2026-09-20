// SPDX-License-Identifier: MPL-2.0
//! VirtIO-Input device model (DeviceID=18, virtio-mmio slot 4 -> SPI 20).
//!
//! Enables guest Linux environments (X11 / Wayland / Chromium) to receive
//! native keyboard and mouse input events from CellOS.
extern crate alloc;
use alloc::collections::VecDeque;

use crate::virtio_mmio::{QueueCfg, VirtioDevice};
use crate::virtqueue::read_descriptor_chain;
pub const INPUT_SPI: u32 = 20;

// VirtIO Input Config Selectors (VirtIO 1.1 §5.8.4)
#[allow(dead_code)]
const VIRTIO_INPUT_CFG_UNSET: u8 = 0x00;
const VIRTIO_INPUT_CFG_ID_NAME: u8 = 0x01;
const VIRTIO_INPUT_CFG_ID_SERIAL: u8 = 0x02;
const VIRTIO_INPUT_CFG_ID_DEVIDS: u8 = 0x03;
#[allow(dead_code)]
const VIRTIO_INPUT_CFG_PROP_BITS: u8 = 0x10;
const VIRTIO_INPUT_CFG_EV_BITS: u8 = 0x11;
#[allow(dead_code)]
const VIRTIO_INPUT_CFG_ABS_INFO: u8 = 0x12;

// Linux Event Types
pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
#[allow(dead_code)]
pub const EV_ABS: u16 = 0x03;

pub const SYN_REPORT: u16 = 0;
pub const REL_X: u16 = 0x00;
pub const REL_Y: u16 = 0x01;
pub const REL_WHEEL: u16 = 0x08;
pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtioInputEvent {
    pub event_type: u16,
    pub code: u16,
    pub value: u32,
}

impl VirtioInputEvent {
    pub fn to_bytes(self) -> [u8; 8] {
        let mut b = [0u8; 8];
        b[0..2].copy_from_slice(&self.event_type.to_le_bytes());
        b[2..4].copy_from_slice(&self.code.to_le_bytes());
        b[4..8].copy_from_slice(&self.value.to_le_bytes());
        b
    }
}

pub struct InputDev {
    select: u8,
    subsel: u8,
    pending_events: VecDeque<VirtioInputEvent>,
    event_last_avail: u16,
    event_used_idx: u16,
    irq: Option<u32>,
}

impl InputDev {
    pub fn new(irq: Option<u32>) -> Self {
        Self {
            select: VIRTIO_INPUT_CFG_UNSET,
            subsel: 0,
            pending_events: VecDeque::new(),
            event_last_avail: 0,
            event_used_idx: 0,
            irq,
        }
    }

    /// Push a key event (press or release) followed by a SYN_REPORT.
    pub fn push_key(&mut self, linux_key_code: u16, pressed: bool) {
        self.pending_events.push_back(VirtioInputEvent {
            event_type: EV_KEY,
            code: linux_key_code,
            value: if pressed { 1 } else { 0 },
        });
        self.pending_events.push_back(VirtioInputEvent {
            event_type: EV_SYN,
            code: SYN_REPORT,
            value: 0,
        });
    }

    /// Push relative mouse movement followed by a SYN_REPORT.
    pub fn push_mouse_move(&mut self, dx: i32, dy: i32) {
        if dx != 0 {
            self.pending_events.push_back(VirtioInputEvent {
                event_type: EV_REL,
                code: REL_X,
                value: dx as u32,
            });
        }
        if dy != 0 {
            self.pending_events.push_back(VirtioInputEvent {
                event_type: EV_REL,
                code: REL_Y,
                value: dy as u32,
            });
        }
        self.pending_events.push_back(VirtioInputEvent {
            event_type: EV_SYN,
            code: SYN_REPORT,
            value: 0,
        });
    }

    /// Push mouse button press or release followed by a SYN_REPORT.
    pub fn push_mouse_button(&mut self, button: u16, pressed: bool) {
        self.pending_events.push_back(VirtioInputEvent {
            event_type: EV_KEY,
            code: button,
            value: if pressed { 1 } else { 0 },
        });
        self.pending_events.push_back(VirtioInputEvent {
            event_type: EV_SYN,
            code: SYN_REPORT,
            value: 0,
        });
    }

    /// Push mouse wheel scroll followed by a SYN_REPORT.
    pub fn push_mouse_scroll(&mut self, dy: i32) {
        self.pending_events.push_back(VirtioInputEvent {
            event_type: EV_REL,
            code: REL_WHEEL,
            value: dy as u32,
        });
        self.pending_events.push_back(VirtioInputEvent {
            event_type: EV_SYN,
            code: SYN_REPORT,
            value: 0,
        });
    }

    /// Flush pending input events into guest eventq buffers.
    pub fn flush_events(&mut self, qcfg: &QueueCfg, vm_id: usize, vcpu_id: usize) -> bool {
        if !qcfg.ready || !qcfg.is_valid() || self.pending_events.is_empty() {
            return false;
        }

        let q_size = qcfg.num as usize;
        let Some(avail_idx_gpa) = crate::virtqueue_guard::checked_gpa(qcfg.avail_gpa, 2, 2) else {
            return false;
        };

        let mut b2 = [0u8; 2];
        if crate::vmm::read_guest_memory(vm_id, avail_idx_gpa, &mut b2) != 2 {
            return false;
        }
        let avail_idx = u16::from_le_bytes(b2);

        let mut published = 0;
        while self.event_last_avail != avail_idx && !self.pending_events.is_empty() {
            let Some(ring_gpa) = crate::virtqueue_guard::avail_entry_gpa(
                qcfg.avail_gpa,
                self.event_last_avail,
                q_size,
            ) else {
                break;
            };

            if crate::vmm::read_guest_memory(vm_id, ring_gpa, &mut b2) != 2 {
                break;
            }
            let head = u16::from_le_bytes(b2) as usize;
            self.event_last_avail = self.event_last_avail.wrapping_add(1);

            let Some(bufs) = read_descriptor_chain(vm_id, qcfg, head) else {
                continue;
            };

            if let Some(ev) = self.pending_events.pop_front() {
                let bytes = ev.to_bytes();
                let mut written = 0usize;

                for buf in &bufs {
                    if !buf.writable {
                        continue;
                    }
                    let n = (8 - written).min(buf.len as usize);
                    if n > 0 {
                        let _ = crate::vmm::write_guest_memory(
                            vm_id,
                            buf.gpa,
                            &bytes[written..written + n],
                        );
                        written += n;
                    }
                    if written >= 8 {
                        break;
                    }
                }

                // Write used ring entry
                if let Some(elem_gpa) = crate::virtqueue_guard::used_entry_gpa(
                    qcfg.used_gpa,
                    self.event_used_idx,
                    q_size,
                ) {
                    let mut elem = [0u8; 8];
                    elem[0..4].copy_from_slice(&(head as u32).to_le_bytes());
                    elem[4..8].copy_from_slice(&(written as u32).to_le_bytes());
                    let _ = crate::vmm::write_guest_memory(vm_id, elem_gpa, &elem);

                    self.event_used_idx = self.event_used_idx.wrapping_add(1);
                    if let Some(used_idx_gpa) =
                        crate::virtqueue_guard::checked_gpa(qcfg.used_gpa, 2, 2)
                    {
                        let _ = crate::vmm::write_guest_memory(
                            vm_id,
                            used_idx_gpa,
                            &self.event_used_idx.to_le_bytes(),
                        );
                    }
                    published += 1;
                }
            }
        }

        if published > 0 {
            if let Some(irq) = self.irq {
                crate::vmm::inject_irq(vm_id, vcpu_id, irq);
            }
            true
        } else {
            false
        }
    }
}

impl VirtioDevice for InputDev {
    fn device_id(&self) -> u32 {
        18 // VIRTIO_ID_INPUT
    }

    fn notify(&mut self, q: usize, qcfg: &QueueCfg, vm_id: usize, vcpu_id: usize) -> bool {
        match q {
            0 => {
                // eventq notify from guest: guest supplied more read buffers -> flush pending
                self.flush_events(qcfg, vm_id, vcpu_id)
            }
            1 => {
                // statusq: driver sent status/LED changes -> consume buffers
                true
            }
            _ => false,
        }
    }

    fn config_read(&self, offset: usize) -> u32 {
        match offset {
            // 0x00: select (u8), 0x01: subsel (u8), 0x02: size (u8), 0x03..0x07: reserved
            0 => {
                let size = match self.select {
                    VIRTIO_INPUT_CFG_ID_NAME => 20,
                    VIRTIO_INPUT_CFG_ID_SERIAL => 12,
                    VIRTIO_INPUT_CFG_ID_DEVIDS => 8,
                    VIRTIO_INPUT_CFG_EV_BITS => match self.subsel {
                        0 => 1,  // EV_SYN
                        1 => 32, // EV_KEY (keyboard & mouse buttons)
                        2 => 2,  // EV_REL (X, Y, Wheel)
                        _ => 0,
                    },
                    _ => 0,
                };
                (self.select as u32) | ((self.subsel as u32) << 8) | ((size as u32) << 16)
            }
            // 0x08..0x88: payload data (128 bytes)
            o if (8..136).contains(&o) => {
                let byte_idx = o - 8;
                match self.select {
                    VIRTIO_INPUT_CFG_ID_NAME => {
                        let name = b"CellOS VirtIO Input\0";
                        if byte_idx < name.len() {
                            name[byte_idx] as u32
                        } else {
                            0
                        }
                    }
                    VIRTIO_INPUT_CFG_ID_SERIAL => {
                        let sn = b"CELLOS-0001\0";
                        if byte_idx < sn.len() {
                            sn[byte_idx] as u32
                        } else {
                            0
                        }
                    }
                    VIRTIO_INPUT_CFG_EV_BITS => {
                        match self.subsel {
                            0 => u32::from(byte_idx == 0),
                            1 => {
                                // EV_KEY: enable standard key range (1..128) + mouse buttons
                                if byte_idx < 16 {
                                    0xFF // keys 0..127
                                } else if byte_idx == 34 {
                                    // BTN_MOUSE / BTN_LEFT (0x110)
                                    0x07 // BTN_LEFT, BTN_RIGHT, BTN_MIDDLE
                                } else {
                                    0
                                }
                            }
                            2 => {
                                // EV_REL: REL_X (0), REL_Y (1), REL_WHEEL (8)
                                if byte_idx == 0 {
                                    0x03 // REL_X | REL_Y
                                } else if byte_idx == 1 {
                                    0x01 // REL_WHEEL
                                } else {
                                    0
                                }
                            }
                            _ => 0,
                        }
                    }
                    _ => 0,
                }
            }
            _ => 0,
        }
    }

    fn reset(&mut self) {
        self.select = VIRTIO_INPUT_CFG_UNSET;
        self.subsel = 0;
        self.event_last_avail = 0;
        self.event_used_idx = 0;
        self.pending_events.clear();
    }

    fn config_write(&mut self, offset: usize, val: u32) {
        match offset {
            0 => {
                self.select = (val & 0xFF) as u8;
                self.subsel = ((val >> 8) & 0xFF) as u8;
            }
            1 => {
                self.subsel = (val & 0xFF) as u8;
            }
            _ => {}
        }
    }
}
