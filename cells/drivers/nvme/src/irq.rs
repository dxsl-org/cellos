//! MSI-X completion wake-ups (x86 phase 04b, CP1 `cp1-04b-waitcompletion-irq.md` §7).
//!
//! Contract: an interrupt is only ever a hint to re-read the completion queue. Every
//! park is bounded by [`PARK_TICKS`] and every return re-reads the queue, so a lost,
//! masked or revoked interrupt degrades to slow polling inside the command's own
//! budget and never to a hang. A refusal by `AllocPcieIrq` (435) or by
//! `WaitCompletion(IRQ)` (242) keeps the polled path and says why.

use abi::completion::source::IRQ;
use abi::pcie_function::{IRQ_KIND_MSIX, IRQ_MODE_ALLOW_MSI, IRQ_MODE_ALLOW_MSIX};
use ostd::io::print_fmt;
use ostd::ipc::{wait_any, Wake};
use ostd::syscall::sys_alloc_pcie_irq;

/// Longest single park, in scheduler ticks (10 ms each).
const PARK_TICKS: u64 = 10;

/// How many park expiries (an interrupt that should have arrived and did not) to log.
const EXPIRY_LOG_LIMIT: u32 = 3;

/// How this driver learns that the controller posted a completion.
pub(crate) struct CompletionIrq {
    /// The MSI-X vectors held, or `None` when polling.
    vectors: Option<u8>,
    expiries_logged: u32,
}

impl CompletionIrq {
    /// Ask for `want` MSI-X vectors (MSI accepted as a fallback) for the owned
    /// function `bdf`, and log the outcome as `irq=msix|msi` or `irq=polled reason=…`.
    pub(crate) fn setup(bdf: u32, want: usize) -> Self {
        let polled = Self {
            vectors: None,
            expiries_logged: 0,
        };
        // Without a monotonic clock the budgets count polls, and a park is not a
        // poll, so a lost interrupt could outlast every budget.
        if ostd::syscall::sys_get_time_ms().is_none() {
            let _ = print_fmt(format_args!("[nvme] irq=polled reason=no-clock\n"));
            return polled;
        }
        match sys_alloc_pcie_irq(bdf, want, IRQ_MODE_ALLOW_MSIX | IRQ_MODE_ALLOW_MSI) {
            Ok(alloc) => {
                let _ = print_fmt(format_args!(
                    "[nvme] irq={} vectors={:#x}+{}\n",
                    if alloc.kind == IRQ_KIND_MSIX {
                        "msix"
                    } else {
                        "msi"
                    },
                    alloc.first_irq,
                    alloc.count
                ));
                Self {
                    vectors: Some(alloc.count),
                    ..polled
                }
            }
            Err(error) => {
                let _ = print_fmt(format_args!(
                    "[nvme] irq=polled reason=alloc-refused ({:?})\n",
                    error
                ));
                polled
            }
        }
    }

    /// Vectors held (0 when polling). Vector `i` is MSI-X table entry `i`.
    pub(crate) fn vectors(&self) -> u8 {
        self.vectors.unwrap_or(0)
    }

    /// Park until one of this task's lines fires or [`PARK_TICKS`] pass.
    ///
    /// `false` means the driver is (now) polled and the caller must use its
    /// poll/yield cadence; `true` means the caller should re-read the queue.
    pub(crate) fn park(&mut self) -> bool {
        if self.vectors.is_none() {
            return false;
        }
        match wait_any(IRQ, PARK_TICKS) {
            Ok(Wake::Irq(_)) => true,
            Ok(Wake::Interrupted) => {
                if self.expiries_logged < EXPIRY_LOG_LIMIT {
                    self.expiries_logged += 1;
                    let _ = print_fmt(format_args!(
                        "[nvme] irq wait expired after {} ticks; re-reading the queue\n",
                        PARK_TICKS
                    ));
                }
                true
            }
            other => {
                let _ = print_fmt(format_args!(
                    "[nvme] irq=polled reason=wait-refused ({:?})\n",
                    other
                ));
                self.vectors = None;
                false
            }
        }
    }
}
