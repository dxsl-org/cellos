//! DWC2 Host Channel transaction engine (Control and Bulk transfers via Data FIFO).

use crate::regs::*;
use core::cell::Cell;
use ostd::mmio::MmioRegion;
use ostd::syscall::sys_yield;
use types::{ViError, ViResult};

/// Control-endpoint max packet size assumed before a device reports its own.
///
/// USB 2.0 §5.5.3: a low-speed control endpoint is 8 bytes, a full-speed one is
/// 8/16/32/64, and a **high-speed one is 64**. The assumption therefore has to
/// follow the negotiated port speed — a fixed 8 makes the core program a
/// high-speed channel the hardware will not run (`HCINT` never sets a single
/// bit and the transfer hangs until timeout), while a fixed 64 mis-frames a
/// full-speed device that answers in 8-byte packets.
pub const CONTROL_MPS_LOW_FULL_SPEED: u8 = 8;

/// HPRT0 `PRTSPD` encoding (DWC2 databook): the value `reset_port` returns.
pub const PORT_SPEED_HIGH: u32 = 0;
pub const PORT_SPEED_FULL: u32 = 1;
pub const PORT_SPEED_LOW: u32 = 2;

/// Control-endpoint packet size to assume for a device on a port at `speed`.
///
/// This is only the starting value: `read_device_descriptor` replaces it with
/// the device's real `bMaxPacketSize0` as soon as it has read one.
pub fn initial_control_mps(speed: u32) -> u8 {
    match speed {
        PORT_SPEED_HIGH => 64,
        _ => CONTROL_MPS_LOW_FULL_SPEED,
    }
}

pub struct UsbHostEngine<'a> {
    mmio: &'a MmioRegion,
    /// Max packet size for control transfers on endpoint 0. Updated from the
    /// device descriptor's `bMaxPacketSize0` once it has been read.
    ctrl_mps: Cell<u8>,
}

impl<'a> UsbHostEngine<'a> {
    pub fn new(mmio: &'a MmioRegion) -> Self {
        Self {
            mmio,
            ctrl_mps: Cell::new(CONTROL_MPS_LOW_FULL_SPEED),
        }
    }

    /// Max packet size currently used for control transfers.
    pub fn control_mps(&self) -> u8 {
        self.ctrl_mps.get()
    }

    /// Adopt a device's control-endpoint max packet size.
    ///
    /// Clamped to the legal 8/16/32/64 set: a device reporting 0 or a bogus
    /// value must not produce a zero-length packet size that hangs the engine.
    pub fn set_control_mps(&self, mps: u8) {
        let clamped = match mps {
            0..=8 => 8,
            9..=16 => 16,
            17..=32 => 32,
            _ => 64,
        };
        self.ctrl_mps.set(clamped);
    }

    #[inline(always)]
    fn read32(&self, offset: usize) -> u32 {
        self.mmio.read::<u32>(offset).unwrap_or(0)
    }

    #[inline(always)]
    fn write32(&self, offset: usize, val: u32) {
        let _ = self.mmio.write::<u32>(offset, val);
    }

    /// Read data word from FIFO (in Host Mode, all channels read from offset 0x1000).
    #[inline(always)]
    fn read_fifo(&self, ch: usize) -> u32 {
        self.read32(0x1000 + ch * 0x1000)
    }

    /// Write data word into FIFO (in Host Mode, all channels write to offset 0x1000).
    #[inline(always)]
    fn write_fifo(&self, ch: usize, val: u32) {
        self.write32(0x1000 + ch * 0x1000, val);
    }

