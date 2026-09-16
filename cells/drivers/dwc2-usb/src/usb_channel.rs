//! DWC2 Host Channel transaction engine (Control and Bulk transfers via Data FIFO).

use crate::regs::*;
use ostd::mmio::MmioRegion;
use ostd::syscall::sys_yield;
use types::{ViError, ViResult};

pub struct UsbHostEngine<'a> {
    mmio: &'a MmioRegion,
}

impl<'a> UsbHostEngine<'a> {
    pub fn new(mmio: &'a MmioRegion) -> Self {
        Self { mmio }
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
        self.write32(hcsplt(ch), 0);
        self.write32(hcintmsk(ch), 0x07FF);
        // 1. Clear pending channel interrupts
        self.write32(hcint(ch), 0xFFFF_FFFF);
        // 2. Configure Transfer Size (HCTSIZ):
        // XFERSIZE = 8, PKTCNT = 1, PID = 3 (SETUP)
        let sctsiz = 8 | (1 << 19) | (3 << 29);
        self.write32(hctsiz(ch), sctsiz);

        // 3. Configure Channel Characteristics (HCCHAR):
        // HCCHAR fields (bits 0-10 MPS, 11-14 EPNUM, 15 EPDIR, 18-19 EPTYPE, 20 MC, 22-28 DEVADDR,
        // 31 CHENA): EPNUM = 0, EPDIR = 0 (OUT), EPTYPE = 0 (Control), MPS = 64, MC = 1, CHENA = 1.
        let scchar = 64 | (1 << 20) | ((dev_addr as u32) << 22) | (1 << 31);
        self.write32(hcchar(ch), scchar);

        // 4. Push 8 bytes (2 x 32-bit words) into FIFO
        let w0 = u32::from_le_bytes([setup[0], setup[1], setup[2], setup[3]]);
        let w1 = u32::from_le_bytes([setup[4], setup[5], setup[6], setup[7]]);
        self.write_fifo(ch, w0);
        self.write_fifo(ch, w1);

        // 5. Poll for completion
        self.wait_channel(ch)
    }

    /// Receive data in the DATA IN phase on Channel 0.
    pub fn recv_data(&self, dev_addr: u8, buf: &mut [u8]) -> ViResult<usize> {
        let ch = 0;
        let mut received = 0;
        let mut toggle = 2; // PID 2 = DATA1 for first data packet

        while received < buf.len() {
            let chunk = (buf.len() - received).min(64);
            self.write32(hcsplt(ch), 0);
            self.write32(hcintmsk(ch), 0x07FF);
            self.write32(hcint(ch), 0xFFFF_FFFF);
            // XFERSIZE = chunk, PKTCNT = 1, PID = toggle
            let sctsiz = (chunk as u32) | (1 << 19) | ((toggle as u32) << 29);
            self.write32(hctsiz(ch), sctsiz);

            // HCCHAR: EPNUM = 0, EPDIR = 1 (IN), EPTYPE = 0 (Control), MPS = 64, MC = 1, CHENA = 1.
            let scchar = 64 | (1 << 15) | (1 << 20) | ((dev_addr as u32) << 22) | (1 << 31);
            self.write32(hcchar(ch), scchar);

            self.wait_channel(ch)?;

            // Read words from FIFO
            let words = chunk.div_ceil(4);
            for (i, word) in (0..words).map(|i| (i, self.read_fifo(ch))) {
                for (j, byte) in word.to_le_bytes().iter().enumerate() {
                    let offset = i * 4 + j;
                    let idx = received + offset;
                    if idx < buf.len() && offset < chunk {
                        buf[idx] = *byte;
                    }
                }
            }

            received += chunk;
            toggle = if toggle == 2 { 0 } else { 2 }; // Toggle DATA1 (2) <-> DATA0 (0)
        }

        Ok(received)
    }

    /// Send STATUS handshake on Channel 0 (0-byte packet with DATA1).
    pub fn send_status(&self, dev_addr: u8, is_in: bool) -> ViResult<()> {
        let ch = 0;
        self.write32(hcsplt(ch), 0);
        self.write32(hcintmsk(ch), 0x07FF);
        self.write32(hcint(ch), 0xFFFF_FFFF);
        // XFERSIZE = 0, PKTCNT = 1, PID = 2 (DATA1)
        let sctsiz = (1 << 19) | (2 << 29);
        self.write32(hctsiz(ch), sctsiz);

        // HCCHAR: EPNUM = 0, EPTYPE = 0 (Control), MPS = 64, MC = 1, CHENA = 1, EPDIR from the caller.
        let epdir = if is_in { 1 } else { 0 };
        let scchar = 64 | (epdir << 15) | (1 << 20) | ((dev_addr as u32) << 22) | (1 << 31);
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

        // 1. SETUP phase
        self.send_setup(dev_addr, &setup)?;

        // 2. DATA phase (optional)
        let mut actual = 0;
        if length > 0 {
            if is_in {
                actual = self.recv_data(dev_addr, buf)?;
            } else {
                // Out data phase
                self.send_data(dev_addr, buf)?;
                actual = buf.len();
            }
        }

        // 3. STATUS phase (handshake in opposite direction)
        self.send_status(dev_addr, !is_in)?;

        Ok(actual)
    }

    /// Send DATA OUT phase on Channel 0.
    fn send_data(&self, dev_addr: u8, data: &[u8]) -> ViResult<()> {
        let ch = 0;
        let mut sent = 0;
        let mut toggle = 2; // PID 2 = DATA1

        while sent < data.len() {
            let chunk = (data.len() - sent).min(64);
            self.write32(hcsplt(ch), 0);
            self.write32(hcintmsk(ch), 0x07FF);
            self.write32(hcint(ch), 0xFFFF_FFFF);
            let sctsiz = (chunk as u32) | (1 << 19) | ((toggle as u32) << 29);
            self.write32(hctsiz(ch), sctsiz);

            // HCCHAR: EPNUM = 0, EPDIR = 0 (OUT), EPTYPE = 0 (Control), MPS = 64, MC = 1, CHENA = 1.
            let scchar = 64 | (1 << 20) | ((dev_addr as u32) << 22) | (1 << 31);
            self.write32(hcchar(ch), scchar);

            // Push words to FIFO
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
            let words_count = chunk.div_ceil(4);

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
                self.write32(hcsplt(ch), 0);
                self.write32(hcintmsk(ch), 0x07FF);
                self.write32(hcint(ch), 0xFFFF_FFFF);
                self.write32(hctsiz(ch), sctsiz);
                self.write32(hcchar(ch), scchar);
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
        self.write32(hcsplt(ch), 0);
        self.write32(hcintmsk(ch), 0x07FF);
        self.write32(hcint(ch), 0xFFFF_FFFF);
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
                // XFERCOMPL: read data from FIFO
                let words = want.div_ceil(4);
                for (i, word) in (0..words).map(|i| (i, self.read_fifo(ch))) {
                    for (j, byte) in word.to_le_bytes().iter().enumerate() {
                        let idx = i * 4 + j;
                        if idx < buf.len() && idx < want {
                            buf[idx] = *byte;
                        }
                    }
                }
                return Ok(want);
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
    fn halt_channel(&self, ch: usize) {
        let reg = hcchar(ch);
        let mut val = self.read32(reg);
        val |= (1 << 30) | (1 << 31); // CHDIS | CHENA
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
