//! MSI-X routing for the bounded bring-up, link and TX-completion waits
//! (H-S3-4 b); the waits themselves are `nic_driver::irq::BoundedWait`.
//!
//! RX is **not** interrupt-driven: the net service asks for frames (`OP_RX`),
//! so the RX cause stays masked (`rx=request-driven`).
//!
//! Vectors (82574 datasheet §10.2.4.9 `IVAR`): RX queue 0 → entry 0, TX queue 0
//! → entry 1, "other" (link status change) → entry 2, folded onto fewer entries
//! when 435 grants fewer. `CTRL_EXT.PBA_SUPPORT` enables the MSI-X PBA path.
//! `EIAC` stays 0: auto-clear would also auto-mask the cause in `IMS` (without
//! `CTRL_EXT.IAME`), so instead every park first clears `ICR` and re-arms `IMS`
//! ([`rearm`]), which makes the next cause a fresh edge, then re-reads the
//! device.

use crate::regs::{
    ivar, RegisterFile, CTRL_EXT, CTRL_EXT_PBA_SUPPORT, EIAC, ICR, ICR_LSC, ICR_OTHER, ICR_TXQ0,
    IMC, IMS, IVAR,
};
use types::ViResult;

/// MSI-X entries asked for: RX queue 0, TX queue 0, other causes.
pub(crate) const WANT_VECTORS: usize = 3;

/// Causes unmasked in MSI-X mode: TX write-back and link changes (via OTHER).
pub(crate) const IMS_MSIX: u32 = ICR_LSC | ICR_TXQ0 | ICR_OTHER;

/// Program the interrupt routing for `vectors` granted MSI-X entries
/// (`None`: polled, every cause masked).
pub(crate) fn program(regs: &impl RegisterFile, vectors: Option<u8>) -> ViResult<()> {
    regs.wr32(IMC, u32::MAX)?;
    let Some(granted) = vectors else {
        return Ok(());
    };
    regs.wr32(CTRL_EXT, regs.rd32(CTRL_EXT)? | CTRL_EXT_PBA_SUPPORT)?;
    regs.wr32(IVAR, ivar(granted))?;
    regs.wr32(EIAC, 0)?;
    regs.wr32(ICR, u32::MAX)?;
    regs.wr32(IMS, IMS_MSIX)
}

/// Clear latched causes and re-arm before a park: a cause that lands after
/// this point raises a fresh MSI-X message.
pub(crate) fn rearm(regs: &impl RegisterFile) -> ViResult<()> {
    regs.wr32(ICR, u32::MAX)?;
    regs.wr32(IMS, IMS_MSIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeRegs;

    #[test]
    fn program_routes_msix_through_ivar_or_masks_everything_when_polled() {
        let regs = FakeRegs::new();
        program(&regs, Some(3)).unwrap();
        assert_eq!(regs.get(IVAR), crate::regs::ivar(3));
        assert_ne!(regs.get(CTRL_EXT) & CTRL_EXT_PBA_SUPPORT, 0);
        assert_eq!(regs.get(EIAC), 0, "no auto-clear/auto-mask");
        assert_eq!(regs.get(IMS), IMS_MSIX);
        // LSC, TXQ0 and OTHER only: RX (RXQ0, bit 20) is request-driven.
        assert_eq!(IMS_MSIX, (1 << 2) | (1 << 22) | (1 << 24));

        let regs = FakeRegs::new();
        program(&regs, None).unwrap();
        assert_eq!(regs.get(IMC), u32::MAX);
        let writes = regs.writes.borrow();
        assert!(!writes.iter().any(|&(o, _)| o == IVAR || o == IMS));
    }

    #[test]
    fn rearm_clears_causes_then_unmasks() {
        let regs = FakeRegs::new();
        rearm(&regs).unwrap();
        let writes = regs.writes.borrow();
        assert_eq!(writes.as_slice(), &[(ICR, u32::MAX), (IMS, IMS_MSIX)]);
    }
}