    /// Execute a standard 8-byte USB SETUP packet on Channel 0.
    pub fn send_setup(&self, dev_addr: u8, setup: &[u8; 8]) -> ViResult<()> {
        let ch = 0;
        self.prepare_channel(ch);
        self.write32(hcsplt(ch), 0);
        self.write32(hcintmsk(ch), 0x07FF);
        // 2. Configure Transfer Size (HCTSIZ):
        // XFERSIZE = 8, PKTCNT = 1, PID = 3 (SETUP)
        let sctsiz = 8 | (1 << 19) | (3 << 29);
        self.write32(hctsiz(ch), sctsiz);

        // 3. Configure Channel Characteristics (HCCHAR):
        // HCCHAR fields (bits 0-10 MPS, 11-14 EPNUM, 15 EPDIR, 18-19 EPTYPE, 20 MC, 22-28 DEVADDR,
        // 31 CHENA): EPNUM = 0, EPDIR = 0 (OUT), EPTYPE = 0 (Control), MC = 1, CHENA = 1.
        let scchar =
            (self.control_mps() as u32) | (1 << 20) | ((dev_addr as u32) << 22) | (1 << 31);
        self.write32(hcchar(ch), scchar);

        // 4. Push 8 bytes (2 x 32-bit words) into the channel's data FIFO.
        let w0 = u32::from_le_bytes([setup[0], setup[1], setup[2], setup[3]]);
        let w1 = u32::from_le_bytes([setup[4], setup[5], setup[6], setup[7]]);
        self.write_fifo(ch, w0);
        self.write_fifo(ch, w1);

        // 5. Poll for completion
        self.wait_channel(ch)
    }

    /// Receive the DATA IN phase of a control transfer on Channel 0.
    ///
    /// Returns the bytes actually received. Packets are sized by the control
    /// endpoint's MPS and the transfer ends at the first **short** packet
    /// (USB 2.0 §8.5.3.1) — a device answering an 18-byte descriptor request
    /// with its real 8-byte report must not be read as if it had 18 bytes
    /// available, which is what a fixed-size read would do.
    pub fn recv_data(&self, dev_addr: u8, buf: &mut [u8]) -> ViResult<usize> {
        let ch = 0;
        let mps = (self.control_mps() as usize).max(1);
        let mut received = 0;
        let mut toggle = 2u32; // PID 2 = DATA1 for the first data packet

        while received < buf.len() {
            let chunk = (buf.len() - received).min(mps);
            self.prepare_channel(ch);
            self.write32(hcsplt(ch), 0);
            self.write32(hcintmsk(ch), 0x07FF);
            let sctsiz = (chunk as u32) | (1 << 19) | (toggle << 29);
            self.write32(hctsiz(ch), sctsiz);

            // HCCHAR: EPNUM = 0, EPDIR = 1 (IN), EPTYPE = 0 (Control), MC = 1, CHENA = 1.
            let scchar =
                (mps as u32) | (1 << 15) | (1 << 20) | ((dev_addr as u32) << 22) | (1 << 31);
            self.write32(hcchar(ch), scchar);

            self.wait_channel(ch)?;

            // The core writes the *remaining* count back into HCTSIZ.
            let remaining = (self.read32(hctsiz(ch)) & 0x7FFFF) as usize;
            let got = chunk.saturating_sub(remaining).min(chunk);

            let words = got.div_ceil(4);
            for (i, word) in (0..words).map(|i| (i, self.read_fifo(ch))) {
                for (j, byte) in word.to_le_bytes().iter().enumerate() {
                    let offset = i * 4 + j;
                    let idx = received + offset;
                    if idx < buf.len() && offset < got {
                        buf[idx] = *byte;
                    }
                }
            }

            received += got;
            if got < chunk {
                break;
            }
            toggle = if toggle == 2 { 0 } else { 2 }; // DATA1 <-> DATA0
        }

        Ok(received)
    }

    /// Send STATUS handshake on Channel 0 (0-byte packet with DATA1).
    pub fn send_status(&self, dev_addr: u8, is_in: bool) -> ViResult<()> {
        let ch = 0;
        self.prepare_channel(ch);
        self.write32(hcsplt(ch), 0);
        self.write32(hcintmsk(ch), 0x07FF);
        // XFERSIZE = 0, PKTCNT = 1, PID = 2 (DATA1)
        let sctsiz = (1 << 19) | (2 << 29);
        self.write32(hctsiz(ch), sctsiz);

        // HCCHAR: EPNUM = 0, EPTYPE = 0 (Control), MPS = 64, MC = 1, CHENA = 1, EPDIR from the caller.
        let epdir = if is_in { 1 } else { 0 };
        let scchar = (self.control_mps() as u32)
            | (epdir << 15)
            | (1 << 20)
            | ((dev_addr as u32) << 22)
            | (1 << 31);
        self.write32(hcchar(ch), scchar);

        self.wait_channel(ch)
    }

