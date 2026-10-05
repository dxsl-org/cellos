//! xHCI controller bring-up, device enumeration, and one HID boot-protocol
//! keyboard.
//!
//! Scope (phase 03, X86-PC-2): PCI class `0C:03:30`, capability parsing, BAR
//! MMIO, HCRST reset, command ring + event ring, port reset/enable, slot and
//! endpoint contexts, one control transfer per enumeration step
//! (GET_DESCRIPTOR / SET_CONFIGURATION / SET_PROTOCOL), and a polled interrupt-IN
//! transfer for the keyboard. Hubs beyond enumeration, SuperSpeed tuning,
//! isochronous, storage and power management are out of scope.
//!
//! The QEMU model (`qemu-xhci`) is NEC uPD720200-class; real controller
//! revisions are not validated here (plan assumption A-03). Unsupported
//! revisions, missing interrupters and non-HID devices fail closed with a named
//! log line; a machine with no xHCI device never reaches this module (the cell
//! exits idle in `main`).
//!
//! Law 4 exception: Driver Cells may use `unsafe` for DMA memory access; MMIO
//! goes through the bounds-checked `ostd::mmio::MmioRegion` and every `unsafe`
//! block carries a `// SAFETY:` comment.

use crate::dma::DmaRegion;
use crate::regs::*;
use core::sync::atomic::{fence, Ordering};
use ostd::io::print_fmt;
use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

/// Enumerated device identity, for the VID/PID oracle marker.
pub struct Enumerated {
    pub vid: u16,
    pub pid: u16,
}

pub struct XhciController {
    mmio: MmioRegion,
    /// BAR + CAPLENGTH (operational registers).
    op: usize,
    /// BAR + RTSOFF (interrupter registers).
    rt: usize,
    /// BAR + DBOFF (doorbell array).
    db: usize,
    max_slots: u8,
    max_ports: u8,
    /// Port the enumerated device sits on (1-based) and its speed ID.
    port: u8,
    speed: u32,
    /// Slot assigned by the controller, and the address-device state.
    slot: u8,
    ep0_mps: u16,
    /// Device Context Index of the interrupt-IN endpoint (DCI = 2*ep + 1).
    /// Used for the add-flag, Context Entries, context offset, event filter and
    /// doorbell target, so all five stay in lockstep.
    int_dci: u8,
    int_mps: u16,
    /// True while an interrupt-IN TRB is outstanding (the device may NAK).
    int_armed: bool,

    // DMA structures the controller fetches; all retained for the cell's lifetime
    // so the authorized IOVAs stay valid.
    dcbaa: DmaRegion,
    cmd_ring: DmaRegion,
    evt_ring: DmaRegion,
    erst: DmaRegion,
    input_ctx: DmaRegion,
    dev_ctx: DmaRegion,
    ep0_ring: DmaRegion,
    int_ring: DmaRegion,
    data: DmaRegion,
    report: DmaRegion,
    /// Scratchpad array + buffers (`HCSPARAMS2` scratchpad count > 0), or `None`.
    _scratch: Option<DmaRegion>,

    // Ring producer/consumer state.
    cmd_enq: usize,
    cmd_cycle: u32,
    evt_deq: usize,
    evt_cycle: u32,
    ep0_enq: usize,
    ep0_cycle: u32,
    int_enq: usize,
    int_cycle: u32,
}

// ── Ring helpers ─────────────────────────────────────────────────────────────

fn ring_write_trb(ring: &DmaRegion, index: usize, trb: Trb) {
    let off = index * TRB_SIZE;
    ring.write_u64(off, trb.parameter);
    ring.write_u32(off + 8, trb.status);
    ring.write_u32(off + 12, trb.control);
}

fn ring_read_trb(ring: &DmaRegion, index: usize) -> Trb {
    let off = index * TRB_SIZE;
    Trb {
        parameter: ring.read_u64(off),
        status: ring.read_u32(off + 8),
        control: ring.read_u32(off + 12),
    }
}

/// Install the Link TRB in the ring's last slot, pointing back at index 0.
fn ring_init_link(ring: &DmaRegion) {
    ring_write_trb(
        ring,
        RING_TRBS - 1,
        Trb {
            parameter: ring.iova(),
            status: 0,
            control: trb_type(TYPE_LINK) | TRB_ENT | TRB_CYCLE,
        },
    );
}

