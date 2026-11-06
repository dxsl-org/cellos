//! Completion waits: MSI-X/MSI wake-ups (H-S3-5) inside a time budget.
//!
//! An interrupt is only a hint to re-read the completion ring: every park is
//! bounded by [`PARK_TICKS`], every return re-reads the ring, and the whole
//! wait is bounded by the command's budget on the monotonic clock, so a lost,
//! masked or revoked interrupt degrades to slow polling, never to a hang.
//! Before each park the completion causes are acknowledged (`INTR_STATUS` is
//! write-1-to-clear) and the ring re-read, so the next completion raises a
//! fresh message. A refusal by 435 or 242 keeps the polled path and says why
//! (`irq=polled reason=…`).

use crate::regs::{RegisterFile, INTR_CMPL_MASK, REG_INTR_STATUS};
use api::completion::source::IRQ;
use api::pcie_function::{IRQ_KIND_MSIX, IRQ_MODE_ALLOW_MSI, IRQ_MODE_ALLOW_MSIX};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use ostd::io::print_fmt;
use ostd::ipc::{wait_any, Wake};
use ostd::syscall::{sys_alloc_pcie_irq, sys_get_time_ms};
use types::ViResult;

/// Bind-line evidence. It outlives the adapter's move into the disk layer,
/// which owns its executor without exposing it.
static WAKES: AtomicU32 = AtomicU32::new(0);
static EXPIRIES: AtomicU32 = AtomicU32::new(0);
static WAIT_REFUSED: AtomicBool = AtomicBool::new(false);

/// Parks ended by an interrupt / by their deadline so far, and whether a
/// refused wait dropped the adapter to polling.
pub(crate) fn evidence() -> (u32, u32, bool) {
    (
        WAKES.load(Ordering::Relaxed),
        EXPIRIES.load(Ordering::Relaxed),
        WAIT_REFUSED.load(Ordering::Relaxed),
    )
}

/// Longest single park, in scheduler ticks (10 ms each).
const PARK_TICKS: u64 = 10;
/// Per-command budget, including SYNCHRONIZE CACHE (a host `fsync`).
pub(crate) const IO_BUDGET_MS: u64 = 30_000;
/// Ring reads before the first park: a fast completion stays fast.
const SPIN_BEFORE_PARK: u32 = 64;
/// Polled path: yield (and read the clock) every this many ring reads.
const POLL_YIELD_EVERY: u64 = 1024;
/// The bound when no monotonic clock is available.
const FALLBACK_POLLS: u64 = 50_000_000;
/// Park expiries logged per cell lifetime.
const EXPIRY_LOG_LIMIT: u32 = 3;

pub(crate) struct CompletionIrq {
    /// `"msix"` or `"msi"` with the vectors held, or `None` when polled.
    mode: Option<(&'static str, u8)>,
    /// Why the adapter is polled (`irq=polled reason=…`).
    pub reason: &'static str,
}

impl CompletionIrq {
    /// Ask 435 for one vector of the owned function `bdf`, MSI-X preferred.
    pub(crate) fn setup(bdf: u32) -> Self {
        let polled = |reason| Self { mode: None, reason };
        // Without a clock the budget counts polls, and a park is not a poll.
        if sys_get_time_ms().is_none() {
            return polled("no-clock");
        }
        match sys_alloc_pcie_irq(bdf, 1, IRQ_MODE_ALLOW_MSIX | IRQ_MODE_ALLOW_MSI) {
            Ok(alloc) => Self {
                mode: Some((
                    if alloc.kind == IRQ_KIND_MSIX {
                        "msix"
                    } else {
                        "msi"
                    },
                    alloc.count,
                )),
                ..polled("")
            },
            Err(error) => {
                let _ = print_fmt(format_args!("[pvscsi] interrupt refused ({error:?})\n"));
                polled("alloc-refused")
            }
        }
    }

    /// `("msix" | "msi", vectors)`, or `None` when polled.
    pub(crate) fn mode(&self) -> Option<(&'static str, u8)> {
        self.mode
    }

    /// Wait until `ready` holds or [`IO_BUDGET_MS`] passes; `false` on timeout.
    pub(crate) fn wait(
        &mut self,
        regs: &impl RegisterFile,
        mut ready: impl FnMut() -> bool,
    ) -> ViResult<bool> {
        let start = sys_get_time_ms();
        let expired = |polls: u64| match (start, sys_get_time_ms()) {
            (Some(start), Some(now)) => now.saturating_sub(start) >= IO_BUDGET_MS,
            _ => polls >= FALLBACK_POLLS,
        };
        for _ in 0..SPIN_BEFORE_PARK {
            if ready() {
                return Ok(true);
            }
            core::hint::spin_loop();
        }
        let mut polls = 0u64;
        loop {
            if self.mode.is_some() {
                regs.wr32(REG_INTR_STATUS, INTR_CMPL_MASK)?;
                if ready() {
                    return Ok(true);
                }
                if self.park() {
                    if ready() {
                        return Ok(true);
                    }
                    if expired(polls) {
                        return Ok(false);
                    }
                    continue;
                }
            }
            if ready() {
                return Ok(true);
            }
            polls += 1;
            if polls.is_multiple_of(POLL_YIELD_EVERY) {
                ostd::task::yield_now();
                if expired(polls) {
                    return Ok(false);
                }
            }
            core::hint::spin_loop();
        }
    }

    /// Park on this task's lines for at most [`PARK_TICKS`]. `false` means the
    /// wait was refused and the adapter is now polled.
    fn park(&mut self) -> bool {
        match wait_any(IRQ, PARK_TICKS) {
            Ok(Wake::Irq(_)) => {
                WAKES.fetch_add(1, Ordering::Relaxed);
                true
            }
            Ok(_) => {
                if EXPIRIES.fetch_add(1, Ordering::Relaxed) < EXPIRY_LOG_LIMIT {
                    let _ = print_fmt(format_args!(
                        "[pvscsi] irq wait expired after {PARK_TICKS} ticks; re-reading the ring\n"
                    ));
                }
                true
            }
            Err(error) => {
                let _ = print_fmt(format_args!(
                    "[pvscsi] irq=polled reason=wait-refused ({error:?})\n"
                ));
                self.mode = None;
                self.reason = "wait-refused";
                WAIT_REFUSED.store(true, Ordering::Relaxed);
                false
            }
        }
    }
}