    /// Execute a complete synchronous USB Control Transfer.
    pub fn control_transfer(
        &self,
        dev_addr: u8,
        req_type: u8,
        req: u8,
        val: u16,
        idx: u16,
        buf: &mut [u8],
    ) -> ViResult<usize> {
        let is_in = (req_type & 0x80) != 0;
        let length = buf.len() as u16;

        let setup = [
            req_type,
            req,
            (val & 0xFF) as u8,
            ((val >> 8) & 0xFF) as u8,
            (idx & 0xFF) as u8,
            ((idx >> 8) & 0xFF) as u8,
            (length & 0xFF) as u8,
            ((length >> 8) & 0xFF) as u8,
        ];

        // Each phase reports its own failure: a control transfer that fails
        // silently is the hardest kind of USB bug to find, and the request
        // fields are the only thing that identifies which one it was.
        if let Err(e) = self.send_setup(dev_addr, &setup) {
            self.report_failure("setup", dev_addr, req_type, req, val, idx, length, e);
            return Err(e);
        }

        // 2. DATA phase (optional)
        let mut actual = 0;
        if length > 0 {
            let result = if is_in {
                self.recv_data(dev_addr, buf)
            } else {
                self.send_data(dev_addr, buf).map(|()| buf.len())
            };
            match result {
                Ok(n) => actual = n,
                Err(e) => {
                    self.report_failure("data", dev_addr, req_type, req, val, idx, length, e);
                    return Err(e);
                }
            }
        }

        // 3. STATUS phase (handshake in opposite direction)
        if let Err(e) = self.send_status(dev_addr, !is_in) {
            self.report_failure("status", dev_addr, req_type, req, val, idx, length, e);
            return Err(e);
        }

        Ok(actual)
    }

    /// Log one failed control-transfer phase with the request that caused it.
    #[allow(clippy::too_many_arguments)]
    fn report_failure(
        &self,
        phase: &str,
        dev_addr: u8,
        req_type: u8,
        req: u8,
        val: u16,
        idx: u16,
        length: u16,
        error: ViError,
    ) {
        ostd::io::print("[dwc2] control ");
        ostd::io::print(phase);
        ostd::io::print(" failed: bmRequestType=0x");
        print_hex_val(req_type as u32);
        ostd::io::print(" bRequest=0x");
        print_hex_val(req as u32);
        ostd::io::print(" wValue=0x");
        print_hex_val(val as u32);
        ostd::io::print(" wIndex=0x");
        print_hex_val(idx as u32);
        ostd::io::print(" wLength=0x");
        print_hex_val(length as u32);
        ostd::io::print(" addr=");
        print_hex_val(dev_addr as u32);
        ostd::io::print(" err=");
        match error {
            ViError::IO => ostd::io::println("IO (stall/babble/timeout)"),
            ViError::WouldBlock => ostd::io::println("WouldBlock (NAK)"),
            _ => ostd::io::println("other"),
        }
    }

    /// Send DATA OUT phase on Channel 0.
    fn send_data(&self, dev_addr: u8, data: &[u8]) -> ViResult<()> {
        let ch = 0;
        let mps = (self.control_mps() as usize).max(1);
        let mut sent = 0;
        let mut toggle = 2; // PID 2 = DATA1

        while sent < data.len() {
            let chunk = (data.len() - sent).min(mps);
            self.prepare_channel(ch);
            self.write32(hcsplt(ch), 0);
            self.write32(hcintmsk(ch), 0x07FF);
            let sctsiz = (chunk as u32) | (1 << 19) | ((toggle as u32) << 29);
            self.write32(hctsiz(ch), sctsiz);

            // HCCHAR: EPNUM = 0, EPDIR = 0 (OUT), EPTYPE = 0 (Control), MC = 1, CHENA = 1.
            let scchar = (mps as u32) | (1 << 20) | ((dev_addr as u32) << 22) | (1 << 31);
            self.write32(hcchar(ch), scchar);

            let words = chunk.div_ceil(4);
            for i in 0..words {
                let mut b = [0u8; 4];
                for (j, byte) in b.iter_mut().enumerate() {
                    let offset = i * 4 + j;
                    if sent + offset < data.len() && offset < chunk {
                        *byte = data[sent + offset];
                    }
                }
                self.write_fifo(ch, u32::from_le_bytes(b));
            }

            self.wait_channel(ch)?;

            sent += chunk;
            toggle = if toggle == 2 { 0 } else { 2 };
        }

        Ok(())
    }