/// Enqueue one TRB, maintaining the Link TRB and the producer cycle on wrap.
fn ring_enqueue(ring: &DmaRegion, enq: &mut usize, cycle: &mut u32, trb: Trb) {
    let index = *enq;
    ring_write_trb(
        ring,
        index,
        Trb {
            control: trb.control | *cycle,
            ..trb
        },
    );
    let mut next = index + 1;
    if next >= RING_TRBS - 1 {
        // Refresh the Link TRB with the producer cycle so the controller follows
        // it, then flip the producer cycle exactly as the controller's consumer
        // cycle flips on the same link (Link TRB Toggle Cycle = 1).
        ring_write_trb(
            ring,
            RING_TRBS - 1,
            Trb {
                parameter: ring.iova(),
                status: 0,
                control: trb_type(TYPE_LINK) | TRB_ENT | *cycle,
            },
        );
        next = 0;
        *cycle ^= 1;
    }
    *enq = next;
}

impl XhciController {
    /// Parse capabilities, reset the controller, program the rings, and start it.
    ///
    /// Returns a typed error (never panics) for an unsupported revision, a
    /// missing interrupter, or an unusable BAR/context size.
    pub fn new(mmio: MmioRegion, bdf: u32) -> ViResult<Self> {
        // Capability registers must be read as aligned 32-bit words: the xHCI
        // capability region's implementation only handles 4-byte accesses
        // (QEMU's `xhci_cap_ops.impl.max_access_size == 4`), so a sub-word read
        // at offset 0x02 is serviced as a 4-byte read at that unaligned offset
        // and returns 0. CAPLENGTH is byte 0 and HCIVERSION bits 31:16.
        let cap0 = mmio.read_u32(CAP_CAPLENGTH)?;
        let caplen = (cap0 & 0xFF) as usize;
        let hciversion = (cap0 >> 16) as u16;
        let hcsparams1 = mmio.read_u32(CAP_HCSPARAMS1)?;
        let hcsparams2 = mmio.read_u32(CAP_HCSPARAMS2)?;
        let hccparams1 = mmio.read_u32(CAP_HCCPARAMS1)?;
        let dboff = mmio.read_u32(CAP_DBOFF)? as usize;
        let rtsoff = mmio.read_u32(CAP_RTSOFF)? as usize;

        if caplen < 0x20 || caplen > 0x100 {
            let _ = print_fmt(format_args!(
                "[xhci] unsupported capability length 0x{:x}; failing closed\n",
                caplen
            ));
            return Err(ViError::NotSupported);
        }
        // Major revision must be 1 (xHCI 1.0/1.1/1.2). Anything else is a
        // different register interface and cannot be driven by this cell.
        if hciversion >> 8 != 1 {
            let _ = print_fmt(format_args!(
                "[xhci] unsupported controller revision 0x{:04x}; failing closed\n",
                hciversion
            ));
            return Err(ViError::NotSupported);
        }
        let max_slots = (hcsparams1 & 0xFF) as u8;
        let max_intrs = (hcsparams1 >> 8) & 0x3FF;
        let max_ports = ((hcsparams1 >> 24) & 0xFF) as u8;
        if max_slots == 0 || max_ports == 0 {
            let _ = print_fmt(format_args!(
                "[xhci] controller reports no device slots or ports; failing closed\n"
            ));
            return Err(ViError::NotSupported);
        }
        if max_intrs == 0 {
            let _ = print_fmt(format_args!(
                "[xhci] controller reports no interrupters; failing closed\n"
            ));
            return Err(ViError::NotSupported);
        }
        // CSZ: 0 = 32-byte contexts, 1 = 64-byte contexts. This driver only
        // programs 32-byte contexts; anything else fails closed rather than
        // mis-addressing the context structures.
        if hccparams1 & (1 << 2) != 0 {
            let _ = print_fmt(format_args!(
                "[xhci] unsupported 64-byte context size (CSZ=1); failing closed\n"
            ));
            return Err(ViError::NotSupported);
        }
        // Scratchpad buffers: Hi = bits 31:27, Lo = bits 25:21; count = Hi<<5 | Lo.
        let scratch_count =
            ((((hcsparams2 >> 27) & 0x1F) << 5) | ((hcsparams2 >> 21) & 0x1F)) as usize;
        if scratch_count > 64 {
            let _ = print_fmt(format_args!(
                "[xhci] controller wants {} scratchpad buffers (max 64); failing closed\n",
                scratch_count
            ));
            return Err(ViError::NotSupported);
        }

        let op = caplen;
        let rt = rtsoff;
        let db = dboff;

        // Allocate every DMA structure up front so a failure leaves nothing
        // partially programmed.
        let dcbaa = alloc(1, bdf, "dcbaa")?;
        let cmd_ring = alloc(1, bdf, "command ring")?;
        let evt_ring = alloc(1, bdf, "event ring")?;
        let erst = alloc(1, bdf, "event ring segment table")?;
        let input_ctx = alloc(1, bdf, "input context")?;
        let dev_ctx = alloc(1, bdf, "device context")?;
        let ep0_ring = alloc(1, bdf, "ep0 transfer ring")?;
        let int_ring = alloc(1, bdf, "interrupt transfer ring")?;
        let data = alloc(1, bdf, "control data buffer")?;
        let report = alloc(1, bdf, "HID report buffer")?;
        let scratch = if scratch_count > 0 {
            // One page for the pointer array plus one page per buffer. The array
            // is at offset 0 (page-aligned) and buffer i at (i+1)*4096.
            let region = alloc(1 + scratch_count, bdf, "scratchpad array")?;
            for i in 0..scratch_count {
                let buf = region.iova() + ((i + 1) * 4096) as u64;
                region.write_u64(i * 8, buf);
            }
            Some(region)
        } else {
            None
        };

        ring_init_link(&cmd_ring);
        ring_init_link(&ep0_ring);
        ring_init_link(&int_ring);

        let ctrl = Self {
            mmio,
            op,
            rt,
            db,
            max_slots,
            max_ports,
            port: 0,
            speed: 0,
            slot: 0,
            ep0_mps: 8,
            int_dci: 0,
            int_mps: 8,
            int_armed: false,
            dcbaa,
            cmd_ring,
            evt_ring,
            erst,
            input_ctx,
            dev_ctx,
            ep0_ring,
            int_ring,
            data,
            report,
            _scratch: scratch,
            cmd_enq: 0,
            cmd_cycle: 1,
            evt_deq: 0,
            evt_cycle: 1,
            ep0_enq: 0,
            ep0_cycle: 1,
            int_enq: 0,
            int_cycle: 1,
        };

        // 1. HCRST — resets every register except the capability registers.
        //    Operational registers live at BAR + CAPLENGTH, not BAR.
        ctrl.wr(ctrl.op + OP_USBCMD, USBCMD_HCRST)?;
        ctrl.wait_register(
            ctrl.op + OP_USBSTS,
            USBSTS_CNR,
            0,
            POLL_LIMIT,
            "HCRST did not clear CNR",
        )?;

        // 2. Program the device context base address array and the command ring.
        if let Some(scratch) = &ctrl._scratch {
            ctrl.dcbaa.write_u64(0, scratch.iova());
        }
        ctrl.wr64(ctrl.op + OP_DCBAAP, ctrl.dcbaa.iova())?;
        ctrl.wr64(ctrl.op + OP_CRCR, ctrl.cmd_ring.iova() | 1)?; // RCS = 1

        // 3. Configure interrupter 0: one event-ring segment, polling only (no
        //    interrupt enable), so events accumulate in the ring for us to drain.
        ctrl.erst.write_u64(0, ctrl.evt_ring.iova());
        ctrl.erst.write_u32(8, EVENT_TRBS as u32);
        ctrl.wr(ctrl.rt + RT_ERSTSZ, 1)?;
        // ERDP before ERSTBA: writing ERSTBA enables the event ring, and the
        // dequeue pointer must already be valid when it does.
        ctrl.wr64(ctrl.rt + RT_ERDP, ctrl.evt_ring.iova())?;
        ctrl.wr64(ctrl.rt + RT_ERSTBA, ctrl.erst.iova())?;
        // Clear any pending interrupt and leave IE off (bit 1) — we poll.
        ctrl.wr(ctrl.rt + RT_IMAN, 1)?;

        // 4. MaxSlotsEn then run. RS requires the rings to be programmed already.
        ctrl.wr(ctrl.op + OP_CONFIG, max_slots as u32)?;
        ctrl.wr(ctrl.op + OP_USBCMD, USBCMD_RS)?;
        ctrl.wait_register(
            ctrl.op + OP_USBSTS,
            USBSTS_HCH,
            0,
            POLL_LIMIT,
            "controller did not start (HCH set)",
        )?;

        let _ = print_fmt(format_args!(
            "[xhci] controller init ok caplen=0x{:x} version=0x{:04x} slots={} ports={} intrs={}\n",
            caplen, hciversion, max_slots, max_ports, max_intrs
        ));
        Ok(ctrl)
    }

