//! DWC2 Host Channel transaction engine (Control and Bulk transfers via Data FIFO).

use crate::regs::*;
use core::cell::{Cell, RefCell};
use ostd::dma::DmaBuf;
use ostd::mmio::MmioRegion;
use ostd::syscall::sys_yield;
use types::{ViError, ViResult};

/// How host channels move payload bytes.
///
/// The BCM2837 DWC2 supports both; which one a given environment implements is
/// not discoverable from the register file, so the driver tries FIFO first and
/// falls back to DMA. FIFO is the board-proven path (the LAN9514 Ethernet runs
/// there); DMA is what QEMU's `hcd-dwc2` model implements, since it sources
/// payloads from guest memory at `HCDMA` and ignores host-channel FIFO writes
/// entirely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferMode {
    /// Programmed I/O: payload goes through the channel's data FIFO.
    Fifo,
    /// The core reads/writes payloads directly from/to guest memory at `HCDMA`.
    Dma,
}

/// Bytes reserved per host channel inside the DMA scratch region.
///
/// Sized for the largest payload this driver moves: a full configuration
/// descriptor (≤512 bytes) and an Ethernet frame. One slot per channel keeps two
/// in-flight channels from aliasing the same memory.
pub const DMA_SLOT_BYTES: usize = 4096;
/// Host channels that get a DMA slot (DWC2 implements up to 16; this driver uses 8).
const DMA_CHANNELS: usize = 8;

/// Control-endpoint max packet size assumed before a device reports its own.
///
/// USB 2.0 §5.5.3 fixes low-speed control endpoints at 8 bytes and allows
/// full-speed ones 8/16/32/64, so 8 is the only value safe to assume for the
/// first descriptor read. Declaring 64 there makes the host expect 64-byte
/// packets from a device that sends 8, and the read fails.
pub const CONTROL_MPS_LOW_FULL_SPEED: u8 = 8;

/// HPRT0 `PRTSPD` encoding (DWC2 databook): the value `reset_port` returns.
pub const PORT_SPEED_HIGH: u32 = 0;
pub const PORT_SPEED_FULL: u32 = 1;
pub const PORT_SPEED_LOW: u32 = 2;

/// Control-endpoint packet size to assume for a device on a port at `speed`.
///
/// USB 2.0 5.5.3: low speed is 8 bytes, full speed is 8/16/32/64, and
/// **high speed is 64**. The assumption has to follow the negotiated link
/// speed, and `read_device_descriptor` replaces it with the device's real
/// `bMaxPacketSize0` as soon as it has read one.
pub fn initial_control_mps(speed: u32) -> u8 {
    match speed {
        PORT_SPEED_HIGH => 64,
        _ => CONTROL_MPS_LOW_FULL_SPEED,
    }
}

/// Complete-splits a periodic poll will issue before giving the hub up.
const SPLIT_ATTEMPTS: usize = 2;

/// Frame-counter reads allowed while waiting out one microframe.
///
/// This is a read of the counter and nothing else -- no yield -- because the wait
/// it measures is a microframe of 125 us. Yielding cannot express a wait that
/// short: a yield hands the CPU away for far longer, which is how an earlier
/// attempt at this ended up spending hundreds of milliseconds per poll.
const MICROFRAME_SPINS: usize = 3_000;

/// Microframes in one full-speed frame.
///
/// `HFNUM` counts microframes, and a hub pairs the two halves of a split inside
/// one millisecond frame, so the counter has to be shifted to ask that question.
/// Both reference drivers derive the frame the same way.
const FULL_FRAME_SHIFT: u32 = 3;

/// Register reads a non-yielding channel wait will make before giving up.
///
/// The frame check inside that wait is the real bound; this only stops a core
/// that never reports anything from spinning forever.
const SPIN_POLLS: usize = 200_000;

/// Poll budget for the ordinary `wait_channel`.
const WAIT_POLLS: usize = 50_000;

/// Poll budget for waiting out a channel halt.
///
/// The core clears `CHENA` in microseconds; this only has to be long enough not
/// to be missed, and every iteration of it is a yield handed to the rest of the
/// system.
const CHANNEL_HALT_POLLS: usize = 64;

/// Complete-split attempts a synchronous transfer will make.
///
/// A control transfer has a caller waiting on its result, so its split is run to
/// completion here rather than spread across calls the way the interrupt poll is.
/// The attempts are issued back to back and bounded by frames rather than spun
/// on, because the caller cannot be handed "not yet".
const SPLIT_COMPLETE_ATTEMPTS: usize = 2;

/// Frames a complete-split may still belong to the start-split that began it.
///
/// The pairing is only meaningful inside the hub's frame budget, so a
/// complete-split issued after this many is started over instead of being taken
/// for a result. `HFNUM` counts microframes at high speed, and a low-speed
/// transaction behind the hub's translator takes up to a full millisecond.
const SPLIT_COMPLETE_FRAMES: u32 = 12;

/// Where a device sits when it has to be reached through a hub.
///
/// A high-speed hub does not pass full- and low-speed traffic through, so a
/// device behind one is addressed in two transactions: a **start-split** the hub
/// buffers, then a **complete-split** that collects the result. The core
/// expresses both through `HCSPLT`.
///
/// This is per device, not per transfer, which is why the engine carries it the
/// same way it carries `ctrl_mps`: both are facts about whoever is being
/// addressed right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Split {
    /// The address the hub itself was assigned.
    pub hub_addr: u8,
    /// The downstream port on that hub holding the device.
    pub port: u8,
    /// The device is low-speed and needs the preamble its speed requires.
    pub low_speed: bool,
}

