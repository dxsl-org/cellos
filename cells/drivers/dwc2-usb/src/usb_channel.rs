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

/// Maximum complete-splits issued while the accepted start-split is still live.
const SPLIT_ATTEMPTS: usize = 4;
/// Spin budget for one split poll attempt.
///
/// The split path keeps `split_pending` across calls, so a shorter spin only
/// means the poll resumes on the next iteration. The full `SPIN_POLLS` budget
/// here cost hundreds of milliseconds per HID poll, and the NIC driver cell
/// shares its loop with HID: a NIC round trip (client -> cell -> front-end ->
/// cell -> USB -> reply) then exceeded the client's reply timeout even though
/// the frame was transmitted.
const SPLIT_SPIN_POLLS: usize = 20_000;

/// U-Boot's working DWC2 path abandons a split after four raw HFNUM ticks.
const PERIODIC_SPLIT_MICROFRAMES: u32 = 4;

/// Microframes in one full-speed frame.
///
/// `HFNUM` counts microframes, and a hub pairs the two halves of a split inside
/// one millisecond frame, so the counter has to be shifted to ask that question.
/// Both reference drivers derive the frame the same way.
const FULL_FRAME_SHIFT: u32 = 3;

/// Register reads a non-yielding channel wait will make before giving up.
///
/// The channel is left to report for itself (a frame check here once halted a
/// split exactly as its low-speed half was finishing), so this is the only bound:
/// it stops a core that never reports anything from spinning forever. Callers
/// that must also stay inside a split's pairing window check the microframe
/// counter themselves -- `run_packet` does that per attempt.
const SPIN_POLLS: usize = 200_000;

/// Poll budget for the ordinary `wait_channel`.
/// Host microframes a *yielding* channel wait may spend before giving up.
///
/// 800 microframes is 100 ms of bus time — far more than a high-speed bulk
/// transfer needs (1514 bytes is ~26 us on the wire) and more than a split
/// sequence needs, but small enough that a stuck channel cannot hold the driver's
/// loop. The count `WAIT_POLLS` it replaces could not express that: each poll
/// yields, and a yield costs ~20 ms here.
const WAIT_MICROFRAMES: u32 = 800;

/// Yields the same wait may spend. A yield costs ~20 ms here, so eight of them is
/// the same ~100 ms expressed in the unit the wait actually spends — and it is
/// what bounds the wait when the microframe counter itself wraps (it is 16 bits
/// wide, so an eight-second stall can mask back to a small elapsed value).
const WAIT_YIELDS: u32 = 8;

/// Retries a bulk OUT makes against a NAK before giving up.
///
/// Each attempt costs the channel wait plus a yield (~20 ms on the board), so 50 of
/// them was a second per give-up and the console showed a line for each — while the
/// Net Cell, which retries the frame itself, waited out its whole deadline inside
/// the driver. Eight keeps a retry inside that deadline.
const BULK_NAK_RETRIES: usize = 8;

/// Register reads spent waiting for a Tx FIFO flush to complete.
///
/// The core clears `GRSTCTL.TXFFLSH` in microseconds; this only has to be long
/// enough not to miss it.
const TX_FLUSH_POLLS: usize = 1_000;

/// Poll budget for waiting out a channel halt.
///
/// The core clears `CHENA` in microseconds; this only has to be long enough not
/// to be missed, and every iteration of it is a yield handed to the rest of the
/// system.
const CHANNEL_HALT_POLLS: usize = 4_000;

/// Complete-split attempts for a synchronous control transfer.
///
/// One per microframe, which is the most a split pairing can use: the window
/// U-Boot bounds a split by is four raw `HFNUM` ticks past its start-split, so a
/// fifth attempt could only land after the pairing is gone. The board trace is
/// what set this: two attempts issued inside the start-split's own microframe
/// both came back NYET and the low-speed keyboard never enumerated.
const SPLIT_COMPLETE_ATTEMPTS: usize = 4;

/// Register reads spent waiting for the microframe counter to move.
///
/// A microframe is 125 us and one `HFNUM` read costs a couple hundred
/// nanoseconds, so this covers a boundary with room to spare while still
/// returning if the counter never moves. The wait is a spin on purpose: a
/// `sys_yield()` hands the CPU away for about twenty milliseconds, which is
/// sixty-four microframes -- the pairing is over long before control comes back.
const MICROFRAME_POLLS: usize = 2_000;

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

/// Register snapshots from one endpoint-zero split packet, printed only after
/// the packet fails. Printing between halves costs multiple USB frames.
#[derive(Clone, Copy)]
struct SplitTiming {
    /// Complete-splits actually issued, including ones the window cut short.
    attempts: usize,
    passes: usize,
    /// Per pass: HFNUM before/after arm and after wait; HCINT, HCCHAR, HCSPLT.
    samples: [[u32; 6]; 3],
}

impl SplitTiming {
    const EMPTY: Self = Self {
        attempts: 0,
        passes: 0,
        samples: [[0; 6]; 3],
    };
}

/// Devices whose endpoint data toggles this engine tracks (USB device addresses).
const TOGGLE_DEVICES: usize = 128;
/// Endpoints per device (USB 2.0 allows 16 per direction).
const TOGGLE_ENDPOINTS: usize = 16;

/// Data toggle (`HCTSIZ.PID`) to use next, per device, endpoint and direction.
///
/// USB 2.0 §8.6: the data toggle belongs to the endpoint, not to a transfer, so it
/// survives from one URB to the next — and the DWC2 reports the value a transfer
/// actually ended on back in `HCTSIZ`. U-Boot's `dwc2.c` keeps exactly this table
/// (`in_data_toggle`/`out_data_toggle`, filled from the register in
/// `wait_for_chhltd`) because without it every packet after the first carries the
/// wrong toggle. The driver here used a constant DATA0 for bulk and interrupt IN
/// and restarted DATA0 for every bulk OUT call, which is what the board reported
/// as `[lan9514] first bulk-IN failure: DTERR - data toggle mismatch` — and why
/// the receive path delivered a handful of frames and then froze (`nic_rx=5`, then
/// nothing, while the keyboard dropped reports).
struct DataToggles {
    in_toggle: [[u8; TOGGLE_ENDPOINTS]; TOGGLE_DEVICES],
    out_toggle: [[u8; TOGGLE_ENDPOINTS]; TOGGLE_DEVICES],
}

impl DataToggles {
    const fn new() -> Self {
        Self {
            in_toggle: [[0; TOGGLE_ENDPOINTS]; TOGGLE_DEVICES],
            out_toggle: [[0; TOGGLE_ENDPOINTS]; TOGGLE_DEVICES],
        }
    }
}

/// Index a device address inside the toggle table.
///
/// `None` for an address or endpoint outside the tracked range: such a transfer
/// runs with DATA0 rather than aliasing another device's toggle.
fn toggle_slot(dev_addr: u8, ep_num: u8) -> Option<(usize, usize)> {
    let dev = dev_addr as usize;
    let ep = ep_num as usize & 0x0F;
    (dev < TOGGLE_DEVICES && ep < TOGGLE_ENDPOINTS).then_some((dev, ep))
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
    /// Last control split's timing, retained until its caller reports failure.
    last_split_timing: Cell<SplitTiming>,
    /// Endpoint data toggles; see [`DataToggles`].
    toggles: RefCell<DataToggles>,
}