    /// Enumerate the first attached device, configure its HID boot keyboard
    /// interface, and arm one interrupt-IN transfer.
    ///
    /// Returns `Err(ViError::NotFound)` when no port has a device attached (the
    /// caller reports an idle machine), and a named log line plus a typed error
    /// for every other failure.
    pub fn enumerate(&mut self) -> ViResult<Enumerated> {
        let port = self.find_device()?;
        self.port = port;
        self.reset_port(port)?;
        let _ = print_fmt(format_args!(
            "[xhci] port {} reset speed={}\n",
            port, self.speed
        ));

        // Enable Slot, then point the DCBAA at the device context for that slot.
        let (code, slot) = self.command(Trb {
            parameter: 0,
            status: 0,
            control: trb_type(TYPE_ENABLE_SLOT),
        })?;
        if code != CMPLT_SUCCESS || slot == 0 || slot > self.max_slots {
            let _ = print_fmt(format_args!(
                "[xhci] enable slot failed (code={} slot={})\n",
                code, slot
            ));
            return Err(ViError::IO);
        }
        self.slot = slot;
        self.dcbaa.write_u64(slot as usize * 8, self.dev_ctx.iova());

        // EP0 max packet: 8 for full/low speed (the device descriptor then tells
        // us the real value), 64 for high speed.
        self.ep0_mps = if self.speed == SPEED_HIGH { 64 } else { 8 };
        self.build_ep0_context();
        let (code, _) = self.command(Trb {
            parameter: self.input_ctx.iova(),
            status: 0,
            control: trb_type(TYPE_ADDRESS_DEVICE) | ((self.slot as u32) << 24),
        })?;
        if code != CMPLT_SUCCESS {
            let _ = print_fmt(format_args!("[xhci] address device failed (code={})\n", code));
            return Err(ViError::IO);
        }
        let _ = print_fmt(format_args!("[xhci] slot {} addressed\n", self.slot));

        // Device descriptor: first 8 bytes for bMaxPacketSize0, then the full 18.
        let mut short = [0u8; 8];
        let got = self.control_in(
            BM_GET_DESCRIPTOR,
            USB_REQ_GET_DESCRIPTOR,
            USB_DT_DEVICE << 8,
            0,
            8,
        )?;
        if got < 8 {
            let _ = print_fmt(format_args!(
                "[xhci] device descriptor read returned {} bytes\n",
                got
            ));
            return Err(ViError::IO);
        }
        self.data.read_bytes(0, &mut short);
        let real_mps = short[7] as u16;
        if real_mps != self.ep0_mps && real_mps >= 8 {
            self.evaluate_ep0(real_mps)?;
            self.ep0_mps = real_mps;
        }

        let mut full = [0u8; 18];
        let got = self.control_in(
            BM_GET_DESCRIPTOR,
            USB_REQ_GET_DESCRIPTOR,
            USB_DT_DEVICE << 8,
            0,
            18,
        )?;
        if got < 18 {
            let _ = print_fmt(format_args!(
                "[xhci] full device descriptor read returned {} bytes\n",
                got
            ));
            return Err(ViError::IO);
        }
        self.data.read_bytes(0, &mut full);
        let vid = u16::from_le_bytes([full[8], full[9]]);
        let pid = u16::from_le_bytes([full[10], full[11]]);
        let _ = print_fmt(format_args!(
            "[xhci] enumerated device vid=0x{:04x} pid=0x{:04x}\n",
            vid, pid
        ));

        // Configuration descriptor: 9-byte header for wTotalLength, then the whole
        // thing, from which the HID boot-keyboard interface + endpoint are found.
        let mut cfg9 = [0u8; 9];
        let got = self.control_in(
            BM_GET_DESCRIPTOR,
            USB_REQ_GET_DESCRIPTOR,
            (USB_DT_CONFIG << 8) | 0,
            0,
            9,
        )?;
        if got < 9 {
            let _ = print_fmt(format_args!(
                "[xhci] config descriptor header read returned {} bytes\n",
                got
            ));
            return Err(ViError::IO);
        }
        self.data.read_bytes(0, &mut cfg9);
        let total = u16::from_le_bytes([cfg9[2], cfg9[3]]) as usize;
        if total < 9 || total > self.data.len() {
            let _ = print_fmt(format_args!(
                "[xhci] config descriptor length {} out of range\n",
                total
            ));
            return Err(ViError::IO);
        }
        let got = self.control_in(
            BM_GET_DESCRIPTOR,
            USB_REQ_GET_DESCRIPTOR,
            USB_DT_CONFIG << 8,
            0,
            total as u16,
        )?;
        if got < total {
            let _ = print_fmt(format_args!(
                "[xhci] config descriptor read returned {} of {} bytes\n",
                got, total
            ));
            return Err(ViError::IO);
        }
        let mut cfg = [0u8; 512];
        let cfg_len = total.min(cfg.len());
        self.data.read_bytes(0, &mut cfg[..cfg_len]);
        let (iface, ep_num, mps, interval) = parse_boot_keyboard(&cfg[..cfg_len]).ok_or_else(|| {
            let _ = print_fmt(format_args!(
                "[xhci] device has no HID boot keyboard interface; failing closed\n"
            ));
            ViError::NotSupported
        })?;
        let _ = print_fmt(format_args!(
            "[xhci] HID boot keyboard interface={} endpoint=0x{:02x} mps={} interval={}\n",
            iface,
            0x80 | ep_num,
            mps,
            interval
        ));

        // SET_CONFIGURATION(1) then SET_PROTOCOL(boot) on the interface.
        self.control_no_data(BM_SET_CONFIGURATION, USB_REQ_SET_CONFIGURATION, 1, 0)?;
        self.control_no_data(BM_SET_PROTOCOL, USB_REQ_SET_PROTOCOL, HID_PROTOCOL_BOOT, iface as u16)?;

        // DCI = 2*endpoint_number + direction (IN = 1): EP1 IN is DCI 3.
        self.int_dci = 2 * ep_num + 1;
        self.int_mps = mps;
        self.configure_interrupt_endpoint(ep_num, mps, interval)?;
        self.arm_interrupt()?;
        Ok(Enumerated { vid, pid })
    }

