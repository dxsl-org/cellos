//! MSI completion wake-ups (x86 phase 04b, CP1 `cp1-04b-waitcompletion-irq.md` §7).
//!
//! Contract: an interrupt is only ever a hint to re-read `PxCI`. Every park is
//! bounded by [`PARK_TICKS`], every return re-reads the port, and every park
//! spends part of the command's poll budget (see `controller::completion_tick`),
//! so a lost, masked or revoked interrupt degrades to slow polling inside the
//! existing budget and never to a hang. A refusal by `AllocPcieIrq` (435) or by
//! `WaitCompletion(IRQ)` (242) keeps the polled path and says why.

use api::completion::source::IRQ;
use api::pcie_function::{IRQ_KIND_MSIX, IRQ_MODE_ALLOW_MSI, IRQ_MODE_ALLOW_MSIX};
use ostd::io::print_fmt;
use ostd::ipc::{wait_any, Wake};
use ostd::syscall::sys_alloc_pcie_irq;

/// Longest single park, in scheduler ticks (10 ms each).
const PARK_TICKS: u64 = 10;

/// AHCI raises every port's events on one message; more vectors buy nothing.
const WANT_VECTORS: usize = 1;

/// How many park expiries (an interrupt that should have arrived and did not) to log.
const EXPIRY_LOG_LIMIT: u32 = 3;

/// How this driver learns that the HBA finished a command.
pub(crate) struct CompletionIrq {
    /// Whether a line is held and the HBA may be armed (`GHC.IE`/`PxIE`).
    active: bool,
    expiries_logged: u32,
}

impl CompletionIrq {
    /// Ask for one MSI (MSI-X accepted if the function has it) for the owned
    /// function `bdf`, and log the outcome as `irq=msi|msix` or `irq=polled reason=…`.
    pub(crate) fn setup(bdf: u32) -> Self {
        match sys_alloc_pcie_irq(bdf, WANT_VECTORS, IRQ_MODE_ALLOW_MSIX | IRQ_MODE_ALLOW_MSI) {
            Ok(alloc) => {
                let _ = print_fmt(format_args!(
                    "[ahci] irq={} vectors={:#x}+{}\n",
                    if alloc.kind == IRQ_KIND_MSIX {
                        "msix"
                    } else {
                        "msi"
                    },
                    alloc.first_irq,
                    alloc.count
                ));
                Self {
                    active: true,
                    expiries_logged: 0,
                }
            }
            Err(error) => {
                let _ = print_fmt(format_args!(
                    "[ahci] irq=polled reason=alloc-refused ({:?})\n",
                    error
                ));
                Self {
                    active: false,
                    expiries_logged: 0,
                }
            }
        }
    }

    /// Whether completions are interrupt-driven (the HBA must be armed).
    pub(crate) fn active(&self) -> bool {
        self.active
    }

    /// Park until this task's line fires or [`PARK_TICKS`] pass.
    ///
    /// `false` means the driver is (now) polled: the caller must disarm the HBA
    /// and use its poll/yield cadence. `true` means the caller should acknowledge
    /// and re-read the port.
    pub(crate) fn park(&mut self) -> bool {
        if !self.active {
            return false;
        }
        match wait_any(IRQ, PARK_TICKS) {
            Ok(Wake::Irq(_)) => true,
            Ok(Wake::Interrupted) => {
                if self.expiries_logged < EXPIRY_LOG_LIMIT {
                    self.expiries_logged += 1;
                    let _ = print_fmt(format_args!(
                        "[ahci] irq wait expired after {} ticks; re-reading the port\n",
                        PARK_TICKS
                    ));
                }
                true
            }
            other => {
                let _ = print_fmt(format_args!(
                    "[ahci] irq=polled reason=wait-refused ({:?})\n",
                    other
                ));
                self.active = false;
                false
            }
        }
    }
}