/// Transfer faults by cause, for the driver's `loop-trace` report.
///
/// Errors are the slow path, so counting them costs nothing on a working
/// transfer, and the *mix* is what names the fault: a board run whose NIC
/// transactions all fail wants to know whether they end in `XACTERR` (the bus
/// protocol, i.e. the split schedule) or in the poll budget (a transfer the
/// driver waited out), because the two need different fixes. Zero in a build
/// that never prints them.
pub static FAULT_XACTERR: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
pub static FAULT_STALL: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
pub static FAULT_OTHER: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
pub static FAULT_TIMEOUT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Fault counts as `[xacterr, stall, other, timeout]`.
pub fn fault_counts() -> [usize; 4] {
    use core::sync::atomic::Ordering;
    [
        FAULT_XACTERR.load(Ordering::Relaxed),
        FAULT_STALL.load(Ordering::Relaxed),
        FAULT_OTHER.load(Ordering::Relaxed),
        FAULT_TIMEOUT.load(Ordering::Relaxed),
    ]
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
            last_split_timing: Cell::new(SplitTiming::EMPTY),
            toggles: RefCell::new(DataToggles::new()),
        }
    }

    /// `HCTSIZ.PID` bits for the next transfer to `(dev_addr, ep_num)`.
    fn next_pid(&self, dev_addr: u8, ep_num: u8, dir_in: bool) -> u32 {
        let Some((dev, ep)) = toggle_slot(dev_addr, ep_num) else {
            return 0; // DATA0
        };
        let toggles = self.toggles.borrow();
        let pid = if dir_in {
            toggles.in_toggle[dev][ep]
        } else {
            toggles.out_toggle[dev][ep]
        };
        (pid as u32) << 29
    }

    /// Adopt the data toggle the core ended the transfer on.
    ///
    /// Read back from `HCTSIZ.PID` rather than flipped locally: the core reports
    /// the value the transfer actually completed with, which is what the endpoint
    /// expects next. U-Boot does the same in `wait_for_chhltd`.
    fn adopt_pid(&self, ch: usize, dev_addr: u8, ep_num: u8, dir_in: bool) {
        let Some((dev, ep)) = toggle_slot(dev_addr, ep_num) else {
            return;
        };
        let pid = ((self.read32(hctsiz(ch)) >> 29) & 0x3) as u8;
        let mut toggles = self.toggles.borrow_mut();
        if dir_in {
            toggles.in_toggle[dev][ep] = pid;
        } else {
            toggles.out_toggle[dev][ep] = pid;
        }
    }

    /// Whether the last failure was a data-toggle mismatch.
    ///
    /// A mismatch means the stored toggle was stale — the endpoint advanced and the
    /// table did not (a port reset under a device, a device that re-enumerated) —
    /// and the fix is to flip the stored value, not to retry it unchanged: the same
    /// value fails the same way forever.
    fn last_was_dterr(&self) -> bool {
        self.last_hcint.get() & (1 << 10) != 0
    }

    /// Flip one endpoint's stored toggle; see [`Self::last_was_dterr`].
    fn flip_pid(&self, dev_addr: u8, ep_num: u8, dir_in: bool) {
        let Some((dev, ep)) = toggle_slot(dev_addr, ep_num) else {
            return;
        };
        let mut toggles = self.toggles.borrow_mut();
        let slot = if dir_in {
            &mut toggles.in_toggle[dev][ep]
        } else {
            &mut toggles.out_toggle[dev][ep]
        };
        *slot = match *slot {
            0 => 2, // DATA0 -> DATA1
            _ => 0, // DATA1 -> DATA0
        };
    }

    /// Put one endpoint's toggle back to DATA0, which is what clearing its halt
    /// does (USB 2.0 §8.6). Only that endpoint: a device-wide reset would also
    /// wrong-foot the endpoints that were not touched.
    fn clear_endpoint_toggle(&self, dev_addr: u8, ep_num: u8, dir_in: bool) {
        let Some((dev, ep)) = toggle_slot(dev_addr, ep_num) else {
            return;
        };
        let mut toggles = self.toggles.borrow_mut();
        if dir_in {
            toggles.in_toggle[dev][ep] = 0;
        } else {
            toggles.out_toggle[dev][ep] = 0;
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
        hcsplt_for(self.split.get(), complete)
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

    /// The millisecond frame, for callers outside this module.
    ///
    /// An endpoint states its interval in milliseconds, so anything comparing
    /// against that number has to count the same unit. `frame_number` counts
    /// microframes, and a gate that compares the two directly opens eight times
    /// too often and at whatever phase the serving loop happens to arrive at.
    pub fn full_frame_number(&self) -> u32 {
        self.full_frame()
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

    /// The last channel failure was the device stalling the endpoint.
    ///
    /// Only this one means the endpoint is halted and needs clearing. A NYET is
    /// the hub asking for time, a NAK is a device with nothing to send, and a bare
    /// halt is the core ending a periodic channel at its frame boundary -- none of
    /// them is a stall, and clearing one that was never set costs a control
    /// transfer on every poll.
    pub fn last_was_stall(&self) -> bool {
        self.last_hcint.get() & (1 << 3) != 0
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
        // The low bit of the counter, which is what Linux uses too: it compares
        // `wire_frame & 1` where wire_frame lives in the same units HFNUM reports,
        // not in millisecond frames. Reading it through the frame shift would be
        // the wrong bit.
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

        // The log's NYET tells us the hub answered CSPLIT but not whether it
        // had time to complete the low-speed transfer. Capture both sides of
        // each channel wait, without printing until the entire packet has ended:
        // sys_yield() can move this thread across many 125-us microframes.
        let mut timing = SplitTiming::EMPTY;
        let before_arm = self.frame_number();
        arm(false);
        let after_arm = self.frame_number();
        let split_reg = self.read32(hcsplt(ch));
        let start = self.wait_channel(ch);
        timing.samples[0] = [
            before_arm,
            after_arm,
            self.frame_number(),
            self.last_hcint.get(),
            self.read32(hcchar(ch)),
            split_reg,
        ];
        timing.passes = 1;
        if let Err(e) = start {
            self.last_split_timing.set(timing);
            return Err(e);
        }

        // A hub publishes the result of a split in the microframe *after* the one
        // that carried the start-split, so a complete-split issued in the same
        // microframe can only ever be answered NYET. The board trace shows exactly
        // that for the low-speed keyboard on hub ports 3 and 5:
        //
        //     split ss  hfnum=0000184A->0000184A->0000184A hcint=0x00000022
        //     split cs1 hfnum=0000184A->0000184A->0000184A hcint=0x00000042
        //     split cs2 hfnum=0000184A->0000184A->0000184A hcint=0x00000042
        //
        // Both retries were spent inside the start-split's own microframe, so both
        // came back NYET and enumeration gave up with the keyboard's descriptor
        // still sitting in the hub. Each retry here waits for the counter to move
        // first, then uses the tight non-yielding wait: yielding would hand the CPU
        // away for ~20 ms and put the pair tens of frames past the window U-Boot
        // bounds a split by -- it abandons one whose complete-split is still NYET
        // more than four raw `HFNUM` ticks after its start-split.
        let started = self.frame_number();
        let mut issued_in = started;
        for attempt in 0..SPLIT_COMPLETE_ATTEMPTS {
            if !self.wait_microframe(issued_in, MICROFRAME_POLLS) {
                break;
            }
            issued_in = self.frame_number();
            if !split_window_open(started, issued_in) {
                break;
            }
            arm(true);
            timing.attempts = attempt + 1;
            let after_arm = self.frame_number();
            let split_reg = self.read32(hcsplt(ch));
            let outcome = self.wait_channel_spin(ch, SPIN_POLLS);
            if timing.passes < timing.samples.len() {
                timing.samples[timing.passes] = [
                    issued_in,
                    after_arm,
                    self.frame_number(),
                    self.last_hcint.get(),
                    self.read32(hcchar(ch)),
                    split_reg,
                ];
                timing.passes += 1;
            }
            match outcome {
                Ok(()) if self.last_reported_complete() => return Ok(()),
                Ok(()) | Err(ViError::WouldBlock) if self.last_was_nyet() => {}
                Err(e) => {
                    self.last_split_timing.set(timing);
                    return Err(e);
                }
                Ok(()) => {
                    self.last_split_timing.set(timing);
                    return Err(ViError::IO);
                }
            }
        }
        self.last_split_timing.set(timing);
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

    /// Log the first failure of each kind, with the request that caused it.
    ///
    /// One line per *failure* is what a flaky bus turns into a flood: the board's
    /// PHY link poll alone issued thousands of control transfers a second, and
    /// every one the bus dropped printed a line that buried the network log it was
    /// meant to explain. A repeat of a kind the console has already shown carries
    /// no new information — the rate is what `[dwc2-loop]` exists for — so each
    /// `(phase, cause)` is reported once and then silenced.
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
        let int = self.last_hcint.get();
        if !claim_failure_report(phase, &error, int) {
            return;
        }
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
        if let Some(split) = self.split.get() {
            ostd::io::print(" hub=");
            print_hex_val(split.hub_addr as u32);
            ostd::io::print(" port=");
            print_hex_val(split.port as u32);
        }
        ostd::io::print(" err=");
        match error {
            ViError::IO if self.split.get().is_some() && self.last_was_nyet() => {
                ostd::io::print("IO - complete split NYET exhausted (");
                print_hex_val(self.last_split_timing.get().attempts as u32);
                ostd::io::print(" attempts)");
            }
            ViError::IO => {
                ostd::io::print("IO - ");
                ostd::io::print(self.describe_hcint(int));
            }
            ViError::WouldBlock => {
                ostd::io::print("WouldBlock - ");
                ostd::io::print(self.describe_hcint(int));
            }
            _ => ostd::io::print("other"),
        }
        ostd::io::print(" hcint=0x");
        print_hex_val(int);
        ostd::io::println("");
        if self.split.get().is_some() {
            let timing = self.last_split_timing.get();
            for (label, sample) in ["ss", "cs1", "cs2"]
                .iter()
                .zip(timing.samples.iter())
                .take(timing.passes)
            {
                ostd::io::print("[dwc2] split ");
                ostd::io::print(label);
                ostd::io::print(" hfnum=");
                print_hex_val(sample[0]);
                ostd::io::print("->");
                print_hex_val(sample[1]);
                ostd::io::print("->");
                print_hex_val(sample[2]);
                ostd::io::print(" hcint=0x");
                print_hex_val(sample[3]);
                ostd::io::print(" hcchar=0x");
                print_hex_val(sample[4]);
                ostd::io::print(" hcsplt=0x");
                print_hex_val(sample[5]);
                ostd::io::println("");
            }
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
            let sctsiz = (chunk as u32) | (1 << 19) | ((toggle as u32) << 29);

            // HCCHAR: EPNUM = 0, EPDIR = 0 (OUT), EPTYPE = 0 (Control), MC = 1, CHENA = 1.
            let scchar = (mps as u32)
                | self.device_flags()
                | (1 << 20)
                | ((dev_addr as u32) << 22)
                | (1 << 31);

            // Through `run_packet`, like the setup and status stages: a data
            // stage that programs `HCSPLT` itself is only right for a device on
            // the root port. Behind a hub the channel would address the
            // low-speed device on the high-speed bus, which nothing answers --
            // the request reaches the keyboard as XACTERR and its LEDs never
            // move. Staging the payload on both passes matches what U-Boot does
            // for an OUT split: the start-split delivers the bytes and the
            // complete-split collects the handshake.
            self.run_packet(ch, |complete| {
                self.write32(hcsplt(ch), self.hcsplt_value(complete));
                self.write32(hcintmsk(ch), 0x07FF);
                self.write32(hctsiz(ch), sctsiz);

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

                self.write32(hcint(ch), 0xFFFF_FFFF);
                self.write32(hcchar(ch), self.start_hcchar(scchar));
            })?;

            sent += chunk;
            toggle = if toggle == 2 { 0 } else { 2 };
        }

        Ok(())
    }

    /// Transmit a raw Ethernet packet via Bulk OUT (Channel 2, EP 2).
    pub fn bulk_transmit(&self, dev_addr: u8, ep_num: u8, packet: &[u8]) -> ViResult<()> {
        let ch = 2;
        if self.port_refuses() {
            return Err(ViError::IO);
        }
        let mut sent = 0;
        let mut halt_cleared = false;
        let mut toggle_flipped = false;

        let use_dma = self.dma_slot(ch).is_some();
        if use_dma {
            self.stage_out(ch, 0, packet);
        }
        while sent < packet.len() {
            let chunk = (packet.len() - sent).min(512); // 512 bytes for High-Speed Bulk

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
                // The packet's PID comes from the endpoint's own toggle, which
                // survives across transfers (see `DataToggles`), and it is read here
                // -- inside the retry loop -- so a toggle the recovery paths flip or
                // reset is what the next attempt actually arms with. Restarting at
                // DATA0 for every frame gave every second and later frame the wrong
                // one, and a receiver discards a mismatched packet even when the
                // handshake completes.
                let sctsiz = (chunk as u32) | (1 << 19) | self.next_pid(dev_addr, ep_num, false);
                self.write32(hctsiz(ch), sctsiz);
                if use_dma {
                    // Point the core at the start of *this* chunk on every attempt,
                    // including retries: the core advances `HCDMA` as it fetches each
                    // packet into the FIFO, so a retry that only rewrote `HCTSIZ`
                    // would fetch the packet after this one as if it were this chunk.
                    // U-Boot and Linux re-issue the packet from its start too.
                    self.program_hcdma(ch, sent);
                }
                self.write32(hcchar(ch), self.start_hcchar(scchar));

                if !use_dma {
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
                    Ok(()) => {
                        // Adopt the toggle this packet ended on, for the next packet
                        // of this frame and for the next frame after it.
                        self.adopt_pid(ch, dev_addr, ep_num, false);
                        break;
                    }
                    Err(ViError::WouldBlock) => {
                        retries += 1;
                        if retries > BULK_NAK_RETRIES {
                            static FIRST_GIVE_UP: core::sync::atomic::AtomicBool =
                                core::sync::atomic::AtomicBool::new(false);
                            if !FIRST_GIVE_UP.swap(true, core::sync::atomic::Ordering::Relaxed) {
                                ostd::io::println(
                                    "[dwc2] bulk_transmit: the endpoint kept answering NAK",
                                );
                            }
                            // Giving up abandons the packet the core staged in the
                            // Tx FIFO, so clear it out like any other abort.
                            self.flush_tx_fifo();
                            return Err(ViError::IO);
                        }
                        sys_yield();
                    }
                    // The device refused the endpoint. A bulk endpoint stays halted
                    // until the host clears it (see `clear_endpoint_halt`), so a
                    // plain retry would fail identically: clear, re-arm and try the
                    // same packet once more.
                    Err(error) if self.last_was_stall() && !halt_cleared => {
                        halt_cleared = true;
                        if !self.clear_endpoint_halt(dev_addr, ep_num, false) {
                            return Err(error);
                        }
                        // Clearing the halt also resets the endpoint's data toggle to
                        // DATA0 (USB 2.0 §8.6), so the retry must go back to it or the
                        // device sees a toggle mismatch (`DTERR`) instead.
                        self.clear_endpoint_toggle(dev_addr, ep_num, false);
                        static FIRST_CLEAR: core::sync::atomic::AtomicBool =
                            core::sync::atomic::AtomicBool::new(false);
                        if !FIRST_CLEAR.swap(true, core::sync::atomic::Ordering::Relaxed) {
                            ostd::io::println(
                                "[dwc2] cleared a halted bulk-OUT endpoint after STALL",
                            );
                        }
                    }
                    // A stale toggle: flip it and re-arm once (see `bulk_receive`).
                    Err(error) if self.last_was_dterr() && !toggle_flipped => {
                        toggle_flipped = true;
                        self.flip_pid(dev_addr, ep_num, false);
                        static FIRST_DTERR: core::sync::atomic::AtomicBool =
                            core::sync::atomic::AtomicBool::new(false);
                        if !FIRST_DTERR.swap(true, core::sync::atomic::Ordering::Relaxed) {
                            ostd::io::println("[dwc2] flipped a stale bulk-OUT toggle after DTERR");
                        }
                        let _ = error;
                    }
                    Err(e) => return Err(e),
                }
            }

            sent += chunk;
        }

        Ok(())
    }

    /// Name the last channel failure for the console.
    ///
    /// `ViError::IO` on its own cannot say whether the device refused the transfer
    /// (`STALL`), the bus protocol broke (`XACTERR`) or the channel never reported
    /// within its budget — and those need different fixes. The board's NIC TX
    /// failed over and over with nothing but `IO` on the console, so the cause went
    /// unrecorded while the frames were dropped.
    pub fn failure_name(&self, error: &ViError) -> &'static str {
        let int = self.last_hcint.get();
        if int == 0 {
            return match error {
                ViError::WouldBlock => "WouldBlock - no status bit, the poll budget ran out",
                _ => "IO - no status bit, the poll budget ran out",
            };
        }
        self.describe_hcint(int)
    }

    /// Flush the non-periodic Tx FIFO after an aborted OUT transfer.
    ///
    /// A packet that was being fetched into the Tx FIFO when its channel was
    /// aborted stays there, and the core keeps feeding the endpoint from it: the
    /// device sees a malformed packet and answers with a transaction error, so the
    /// failure repeats on every retry until the FIFO is cleared. The board's
    /// `first bulk-OUT failure: XACTERR` behaved exactly that way — the first frames
    /// went out, then a HID `SET_REPORT` (a control OUT) aborted mid-packet and every
    /// later frame failed. Linux's `dwc2_hc_cleanup` flushes the Tx FIFO of an OUT
    /// channel for this reason (`if (!chan->ep_is_in)`); this driver shares one
    /// non-periodic FIFO, so the coarse `TXFNUM_ALL` form is used. U-Boot only
    /// flushes at core init, which is why a run that never aborts an OUT transfer
    /// never needs this.
    fn flush_tx_fifo_after_abort(&self, ch: usize) {
        if self.read32(hcchar(ch)) & HCCHAR_EPDIR != 0 {
            return; // an IN channel leaves the Tx FIFO alone
        }
        self.flush_tx_fifo();
    }

    /// Start a Tx FIFO flush and wait (bounded, non-yielding) for it to finish.
    fn flush_tx_fifo(&self) {
        self.write32(GRSTCTL, GRSTCTL_TXFFLSH | GRSTCTL_TXFNUM_ALL);
        for _ in 0..TX_FLUSH_POLLS {
            if self.read32(GRSTCTL) & GRSTCTL_TXFFLSH == 0 {
                break;
            }
        }
        static FIRST_FLUSH: core::sync::atomic::AtomicBool =
            core::sync::atomic::AtomicBool::new(false);
        if !FIRST_FLUSH.swap(true, core::sync::atomic::Ordering::Relaxed) {
            ostd::io::println("[dwc2] flushed the Tx FIFO after an aborted OUT transfer");
        }
    }

    /// Whether the root port is enabled (`HPRT0.PRTENA`).
    ///
    /// The core clears this itself on a bus-level event (USB 2.0 §11.8, a
    /// disconnect), and a disabled port fails *every* transfer — control, bulk and
    /// the hub's splits alike — with no channel status to report, which is exactly
    /// the `no status bit, the poll budget ran out` the board printed for
    /// `bulk_transmit` after its first frames had gone out. It is a local register
    /// read, so the serving loop can watch it.
    pub fn port_enabled(&self) -> bool {
        self.read32(HPRT0) & HPRT0_PRTENA != 0
    }

    /// Core state at the moment the port changed, for the console.
    ///
    /// The board's dead-port runs left one question open: was the *port* lost, or
    /// the whole core reset? A core soft reset re-programs `GINTMSK` to zero
    /// (Linux re-writes it after `dwc2_core_reset` for exactly that reason), so
    /// printing these four at the transition answers it from the log alone —
    /// `GINTMSK` intact with `HPRT0` back at its power-on default means the core is
    /// alive and the *device* went away, which is a different fix from a core that
    /// reset itself.
    pub fn core_state(&self) -> [(&'static str, u32); 4] {
        [
            ("HPRT0", self.read32(HPRT0)),
            ("GINTSTS", self.read32(GINTSTS)),
            ("GINTMSK", self.read32(GINTMSK)),
            ("GRSTCTL", self.read32(GRSTCTL)),
        ]
    }

    /// Clear the root port's change bits.
    ///
    /// `PRTCONNDET`, `PRTENCHNG` and `PRTOVRCURRCHNG` latch and are never cleared by
    /// this driver, so a *later* connect or enable change looked the same as the
    /// boot-time one — which is why the run that ended with `HPRT0.PRTENA = 0` could
    /// not show when the port was disabled. `PRTENA` is deliberately not written:
    /// in this register it is write-1-to-change, so including it would disable a
    /// working port (and the boot path's own writes leave a zero there, which the
    /// board confirms by still reporting the port enabled afterwards).
    pub fn clear_port_change_bits(&self) {
        self.write32(
            HPRT0,
            HPRT0_PRTCONNDET | HPRT0_PRTENCHNG | HPRT0_PRTOVRCURRCHNG,
        );
    }

    /// Clear a halted endpoint: `CLEAR_FEATURE(ENDPOINT_HALT)`.
    ///
    /// USB 2.0 §8.5.3.4: a **bulk** endpoint that stalls stays halted until the host
    /// clears the halt, so a single device `STALL` fails every later transfer to
    /// that endpoint — the shape of a NIC that transmits once and then never again.
    /// The control endpoint is the exception (its next `SETUP` clears the halt),
    /// which is why only the bulk paths call this.
    pub fn clear_endpoint_halt(&self, dev_addr: u8, ep_num: u8, dir_in: bool) -> bool {
        let index = (ep_num & 0x0F) | if dir_in { 0x80 } else { 0x00 };
        self.control_transfer(dev_addr, 0x02, 0x01, 0, index as u16, &mut [])
            .is_ok()
    }

    /// Refuse to start a transfer while the root port is disabled.
    ///
    /// Nothing on a disabled port can complete: the board's log was hundreds of
    /// `bulk_transmit: exceeded 50 NAK retries` lines while the Net Cell waited
    /// seconds per frame, all of it spent discovering what this one register read
    /// says up front. Reported once, because the serving loop already names the
    /// transition.
    fn port_refuses(&self) -> bool {
        if self.port_enabled() {
            return false;
        }
        static FIRST: core::sync::atomic::AtomicBool =
            core::sync::atomic::AtomicBool::new(false);
        if !FIRST.swap(true, core::sync::atomic::Ordering::Relaxed) {
            ostd::io::println("[dwc2] a transfer was refused: the root port is disabled");
        }
        true
    }

    /// Receive a raw packet via Bulk IN (Channel 1, EP 1). Returns received length or 0 if nothing.
    pub fn bulk_receive(&self, dev_addr: u8, ep_num: u8, buf: &mut [u8]) -> ViResult<usize> {
        let ch = 1;
        if self.port_refuses() {
            return Err(ViError::IO);
        }
        let want = buf.len().min(512);
        // HCCHAR: EPDIR = 1 (IN), EPTYPE = 2 (Bulk), MC = 1 packet, MPS = 512 (HS Bulk).
        let scchar = 512
            | ((ep_num as u32) << 11)
            | (1 << 15) // IN
            | (2 << 18) // Bulk
            | (1 << 20) // MC = 1
            | ((dev_addr as u32) << 22)
            | (1 << 31);

        let mut halt_cleared = false;
        let mut toggle_flipped = false;
        loop {
            self.prepare_channel(ch);
            self.write32(hcsplt(ch), 0);
            self.write32(hcintmsk(ch), 0x07FF);
            // PID from the endpoint's own toggle, not a constant DATA0: the toggle
            // belongs to the endpoint and survives across transfers (see
            // `DataToggles`), and a stale one answers every later packet with
            // `DTERR`.
            let sctsiz = (want as u32) | (1 << 19) | self.next_pid(dev_addr, ep_num, true);
            self.write32(hctsiz(ch), sctsiz);
            self.program_hcdma(ch, 0);
            self.write32(hcchar(ch), self.start_hcchar(scchar));

            // A poll, not a wait: the device answers NAK immediately when it has
            // nothing, so a long budget here only ever happens when the core reports
            // nothing at all — and the old 1000 yields is twenty seconds of the
            // serving loop that the Net Cell's request deadline is sitting inside.
            // Same two units as `wait_channel`: host microframes and yields.
            let started = self.frame_number();
            let mut yields = 0;
            let outcome = loop {
                let int = self.read32(hcint(ch));
                if int & (1 << 0) != 0 {
                    // The core reports the actual length in HCTSIZ either way, but
                    // only DMA deposits the payload outside the FIFO. Read the
                    // endpoint's new toggle out of the same register first.
                    self.adopt_pid(ch, dev_addr, ep_num, true);
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
                    break Ok(got);
                }
                if int & (1 << 4) != 0 {
                    // NAK: device has no packet right now.
                    break Ok(0);
                }
                // Error conditions the old read loop ignored (only XFRC and NAK were
                // tested): a refused or broken transfer was reported as "no frame",
                // which hid a halted endpoint behind an ordinary idle poll and made
                // every later poll fail the same silent way.
                if int & ((1 << 3) | (1 << 7) | (1 << 8) | (1 << 10)) != 0 {
                    self.last_hcint.set(int);
                    self.halt_channel(ch);
                    break Err(ViError::IO);
                }
                if yields >= WAIT_YIELDS
                    || self.frame_number().wrapping_sub(started) & HFNUM_FRNUM_MASK
                        >= WAIT_MICROFRAMES
                {
                    break Ok(0);
                }
                yields += 1;
                sys_yield();
            };

            match outcome {
                Ok(n) => return Ok(n),
                // The device refused this endpoint. Clear the halt and re-arm once:
                // until the halt is cleared, no later transfer can succeed.
                Err(error) if self.last_was_stall() && !halt_cleared => {
                    halt_cleared = true;
                    if !self.clear_endpoint_halt(dev_addr, ep_num, true) {
                        return Err(error);
                    }
                    static FIRST_CLEAR: core::sync::atomic::AtomicBool =
                        core::sync::atomic::AtomicBool::new(false);
                    if !FIRST_CLEAR.swap(true, core::sync::atomic::Ordering::Relaxed) {
                        ostd::io::println("[dwc2] cleared a halted bulk-IN endpoint after STALL");
                    }
                }
                // A stale toggle: flip it and re-arm once, or the same packet fails
                // with `DTERR` for as long as the device and the table disagree.
                Err(error) if self.last_was_dterr() && !toggle_flipped => {
                    toggle_flipped = true;
                    self.flip_pid(dev_addr, ep_num, true);
                    static FIRST_DTERR: core::sync::atomic::AtomicBool =
                        core::sync::atomic::AtomicBool::new(false);
                    if !FIRST_DTERR.swap(true, core::sync::atomic::Ordering::Relaxed) {
                        ostd::io::println("[dwc2] flipped a stale bulk-IN toggle after DTERR");
                    }
                    let _ = error;
                }
                Err(error) => return Err(error),
            }
        }
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

        // HCTSIZ: XFERSIZE (= want), PKTCNT = 1, and the PID this endpoint's
        // toggle says (DATA0 only for the first report; see `DataToggles`). The core
        // overwrites XFERSIZE with the remaining count as it transfers.
        let sctsiz = (want as u32) | (1 << 19) | self.next_pid(dev_addr, ep_num, true);
        self.write32(hctsiz(ch), sctsiz);

        // HCCHAR: MPS from the endpoint descriptor, EPNUM, IN direction, DEVCTL,
        // and EPTYPE = Interrupt, which is what the endpoint is.
        //
        // EPTYPE was briefly Bulk here, to move the transfer out of the core's
        // periodic class while it was thought the missing periodic frame list was
        // the problem. It is not: a low-speed device has no bulk endpoints at all.
        // USB 2.0 gives low speed control and interrupt transfers and nothing else,
        // and the hub's translator refuses a bulk transaction aimed at one -- which
        // the trace shows as a STALL on the very first poll of both endpoints, long
        // before the device has had any reason to refuse anything.
        let scchar = (mps as u32 & 0x7FF)
            | ((ep_num as u32) << 11)
            | self.device_flags()
            | (1 << 15) // EPDIR = IN
            | (3 << 18) // EPTYPE = Interrupt: the only type a low-speed endpoint has
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
            // Errors (STALL / babble / transaction error / toggle mismatch) end the
            // poll cycle. `DTERR` used to be missing here, so a mismatched report
            // simply ended the poll with "no data" and the device's toggle and ours
            // stayed out of step.
            if int & ((1 << 2) | (1 << 3) | (1 << 7) | (1 << 10)) != 0 {
                if int & (1 << 10) != 0 {
                    self.flip_pid(dev_addr, ep_num, true);
                }
                self.halt_channel(ch);
                return Err(ViError::IO);
            }

            let complete = int & (1 << 0) != 0 || int & (1 << 1) != 0 || int & (1 << 5) != 0;
            if complete {
                // The report that just arrived advanced this endpoint's toggle.
                self.adopt_pid(ch, dev_addr, ep_num, true);
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

    /// Poll one interrupt-IN packet through a high-speed hub's translator.
    ///
    /// DWC2 runs the start- and complete-splits as separate host-channel
    /// transactions. In DMA mode each transaction is complete only once CHHLTD
    /// arrives. After an accepted start-split the complete-split is armed
    /// immediately; ODDFRM schedules it into the following microframe. A NYET is
    /// retried while the original start-split remains within U-Boot's
    /// board-proven four-microframe budget.
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

        // HCTSIZ: XFERSIZE = one max packet, PKTCNT = 1, and the PID this
        // endpoint's toggle says (see `DataToggles`). A split carries exactly one
        // packet, whatever the caller asked for, so the same value is used for both
        // halves of the pair and adopted only once the data has arrived.
        //
        // HCCHAR: MPS, EPNUM, IN, and EPTYPE = Interrupt, for the same reason as
        // the channel above: a low-speed endpoint has no other type.
        let sctsiz = (want as u32) | (1 << 19) | self.next_pid(dev_addr, ep_num, true);
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

        // Match U-Boot's working channel loop on this controller: complete the
        // start-split channel, then arm the complete-split immediately. The old
        // path forced SSPLIT into microframe 0 and waited until microframe 5 for
        // CSPLIT. That schedule is the reverse of USB interrupt-IN split
        // scheduling and left only the frame boundary for retries.

        arm(false);
        let outcome = self.wait_channel_spin(ch, SPLIT_SPIN_POLLS);
        if !matches!(outcome, Ok(())) {
            *pending = false;
            return match outcome {
                Err(e) => Err(e),
                _ => Ok(0),
            };
        }

        let started = self.frame_number();
        for _ in 0..SPLIT_ATTEMPTS {
            arm(true);
            let outcome = self.wait_channel_spin(ch, SPLIT_SPIN_POLLS);

            match outcome {
                Ok(()) if !self.last_reported_complete() => {
                    // ACK on a complete-split carries no data.
                    *pending = false;
                    return Ok(0);
                }
                Ok(()) => {
                    *pending = false;
                    // The report arrived: this endpoint's toggle has advanced to
                    // what the next poll must ask for.
                    self.adopt_pid(ch, dev_addr, ep_num, true);
                    let remaining = (self.read32(hctsiz(ch)) & 0x7FFFF) as usize;
                    let got = want.saturating_sub(remaining);
                    if got == 0 {
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
                Err(ViError::WouldBlock) => {
                    if !self.last_was_nyet() {
                        *pending = false;
                        return Ok(0);
                    }
                    if !split_window_open(started, self.frame_number()) {
                        *pending = false;
                        return Ok(0);
                    }
                }
                Err(e) => {
                    *pending = false;
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
        // `HAINT` names every channel with an unserviced interrupt, and the dump
        // above covers only the channel this transfer used. The board's timeouts
        // all read `HAINT=0x2` with the dumped channel's own `HCINT` zero, which
        // cannot be told apart from a stale aggregate without reading the channel
        // that owns the bit.
        let haint = self.read32(HAINT);
        for pending in 0..DMA_CHANNELS {
            if haint & (1 << pending) == 0 {
                continue;
            }
            ostd::io::print("[dwc2]   HAINT ch=");
            print_hex_val(pending as u32);
            ostd::io::print(" HCCHAR=0x");
            print_hex_val(self.read32(hcchar(pending)));
            ostd::io::print(" HCINT=0x");
            print_hex_val(self.read32(hcint(pending)));
            ostd::io::println("");
        }
        self.dump_reg("GINTSTS", self.read32(GINTSTS));
        self.dump_reg("GINTMSK", self.read32(GINTMSK));
        self.dump_reg("HPRT0", self.read32(HPRT0));
        self.dump_reg("GRSTCTL", self.read32(GRSTCTL));
        self.dump_reg("GNPTXSTS", self.read32(GNPTXSTS));
        self.dump_reg("MODE", self.mode.get() as u32);
    }

    /// Log a channel error once per distinct cause, with the register state.
    ///
    /// The dump is expensive on a 115200 baud console, so only the first few get
    /// one; the line itself is deduplicated like `report_failure`, because a bus
    /// that fails the same transfer repeatedly would otherwise fill the log.
    fn report_channel_error(&self, ch: usize, int: u32) {
        /// Dumps printed before the line alone carries the count.
        const DUMPS: usize = 4;
        static DUMPED: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        if !claim_failure_report("channel", &ViError::IO, int) {
            return;
        }
        ostd::io::print("[dwc2] channel error: ");
        ostd::io::println(self.describe_hcint(int));
        if DUMPED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < DUMPS {
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
    /// In DMA mode `CHHLTD` is the completion event. Handshake bits can become
    /// visible first, but the channel still owns its registers until the halt
    /// arrives; [`Self::channel_outcome`] keeps waiting rather than letting the
    /// caller re-arm the channel against that delayed completion.
    /// Wait for channel completion or error, bounded by host time rather than by
    /// a poll count.
    ///
    /// This wait yields, and a yield costs the board around twenty milliseconds —
    /// so a *count* budget is not a time budget. `WAIT_POLLS` (50 000) was minutes
    /// of host time, and the board measured a single driver turn at **404 s**: the
    /// driver is out of `Recv` for that whole turn, so the net service's 200 ms
    /// offer and the hypervisor's 2 s L2 deadline both expire long before it comes
    /// back, and the guest's frame is lost with them. A transfer that has not
    /// reported inside [`WAIT_MICROFRAMES`] is stuck; failing it fast is what lets
    /// the loop turn again, and every caller above retries.
    fn wait_channel(&self, ch: usize) -> ViResult<()> {
        let started = self.frame_number();
        let mut yields = 0u32;
        loop {
            if let Some(outcome) = self.channel_outcome(ch) {
                return outcome;
            }
            yields += 1;
            if yields >= WAIT_YIELDS
                || self.frame_number().wrapping_sub(started) & HFNUM_FRNUM_MASK >= WAIT_MICROFRAMES
            {
                break;
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
    /// The channel is left to report for itself. The frame check this used to
    /// carry halted the channel the moment the frame turned over, which is exactly
    /// when a full- or low-speed transaction behind a hub is finishing -- and the
    /// halt it issued was then read straight back as the outcome, so every split
    /// came back as a halt carrying no status:
    ///
    ///     half=ssplit outcome=NAK hcint=0x00000002
    ///     half=csplit outcome=NAK hcint=0x00000002
    ///
    /// The core halts a periodic channel at its own frame boundary and says so, so
    /// nothing here needs to do it, and nothing here should.
    fn wait_channel_spin(&self, ch: usize, polls: usize) -> ViResult<()> {
        for _ in 0..polls {
            if let Some(outcome) = self.channel_outcome(ch) {
                return outcome;
            }
        }
        self.channel_timeout(ch)
    }

    /// Wait for the microframe counter to leave `from`. Returns whether it moved.
    ///
    /// A hub runs the low-speed half of a split in a microframe later than the
    /// start-split's, so this is what puts each complete-split retry somewhere the
    /// hub could actually have an answer for it. Without it every retry lands in
    /// the start-split's own microframe and the core answers NYET every time.
    fn wait_microframe(&self, from: u32, polls: usize) -> bool {
        for _ in 0..polls {
            if self.frame_number() != from {
                return true;
            }
        }
        false
    }

    /// Classify a channel's interrupt register, or `None` while it is still running.
    fn channel_outcome(&self, ch: usize) -> Option<ViResult<()>> {
        let int = self.read32(hcint(ch));
        if int == 0 {
            return None;
        }

        // Buffer-DMA completion is reported by CHHLTD. ACK, NAK, NYET, and even
        // XFERCOMPL may be visible before it; treating that prefix as the final
        // result races the old channel's delayed halt against the next arm. The
        // observed sequence was ACK (0x20), NYET (0x40), then a stale bare
        // CHHLTD (0x02) immediately after re-arm. U-Boot waits for CHHLTD before
        // interpreting the same bits, and Linux masks every DMA channel event
        // except CHHLTD and AHBERR for this reason.
        if self.mode.get() == TransferMode::Dma && int & ((1 << 1) | (1 << 2)) == 0 {
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
                // The core has halted this channel (CHHLTD), so an OUT transfer's
                // half-fetched packet can be cleared out of the Tx FIFO here.
                self.flush_tx_fifo_after_abort(ch);
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
            // A halt carrying nothing else is not a result. CHHLTD says the channel
            // stopped -- at a frame boundary, or because it was disabled -- and
            // answering "done" to it hands the caller an empty buffer for a transfer
            // that never completed. Linux is explicit that the only completion is
            // XFERCOMPL, with ACK belonging to the start-split.
            if int & ((1 << 0) | (1 << 5)) != 0 {
                self.last_hcint.set(int);
                return Some(Ok(()));
            }
            self.last_hcint.set(int);
            return Some(Err(ViError::WouldBlock));
        }

        // ── Transfer complete ─────────────────────────────────────
        if int & (1 << 0) != 0 {
            self.halt_channel(ch);
            self.last_hcint.set(int);
            return Some(Ok(()));
        }

        // ── ACK = the packet was accepted
        //
        // No halt here. On a split, an ACK to the start-split means the hub has
        // taken the transaction and its translator is running it -- the pair is
        // live at that moment and the complete-split is what collects it.
        // Disabling the channel in between asks the core to abandon a transfer
        // that is still in flight, and the trace shows where that lands: the
        // channel is armed for the complete-split with CHDIS already set, and it
        // comes back halted with no status at all, on the very first poll, on both
        // endpoints, before the device has had any chance to refuse anything. A
        // transfer the core has finished needs no halt; a transfer being abandoned
        // gets one from its caller.
        if int & (1 << 5) != 0 {
            // A transfer that is not part of a split is over when the packet is
            // accepted, and halting it is the ordinary end of it. A split is a
            // different thing: the ACK belongs to the start-split, the hub's
            // translator is running the transaction because of it, and the pair is
            // live. Disabling the channel in that moment asks the core to abandon
            // a transfer that is still in flight, and the trace shows where that
            // lands -- the channel armed for the complete-split with CHDIS already
            // set, coming back halted with no status at all, on the very first
            // poll, on both endpoints, before the device has refused anything.
            if self.split.get().is_none() {
                self.halt_channel(ch);
            }
            self.last_hcint.set(int);
            return Some(Ok(()));
        }

        // ── Error conditions ──────────────────────────────────────
        if int & ((1 << 2) | (1 << 3) | (1 << 7) | (1 << 8) | (1 << 10)) != 0 {
            {
                use core::sync::atomic::Ordering;
                let counter = if int & (1 << 7) != 0 {
                    &FAULT_XACTERR
                } else if int & (1 << 3) != 0 {
                    &FAULT_STALL
                } else {
                    &FAULT_OTHER
                };
                counter.fetch_add(1, Ordering::Relaxed);
            }
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
        // Read the direction before the halt clears EPDIR, then clear the Tx FIFO
        // an abandoned OUT transfer may have left a packet in.
        let was_out = char_val & HCCHAR_EPDIR == 0;
        self.halt_channel(ch);
        if was_out {
            self.flush_tx_fifo();
        }
        FAULT_TIMEOUT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
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

    /// Halt an active host channel before it can be reused.
    ///
    /// DWC2 accepts a halt request as `CHDIS|CHENA` while the channel is
    /// active, then clears `CHENA` when stopping has completed. Both U-Boot and
    /// Linux use that sequence and wait for `CHENA` to clear. Writing `CHDIS`
    /// while clearing `CHENA` is not an active-channel halt request.
    ///
    /// A channel which has already stopped needs no request: setting `CHENA` in
    /// that state can start a spurious transaction. Its pending W1C status is
    /// cleared below so it cannot be mistaken for a later transfer's result.
    fn halt_channel(&self, ch: usize) {
        let reg = hcchar(ch);
        let current = self.read32(reg);
        if let Some(request) = active_halt_request(current) {
            self.write32(reg, request);

            // This is deliberately a short non-yielding wait: yielding costs
            // multiple USB frames. The next prepare_channel() retries the halt
            // if a faulty core leaves CHENA asserted beyond this bound.
            for _ in 0..CHANNEL_HALT_POLLS {
                if self.read32(reg) & HCCHAR_CHENA == 0 {
                    break;
                }
            }
        }
        self.write32(hcint(ch), 0xFFFF_FFFF);
    }
}

/// Whether a `(phase, cause)` control-transfer failure still needs reporting.
///
/// Keeps one bit per kind in [`REPORTED_FAILURES`]; the first caller for a kind
/// prints it and every later one is silent (see `UsbHostEngine::report_failure`).
fn claim_failure_report(phase: &str, error: &ViError, hcint: u32) -> bool {
    /// Phases a control transfer is logged with; a channel error uses the next
    /// slot so the two do not share bits.
    const PHASES: u64 = 4;
    /// Causes [`failure_cause`] can return.
    const CAUSES: u64 = 12;
    let phase_index = match phase {
        "setup" => 0,
        "data" => 1,
        "status" => 2,
        _ => 3,
    };
    let index = (phase_index * CAUSES + failure_cause(error, hcint)).min(PHASES * CAUSES - 1);
    let bit = 1u64 << index;
    REPORTED_FAILURES.fetch_or(bit, core::sync::atomic::Ordering::Relaxed) & bit == 0
}

/// Which cause a failure ended in, as a small index.
///
/// The order follows [`UsbHostEngine::describe_hcint`], so two failures that
/// print the same sentence share a report: the `NAK` of an interrupt-IN poll and
/// the `XACTERR` of a register read with a low-speed split in flight are different
/// kinds and both deserve their first line, but the second of each does not.
fn failure_cause(error: &ViError, hcint: u32) -> u64 {
    if matches!(error, ViError::WouldBlock) {
        // NYET is the hub still working; NAK is the device having nothing to send.
        return if hcint & (1 << 6) != 0 { 1 } else { 0 };
    }
    if !matches!(error, ViError::IO) {
        return 2;
    }
    for (index, bit) in [2u32, 8, 3, 7, 10, 9, 6, 4].into_iter().enumerate() {
        if hcint & (1 << bit) != 0 {
            return index as u64 + 3;
        }
    }
    // An `IO` with no status bit at all (an abandoned channel, a poll budget).
    11
}

/// Kinds of transfer failure the console has already named, one bit each.
///
/// See `claim_failure_report`: the bit index is `phase * 12 + cause`, and the last
/// phase is the channel-error path, so the whole set fits in one word.
static REPORTED_FAILURES: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Build a DWC2 halt request only for a channel that is actually active.
///
/// `CHDIS|CHENA` requests the active-channel halt; a stopped channel must not
/// receive `CHENA`, because that would begin a new transaction.
#[inline]
fn active_halt_request(hcchar: u32) -> Option<u32> {
    (hcchar & HCCHAR_CHENA != 0).then_some((hcchar | HCCHAR_CHDIS | HCCHAR_CHENA) & !HCCHAR_EPDIR)
}

/// Whether a split's pairing window is still open `now` raw `HFNUM` ticks after
/// the start-split it belongs to.
///
/// U-Boot abandons a split whose complete-split is still NYET more than four
/// ticks past that point, on the grounds that the hub has dropped the pairing by
/// then. The counter is 16 bits and wraps about every eight seconds, so the
/// difference has to wrap with it -- a stale start must read as closed, never as
/// a fresh window.
#[inline]
fn split_window_open(started: u32, now: u32) -> bool {
    now.wrapping_sub(started) & HFNUM_FRNUM_MASK <= PERIODIC_SPLIT_MICROFRAMES
}

/// `HCSPLT` for a transfer addressed through `split`, or 0 when there is none.
///
/// Every transfer to a device behind a hub needs this, not just the periodic
/// poll: a channel armed without it addresses the low-speed device on the
/// high-speed bus, which nothing answers and the core reports as XACTERR. The
/// value is built here rather than read from the engine so a stage that forgets
/// to program it can be caught by a test instead of by a keyboard whose LEDs
/// never light.
#[inline]
fn hcsplt_for(split: Option<Split>, complete: bool) -> u32 {
    let Some(split) = split else {
        return 0;
    };
    let mut value = HCSPLT_SPLTENA
        | ((split.hub_addr as u32 & HCSPLT_HUBADDR_MASK >> HCSPLT_HUBADDR_SHIFT)
            << HCSPLT_HUBADDR_SHIFT)
        | (split.port as u32 & HCSPLT_PRTADDR_MASK)
        // Every split this driver issues carries one packet, so the whole
        // payload is what the complete-split collects. Linux programs this
        // field for both halves, next to the split address it is easy to
        // mistake for the whole of HCSPLT.
        | HCSPLT_XACTPOS_ALL;
    if complete {
        value |= HCSPLT_COMPSPLT;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::{
        active_halt_request, failure_cause, hcsplt_for, split_window_open, toggle_slot, Split,
        HCCHAR_CHDIS, HCCHAR_CHENA, HCCHAR_EPDIR, HCSPLT_COMPSPLT, HCSPLT_HUBADDR_MASK,
        HCSPLT_HUBADDR_SHIFT, HCSPLT_PRTADDR_MASK, HCSPLT_SPLTENA, HCSPLT_XACTPOS_ALL,
        TOGGLE_DEVICES,
    };
    use types::ViError;

    /// Failing transfers are reported once per kind, so the kinds must not collide:
    /// two causes sharing a slot would silence whichever reaches the console
    /// second, which is exactly the visibility the dedup exists to preserve.
    #[test]
    fn failure_kinds_do_not_collide() {
        /// Causes one phase can name; must cover `claim_failure_report`'s range.
        const CAUSES: u64 = 12;
        let distinct = [
            failure_cause(&ViError::WouldBlock, 1 << 4), // NAK: the device had nothing
            failure_cause(&ViError::WouldBlock, 1 << 6), // NYET: the hub is still working
            failure_cause(&ViError::NotSupported, 0),    // deadline, unsupported request
            failure_cause(&ViError::IO, 1 << 2),         // AHBERR
            failure_cause(&ViError::IO, 1 << 8),         // BBLERR
            failure_cause(&ViError::IO, 1 << 3),         // STALL
            failure_cause(&ViError::IO, 1 << 7),         // XACTERR
            failure_cause(&ViError::IO, 1 << 10),        // DTERR
            failure_cause(&ViError::IO, 1 << 9),         // FRMOVRN
            failure_cause(&ViError::IO, 1 << 6),         // NYET
            failure_cause(&ViError::IO, 1 << 4),         // NAK
            failure_cause(&ViError::IO, 0),              // no status bit: "unknown"
        ];

        let mut seen = 0u32;
        for cause in distinct {
            assert!(cause < CAUSES, "cause {cause} is outside one phase's range");
            let bit = 1u32 << cause;
            assert_eq!(seen & bit, 0, "two causes share report slot {cause}");
            seen |= bit;
        }

        // An `ACK` that never halted prints `unknown` through `describe_hcint`,
        // like a missing status bit, so one report covers both.
        assert_eq!(failure_cause(&ViError::IO, 1 << 5), failure_cause(&ViError::IO, 0));
    }

    /// Endpoint toggles are indexed per device and endpoint, so a mis-indexed slot
    /// would let one device's transfer change another's toggle — random `DTERR`
    /// with no failing transfer to point at. Out-of-range addresses must fall
    /// outside the table rather than wrap into a slot that exists.
    #[test]
    fn toggle_slots_do_not_alias_out_of_range_addresses() {
        assert_eq!(toggle_slot(1, 2), Some((1, 2)));
        assert_eq!(toggle_slot(TOGGLE_DEVICES as u8, 2), None);
        assert_eq!(toggle_slot(255, 2), None);
        // The endpoint field is masked to the tracked range: an endpoint number that
        // cannot exist on the wire cannot reach outside its device's row either.
        assert_eq!(toggle_slot(1, 0xF0), Some((1, 0)));
    }

    #[test]
    fn active_halt_request_requests_halt_without_changing_channel_configuration() {
        let channel_config = 0x0123_4567 | HCCHAR_CHENA | HCCHAR_EPDIR;
        let request = active_halt_request(channel_config).expect("active channel");

        assert_eq!(request & HCCHAR_CHENA, HCCHAR_CHENA);
        assert_eq!(request & HCCHAR_CHDIS, HCCHAR_CHDIS);
        assert_eq!(request & HCCHAR_EPDIR, 0);
        assert_eq!(
            request & !(HCCHAR_CHENA | HCCHAR_CHDIS | HCCHAR_EPDIR),
            channel_config & !(HCCHAR_CHENA | HCCHAR_CHDIS | HCCHAR_EPDIR)
        );
    }

    #[test]
    fn stopped_channel_does_not_receive_a_halt_request() {
        assert_eq!(active_halt_request(0x0123_4567 & !HCCHAR_CHENA), None);
    }

    /// Every pass of a split names the hub and port it is addressed through, and
    /// only the second pass asks for a complete-split.
    #[test]
    fn split_register_addresses_the_hub_on_both_halves() {
        let split = Split {
            hub_addr: 1,
            port: 5,
            low_speed: true,
        };

        let start = hcsplt_for(Some(split), false);
        assert_eq!(start & HCSPLT_SPLTENA, HCSPLT_SPLTENA);
        assert_eq!(start & HCSPLT_COMPSPLT, 0);
        assert_eq!(
            (start & HCSPLT_HUBADDR_MASK) >> HCSPLT_HUBADDR_SHIFT,
            split.hub_addr as u32
        );
        assert_eq!(start & HCSPLT_PRTADDR_MASK, split.port as u32);
        assert_eq!(start & HCSPLT_XACTPOS_ALL, HCSPLT_XACTPOS_ALL);

        let complete = hcsplt_for(Some(split), true);
        assert_eq!(complete & HCSPLT_COMPSPLT, HCSPLT_COMPSPLT);
        assert_eq!(
            complete & !HCSPLT_COMPSPLT,
            start & !HCSPLT_COMPSPLT,
            "the two halves address the same hub and port"
        );
    }

    /// A device on the root port must not be sent a split at all.
    #[test]
    fn no_split_context_disables_the_split_register() {
        assert_eq!(hcsplt_for(None, false), 0);
        assert_eq!(hcsplt_for(None, true), 0);
    }

    /// The pairing window is four raw `HFNUM` ticks wide and follows the counter
    /// across its wrap, so a start from before the wrap is still recognised as the
    /// same split -- and a stale one is closed rather than read as fresh.
    #[test]
    fn split_window_is_four_ticks_and_wraps_with_the_counter() {
        assert!(
            split_window_open(0x1234, 0x1234),
            "the start-split's own tick"
        );
        assert!(split_window_open(0x1234, 0x1238), "four ticks later");
        assert!(!split_window_open(0x1234, 0x1239), "past the window");
        assert!(split_window_open(0xFFFF, 0x0002), "across the wrap");
        assert!(!split_window_open(0xFFFF, 0x0005), "past the wrap window");
        assert!(
            !split_window_open(0x1234, 0x0234),
            "a stale start, not a new one"
        );
    }
}

pub fn print_hex_val(val: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut buf = [0u8; 8];
    for i in 0..8 {
        buf[7 - i] = HEX[((val >> (i * 4)) & 0xF) as usize];
    }
    if let Ok(s) = core::str::from_utf8(&buf) {
        ostd::io::print(s);
    }
}