    /// Drain pending events and return the latest HID report from the keyboard,
    /// re-arming the interrupt-IN transfer when it completes.
    pub fn poll_report(&mut self) -> Option<[u8; 8]> {
        let mut got = None;
        while let Some(trb) = self.next_event() {
            if event_type(trb.control) == TYPE_TRANSFER_EVENT
                && event_slot(trb.control) == self.slot
                && event_ep_id(trb.control) == self.int_dci
            {
                let code = event_code(trb.status);
                if code == CMPLT_SUCCESS || code == CMPLT_SHORT_PACKET {
                    let mut report = [0u8; 8];
                    self.report.read_bytes(0, &mut report);
                    got = Some(report);
                } else {
                    let _ = print_fmt(format_args!(
                        "[xhci] interrupt IN failed (code={})\n",
                        code
                    ));
                }
                self.int_armed = false;
            }
        }
        if !self.int_armed {
            let _ = self.arm_interrupt();
        }
        got
    }

    // ── Register access ──────────────────────────────────────────────────────

    fn rd(&self, off: usize) -> ViResult<u32> {
        self.mmio.read_u32(off)
    }
    fn wr(&self, off: usize, value: u32) -> ViResult<()> {
        self.mmio.write_u32(off, value)
    }

    /// Write a 64-bit register as two 32-bit halves, low dword first.
    ///
    /// xHCI's 64-bit registers take their side effect on the **high** dword
    /// write — CRCR initializes the command ring, ERSTBA the event ring — and
    /// the controller model dispatches an 8-byte access to the low-dword case
    /// only (`qemu-xhci`'s `xhci_oper_write`/`xhci_runtime_write`). Writing the
    /// halves explicitly is both model-correct and hardware-safe.
    fn wr64(&self, off: usize, value: u64) -> ViResult<()> {
        self.mmio.write_u32(off, value as u32)?;
        self.mmio.write_u32(off + 4, (value >> 32) as u32)
    }