pub struct UsbHostEngine<'a> {
    mmio: &'a MmioRegion,
    /// Max packet size for control transfers on endpoint 0. Updated from the
    /// device descriptor's `bMaxPacketSize0` once it has been read.
    ctrl_mps: Cell<u8>,
    mode: Cell<TransferMode>,
    /// Per-channel DMA scratch, allocated on first use in DMA mode.
    dma: RefCell<Option<DmaBuf>>,
    /// Hub the device currently being addressed sits behind, when it is not on
    /// the root port itself.
    split: Cell<Option<Split>>,
    /// `HCINT` captured at the last channel outcome, success or failure.
    ///
    /// `wait_channel` collapses every hardware result into an error kind, and a
    /// device refusing a request, the bus failing to carry it, and the core
    /// never finishing look identical from the caller's side. Keeping the raw
    /// register lets `report_failure` name which one happened -- and lets the
    /// split paths tell a completed complete-split from one that ended on a
    /// handshake that carries no data.
    last_hcint: Cell<u32>,
}

impl<'a> UsbHostEngine<'a> {
    pub fn new(mmio: &'a MmioRegion) -> Self {
        Self {
            mmio,
            ctrl_mps: Cell::new(CONTROL_MPS_LOW_FULL_SPEED),
            mode: Cell::new(TransferMode::Fifo),
            dma: RefCell::new(None),
            split: Cell::new(None),
            last_hcint: Cell::new(0),
        }
    }

    /// Max packet size currently used for control transfers.
    pub fn control_mps(&self) -> u8 {
        self.ctrl_mps.get()
    }

    /// Address the next transfers through `split`, or the root port when `None`.
    ///
    /// Set before bringing up a device behind a hub and cleared when addressing
    /// something that is not, because a stale context routes traffic through a
    /// hub port it does not belong to.
    pub fn set_split(&self, split: Option<Split>) {
        self.split.set(split);
    }

    /// The split context transfers are currently addressed with.
    pub fn split(&self) -> Option<Split> {
        self.split.get()
    }

    /// `HCCHAR` bits that describe the device rather than the endpoint.
    ///
    /// A low-speed device needs `HCCHAR.LSPDDEV` on every transfer, including
    /// the ones a hub relays, because that is what selects the preamble. It is
    /// carried on the split context because that is where the port's negotiated
    /// speed is known.
    fn device_flags(&self) -> u32 {
        match self.split.get() {
            Some(split) if split.low_speed => HCCHAR_LSPDDEV,
            _ => 0,
        }
    }

    /// `HCSPLT` for the current context; `complete` selects the second pass.
    fn hcsplt_value(&self, complete: bool) -> u32 {
        let Some(split) = self.split.get() else {
            return 0;
        };
        let mut value = HCSPLT_SPLTENA
            | ((split.hub_addr as u32 & HCSPLT_HUBADDR_MASK >> HCSPLT_HUBADDR_SHIFT)
                << HCSPLT_HUBADDR_SHIFT)
            | (split.port as u32 & HCSPLT_PRTADDR_MASK);
        if complete {
            value |= HCSPLT_COMPSPLT;
        }
        value
    }

    /// Current USB frame number, used to bound a complete-split.
    pub fn frame_number(&self) -> u32 {
        self.read32(HFNUM) & HFNUM_FRNUM_MASK
    }

    /// The millisecond frame a periodic split is paired within.
    ///
    /// `HFNUM` counts microframes, so the raw counter advances every 125 us and
    /// cannot answer a question about millisecond frames. Both reference drivers
    /// shift it right three for exactly this check.
    #[inline]
    fn full_frame(&self) -> u32 {
        self.frame_number() >> FULL_FRAME_SHIFT
    }

    /// Wait out one microframe boundary, without yielding.
    ///
    /// A complete-split has to reach the hub in a later microframe than the
    /// start-split it belongs to, and -- for a periodic transfer -- inside the
    /// same millisecond frame, because the hub stops pairing the two across a
    /// frame boundary and the driver treats that as a transaction error.
    ///
    /// Returns false once the millisecond frame has moved, which is the signal
    /// that the pairing is gone and the next poll has to start over. The frame
    /// that must not move is the shifted one: the counter underneath it moves
    /// every microframe, so comparing that directly would fail on the first read
    /// and no complete-split would ever be sent.
    fn await_microframe(&self) -> bool {
        let frame = self.full_frame();
        let start = self.frame_number();
        for _ in 0..MICROFRAME_SPINS {
            let counter = self.read32(HFNUM) & HFNUM_FRNUM_MASK;
            if counter >> FULL_FRAME_SHIFT != frame {
                return false;
            }
            // The counter advances once per microframe, so a change in it is the
            // boundary itself. `FRREM` would say the same thing and depends on
            // the core filling the field in, which this does not.
            if counter != start {
                return true;
            }
        }
        false
    }

    /// The last channel outcome reported `XFERCOMPL`.
    ///
    /// A complete-split that ends on ACK has delivered nothing: Linux is
    /// explicit that ACK belongs to the start-split and "should not occur in
    /// CSPLIT". Reading the raw bit is the only way to tell, because every
    /// completion collapses into the same `Ok` on the way out.
    fn last_reported_complete(&self) -> bool {
        self.last_hcint.get() & (1 << 0) != 0
    }

    /// The last channel failure was NYET rather than NAK.
    ///
    /// Both arrive as `WouldBlock` and they mean opposite things for a split: a
    /// hub answering NYET has not finished the transaction and is worth asking
    /// again, while a NAK ends the pairing and the next attempt has to begin
    /// with a fresh start-split.
    fn last_was_nyet(&self) -> bool {
        self.last_hcint.get() & (1 << 6) != 0
    }

    /// `HCCHAR` for this instant, with `ODDFRM` set from the frame the transfer
    /// will be sent in.
    ///
    /// The core latches the parity of the frame a transfer lands in when the
    /// channel starts, and a transfer whose parity disagrees is refused. The
    /// frame read here is the one just started, and the transfer goes out in the
    /// next, which is why the parity is inverted -- the same reading U-Boot
    /// takes before it starts a channel.
    #[inline]
    fn start_hcchar(&self, base: u32) -> u32 {
        if self.frame_number() & 1 == 0 {
            base | HCCHAR_ODDFRM
        } else {
            base & !HCCHAR_ODDFRM
        }
    }

