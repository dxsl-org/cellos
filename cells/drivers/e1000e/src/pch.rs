//! Intel I219 (PCH LAN) support: SW/FW semaphore, ULP exit, K1 workarounds,
//! and flash NVM access (Intel I219 Datasheet & Spec Update).

use crate::identity::PchGen;
use crate::regs::RegisterFile;
use types::{ViError, ViResult};

// ── PCH LAN Registers & Bits ──────────────────────────────────────────────────

pub(crate) const EXTCNF_CTRL: usize = 0x0_0F00;
pub(crate) const EXTCNF_CTRL_SWFLAG: u32 = 1 << 5;

pub(crate) const FWSM: usize = 0x0_5B54;
pub(crate) const FWSM_FW_VALID: u32 = 1 << 15;
pub(crate) const FWSM_ULP_CFG_DONE: u32 = 1 << 10;

pub(crate) const H2ME: usize = 0x0_5B50;
pub(crate) const H2ME_ULP: u32 = 1 << 11;
pub(crate) const H2ME_ENFORCE_SETTINGS: u32 = 1 << 12;

pub(crate) const FEXTNVM6: usize = 0x0_0010;
pub(crate) const FEXTNVM6_REQ_PLL_CLK: u32 = 1 << 8;
pub(crate) const FEXTNVM6_ENABLE_K1_ENTRY_CONDITION: u32 = 1 << 9;
pub(crate) const FEXTNVM6_K1_OFF_ENABLE: u32 = 1 << 31;

// Flash Registers (BAR0 + 0xE000 in SPT/I219)
pub(crate) const FLASH_HSFSTS: usize = 0x0_E004;
pub(crate) const FLASH_FADDR: usize = 0x0_E008;
pub(crate) const FLASH_FDATA0: usize = 0x0_E010;

pub(crate) const HSFSTS_FLCDONE: u32 = 1 << 0;
pub(crate) const HSFSTS_FLCERR: u32 = 1 << 1;
pub(crate) const HSFCTL_CYCLE_READ: u32 = 0 << 17;
pub(crate) const HSFCTL_START: u32 = 1 << 16;
pub(crate) const HSFCTL_BYTE_COUNT_2: u32 = 1 << 19;

const SWFLAG_POLLS: u32 = 100_000;
const ULP_POLLS: u32 = 100_000;
const FLASH_POLLS: u32 = 100_000;

// ── Semaphore: SW / FW Flag ──────────────────────────────────────────────────

/// Acquire the software semaphore flag (`EXTCNF_CTRL.SWFLAG`).
pub(crate) fn acquire_swflag(regs: &impl RegisterFile) -> ViResult<()> {
    // 1. Wait until current SWFLAG is clear.
    for _ in 0..SWFLAG_POLLS {
        let extcnf = regs.rd32(EXTCNF_CTRL)?;
        if extcnf & EXTCNF_CTRL_SWFLAG == 0 {
            break;
        }
    }

    // 2. Request semaphore.
    let extcnf = regs.rd32(EXTCNF_CTRL)? | EXTCNF_CTRL_SWFLAG;
    regs.wr32(EXTCNF_CTRL, extcnf)?;

    // 3. Confirm acquisition.
    for _ in 0..SWFLAG_POLLS {
        let extcnf = regs.rd32(EXTCNF_CTRL)?;
        if extcnf & EXTCNF_CTRL_SWFLAG != 0 {
            return Ok(());
        }
    }

    // Timeout: ensure flag is cleared.
    let extcnf = regs.rd32(EXTCNF_CTRL)? & !EXTCNF_CTRL_SWFLAG;
    let _ = regs.wr32(EXTCNF_CTRL, extcnf);
    Err(ViError::WouldBlock)
}

/// Release the software semaphore flag (`EXTCNF_CTRL.SWFLAG`).
pub(crate) fn release_swflag(regs: &impl RegisterFile) -> ViResult<()> {
    let extcnf = regs.rd32(EXTCNF_CTRL)? & !EXTCNF_CTRL_SWFLAG;
    regs.wr32(EXTCNF_CTRL, extcnf)
}

/// Execute `op` under the software semaphore, guaranteeing release on every exit path.
pub(crate) fn with_swflag<R>(
    regs: &impl RegisterFile,
    op: impl FnOnce() -> ViResult<R>,
) -> ViResult<R> {
    acquire_swflag(regs)?;
    let result = op();
    let release_res = release_swflag(regs);
    match (result, release_res) {
        (Ok(val), Ok(())) => Ok(val),
        (Err(e), _) => Err(e),
        (Ok(_), Err(e)) => Err(e),
    }
}

// ── ULP Exit State Machine ───────────────────────────────────────────────────

