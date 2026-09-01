//! Emulated 8253/8254 PIT (channels 0x40–0x42, control 0x43, gate 0x61),
//! backed by **real time** (`sys_get_time`, HPET nanoseconds on x86).
//!
//!   * **Channel 0** is the periodic tick. Once its real-time period elapses,
//!     [`Pit8253::take_irq0_due`] reports one coalesced IRQ0 so the run loop can
//!     advance guest jiffies without starving backend polling.
//!   * **Channel 2** + port 0x61 drive Linux's PIT-based TSC calibration. The
//!     counter decrements against real elapsed time and OUT2 (0x61 bit5)
//!     latches high when the programmed interval genuinely elapses, so
//!     `pit_calibrate_tsc` measures a true 50 ms window and derives a sane TSC
//!     frequency instead of failing ("Unable to calibrate against PIT").

/// 8254 input clock (Hz).
const PIT_HZ: u64 = 1_193_182;
/// `sys_get_time` unit on x86: HPET nanoseconds (kernel GetTime op=0).
const NS_PER_SEC: u64 = 1_000_000_000;

const OUT2: u8 = 1 << 5; // port 0x61 bit5 = channel-2 output level

fn now_ns() -> u64 {
    ostd::syscall::sys_get_time()
}

/// One PIT counter's programmable state.
#[derive(Clone, Copy)]
struct Channel {
    reload: u16,
    armed: bool,
    /// Modes 0/1/4/5 stop at terminal count; modes 2/3 (rate/square) wrap.
    one_shot: bool,
    hi_phase: bool, // lo/hi access: false = next byte is low, true = high
    /// Count snapshot taken at latch / low-byte read so the hi byte is
    /// coherent with the lo byte (8254 latching contract).
    snap: u16,
    /// `now_ns` at the moment the count was (re)loaded.
    start_ns: u64,
}

impl Channel {
    const fn new() -> Self {
        Self {
            reload: 0,
            armed: false,
            one_shot: false,
            hi_phase: false,
            snap: 0,
            start_ns: 0,
        }
    }

    /// Reload value in counts (0 encodes the 8254 maximum, 65536).
    fn period(&self) -> u64 {
        if self.reload == 0 {
            65536
        } else {
            self.reload as u64
        }
    }

    /// Elapsed PIT input-clock ticks since the count was loaded.
    fn elapsed(&self) -> u64 {
        let dt = now_ns().saturating_sub(self.start_ns);
        ((dt as u128 * PIT_HZ as u128) / NS_PER_SEC as u128) as u64
    }

    /// Live counter value derived from real elapsed time.
    fn current(&self) -> u16 {
        if !self.armed {
            return 0;
        }
        let n = self.period();
        let e = self.elapsed();
        if self.one_shot {
            n.saturating_sub(e) as u16
        } else {
            (n - (e % n)) as u16 // periodic: wraps at the reload value
        }
    }

    /// One-shot terminal count reached (drives OUT2 for channel 2).
    fn expired(&self) -> bool {
        self.armed && self.elapsed() >= self.period()
    }

    fn load_byte(&mut self, b: u8) {
        if self.hi_phase {
            self.reload = (self.reload & 0x00FF) | ((b as u16) << 8);
            self.armed = true;
            self.start_ns = now_ns();
        } else {
            self.reload = (self.reload & 0xFF00) | b as u16;
        }
        self.hi_phase = !self.hi_phase;
    }

    fn read_byte(&mut self) -> u8 {
        let byte = if self.hi_phase {
            (self.snap >> 8) as u8
        } else {
            self.snap = self.current(); // fresh snapshot at the lo byte
            (self.snap & 0xFF) as u8
        };
        self.hi_phase = !self.hi_phase;
        byte
    }
}

/// The three-counter PIT plus the 0x61 control-port shadow.
pub struct Pit8253 {
    ch: [Channel; 3],
    port61: u8,
    irq0_periods_delivered: u64,
}

impl Pit8253 {
    pub const fn new() -> Self {
        Self {
            ch: [Channel::new(); 3],
            port61: 0,
            irq0_periods_delivered: 0,
        }
    }

    /// True if `port` addresses the PIT or its 0x61 gate.
    pub fn owns(port: u16) -> bool {
        matches!(port, 0x40..=0x43 | 0x61)
    }

    /// Handle a guest `out`.
    pub fn write(&mut self, port: u16, val: u32) {
        let b = (val & 0xFF) as u8;
        match port {
            0x40 => {
                let completes_reload = self.ch[0].hi_phase;
                self.ch[0].load_byte(b);
                if completes_reload {
                    self.irq0_periods_delivered = 0;
                }
            }
            0x41..=0x42 => self.ch[(port - 0x40) as usize].load_byte(b),
            0x43 => {
                let sel = (b >> 6) & 0x3;
                if sel == 3 {
                    return; // read-back command: unmodelled
                }
                let ch = &mut self.ch[sel as usize];
                if (b >> 4) & 0x3 == 0 {
                    // Counter-latch command: freeze the live count for the
                    // following lo/hi read pair.
                    ch.snap = ch.current();
                    ch.hi_phase = false;
                } else {
                    // Mode/command: record wrap behaviour, reset byte phase.
                    ch.one_shot = matches!((b >> 1) & 0x7, 0 | 1 | 4 | 5);
                    ch.hi_phase = false;
                }
            }
            0x61 => self.port61 = b & 0x03, // keep gate/speaker enables only
            _ => {}
        }
    }

    /// Handle a guest `in`.
    pub fn read(&mut self, port: u16) -> u32 {
        let v: u8 = match port {
            0x40..=0x42 => self.ch[(port - 0x40) as usize].read_byte(),
            0x61 => {
                // Reflect channel 2's real output level in bit 5.
                if self.ch[2].expired() {
                    self.port61 | OUT2
                } else {
                    self.port61
                }
            }
            _ => 0,
        };
        v as u32
    }

    /// Consume one due channel-0 interrupt, coalescing missed periods.
    pub fn take_irq0_due(&mut self) -> bool {
        let channel = &self.ch[0];
        if !channel.armed {
            return false;
        }
        let elapsed_periods = if channel.one_shot {
            u64::from(channel.expired())
        } else {
            channel.elapsed() / channel.period()
        };
        if elapsed_periods <= self.irq0_periods_delivered {
            return false;
        }
        self.irq0_periods_delivered = elapsed_periods;
        true
    }
}