    /// Start a channel and, behind a hub, run the split handshake that reaches
    /// the device at all.
    ///
    /// Every packet takes two passes over the channel: the start-split hands the
    /// transaction to the hub, and the complete-split collects what it buffered.
    /// A hub that has not finished answers the complete-split with NYET, which is
    /// why that is a retry here rather than a failure.
    ///
    /// Without this a full- or low-speed device on a hub port is never reached:
    /// nothing on the bus answers, and the channel comes back with XACTERR. That
    /// reads like a protocol error and is really a missing transaction.
    ///
    /// `arm(complete)` stages the payload and starts the channel for one pass.
    fn run_packet(&self, ch: usize, arm: impl Fn(bool)) -> ViResult<()> {
        if self.split.get().is_none() {
            arm(false);
            return self.wait_channel(ch);
        }

        arm(false);
        self.wait_channel(ch)?;

        let started = self.frame_number();
        for _ in 0..SPLIT_COMPLETE_ATTEMPTS {
            arm(true);
            match self.wait_channel(ch) {
                // Only a real transfer completion is the result. An ACK here is
                // the hub acknowledging the request, not delivering it.
                Ok(()) if self.last_reported_complete() => return Ok(()),
                Ok(()) => {}
                Err(ViError::WouldBlock) => {}
                Err(e) => return Err(e),
            }
            if self.frame_number().wrapping_sub(started) & HFNUM_FRNUM_MASK > SPLIT_COMPLETE_FRAMES
            {
                return Err(ViError::IO);
            }
        }
        Err(ViError::IO)
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

    /// Current payload-transfer mode.
    pub fn mode(&self) -> TransferMode {
        self.mode.get()
    }

    /// Select the payload-transfer mode.
    ///
    /// Switching to DMA allocates the scratch region on first use; a failed
    /// allocation leaves the engine in FIFO mode rather than half-configured.
    pub fn set_mode(&self, mode: TransferMode) -> bool {
        if mode == TransferMode::Dma && self.dma.borrow().is_none() {
            let Some(buf) = DmaBuf::alloc(DMA_SLOT_BYTES * DMA_CHANNELS / 4096) else {
                return false;
            };
            *self.dma.borrow_mut() = Some(buf);
        }
        self.mode.set(mode);
        true
    }

    /// Guest-physical base of `ch`'s DMA slot, or `None` outside DMA mode.
    ///
    /// In SAS grant pages are identity-mapped, so the value programmed into
    /// `HCDMA` is the same address the CPU uses.
    fn dma_slot(&self, ch: usize) -> Option<usize> {
        if self.mode.get() != TransferMode::Dma || ch >= DMA_CHANNELS {
            return None;
        }
        let guard = self.dma.borrow();
        guard.as_ref().map(|b| b.phys() + ch * DMA_SLOT_BYTES)
    }

    /// Bus address of a RAM physical address on this SoC.
    ///
    /// A bus master does not see ARM physical addresses. On the BCM283x the
    /// Raspberry Pi device tree's `_DMA` resource states the translation
    /// outright: "Bus 0xC0000000 -> CPU 0x00000000", over the first gigabyte
    /// and marked NonCacheable. U-Boot's working dwc2 driver programs `HCDMA`
    /// through exactly this translation (`phys_to_bus`).
    ///
    /// Programming the raw ARM physical address asks the core to read and write
    /// a different location, and the failure is quiet: the address still lands
    /// in mapped memory, so no AHB error is raised, and the CPU-side read-back
    /// of the slot looks correct because the CPU and the core are simply
    /// looking at different bytes.
    #[inline]
    fn bus_address(phys: usize) -> u32 {
        (phys | 0xC000_0000) as u32
    }

    /// Point the channel's DMA engine at `offset` bytes into its slot.
    ///
    /// Programmed **once per transfer, and always before `CHENA`**. The DWC2
    /// advances `HCDMA` itself as it moves each packet (`hcdma += actual`), so a
    /// multi-packet transfer walks the slot on its own; re-arming mid-transfer
    /// both defeats that walk and races the core's asynchronous completion.
    ///
    /// The ordering against `CHENA` is not a preference. The core latches its
    /// DMA address when the channel is enabled, so a start that arms `HCDMA`
    /// afterwards -- or never -- sends the first transaction of the transfer to
    /// whatever address the previous one left behind. On a SETUP that is a
    /// request the device ACKs and then STALLs one stage later, and the retry
    /// succeeds because by then the register holds the right address.
    fn program_hcdma(&self, ch: usize, offset: usize) {
        if let Some(base) = self.dma_slot(ch) {
            let addr = base + offset.min(DMA_SLOT_BYTES);
            self.write32(hcdma(ch), Self::bus_address(addr));
        }
    }

    /// Publish CPU writes in `[offset, offset+len)` to the device.
    ///
    /// A refused sync is reported, never skipped: without the clean the core
    /// reads stale memory, so the device receives a request whose bytes are not
    /// the ones staged and answers with a STALL -- a failure that looks like a
    /// protocol problem and is actually a missing cache operation.
    fn cache_clean(&self, ch: usize, offset: usize, len: usize) {
        let guard = self.dma.borrow();
        if let Some(buf) = guard.as_ref() {
            let start = ch * DMA_SLOT_BYTES + offset.min(DMA_SLOT_BYTES);
            let end = (start + len).min((ch + 1) * DMA_SLOT_BYTES);
            let span = end.saturating_sub(start);
            if span == 0 {
                // Nothing to publish. The kernel rejects a zero-length sync as
                // an invalid range, so asking for one turns a no-op into a
                // reported failure -- and a transfer that legitimately moves no
                // bytes (an idle bulk-IN poll) would warn on every poll.
                return;
            }
            match buf.begin_cache_sync(start, span) {
                Some(token) => {
                    if !buf.complete_cache_sync(token) {
                        ostd::io::println("[dwc2] WARN: cache sync completion refused");
                    }
                }
                None => {
                    ostd::io::println(
                        "[dwc2] WARN: cache sync refused - device may read stale memory",
                    );
                }
            }
        }
    }

    /// Drop stale cache lines over a range the device just wrote.
    fn cache_invalidate(&self, ch: usize, offset: usize, len: usize) {
        self.cache_clean(ch, offset, len);
    }

    /// Copy an outgoing payload into the channel slot at `offset` and publish it.
    ///
    /// This is one of the crate's two `allow(unsafe_code)` islands (see
    /// `scripts/unsafe-allowlist.toml`): the destination is a grant region this
    /// cell owns for its whole lifetime, `n` is bounded by the slot size and the
    /// source length, and the region is not aliased by any other live reference.
    #[allow(unsafe_code)]
    fn stage_out(&self, ch: usize, offset: usize, data: &[u8]) {
        let guard = self.dma.borrow();
        let n = data.len().min(DMA_SLOT_BYTES - offset.min(DMA_SLOT_BYTES));
        if let Some(buf) = guard.as_ref() {
            // SAFETY: destination is this cell's grant slot at a bounded
            // offset; `n` is bounded by the remaining slot and by `data`.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    buf.virt().wrapping_add(ch * DMA_SLOT_BYTES + offset),
                    n,
                );
            }
        }
        drop(guard);
        self.cache_clean(ch, offset, n);
    }

    /// The third `allow(unsafe_code)` island: a volatile read of this cell's
    /// own grant slot at a bounded offset, used only for the trace below.
    #[allow(unsafe_code)]
    /// Print the first bytes staged in a channel slot.
    ///
    /// Proves what the core is about to hand the device. If these are not the
    /// request the caller built, the fault is in staging or cache maintenance;
    /// if they are, the fault is between memory and the bus.
    pub fn trace_slot(&self, ch: usize, label: &str, len: usize) {
        let guard = self.dma.borrow();
        let Some(buf) = guard.as_ref() else {
            return;
        };
        ostd::io::print("[dwc2] slot ");
        ostd::io::print(label);
        ostd::io::print(" =");
        let n = len.min(16).min(DMA_SLOT_BYTES);
        for i in 0..n {
            // SAFETY: reading this cell's own grant slot at a bounded offset.
            let byte = unsafe {
                core::ptr::read_volatile(
                    buf.virt().wrapping_add(ch * DMA_SLOT_BYTES).wrapping_add(i),
                )
            };
            ostd::io::print(" ");
            print_hex_val(byte as u32);
        }
        ostd::io::println("");
    }

    /// Copy a received payload out of the channel slot at `offset`.
    ///
    /// The second `allow(unsafe_code)` island: same owned-grant argument as
    /// [`Self::stage_out`], with `n` additionally bounded by the destination.
    #[allow(unsafe_code)]
    fn collect_in(&self, ch: usize, offset: usize, dst: &mut [u8], len: usize) {
        self.cache_invalidate(ch, offset, len);
        let guard = self.dma.borrow();
        if let Some(buf) = guard.as_ref() {
            let n = len
                .min(dst.len())
                .min(DMA_SLOT_BYTES.saturating_sub(offset.min(DMA_SLOT_BYTES)));
            // SAFETY: source is this cell's grant slot at a bounded offset;
            // `n` is bounded by the slot remainder and the destination.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    buf.virt().wrapping_add(ch * DMA_SLOT_BYTES + offset) as *const u8,
                    dst.as_mut_ptr(),
                    n,
                );
            }
        }
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
        self.write32(hcintmsk(ch), 0x07FF);

        // HCTSIZ: XFERSIZE = 8, PKTCNT = 1, PID = 3 (SETUP).
        // HCCHAR: EPNUM = 0, EPDIR = 0 (OUT), EPTYPE = 0 (Control), MC = 1.
        let sctsiz = 8 | (1 << 19) | (3 << 29);
        let scchar = (self.control_mps() as u32)
            | self.device_flags()
            | (1 << 20)
            | ((dev_addr as u32) << 22)
            | (1 << 31);
        let dma = self.dma_slot(ch).is_some();

        // Stage once up front so the trace has something to show. Each pass
        // stages again, which is also what hands the bytes to the core.
        if dma {
            self.stage_out(ch, 0, setup);
            static SETUP_TRACED: core::sync::atomic::AtomicBool =
                core::sync::atomic::AtomicBool::new(false);
            if !SETUP_TRACED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                self.trace_slot(ch, "setup", setup.len());
            }
        }

        // The payload is staged and HCDMA armed *before* CHENA on every pass:
        // the core latches its DMA address when the channel starts, so enabling
        // first reads the previous transfer's address and the device ACKs eight
        // bytes it never looked at.
        self.run_packet(ch, |complete| {
            self.write32(hcsplt(ch), self.hcsplt_value(complete));
            self.write32(hctsiz(ch), sctsiz);
            if dma {
                self.program_hcdma(ch, 0);
                self.stage_out(ch, 0, setup);
            } else {
                self.write_fifo(
                    ch,
                    u32::from_le_bytes([setup[0], setup[1], setup[2], setup[3]]),
                );
                self.write_fifo(
                    ch,
                    u32::from_le_bytes([setup[4], setup[5], setup[6], setup[7]]),
                );
            }
            self.write32(hcint(ch), 0xFFFF_FFFF);
            self.write32(hcchar(ch), self.start_hcchar(scchar));
        })
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
                               // Arm the DMA engine once; the core advances it per packet.
        self.program_hcdma(ch, 0);

        while received < buf.len() {
            let chunk = (buf.len() - received).min(mps);
            self.prepare_channel(ch);
            self.write32(hcintmsk(ch), 0x07FF);

            // HCCHAR: EPNUM = 0, EPDIR = 1 (IN), EPTYPE = 0 (Control), MC = 1.
            let sctsiz = (chunk as u32) | (1 << 19) | (toggle << 29);
            let scchar = (mps as u32)
                | self.device_flags()
                | (1 << 15)
                | (1 << 20)
                | ((dev_addr as u32) << 22)
                | (1 << 31);

            self.run_packet(ch, |complete| {
                self.write32(hcsplt(ch), self.hcsplt_value(complete));
                self.write32(hctsiz(ch), sctsiz);
                // Directly, the address is armed once before the loop and the
                // core walks the slot itself, so re-arming per packet would
                // overwrite what it has already written. Behind a hub the core
                // moves a single packet per pass, so the address is re-armed
                // every pass and advanced by what has arrived so far.
                if complete || self.split.get().is_some() {
                    self.program_hcdma(ch, received);
                }
                self.write32(hcint(ch), 0xFFFF_FFFF);
                self.write32(hcchar(ch), self.start_hcchar(scchar));
            })?;

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

        if self.dma_slot(ch).is_some() && received > 0 {
            let mut tmp = [0u8; DMA_SLOT_BYTES];
            let n = received.min(buf.len()).min(DMA_SLOT_BYTES);
            self.collect_in(ch, 0, &mut tmp, n);
            buf[..n].copy_from_slice(&tmp[..n]);
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
        // HCTSIZ: XFERSIZE = 0, PKTCNT = 1, PID = 2 (DATA1).
        // HCCHAR: EPNUM = 0, EPTYPE = 0 (Control), MPS = 64, MC = 1, EPDIR from
        // the caller.
        let sctsiz = (1 << 19) | (2 << 29);
        let epdir = if is_in { 1 } else { 0 };
        let scchar = (self.control_mps() as u32)
            | self.device_flags()
            | (epdir << 15)
            | (1 << 20)
            | ((dev_addr as u32) << 22)
            | (1 << 31);

        self.run_packet(ch, |complete| {
            self.write32(hcsplt(ch), self.hcsplt_value(complete));
            self.write32(hctsiz(ch), sctsiz);
            // A zero-length handshake moves no bytes, but the channel still
            // latches an address when it starts and the previous transfer's is
            // not a position this stage may begin from.
            self.program_hcdma(ch, 0);
            self.write32(hcint(ch), 0xFFFF_FFFF);
            self.write32(hcchar(ch), self.start_hcchar(scchar));
        })
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
            ViError::IO => {
                // The raw outcome, not a bucket: a STALL, an AHB error and a
                // core that never finished all used to print the same word.
                ostd::io::print("IO - ");
                ostd::io::println(self.describe_hcint(self.last_hcint.get()));
            }
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
                            // Stage the whole payload once and arm DMA once: the core reads it
                            // packet by packet, advancing HCDMA itself.
        if self.dma_slot(ch).is_some() {
            self.stage_out(ch, 0, data);
            self.program_hcdma(ch, 0);
        }

        while sent < data.len() {
            let chunk = (data.len() - sent).min(mps);
            self.prepare_channel(ch);
            self.write32(hcsplt(ch), 0);
            self.write32(hcintmsk(ch), 0x07FF);
            let sctsiz = (chunk as u32) | (1 << 19) | ((toggle as u32) << 29);
            self.write32(hctsiz(ch), sctsiz);

            // HCCHAR: EPNUM = 0, EPDIR = 0 (OUT), EPTYPE = 0 (Control), MC = 1, CHENA = 1.
            let scchar = (mps as u32)
                | self.device_flags()
                | (1 << 20)
                | ((dev_addr as u32) << 22)
                | (1 << 31);
            self.write32(hcchar(ch), self.start_hcchar(scchar));

            if self.dma_slot(ch).is_some() {
                // Payload already staged and HCDMA already armed.
            } else {
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

        let use_dma = self.dma_slot(ch).is_some();
        if use_dma {
            self.stage_out(ch, 0, packet);
            self.program_hcdma(ch, 0);
        }
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
                self.write32(hcchar(ch), self.start_hcchar(scchar));

                if use_dma {
                    // Staged and armed before the loop; the core walks it.
                } else {
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
        self.program_hcdma(ch, 0);
        self.write32(hcchar(ch), self.start_hcchar(scchar));

        // Non-blocking wait: check if transfer completed or NAK
        let mut count = 0;
        while count < 1000 {
            let int = self.read32(hcint(ch));
            if int & (1 << 0) != 0 {
                // The core reports the actual length in HCTSIZ either way, but
                // only DMA deposits the payload outside the FIFO.
                let remaining = (self.read32(hctsiz(ch)) & 0x7FFFF) as usize;
                let got = want.saturating_sub(remaining);
                if self.dma_slot(ch).is_some() {
                    self.collect_in(ch, 0, buf, got);
                } else {
                    let words = want.div_ceil(4);
                    for (i, word) in (0..words).map(|i| (i, self.read_fifo(ch))) {
                        for (j, byte) in word.to_le_bytes().iter().enumerate() {
                            let idx = i * 4 + j;
                            if idx < buf.len() && idx < want {
                                buf[idx] = *byte;
                            }
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
        split_pending: &mut bool,
    ) -> ViResult<usize> {
        let want = buf.len().min(mps.max(1) as usize).min(1024);
        if want == 0 {
            return Ok(0);
        }

        // Behind a hub the poll is a pair of transactions, one half per call.
        if self.split.get().is_some() {
            return self.poll_split(ch, dev_addr, ep_num, mps, want, buf, split_pending);
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
            | self.device_flags()
            | (1 << 15) // EPDIR = IN
            | (3 << 18) // EPTYPE = Interrupt
            | (1 << 20) // MC = 1
            | ((dev_addr as u32) << 22)
            | (1 << 31); // CHENA
        self.program_hcdma(ch, 0);
        self.write32(hcchar(ch), self.start_hcchar(scchar));

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
                    if self.dma_slot(ch).is_some() {
                        // DMA deposited the report in the channel slot.
                        let mut tmp = [0u8; DMA_SLOT_BYTES];
                        self.collect_in(ch, 0, &mut tmp, got);
                        let n = got.min(buf.len());
                        buf[..n].copy_from_slice(&tmp[..n]);
                    } else {
                        // Drain ceil(got/4) FIFO words; only the first `got`
                        // bytes are meaningful (the tail word is padding).
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
                    }
                } else if self.dma_slot(ch).is_none() {
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

    /// One half of an interrupt IN split, for a device behind a hub.
    ///
    /// A split is a pair of transactions the hub has to see in different
    /// microframes: a start-split that hands it the request, and a complete-split
    /// issued later that collects the result. One half is issued per call, with
    /// the pairing carried in `pending`, so the gap between them is the gap
    /// between two polls rather than a wait taken here.
    ///
    /// Waiting here is what this used to do, and it cost the machine: `HFNUM`
    /// counts frames of a millisecond, not microframes, so crossing one boundary
    /// meant spinning for a whole millisecond of system calls -- every poll, on
    /// every interface, exactly when a device had something to say.
    ///
    /// The three ways a complete-split can end mean different things:
    ///
    /// * `XFERCOMPL` is the report, and ends the pairing;
    /// * `NYET` is the hub still working, so the pairing stays live;
    /// * `NAK` ends the pairing with nothing.
    ///
    /// ACK carries no data here -- Linux is explicit that it "should not occur in
    /// CSPLIT" -- so it is not taken for a result.
    #[allow(clippy::too_many_arguments)]
    fn poll_split(
        &self,
        ch: usize,
        dev_addr: u8,
        ep_num: u8,
        mps: u16,
        want: usize,
        buf: &mut [u8],
        pending: &mut bool,
    ) -> ViResult<usize> {
        self.prepare_channel(ch);
        self.write32(hcintmsk(ch), 0x07FF);

        // HCTSIZ: XFERSIZE = one max packet, PKTCNT = 1, PID = DATA0. A split
        // carries exactly one packet, whatever the caller asked for.
        //
        // HCCHAR: MPS, EPNUM, IN, EPTYPE = 3 (Interrupt), and MC = 3. The
        // multicount is how many times the core retries the transaction itself,
        // and for a periodic split it is more than one: the hub answers NYET
        // until it has run the full- or low-speed transaction, so a single
        // attempt meets that NYET and ends the transfer with nothing.
        let sctsiz = (want as u32) | (1 << 19);
        let scchar = (mps as u32 & 0x7FF)
            | ((ep_num as u32) << 11)
            | self.device_flags()
            | (1 << 15)
            | (3 << 18)
            | (3 << 20)
            | ((dev_addr as u32) << 22)
            | (1 << 31);
        let arm = |complete: bool| {
            self.write32(hcsplt(ch), self.hcsplt_value(complete));
            self.write32(hctsiz(ch), sctsiz);
            self.program_hcdma(ch, 0);
            self.write32(hcint(ch), 0xFFFF_FFFF);
            self.write32(hcchar(ch), self.start_hcchar(scchar));
        };

        // Both halves are issued here, one microframe apart and inside a single
        // frame. The second half cannot be handed to a later poll: this loop's
        // RecvTimeout puts hundreds of frames between polls and the hub pairs a
        // split for less than one, which is what made every complete-split here
        // answer NYET.
        // Both halves are issued here, one microframe apart and inside a single
        // frame, and nothing is written to the console between them. A line of
        // console output costs about seven milliseconds at this baud, which is
        // seven of the frames the hub pairs a split within -- printing the first
        // half before issuing the second is what kept the pair apart. Every report
        // below therefore sits on a path where no further channel work depends on
        // it, and the retry path has none at all.
        let frame = self.frame_number();
        arm(false);
        let outcome = self.wait_channel_spin(ch);
        if !matches!(outcome, Ok(())) {
            *pending = false;
            report_split_progress(
                false,
                &outcome,
                self.last_hcint.get(),
                self.last_was_nyet(),
                want,
                frame,
            );
            return match outcome {
                Err(e) => Err(e),
                _ => Ok(0),
            };
        }

        for _ in 0..SPLIT_ATTEMPTS {
            // The hub needs the full- or low-speed transaction run before it can
            // answer; that is what its NYET reports. One microframe is the unit
            // it is measured in.
            if !self.await_microframe() {
                // The frame moved, so the pairing is gone whether or not the hub
                // ever answered.
                *pending = false;
                return Ok(0);
            }

            arm(true);
            let outcome = self.wait_channel_spin(ch);
            let int = self.last_hcint.get();

            match outcome {
                Ok(()) if !self.last_reported_complete() => {
                    // ACK on a complete-split carries no data.
                    *pending = false;
                    report_split_progress(true, &outcome, int, self.last_was_nyet(), want, frame);
                    return Ok(0);
                }
                Ok(()) => {
                    *pending = false;
                    let remaining = (self.read32(hctsiz(ch)) & 0x7FFFF) as usize;
                    let got = want.saturating_sub(remaining);
                    if got == 0 {
                        report_split_progress(
                            true,
                            &outcome,
                            int,
                            self.last_was_nyet(),
                            want,
                            frame,
                        );
                        return Ok(0);
                    }
                    if self.dma_slot(ch).is_some() {
                        let mut tmp = [0u8; DMA_SLOT_BYTES];
                        self.collect_in(ch, 0, &mut tmp, got.min(DMA_SLOT_BYTES));
                        let n = got.min(buf.len());
                        buf[..n].copy_from_slice(&tmp[..n]);
                        return Ok(n);
                    }
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
                    return Ok(got.min(buf.len()));
                }
                // Still working, or NAK which ends the pairing. Either way the
                // next poll starts over rather than asking again here -- and a
                // hub that answered NYET is asked again below, with nothing
                // written in between for the same reason as above.
                Err(ViError::WouldBlock) => {
                    if !self.last_was_nyet() {
                        *pending = false;
                        report_split_progress(
                            true,
                            &outcome,
                            int,
                            self.last_was_nyet(),
                            want,
                            frame,
                        );
                        return Ok(0);
                    }
                }
                Err(e) => {
                    *pending = false;
                    report_split_progress(true, &outcome, int, self.last_was_nyet(), want, frame);
                    return Err(e);
                }
            }
        }

        // The hub never finished. Starting over on the next poll is the only
        // thing left: the frame this pairing belonged to is gone with it.
        *pending = false;
        Ok(0)
    }

    /// Name the host-channel interrupt bit that ended a transfer.
    ///
    /// The DWC2 encodes one cause per bit and they need different fixes:
    /// `AHBERR` is the core failing to reach memory at `HCDMA`, `XACTERR` is a
    /// bus protocol error, `BBLERR` is the device sending more than the
    /// programmed packet size, `DTERR` is a data-toggle mismatch, and `STALL`
    /// is the device itself refusing. Collapsing them into one "IO error" hides
    /// which of those actually happened.
    fn describe_hcint(&self, int: u32) -> &'static str {
        if int & (1 << 2) != 0 {
            return "AHBERR - core could not reach memory at HCDMA";
        }
        if int & (1 << 8) != 0 {
            return "BBLERR - babble, device exceeded the packet size";
        }
        if int & (1 << 3) != 0 {
            return "STALL - endpoint refused the request";
        }
        if int & (1 << 7) != 0 {
            return "XACTERR - transaction error";
        }
        if int & (1 << 10) != 0 {
            return "DTERR - data toggle mismatch";
        }
        if int & (1 << 9) != 0 {
            return "FRMOVRN - frame overrun";
        }
        if int & (1 << 6) != 0 {
            return "NYET - not ready";
        }
        if int & (1 << 4) != 0 {
            return "NAK";
        }
        "unknown"
    }

    /// Print one register as `[dwc2]   NAME=0xVALUE`.
    fn dump_reg(&self, name: &str, val: u32) {
        ostd::io::print("[dwc2]   ");
        ostd::io::print(name);
        ostd::io::print("=0x");
        print_hex_val(val);
        ostd::io::println("");
    }

    /// Dump the channel and core registers that explain a stalled transfer.
    ///
    /// A timeout with no status bit set (`HCINT == 0`) is ambiguous from the
    /// outside: the channel may never have been issued, may have been issued and
    /// silently abandoned by the core, or the port may have dropped underneath
    /// it. These registers distinguish those cases.
    pub fn dump_transfer_state(&self, ch: usize, why: &str) {
        ostd::io::print("[dwc2] state at ");
        ostd::io::print(why);
        ostd::io::println(":");
        for (name, offset) in [
            ("HCCHAR", hcchar(ch)),
            ("HCTSIZ", hctsiz(ch)),
            ("HCDMA", hcdma(ch)),
            ("HCINTMSK", hcintmsk(ch)),
            ("HCSPLT", hcsplt(ch)),
            ("HFNUM", HFNUM),
        ] {
            self.dump_reg(name, self.read32(offset));
        }
        self.dump_reg("HCINT", self.read32(hcint(ch)));
        self.dump_reg("HAINT", self.read32(HAINT));
        self.dump_reg("GINTSTS", self.read32(GINTSTS));
        self.dump_reg("GINTMSK", self.read32(GINTMSK));
        self.dump_reg("HPRT0", self.read32(HPRT0));
        self.dump_reg("GRSTCTL", self.read32(GRSTCTL));
        self.dump_reg("GNPTXSTS", self.read32(GNPTXSTS));
        self.dump_reg("MODE", self.mode.get() as u32);
    }

    /// Log a channel error once per distinct cause, with the register state.
    fn report_channel_error(&self, ch: usize, int: u32) {
        static REPORTED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
        if REPORTED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 4 {
            ostd::io::print("[dwc2] channel error: ");
            ostd::io::println(self.describe_hcint(int));
            self.dump_transfer_state(ch, "error");
        }
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

    /// Wait for channel transfer completion or error with timeout.
    ///
    /// BCM2837 DWC2 Slave mode: the core does NOT reliably set XFERCOMPL
    /// or CHHLTD after a successful transaction.  ACK (bit 5) from the
    /// device is the definitive completion signal; we halt the channel
    /// manually after seeing it.
    fn wait_channel(&self, ch: usize) -> ViResult<()> {
        self.wait_channel_with(ch, WAIT_POLLS)
    }

    /// Wait for channel completion or error, spending at most `budget` polls.
    ///
    /// The budget is a real cost rather than a formality: every poll yields, and
    /// a transfer that never reports anything spends the whole of it before it is
    /// given up on. Callers on the driver's own serving thread want a small one.
    fn wait_channel_with(&self, ch: usize, budget: usize) -> ViResult<()> {
        for _ in 0..budget {
            if let Some(outcome) = self.channel_outcome(ch) {
                return outcome;
            }
            sys_yield();
        }
        self.channel_timeout(ch)
    }

    /// Wait for a channel without handing the CPU away.
    ///
    /// A periodic split has to keep both halves inside one millisecond frame. The
    /// yielding wait gives the CPU away for around twenty milliseconds per poll,
    /// which put the complete-split twenty frames past the start-split it belongs
    /// to -- outside the window the hub pairs them within, so it came back as a
    /// transaction error however many times it was tried.
    ///
    /// The bound is the frame itself rather than a count: past that the pairing is
    /// gone and starting over on the next poll is the only useful thing left. This
    /// runs only when a device has something to report.
    fn wait_channel_spin(&self, ch: usize) -> ViResult<()> {
        let frame = self.full_frame();
        for _ in 0..SPIN_POLLS {
            if let Some(outcome) = self.channel_outcome(ch) {
                return outcome;
            }
            if self.full_frame() != frame {
                self.halt_channel(ch);
                self.last_hcint.set(self.read32(hcint(ch)));
                return Err(ViError::WouldBlock);
            }
        }
        self.channel_timeout(ch)
    }

    /// Classify a channel's interrupt register, or `None` while it is still running.
    fn channel_outcome(&self, ch: usize) -> Option<ViResult<()>> {
        let int = self.read32(hcint(ch));
        if int == 0 {
            return None;
        }

        // ── Hardware-generated halt ───────────────────────────────
        if int & (1 << 1) != 0 {
            // CHHLTD
            self.write32(hcint(ch), 0xFFFF_FFFF);
            // Babble (8) and data-toggle error (10) end a transfer just as
            // surely as the bits that were already checked.
            if int & ((1 << 2) | (1 << 3) | (1 << 7) | (1 << 8) | (1 << 10)) != 0 {
                self.report_channel_error(ch, int);
                self.last_hcint.set(int);
                return Some(Err(ViError::IO));
            }
            // NAK and NYET both mean "ask again", and both have to be reported
            // rather than folded into success: a hub answering a complete-split
            // with NYET has not produced a result yet, and returning Ok there
            // hands the caller an empty buffer as if the transfer had happened.
            if int & ((1 << 4) | (1 << 6)) != 0 {
                self.last_hcint.set(int);
                return Some(Err(ViError::WouldBlock));
            }
            self.last_hcint.set(int);
            return Some(Ok(()));
        }

        // ── Transfer complete ─────────────────────────────────────
        if int & (1 << 0) != 0 {
            self.halt_channel(ch);
            self.last_hcint.set(int);
            return Some(Ok(()));
        }

        // ── ACK = device accepted the packet (BCM2837 primary path)
        if int & (1 << 5) != 0 {
            self.halt_channel(ch);
            self.last_hcint.set(int);
            return Some(Ok(()));
        }

        // ── Error conditions ──────────────────────────────────────
        if int & ((1 << 2) | (1 << 3) | (1 << 7) | (1 << 8) | (1 << 10)) != 0 {
            self.report_channel_error(ch, int);
            self.halt_channel(ch);
            self.last_hcint.set(int);
            return Some(Err(ViError::IO));
        }
        if int & ((1 << 4) | (1 << 6)) != 0 {
            self.halt_channel(ch);
            self.last_hcint.set(int);
            return Some(Err(ViError::WouldBlock));
        }

        None
    }

    /// Give up on a channel that reported nothing within its budget.
    fn channel_timeout(&self, ch: usize) -> ViResult<()> {
        // Timeout — capture hcint BEFORE halt clears it
        let int = self.read32(hcint(ch));
        let char_val = self.read32(hcchar(ch));
        self.halt_channel(ch);
        static TIMEOUT_COUNT: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        let attempt = TIMEOUT_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if attempt < 3 {
            ostd::io::print("[dwc2] TIMEOUT ch=");
            print_hex_val(ch as u32);
            ostd::io::print(" hcint=0x");
            print_hex_val(int);
            ostd::io::print(" hcchar=0x");
            print_hex_val(char_val);
            ostd::io::println("");
            self.dump_transfer_state(ch, "timeout");
        }
        self.last_hcint.set(int);
        Err(ViError::IO)
    }

    /// Explicitly halt a host channel (required in DWC2 Slave mode).
    ///
    /// `CHDIS` and `CHENA` must never be set together. The core reads
    /// `CHENA=1` as a fresh enable, so raising it while requesting a disable
    /// **re-arms the channel with the parameters still in `HCCHAR`/`HCTSIZ`** --
    /// a second, unwanted transaction launched from a completed one. Only
    /// `CHDIS` is raised; the core clears `CHENA` and reports `CHHLTD`.
    fn halt_channel(&self, ch: usize) {
        let reg = hcchar(ch);
        let mut val = self.read32(reg);
        val |= 1 << 30; // CHDIS
        val &= !(1 << 31); // CHENA
        self.write32(reg, val);
        // Wait for CHHLTD (bit 1) with a short timeout. The core clears CHENA in
        // microseconds, so this only has to be long enough not to be missed --
        // and it runs on the serving thread, so it must not be long enough to be
        // felt if the core never answers.
        for _ in 0..CHANNEL_HALT_POLLS {
            if self.read32(hcint(ch)) & (1 << 1) != 0 {
                break;
            }
            sys_yield();
        }
        self.write32(hcint(ch), 0xFFFF_FFFF);
    }
}

/// Report the first split poll in full, once.
///
/// Report how each half of a split ended, a few times per half.
///
/// Which half ran and how it ended is the whole question when a split does not
/// deliver: a start-split the hub refuses, a complete-split it is not ready for,
/// and a complete-split that ends without data are three different faults, and
/// nothing else in the driver tells them apart. Both halves are reported
/// separately -- a single shared flag reports one half and hides the other, which
/// is exactly the one worth seeing. The frame number comes along so the gap
/// between the halves is visible rather than inferred.
fn report_split_progress(
    complete: bool,
    outcome: &ViResult<()>,
    hcint: u32,
    nyet: bool,
    want: usize,
    frame: u32,
) {
    static SEEN: [core::sync::atomic::AtomicUsize; 2] =
        [const { core::sync::atomic::AtomicUsize::new(0) }; 2];
    let slot = usize::from(complete);
    if SEEN[slot].fetch_add(1, core::sync::atomic::Ordering::Relaxed) >= 3 {
        return;
    }
    ostd::io::print("[dwc2] split: half=");
    ostd::io::print(if complete { "csplit" } else { "ssplit" });
    ostd::io::print(" outcome=");
    match outcome {
        Ok(()) => ostd::io::print("Ok"),
        Err(ViError::WouldBlock) if nyet => ostd::io::print("NYET"),
        Err(ViError::WouldBlock) => ostd::io::print("NAK"),
        Err(_) => ostd::io::print("error"),
    }
    ostd::io::print(" hcint=0x");
    print_hex_val(hcint);
    ostd::io::print(" frame=0x");
    print_hex_val(frame);
    ostd::io::print(" want=");
    print_usize_val(want);
    ostd::io::println("");
}

fn print_usize_val(v: usize) {
    let mut out = [0u8; 20];
    let mut n = v;
    let mut len = 0;
    if n == 0 {
        ostd::io::print("0");
        return;
    }
    while n > 0 && len < out.len() {
        out[len] = b'0' + (n % 10) as u8;
        n /= 10;
        len += 1;
    }
    out[..len].reverse();
    if let Ok(s) = core::str::from_utf8(&out[..len]) {
        ostd::io::print(s);
    }
}

pub(crate) fn print_hex_val(val: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut buf = [0u8; 8];
    for i in 0..8 {
        buf[7 - i] = HEX[((val >> (i * 4)) & 0xF) as usize];
    }
    if let Ok(s) = core::str::from_utf8(&buf) {
        ostd::io::print(s);
    }
}