    /// Transmit a raw Ethernet packet via Bulk OUT (Channel 2, EP 2).
    pub fn bulk_transmit(&self, dev_addr: u8, ep_num: u8, packet: &[u8]) -> ViResult<()> {
        let ch = 2;
        let mut sent = 0;
        let mut toggle = 0; // Starts at DATA0

        while sent < packet.len() {
            let chunk = (packet.len() - sent).min(512); // 512 bytes for High-Speed Bulk

            let sctsiz = (chunk as u32) | (1 << 19) | ((toggle as u32) << 29);
            // HCCHAR: EPDIR = 0 (OUT), EPTYPE = 2 (Bulk), MC = 1 packet, MPS = 512 (HS Bulk).
            let scchar = 512
                | ((ep_num as u32) << 11)
                | (2 << 18)
                | (1 << 20)
                | ((dev_addr as u32) << 22)
                | (1 << 31);

            let mut retries = 0;
            loop {
                self.prepare_channel(ch);
                self.write32(hcsplt(ch), 0);
                self.write32(hcintmsk(ch), 0x07FF);
                self.write32(hctsiz(ch), sctsiz);
                self.write32(hcchar(ch), scchar);

                let words_count = chunk.div_ceil(4);
                for i in 0..words_count {
                    let mut b = [0u8; 4];
                    for (j, byte) in b.iter_mut().enumerate() {
                        let offset = i * 4 + j;
                        let idx = sent + offset;
                        if idx < packet.len() && offset < chunk {
                            *byte = packet[idx];
                        }
                    }
                    self.write_fifo(ch, u32::from_le_bytes(b));
                }

                match self.wait_channel(ch) {
                    Ok(()) => break,
                    Err(ViError::WouldBlock) => {
                        retries += 1;
                        if retries > 50 {
                            ostd::io::println("[dwc2] bulk_transmit: exceeded 50 NAK retries");
                            return Err(ViError::IO);
                        }
                        sys_yield();
                    }
                    Err(e) => return Err(e),
                }
            }

            sent += chunk;
            toggle = if toggle == 0 { 2 } else { 0 };
        }

        Ok(())
    }

    /// Receive a raw packet via Bulk IN (Channel 1, EP 1). Returns received length or 0 if nothing.
    pub fn bulk_receive(&self, dev_addr: u8, ep_num: u8, buf: &mut [u8]) -> ViResult<usize> {
        let ch = 1;
        self.prepare_channel(ch);
        self.write32(hcsplt(ch), 0);
        self.write32(hcintmsk(ch), 0x07FF);
        let want = buf.len().min(512);
        let sctsiz = (want as u32) | (1 << 19);
        self.write32(hctsiz(ch), sctsiz);

        let scchar = 512
            | ((ep_num as u32) << 11)
            | (1 << 15) // IN
            | (2 << 18) // Bulk
            | (1 << 20) // MC = 1
            | ((dev_addr as u32) << 22)
            | (1 << 31);
        self.write32(hcchar(ch), scchar);

        // Non-blocking wait: check if transfer completed or NAK
        let mut count = 0;
        while count < 1000 {
            let int = self.read32(hcint(ch));
            if int & (1 << 0) != 0 {
                // The core reports the actual length in HCTSIZ either way, but
                // only DMA deposits the payload outside the FIFO.
                let remaining = (self.read32(hctsiz(ch)) & 0x7FFFF) as usize;
                let got = want.saturating_sub(remaining);
                let words = want.div_ceil(4);
                for (i, word) in (0..words).map(|i| (i, self.read_fifo(ch))) {
                    for (j, byte) in word.to_le_bytes().iter().enumerate() {
                        let idx = i * 4 + j;
                        if idx < buf.len() && idx < want {
                            buf[idx] = *byte;
                        }
                    }
                }
                return Ok(got);
            }
            if int & (1 << 4) != 0 {
                // NAK: device has no packet right now
                return Ok(0);
            }
            count += 1;
            sys_yield();
        }

        Ok(0)
    }

