//! MSI-X wake-ups for bounded bring-up, link and TX-completion waits (H-S3-4 b).
//!
//! An interrupt is only a hint to re-read the device: every park is bounded by
//! [`PARK_TICKS`], every return re-reads the condition, and every wait has its
//! own park budget, so a lost or revoked interrupt degrades to slow polling,
//! never to a hang. RX is **not** interrupt-driven: the net service asks for
//! frames (`OP_RX`), so the RX cause stays masked (`rx=request-driven`).
//!
//! Vectors (82574 datasheet §10.2.4.9 `IVAR`): RX queue 0 → entry 0, TX queue 0
//! → entry 1, "other" (link status change) → entry 2, folded onto fewer entries
//! when 435 grants fewer. `CTRL_EXT.PBA_SUPPORT` enables the MSI-X PBA path.
//! `EIAC` stays 0: auto-clear would also auto-mask the cause in `IMS` (without
//! `CTRL_EXT.IAME`), so instead every park first clears `ICR` and re-arms `IMS`,
//! which makes the next cause a fresh edge, then re-reads the device.

use crate::regs::{
    ivar, RegisterFile, CTRL_EXT, CTRL_EXT_PBA_SUPPORT, EIAC, ICR, ICR_LSC, ICR_OTHER, ICR_TXQ0,
    IMC, IMS, IVAR,
};
use abi::completion::source::IRQ;
use abi::pcie_function::IRQ_MODE_ALLOW_MSIX;
use ostd::io::print_fmt;
use ostd::ipc::{wait_any, Wake};
use ostd::syscall::sys_alloc_pcie_irq;
use types::ViResult;

/// Longest single park, in scheduler ticks (10 ms each).
pub(crate) const PARK_TICKS: u64 = 10;
/// MSI-X entries asked for: RX queue 0, TX queue 0, other causes.
pub(crate) const WANT_VECTORS: usize = 3;
/// Condition polls before the first park (QEMU completes TX synchronously).
const SPIN_BEFORE_PARK: u32 = 4_096;
/// Park expiries logged per cell lifetime.
const EXPIRY_LOG_LIMIT: u32 = 3;

/// Causes unmasked in MSI-X mode: TX write-back and link changes (via OTHER).
pub(crate) const IMS_MSIX: u32 = ICR_LSC | ICR_TXQ0 | ICR_OTHER;

/// How the controller's waits learn that something changed.
pub(crate) struct NicIrq {
    /// MSI-X entries held, or `None` when polled.
    vectors: Option<u8>,
    /// Why the cell is polled (`irq=polled reason=…`).
    pub reason: &'static str,
    /// Parks ended by an interrupt / by their deadline (bind-line evidence).
    pub wakes: u32,
    pub expiries: u32,
}

impl NicIrq {
    /// Ask 435 for [`WANT_VECTORS`] MSI-X entries of the owned function `bdf`.
    /// MSI is not accepted: the 82574 routes queue causes only through `IVAR`.
    pub(crate) fn setup(bdf: u32) -> Self {
        match sys_alloc_pcie_irq(bdf, WANT_VECTORS, IRQ_MODE_ALLOW_MSIX) {
            Ok(alloc) => Self {
                vectors: Some(alloc.count),
                reason: "",
                wakes: 0,
                expiries: 0,
            },
            Err(error) => {
                let _ = print_fmt(format_args!(
                    "[e1000e] MSI-X refused ({:?}); polling\n",
                    error
                ));
                Self {
                    vectors: None,
                    reason: "alloc-refused",
                    wakes: 0,
                    expiries: 0,
                }
            }
        }
    }

    /// MSI-X entries held (0 when polled).
    pub(crate) fn vectors(&self) -> u8 {
        self.vectors.unwrap_or(0)
    }

