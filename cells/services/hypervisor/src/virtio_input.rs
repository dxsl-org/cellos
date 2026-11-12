// SPDX-License-Identifier: MPL-2.0
//! VirtIO-Input device model (DeviceID=18, virtio-mmio slot 4 -> SPI 20).
//!
//! One keyboard + absolute-pointer (tablet) device. Events come only from the
//! launcher bound to the session (`app_session_x86`), which forwards its own
//! window's input in window coordinates.
extern crate alloc;
use alloc::collections::VecDeque;

use crate::virtio_mmio::{QueueCfg, VirtioDevice};
use crate::virtqueue::read_descriptor_chain;
#[cfg(target_arch = "aarch64")]
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
const VIRTIO_INPUT_CFG_ABS_INFO: u8 = 0x12;

// Linux Event Types
pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
pub const EV_ABS: u16 = 0x03;

pub const SYN_REPORT: u16 = 0;
pub const REL_WHEEL: u16 = 0x08;
pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;
pub const BTN_SIDE: u16 = 0x113;
pub const BTN_EXTRA: u16 = 0x114;

/// Queued events cap: a guest that never posts eventq buffers must not grow
/// host memory without bound.
const MAX_PENDING_EVENTS: usize = 256;

/// Linux evdev code for a host mouse button; side buttons map the way Linux
/// HID does so guest browsers see them as back/forward, not as a left click.
pub fn button_code(button: api::input::MouseButton) -> u16 {
    match button {
        api::input::MouseButton::Left => BTN_LEFT,
        api::input::MouseButton::Right => BTN_RIGHT,
        api::input::MouseButton::Middle => BTN_MIDDLE,
        api::input::MouseButton::Back => BTN_SIDE,
        api::input::MouseButton::Forward => BTN_EXTRA,
    }
}

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

    /// Queue `events` plus a trailing SYN_REPORT. When the queue is full the
    /// oldest whole reports are dropped, keeping the newest state (e.g. a key
    /// release) and SYN-terminated report boundaries.
    fn push_report(&mut self, events: &[VirtioInputEvent]) {
        let needed = events.len() + 1;
        while !self.pending_events.is_empty()
            && self.pending_events.len() + needed > MAX_PENDING_EVENTS
        {
            while let Some(ev) = self.pending_events.pop_front() {
                if ev.event_type == EV_SYN {
                    break;
                }
            }
        }
        self.pending_events.extend(events.iter().copied());
        self.pending_events.push_back(VirtioInputEvent {
            event_type: EV_SYN,
            code: SYN_REPORT,
            value: 0,
        });
    }

    /// Push a key event (press or release) followed by a SYN_REPORT.
    pub fn push_key(&mut self, linux_key_code: u16, pressed: bool) {
        self.push_report(&[VirtioInputEvent {
            event_type: EV_KEY,
            code: linux_key_code,
            value: u32::from(pressed),
        }]);
    }

    /// Push an absolute pointer position on `axis` (ABS_X/ABS_Y, range
    /// `0..=ABS_MAX`) followed by a SYN_REPORT.
    pub fn push_abs(&mut self, axis: u16, value: i32) {
        self.push_report(&[VirtioInputEvent {
            event_type: EV_ABS,
            code: axis,
            value: value as u32,
        }]);
    }

    /// Push mouse button press or release followed by a SYN_REPORT.
    pub fn push_mouse_button(&mut self, button: u16, pressed: bool) {
        self.push_key(button, pressed);
    }

    /// Push mouse wheel scroll followed by a SYN_REPORT.
    pub fn push_mouse_scroll(&mut self, dy: i32) {
        self.push_report(&[VirtioInputEvent {
            event_type: EV_REL,
            code: REL_WHEEL,
            value: dy as u32,
        }]);
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

    /// Payload byte `byte_idx` of the currently selected config item.
    fn config_payload_byte(&self, byte_idx: usize) -> u8 {
        match self.select {
            VIRTIO_INPUT_CFG_ID_NAME => {
                b"CellOS VirtIO Input\0".get(byte_idx).copied().unwrap_or(0)
            }
            VIRTIO_INPUT_CFG_ID_SERIAL => b"CELLOS-0001\0".get(byte_idx).copied().unwrap_or(0),
            VIRTIO_INPUT_CFG_EV_BITS => match (self.subsel, byte_idx) {
                (0, 0) => 1,
                // EV_KEY: keys 0..127, then BTN_LEFT..BTN_EXTRA at 0x110 / 8.
                (1, 0..=15) => 0xFF,
                (1, 34) => 0x1F,
                // EV_REL: REL_WHEEL (8) only; the pointer is absolute.
                (2, 1) => 0x01,
                // EV_ABS: ABS_X | ABS_Y (a tablet: the launcher knows window coordinates).
                (3, 0) => 0x03,
                _ => 0,
            },
            VIRTIO_INPUT_CFG_ABS_INFO if self.subsel <= 1 => {
                // virtio_input_absinfo: min, max, fuzz, flat, res (u32 LE each).
                let max = (api::services::vmm_session::ABS_MAX as u32).to_le_bytes();
                match byte_idx {
                    4..=7 => max[byte_idx - 4],
                    _ => 0,
                }
            }
            _ => 0,
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
                        1 => 35, // EV_KEY: keys + the button byte at 0x110 / 8
                        2 => 2,  // EV_REL (Wheel)
                        3 => 1,  // EV_ABS (X, Y)
                        _ => 0,
                    },
                    VIRTIO_INPUT_CFG_ABS_INFO if self.subsel <= 1 => 20,
                    _ => 0,
                };
                (self.select as u32) | ((self.subsel as u32) << 8) | ((size as u32) << 16)
            }
            // 0x08..0x88: payload. The MMIO layer reads one aligned word and
            // shifts out the requested byte, so pack four payload bytes per word.
            o if (8..136).contains(&o) => {
                let b = o - 8;
                u32::from_le_bytes([
                    self.config_payload_byte(b),
                    self.config_payload_byte(b + 1),
                    self.config_payload_byte(b + 2),
                    self.config_payload_byte(b + 3),
                ])
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One config byte as the MMIO layer reads it: the aligned word, shifted.
    fn cfg_byte(dev: &InputDev, byte: usize) -> u8 {
        (dev.config_read(byte & !3) >> ((byte & 3) * 8)) as u8
    }

    #[test]
    fn ev_key_bitmap_is_byte_addressable_and_advertises_buttons() {
        let mut dev = InputDev::new(None);
        dev.config_write(
            0,
            u32::from(VIRTIO_INPUT_CFG_EV_BITS) | (u32::from(EV_KEY) << 8),
        );
        assert!(usize::from(cfg_byte(&dev, 2)) > 34);
        assert_eq!(cfg_byte(&dev, 8 + 1), 0xFF);
        assert_eq!(cfg_byte(&dev, 8 + 15), 0xFF);
        assert_eq!(cfg_byte(&dev, 8 + 34), 0x1F);
    }

    #[test]
    fn pointer_is_absolute_with_the_session_range() {
        let mut dev = InputDev::new(None);
        dev.config_write(0, u32::from(VIRTIO_INPUT_CFG_EV_BITS) | (u32::from(EV_ABS) << 8));
        assert_eq!(cfg_byte(&dev, 2), 1);
        assert_eq!(cfg_byte(&dev, 8), 0x03);
        dev.config_write(0, u32::from(VIRTIO_INPUT_CFG_EV_BITS) | (u32::from(EV_REL) << 8));
        assert_eq!((cfg_byte(&dev, 8), cfg_byte(&dev, 9)), (0, 0x01), "wheel only");
        dev.config_write(0, u32::from(VIRTIO_INPUT_CFG_ABS_INFO) | (1 << 8));
        assert_eq!(cfg_byte(&dev, 2), 20);
        let max: alloc::vec::Vec<u8> = (12..16).map(|i| cfg_byte(&dev, i)).collect();
        assert_eq!(max, (api::services::vmm_session::ABS_MAX as u32).to_le_bytes());
    }

    #[test]
    fn name_reads_back_every_byte() {
        let mut dev = InputDev::new(None);
        dev.config_write(0, u32::from(VIRTIO_INPUT_CFG_ID_NAME));
        let name: alloc::vec::Vec<u8> = (0..6).map(|i| cfg_byte(&dev, 8 + i)).collect();
        assert_eq!(name, b"CellOS");
    }

    #[test]
    fn full_queue_drops_oldest_reports_and_keeps_newest() {
        let mut dev = InputDev::new(None);
        for _ in 0..MAX_PENDING_EVENTS {
            dev.push_key(30, true);
        }
        dev.push_key(30, false);
        let n = dev.pending_events.len();
        assert!(n <= MAX_PENDING_EVENTS);
        assert_eq!(dev.pending_events[0].event_type, EV_KEY);
        assert_eq!(dev.pending_events[n - 2].value, 0);
        assert_eq!(dev.pending_events[n - 1].event_type, EV_SYN);
    }
}