    /// Poll `reg` until `(value & mask) == expected`, or fail with `reason`.
    fn wait_register(
        &self,
        reg: usize,
        mask: u32,
        expected: u32,
        limit: u64,
        reason: &str,
    ) -> ViResult<()> {
        let mut i = 0u64;
        while i < limit {
            if self.rd(reg)? & mask == expected {
                return Ok(());
            }
            if i % POLL_YIELD_EVERY == 0 {
                ostd::task::yield_now();
            }
            i += 1;
        }
        let _ = print_fmt(format_args!("[xhci] {} (reg=0x{:x})\n", reason, reg));
        Err(ViError::IO)
    }

    fn doorbell(&self, slot: u8, target: u32) -> ViResult<()> {
        self.wr(self.db + (slot as usize) * 4, target)
    }

    // ── Event ring ───────────────────────────────────────────────────────────

    /// Pop the next event if the consumer cycle matches, and advance ERDP.
    fn next_event(&mut self) -> Option<Trb> {
        let trb = ring_read_trb(&self.evt_ring, self.evt_deq);
        if trb.control & TRB_CYCLE != self.evt_cycle {
            return None;
        }
        self.evt_deq += 1;
        if self.evt_deq == EVENT_TRBS {
            self.evt_deq = 0;
            self.evt_cycle ^= 1;
        }
        let erdp = self.evt_ring.iova() + (self.evt_deq * TRB_SIZE) as u64;
        // EHB (bit 3) is write-1-to-clear; writing it acknowledges the consumed
        // event and lets the controller reuse the slot.
        let _ = self.wr64(self.rt + RT_ERDP, erdp | 0x8);
        Some(trb)
    }

    // ── Commands ─────────────────────────────────────────────────────────────