/// Take the PHY out of Ultra Low Power (ULP) mode if active.
/// Required for Tiger Lake and Alder Lake PCH generations before PHY registers
/// can be accessed over MDIC.
pub(crate) fn disable_ulp(regs: &impl RegisterFile, gen: PchGen) -> ViResult<()> {
    // Only TigerLake and later generations can power up in ULP by default.
    if !matches!(gen, PchGen::TigerLake | PchGen::AlderLake) {
        return Ok(());
    }

    let fwsm = regs.rd32(FWSM)?;
    if fwsm & FWSM_FW_VALID != 0 {
        // ME firmware is running: signal ME to unconfigure ULP mode in PHY.
        let mut h2me = regs.rd32(H2ME)?;
        h2me &= !H2ME_ULP;
        h2me |= H2ME_ENFORCE_SETTINGS;
        regs.wr32(H2ME, h2me)?;

        // Wait for ME to complete ULP configuration.
        let mut cleared = false;
        for _ in 0..ULP_POLLS {
            if regs.rd32(FWSM)? & FWSM_ULP_CFG_DONE == 0 {
                cleared = true;
                break;
            }
        }
        if !cleared {
            return Err(ViError::IO);
        }

        // Clear enforce settings.
        let h2me = regs.rd32(H2ME)? & !H2ME_ENFORCE_SETTINGS;
        regs.wr32(H2ME, h2me)?;
    }
    Ok(())
}

// ── K1 & PLL Workarounds ─────────────────────────────────────────────────────

/// Apply mandatory K1/PLL workarounds for I219 (Spec Update §4.4.1).
pub(crate) fn configure_k1_workarounds(regs: &impl RegisterFile) -> ViResult<()> {
    let mut val = regs.rd32(FEXTNVM6)?;
    val |= FEXTNVM6_REQ_PLL_CLK | FEXTNVM6_ENABLE_K1_ENTRY_CONDITION;
    val &= !FEXTNVM6_K1_OFF_ENABLE;
    regs.wr32(FEXTNVM6, val)
}

// ── Flash NVM Read ───────────────────────────────────────────────────────────

/// Read a 16-bit word from SPI flash at word offset `word_offset`.
pub(crate) fn read_flash_word(regs: &impl RegisterFile, word_offset: u16) -> ViResult<u16> {
    let byte_addr = (word_offset as u32) << 1;
    regs.wr32(FLASH_FADDR, byte_addr)?;

    // Set read command (2 bytes, cycle=read, start) in HSFSTS/HSFCTL.
    let cmd = HSFCTL_START | HSFCTL_CYCLE_READ | HSFCTL_BYTE_COUNT_2;
    regs.wr32(FLASH_HSFSTS, cmd)?;

    // Poll for FLCDONE.
    for _ in 0..FLASH_POLLS {
        let sts = regs.rd32(FLASH_HSFSTS)?;
        if sts & HSFSTS_FLCERR != 0 {
            return Err(ViError::IO);
        }
        if sts & HSFSTS_FLCDONE != 0 {
            // Read data and clear FLCDONE.
            let data = regs.rd32(FLASH_FDATA0)? as u16;
            regs.wr32(FLASH_HSFSTS, HSFSTS_FLCDONE)?;
            return Ok(data);
        }
    }
    Err(ViError::IO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeRegs;

    #[test]
    fn semaphore_acquire_and_release_flow() {
        let fake = FakeRegs::new();
        assert_eq!(fake.get(EXTCNF_CTRL) & EXTCNF_CTRL_SWFLAG, 0);

        with_swflag(&fake, || {
            assert_ne!(fake.get(EXTCNF_CTRL) & EXTCNF_CTRL_SWFLAG, 0);
            Ok(())
        })
        .expect("with_swflag succeeds");

        assert_eq!(fake.get(EXTCNF_CTRL) & EXTCNF_CTRL_SWFLAG, 0);
    }

    #[test]
    fn semaphore_released_on_error() {
        let fake = FakeRegs::new();
        let res: ViResult<()> = with_swflag(&fake, || Err(ViError::IO));
        assert_eq!(res, Err(ViError::IO));
        assert_eq!(fake.get(EXTCNF_CTRL) & EXTCNF_CTRL_SWFLAG, 0);
    }

    #[test]
    fn ulp_exit_skips_older_generations() {
        let fake = FakeRegs::new();
        assert_eq!(disable_ulp(&fake, PchGen::SunrisePoint), Ok(()));
        assert_eq!(disable_ulp(&fake, PchGen::CannonPoint), Ok(()));
        assert_eq!(disable_ulp(&fake, PchGen::CometLake), Ok(()));
    }

    #[test]
    fn k1_workarounds_sets_pll_and_entry_bits() {
        let fake = FakeRegs::new();
        configure_k1_workarounds(&fake).expect("k1 workarounds succeed");
        let val = fake.get(FEXTNVM6);
        assert_ne!(val & FEXTNVM6_REQ_PLL_CLK, 0);
        assert_ne!(val & FEXTNVM6_ENABLE_K1_ENTRY_CONDITION, 0);
        assert_eq!(val & FEXTNVM6_K1_OFF_ENABLE, 0);
    }
    #[test]
    fn ulp_exit_handles_tiger_lake_me_flow() {
        let fake = FakeRegs::new();
        fake.regs
            .borrow_mut()
            .insert(FWSM, FWSM_FW_VALID | FWSM_ULP_CFG_DONE);
        assert_eq!(disable_ulp(&fake, PchGen::TigerLake), Ok(()));
        assert_eq!(fake.get(FWSM) & FWSM_ULP_CFG_DONE, 0);
        assert_eq!(fake.get(H2ME) & H2ME_ENFORCE_SETTINGS, 0);
    }

    #[test]
    fn flash_word_read_reads_from_nvm() {
        let mut fake = FakeRegs::new();
        fake.nvm[5] = 0x1234;
        let data = read_flash_word(&fake, 5).expect("read_flash_word succeeds");
        assert_eq!(data, 0x1234);
    }
}