    /// Receive up to one packet via Interrupt IN on `ch`.
    ///
    /// HID input reports ride interrupt endpoints. The device NAKs until it has
    /// a report, so a NAK is a normal "no data yet" answer, not an error — the
    /// caller polls. Returns the *actual* transferred length, read back from
    /// `HCTSIZ` rather than assumed: a HID report can be shorter than the buffer
    /// (an 8-byte keyboard report into a 64-byte buffer), and using the request
    /// length would hand the decoder trailing garbage.
    ///
    /// `ch` must not collide with the control (0) or Ethernet bulk (1, 2)
    /// channels.
    pub fn interrupt_receive(
        &self,
        ch: usize,
        dev_addr: u8,
        ep_num: u8,
        mps: u16,
        buf: &mut [u8],
    ) -> ViResult<usize> {
        let want = buf.len().min(mps.max(1) as usize).min(1024);
        if want == 0 {
            return Ok(0);
        }

        self.prepare_channel(ch);
        self.write32(hcsplt(ch), 0);
        self.write32(hcintmsk(ch), 0x07FF);

        // HCTSIZ: XFERSIZE (= want), PKTCNT = 1, PID = DATA0 (0). The core
        // overwrites XFERSIZE with the remaining count as it transfers.
        let sctsiz = (want as u32) | (1 << 19);
        self.write32(hctsiz(ch), sctsiz);

        // HCCHAR: MPS from the endpoint descriptor, EPNUM, IN direction,
        // EPTYPE = 3 (Interrupt), MC = 1, DEVADDR, CHENA.
        let scchar = (mps as u32 & 0x7FF)
            | ((ep_num as u32) << 11)
            | (1 << 15) // EPDIR = IN
            | (3 << 18) // EPTYPE = Interrupt
            | (1 << 20) // MC = 1
            | ((dev_addr as u32) << 22)
            | (1 << 31); // CHENA
        self.write32(hcchar(ch), scchar);

        let mut polls = 0u32;
        while polls < 2_000 {
            let int = self.read32(hcint(ch));

            // NAK — device has no report queued. Halt and report "no data".
            if int & (1 << 4) != 0 {
                self.halt_channel(ch);
                return Ok(0);
            }
            // Errors (STALL / babble / transaction error) end the poll cycle.
            if int & ((1 << 2) | (1 << 3) | (1 << 7)) != 0 {
                self.halt_channel(ch);
                return Err(ViError::IO);
            }

            let complete = int & (1 << 0) != 0 || int & (1 << 1) != 0 || int & (1 << 5) != 0;
            if complete {
                let remaining = (self.read32(hctsiz(ch)) & 0x7FFFF) as usize;
                let got = want.saturating_sub(remaining);
                self.halt_channel(ch);

                if got > 0 {
                    // Drain ceil(got/4) FIFO words; only the first `got` bytes
                    // are meaningful (the tail word is padding).
                    let words = got.div_ceil(4);
                    for w in 0..words {
                        let word = self.read_fifo(ch);
                        for (j, byte) in word.to_le_bytes().iter().enumerate() {
                            let idx = w * 4 + j;
                            if idx < got && idx < buf.len() {
                                buf[idx] = *byte;
                            }
                        }
                    }
                } else {
                    // A zero-length packet still has a FIFO word to retire on
                    // some revisions; drain one so the channel is clean.
                    let _ = self.read_fifo(ch);
                }
                return Ok(got);
            }

            polls += 1;
            sys_yield();
        }

        self.halt_channel(ch);
        Ok(0)
    }