    fn command(&mut self, trb: Trb) -> ViResult<(u8, u8)> {
        ring_enqueue(&self.cmd_ring, &mut self.cmd_enq, &mut self.cmd_cycle, trb);
        fence(Ordering::SeqCst);
        self.doorbell(0, 0)?;
        let mut i = 0u64;
        while i < POLL_LIMIT {
            if let Some(event) = self.next_event() {
                if event_type(event.control) == TYPE_COMMAND_COMPLETE {
                    return Ok((event_code(event.status), event_slot(event.control)));
                }
            } else if i % POLL_YIELD_EVERY == 0 {
                ostd::task::yield_now();
            }
            i += 1;
        }
        let _ = print_fmt(format_args!("[xhci] command timed out (type=0x{:x})\n", trb.control));
        Err(ViError::IO)
    }

    // ── Ports ────────────────────────────────────────────────────────────────

    fn port_reg(&self, port: u8) -> usize {
        self.op + OP_PORT_BASE + ((port as usize - 1) * OP_PORT_STRIDE)
    }

    /// Return the first port with a device attached, powering ports as needed.
    fn find_device(&self) -> ViResult<u8> {
        for port in 1..=self.max_ports {
            let reg = self.port_reg(port);
            let mut portsc = self.rd(reg)?;
            if portsc & PORTSC_PP == 0 {
                self.wr(reg, (portsc & PORTSC_PRESERVE) | PORTSC_PP)?;
                ostd::task::yield_now();
                portsc = self.rd(reg)?;
            }
            if portsc & PORTSC_CCS != 0 {
                return Ok(port);
            }
        }
        Err(ViError::NotFound)
    }

    /// Reset `port`, wait for it to enable, and record the negotiated speed.
    fn reset_port(&mut self, port: u8) -> ViResult<()> {
        let reg = self.port_reg(port);
        let portsc = self.rd(reg)?;
        // Assert Port Reset, preserving power/indicator and acknowledging any
        // latched change bits.
        self.wr(
            reg,
            (portsc & PORTSC_PRESERVE) | PORTSC_PR | PORTSC_CHANGE,
        )?;
        self.wait_register(reg, PORTSC_PRC, PORTSC_PRC, RESET_POLL_LIMIT, "port reset never completed")?;
        // Clear PRC, keep the port powered.
        let portsc = self.rd(reg)?;
        self.wr(reg, (portsc & PORTSC_PRESERVE) | PORTSC_PRC)?;
        // Wait for the port to be enabled.
        self.wait_register(reg, PORTSC_PED, PORTSC_PED, RESET_POLL_LIMIT, "port never enabled")?;
        let portsc = self.rd(reg)?;
        self.speed = (portsc >> 10) & 0xF;
        Ok(())
    }

    // ── Contexts and transfers ───────────────────────────────────────────────

    /// Build the input context for the initial Address Device (slot + EP0).
    fn build_ep0_context(&self) {
        self.input_ctx.zero();
        self.input_ctx.write_u32(ICC_ADD, ctx_flag(0) | ctx_flag(1));
        self.input_ctx
            .write_u32(SLOT_CTX, slot_info(1, self.speed));
        // Root Hub Port Number is slot-context dword 1 bits 23:16.
        self.input_ctx
            .write_u32(SLOT_CTX + 4, (self.port as u32) << 16);
        let ep0 = ep_ctx(1); // EP0 is DCI 1
        self.input_ctx.write_u32(ep0, 0);
        self.input_ctx
            .write_u32(ep0 + 4, ep_dw1(self.ep0_mps, EP_TYPE_CONTROL));
        self.input_ctx.write_u64(ep0 + 8, self.ep0_ring.iova() | 1);
        self.input_ctx.write_u32(ep0 + 16, 8);
    }

    /// Update EP0's max packet size after reading the device descriptor.
    fn evaluate_ep0(&mut self, mps: u16) -> ViResult<()> {
        self.input_ctx.zero();
        self.input_ctx.write_u32(ICC_ADD, ctx_flag(0) | ctx_flag(1));
        self.input_ctx
            .write_u32(SLOT_CTX, slot_info(1, self.speed));
        self.input_ctx
            .write_u32(SLOT_CTX + 4, (self.port as u32) << 16);
        let ep0 = ep_ctx(1); // EP0 is DCI 1
        self.input_ctx.write_u32(ep0, 0);
        self.input_ctx.write_u32(ep0 + 4, ep_dw1(mps, EP_TYPE_CONTROL));
        self.input_ctx.write_u64(ep0 + 8, self.ep0_ring.iova() | 1);
        self.input_ctx.write_u32(ep0 + 16, 8);
        let (code, _) = self.command(Trb {
            parameter: self.input_ctx.iova(),
            status: 0,
            control: trb_type(TYPE_EVALUATE_CONTEXT) | ((self.slot as u32) << 24),
        })?;
        if code != CMPLT_SUCCESS {
            let _ = print_fmt(format_args!("[xhci] evaluate context failed (code={})\n", code));
            return Err(ViError::IO);
        }
        Ok(())
    }