    /// Program the interrupt routing for the current mode.
    pub(crate) fn program(&self, regs: &impl RegisterFile) -> ViResult<()> {
        regs.wr32(IMC, u32::MAX)?;
        let Some(granted) = self.vectors else {
            return Ok(());
        };
        regs.wr32(CTRL_EXT, regs.rd32(CTRL_EXT)? | CTRL_EXT_PBA_SUPPORT)?;
        regs.wr32(IVAR, ivar(granted))?;
        regs.wr32(EIAC, 0)?;
        regs.wr32(ICR, u32::MAX)?;
        regs.wr32(IMS, IMS_MSIX)
    }

    /// Wait (bounded) until `ready` holds: a short spin, then up to `parks`
    /// interrupt parks with MSI-X, or `polls` spins when polled. The device is
    /// re-read after every return. `Ok(false)` means the budget ran out.
    pub(crate) fn wait_for(
        &mut self,
        regs: &impl RegisterFile,
        parks: u32,
        polls: u32,
        mut ready: impl FnMut() -> ViResult<bool>,
    ) -> ViResult<bool> {
        for _ in 0..SPIN_BEFORE_PARK {
            if ready()? {
                return Ok(true);
            }
            core::hint::spin_loop();
        }
        if self.vectors.is_some() {
            for _ in 0..parks {
                // Clear latched causes and re-arm, then re-check: a cause that
                // lands after this point raises a fresh MSI-X message.
                regs.wr32(ICR, u32::MAX)?;
                regs.wr32(IMS, IMS_MSIX)?;
                if ready()? {
                    return Ok(true);
                }
                if !self.park() {
                    break;
                }
                if ready()? {
                    return Ok(true);
                }
            }
            if self.vectors.is_some() {
                return Ok(false);
            }
        }
        for _ in 0..polls {
            if ready()? {
                return Ok(true);
            }
            core::hint::spin_loop();
        }
        Ok(false)
    }

    /// Park on this task's MSI-X lines for at most [`PARK_TICKS`]. `false`
    /// means the wait was refused and the cell is now polled.
    fn park(&mut self) -> bool {
        match wait_any(IRQ, PARK_TICKS) {
            Ok(Wake::Irq(_)) => {
                self.wakes += 1;
                true
            }
            Ok(_) => {
                self.expiries += 1;
                if self.expiries <= EXPIRY_LOG_LIMIT {
                    let _ = print_fmt(format_args!(
                        "[e1000e] irq wait expired after {} ticks; re-reading the device\n",
                        PARK_TICKS
                    ));
                }
                true
            }
            Err(error) => {
                let _ = print_fmt(format_args!(
                    "[e1000e] irq=polled reason=wait-refused ({:?})\n",
                    error
                ));
                self.vectors = None;
                self.reason = "wait-refused";
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeRegs;

    #[test]
    fn program_routes_msix_through_ivar_or_masks_everything_when_polled() {
        let regs = FakeRegs::new();
        let mut irq = NicIrq {
            vectors: Some(3),
            reason: "",
            wakes: 0,
            expiries: 0,
        };
        irq.program(&regs).unwrap();
        assert_eq!(regs.get(IVAR), crate::regs::ivar(3));
        assert_ne!(regs.get(CTRL_EXT) & CTRL_EXT_PBA_SUPPORT, 0);
        assert_eq!(regs.get(EIAC), 0, "no auto-clear/auto-mask");
        assert_eq!(regs.get(IMS), IMS_MSIX);
        // LSC, TXQ0 and OTHER only: RX (RXQ0, bit 20) is request-driven.
        assert_eq!(IMS_MSIX, (1 << 2) | (1 << 22) | (1 << 24));

        let regs = FakeRegs::new();
        irq.vectors = None;
        irq.program(&regs).unwrap();
        assert_eq!(regs.get(IMC), u32::MAX);
        let writes = regs.writes.borrow();
        assert!(!writes.iter().any(|&(o, _)| o == IVAR || o == IMS));
    }
}