    /// Wait for channel transfer completion or error with timeout.
    ///
    /// BCM2837 DWC2 Slave mode: the core does NOT reliably set XFERCOMPL
    /// or CHHLTD after a successful transaction.  ACK (bit 5) from the
    /// device is the definitive completion signal; we halt the channel
    /// manually after seeing it.
    fn wait_channel(&self, ch: usize) -> ViResult<()> {
        let mut count = 0;
        while count < 50_000 {
            let int = self.read32(hcint(ch));

            // ── Hardware-generated halt ───────────────────────────────
            if int & (1 << 1) != 0 {
                // CHHLTD
                self.write32(hcint(ch), 0xFFFF_FFFF);
                if int & (1 << 7) != 0 || int & (1 << 2) != 0 {
                    return Err(ViError::IO);
                }
                if int & (1 << 3) != 0 {
                    return Err(ViError::IO);
                }
                if int & (1 << 4) != 0 {
                    return Err(ViError::WouldBlock);
                }
                return Ok(());
            }

            // ── Transfer complete ─────────────────────────────────────
            if int & (1 << 0) != 0 {
                self.halt_channel(ch);
                return Ok(());
            }

            // ── ACK = device accepted the packet (BCM2837 primary path)
            if int & (1 << 5) != 0 {
                self.halt_channel(ch);
                return Ok(());
            }

            // ── Error conditions ──────────────────────────────────────
            if int & (1 << 7) != 0 || int & (1 << 2) != 0 {
                self.halt_channel(ch);
                return Err(ViError::IO);
            }
            if int & (1 << 3) != 0 {
                self.halt_channel(ch);
                return Err(ViError::IO);
            }
            if int & (1 << 4) != 0 {
                self.halt_channel(ch);
                return Err(ViError::WouldBlock);
            }

            count += 1;
            sys_yield();
        }
        // Timeout — capture hcint BEFORE halt clears it
        let int = self.read32(hcint(ch));
        let char_val = self.read32(hcchar(ch));
        self.halt_channel(ch);
        static TIMEOUT_COUNT: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        if TIMEOUT_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 3 {
            ostd::io::print("[dwc2] TIMEOUT ch=");
            print_hex_val(ch as u32);
            ostd::io::print(" hcint=0x");
            print_hex_val(int);
            ostd::io::print(" hcchar=0x");
            print_hex_val(char_val);
            ostd::io::println("");
        }
        Err(ViError::IO)
    }

    /// Explicitly halt a host channel (required in DWC2 Slave mode).
    ///
    /// `CHDIS` and `CHENA` must never be set together. The core reads
    /// `CHENA=1` as a fresh enable, so raising it while requesting a disable
    /// **re-arms the channel with the parameters still in `HCCHAR`/`HCTSIZ`** —
    /// which starts a second, unwanted transaction from a completed one. That
    /// stray transfer then holds the channel forever, and every later transfer
    /// on it times out with no status bit set, because the channel never
    /// becomes free. Only `CHDIS` is raised here; the core clears `CHENA` and
    /// reports `CHHLTD` when it has actually stopped.
    fn halt_channel(&self, ch: usize) {
        let reg = hcchar(ch);
        let mut val = self.read32(reg);
        val |= 1 << 30; // CHDIS
        val &= !(1 << 31); // CHENA
        self.write32(reg, val);
        // Wait for CHHLTD (bit 1) with a short timeout
        for _ in 0..10_000 {
            if self.read32(hcint(ch)) & (1 << 1) != 0 {
                break;
            }
            sys_yield();
        }
        self.write32(hcint(ch), 0xFFFF_FFFF);
    }

    /// Leave `ch` disabled before arming it, whatever state it was left in.
    ///
    /// A channel that is still enabled from an abandoned transfer ignores the
    /// next `CHENA`, so each transfer starts by making sure the core has really
    /// stopped and its interrupt bits are cleared.
    fn prepare_channel(&self, ch: usize) {
        if self.read32(hcchar(ch)) & (1 << 31) != 0 {
            self.halt_channel(ch);
        }
        self.write32(hcint(ch), 0xFFFF_FFFF);
    }
}

fn print_hex_val(val: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut buf = [0u8; 8];
    for i in 0..8 {
        buf[7 - i] = HEX[((val >> (i * 4)) & 0xF) as usize];
    }
    if let Ok(s) = core::str::from_utf8(&buf) {
        ostd::io::print(s);
    }
}