    /// Add the interrupt-IN endpoint to the device via Configure Endpoint.
    fn configure_interrupt_endpoint(&mut self, ep_num: u8, mps: u16, interval: u8) -> ViResult<()> {
        // The add-flag bit, Context Entries and context offset are all keyed on
        // the DCI, not the endpoint number: EP1 IN is DCI 3.
        let dci = 2 * ep_num + 1;
        self.input_ctx.zero();
        self.input_ctx.write_u32(ICC_ADD, ctx_flag(0) | ctx_flag(dci));
        self.input_ctx
            .write_u32(SLOT_CTX, slot_info(dci as u32, self.speed));
        self.input_ctx
            .write_u32(SLOT_CTX + 4, (self.port as u32) << 16);
        let ep = ep_ctx(dci);
        // Endpoint Interval is dword 0 bits 23:16. Full/low speed encodes the
        // endpoint descriptor's bInterval (frames) directly; high/SuperSpeed
        // encode bInterval - 1 (125 us microframes).
        let interval_field = if self.speed >= SPEED_HIGH {
            interval.saturating_sub(1)
        } else {
            interval
        };
        self.input_ctx.write_u32(ep, (interval_field as u32) << 16);
        self.input_ctx
            .write_u32(ep + 4, ep_dw1(mps, EP_TYPE_INTERRUPT_IN));
        self.input_ctx.write_u64(ep + 8, self.int_ring.iova() | 1);
        self.input_ctx.write_u32(ep + 16, mps as u32);
        let (code, _) = self.command(Trb {
            parameter: self.input_ctx.iova(),
            status: 0,
            control: trb_type(TYPE_CONFIGURE_ENDPOINT) | ((self.slot as u32) << 24),
        })?;
        if code != CMPLT_SUCCESS {
            let _ = print_fmt(format_args!(
                "[xhci] configure endpoint failed (code={})\n",
                code
            ));
            return Err(ViError::IO);
        }
        Ok(())
    }

    /// Enqueue one interrupt-IN normal TRB and ring the endpoint doorbell.
    fn arm_interrupt(&mut self) -> ViResult<()> {
        ring_enqueue(
            &self.int_ring,
            &mut self.int_enq,
            &mut self.int_cycle,
            Trb {
                parameter: self.report.iova(),
                status: self.int_mps as u32,
                control: trb_type(TYPE_NORMAL) | TRB_IOC,
            },
        );
        fence(Ordering::SeqCst);
        self.doorbell(self.slot, self.int_dci as u32)?;
        self.int_armed = true;
        Ok(())
    }

    /// Wait for the transfer event of one control TD on EP0.
    fn wait_transfer(&mut self, ep_id: u8) -> ViResult<(u8, u32)> {
        let mut i = 0u64;
        while i < POLL_LIMIT {
            if let Some(event) = self.next_event() {
                if event_type(event.control) == TYPE_TRANSFER_EVENT
                    && event_slot(event.control) == self.slot
                    && event_ep_id(event.control) == ep_id
                {
                    return Ok((event_code(event.status), event_residual(event.status)));
                }
            } else if i % POLL_YIELD_EVERY == 0 {
                ostd::task::yield_now();
            }
            i += 1;
        }
        Err(ViError::IO)
    }

    /// Control transfer with an IN data stage into `self.data`; returns the
    /// number of bytes actually transferred.
    fn control_in(
        &mut self,
        bm: u8,
        breq: u8,
        wvalue: u16,
        windex: u16,
        len: u16,
    ) -> ViResult<usize> {
        let setup = setup_packet(bm, breq, wvalue, windex, len);
        ring_enqueue(
            &self.ep0_ring,
            &mut self.ep0_enq,
            &mut self.ep0_cycle,
            Trb {
                parameter: setup,
                status: 8,
                control: trb_type(TYPE_SETUP) | TRB_IDT | TRB_CHAIN | (TRT_IN << 16),
            },
        );
        ring_enqueue(
            &self.ep0_ring,
            &mut self.ep0_enq,
            &mut self.ep0_cycle,
            Trb {
                parameter: self.data.iova(),
                status: len as u32,
                control: trb_type(TYPE_DATA) | TRB_CHAIN | TRB_DIR_IN,
            },
        );
        ring_enqueue(
            &self.ep0_ring,
            &mut self.ep0_enq,
            &mut self.ep0_cycle,
            Trb {
                parameter: 0,
                status: 0,
                control: trb_type(TYPE_STATUS) | TRB_IOC,
            },
        );
        fence(Ordering::SeqCst);
        self.doorbell(self.slot, 1)?;
        let (code, residual) = self.wait_transfer(1)?;
        if code != CMPLT_SUCCESS && code != CMPLT_SHORT_PACKET {
            let _ = print_fmt(format_args!(
                "[xhci] control IN failed (bm=0x{:02x} req=0x{:02x} code={})\n",
                bm, breq, code
            ));
            return Err(ViError::IO);
        }
        Ok((len as u32).saturating_sub(residual) as usize)
    }

    /// Control transfer with no data stage.
    fn control_no_data(&mut self, bm: u8, breq: u8, wvalue: u16, windex: u16) -> ViResult<()> {
        let setup = setup_packet(bm, breq, wvalue, windex, 0);
        ring_enqueue(
            &self.ep0_ring,
            &mut self.ep0_enq,
            &mut self.ep0_cycle,
            Trb {
                parameter: setup,
                status: 8,
                control: trb_type(TYPE_SETUP) | TRB_IDT | TRB_CHAIN | (TRT_NO_DATA << 16),
            },
        );
        ring_enqueue(
            &self.ep0_ring,
            &mut self.ep0_enq,
            &mut self.ep0_cycle,
            Trb {
                parameter: 0,
                status: 0,
                control: trb_type(TYPE_STATUS) | TRB_IOC | TRB_DIR_IN,
            },
        );
        fence(Ordering::SeqCst);
        self.doorbell(self.slot, 1)?;
        let (code, _) = self.wait_transfer(1)?;
        if code != CMPLT_SUCCESS && code != CMPLT_SHORT_PACKET {
            let _ = print_fmt(format_args!(
                "[xhci] control no-data failed (bm=0x{:02x} req=0x{:02x} code={})\n",
                bm, breq, code
            ));
            return Err(ViError::IO);
        }
        Ok(())
    }
}

/// Pack a USB setup packet into the Setup Stage TRB parameter field.
fn setup_packet(bm: u8, breq: u8, wvalue: u16, windex: u16, wlength: u16) -> u64 {
    (bm as u64)
        | ((breq as u64) << 8)
        | ((wvalue as u64) << 16)
        | ((windex as u64) << 32)
        | ((wlength as u64) << 48)
}

/// Allocate and authorize a named DMA region, logging the reason on failure.
fn alloc(pages: usize, bdf: u32, what: &str) -> ViResult<DmaRegion> {
    match DmaRegion::new(pages, bdf) {
        Some(region) => Ok(region),
        None => {
            let _ = print_fmt(format_args!(
                "[xhci] DMA allocation failed for {} ({} pages)\n",
                what, pages
            ));
            Err(ViError::OutOfMemory)
        }
    }
}

/// Find a HID boot keyboard interface and its interrupt-IN endpoint.
///
/// Walks the configuration descriptor: interface descriptors set the current
/// interface, endpoint descriptors belong to it. Returns
/// `(interface_number, endpoint_number, max_packet_size, interval)`.
fn parse_boot_keyboard(cfg: &[u8]) -> Option<(u8, u8, u16, u8)> {
    let mut i = 0usize;
    let mut current_iface: Option<(u8, u8, u8, u8)> = None;
    while i + 2 <= cfg.len() {
        let len = cfg[i] as usize;
        let kind = cfg[i + 1];
        if len < 2 || i + len > cfg.len() {
            break;
        }
        if kind == DESC_INTERFACE && len >= 9 {
            // bInterfaceNumber(2), bInterfaceClass(5), bInterfaceSubClass(6),
            // bInterfaceProtocol(7).
            current_iface = Some((cfg[i + 2], cfg[i + 5], cfg[i + 6], cfg[i + 7]));
        } else if kind == DESC_ENDPOINT && len >= 7 {
            if let Some((iface, class, subclass, protocol)) = current_iface {
                let is_boot_keyboard = class == HID_CLASS
                    && subclass == HID_SUBCLASS_BOOT
                    && protocol == HID_PROTOCOL_KEYBOARD;
                let addr = cfg[i + 2];
                let attrs = cfg[i + 3] & 0x03;
                let is_in = addr & 0x80 != 0;
                if is_boot_keyboard && attrs == XFER_INTERRUPT && is_in {
                    let mps = u16::from_le_bytes([cfg[i + 4], cfg[i + 5]]) & 0x07FF;
                    return Some((iface, addr & 0x0F, mps, cfg[i + 6]));
                }
            }
        }
        i += len;
    }
    None
}
