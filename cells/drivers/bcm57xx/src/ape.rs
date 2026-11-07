//! Broadcom APE (Application Processing Engine) lock protocol and driver state.
//!
//! Broadcom NetXtreme BCM5719/5720 Programmer's Reference Guide §6.4; FreeBSD `if_bgereg.h`.

#![allow(dead_code)]

use crate::regs::RegisterFile;
use types::{ViError, ViResult};

pub(crate) const APE_EVENT: usize = 0x000C;
pub(crate) const APE_PER_LOCK_REQ: usize = 0x8400;
pub(crate) const APE_PER_LOCK_GRANT: usize = 0x8420;

pub(crate) const APE_LOCK_PHY0: usize = 0;
pub(crate) const APE_LOCK_GRC: usize = 1;
pub(crate) const APE_LOCK_PHY1: usize = 2;
pub(crate) const APE_LOCK_PHY2: usize = 3;
pub(crate) const APE_LOCK_MEM: usize = 4;
pub(crate) const APE_LOCK_PHY3: usize = 5;
pub(crate) const APE_LOCK_GPIO: usize = 7;

pub(crate) const APE_LOCK_REQ_DRIVER0: u32 = 0x0000_1000;
pub(crate) const APE_LOCK_GRANT_DRIVER0: u32 = 0x0000_1000;

pub(crate) const APE_HOST_SEG_SIG: usize = 0x4200;
pub(crate) const APE_HOST_SEG_LEN: usize = 0x4204;
pub(crate) const APE_HOST_DRVR_STATE: usize = 0x421C;

pub(crate) const APE_HOST_SEG_SIG_MAGIC: u32 = 0x484F_5354; // "HOST"
pub(crate) const APE_HOST_SEG_LEN_MAGIC: u32 = 0x0000_0020;

pub(crate) const APE_HOST_DRVR_STATE_START: u32 = 0x0000_0001;
pub(crate) const APE_HOST_DRVR_STATE_UNLOAD: u32 = 0x0000_0002;

pub(crate) const APE_EVENT_STATUS: usize = 0x4300;
pub(crate) const APE_EVENT_STATUS_EVENT_PENDING: u32 = 0x8000_0000;
pub(crate) const APE_EVENT_STATUS_DRIVER_EVNT: u32 = 0x0000_0010;
pub(crate) const APE_EVENT_STATUS_STATE_CHNGE: u32 = 0x0000_0500;
pub(crate) const APE_EVENT_STATUS_STATE_START: u32 = 0x0001_0000;
pub(crate) const APE_EVENT_STATUS_STATE_UNLOAD: u32 = 0x0002_0000;
pub(crate) const APE_EVENT_1: u32 = 0x0000_0001;

const APE_LOCK_POLLS: u32 = 50_000;

fn lock_bit(func: u8, locknum: usize) -> u32 {
    match locknum {
        APE_LOCK_PHY0 | APE_LOCK_PHY1 | APE_LOCK_PHY2 | APE_LOCK_PHY3 => APE_LOCK_REQ_DRIVER0,
        _ => {
            if func == 0 {
                APE_LOCK_REQ_DRIVER0
            } else {
                1 << func
            }
        }
    }
}

pub(crate) fn phy_lock_for_func(func: u8) -> usize {
    match func {
        0 => APE_LOCK_PHY0,
        1 => APE_LOCK_PHY1,
        2 => APE_LOCK_PHY2,
        _ => APE_LOCK_PHY3,
    }
}

pub(crate) fn acquire_ape_lock(regs: &impl RegisterFile, func: u8, locknum: usize) -> ViResult<()> {
    let bit = lock_bit(func, locknum);
    let off = 4 * locknum;
    regs.wr32(APE_PER_LOCK_REQ + off, bit)?;

    for _ in 0..APE_LOCK_POLLS {
        let status = regs.rd32(APE_PER_LOCK_GRANT + off)?;
        if status == bit {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    // Revoke request on timeout
    let _ = regs.wr32(APE_PER_LOCK_GRANT + off, bit);
    Err(ViError::WouldBlock)
}

pub(crate) fn release_ape_lock(regs: &impl RegisterFile, func: u8, locknum: usize) -> ViResult<()> {
    let bit = lock_bit(func, locknum);
    let off = 4 * locknum;
    regs.wr32(APE_PER_LOCK_GRANT + off, bit)
}

pub(crate) fn with_ape_lock<R>(
    regs: &impl RegisterFile,
    func: u8,
    locknum: usize,
    op: impl FnOnce() -> ViResult<R>,
) -> ViResult<R> {
    acquire_ape_lock(regs, func, locknum)?;
    let result = op();
    let release_res = release_ape_lock(regs, func, locknum);
    match (result, release_res) {
        (Ok(val), Ok(())) => Ok(val),
        (Err(e), _) => Err(e),
        (Ok(_), Err(e)) => Err(e),
    }
}

pub(crate) fn notify_driver_state(regs: &impl RegisterFile, func: u8, state: u32) -> ViResult<()> {
    let event = match state {
        APE_HOST_DRVR_STATE_START => {
            regs.wr32(APE_HOST_SEG_SIG, APE_HOST_SEG_SIG_MAGIC)?;
            regs.wr32(APE_HOST_SEG_LEN, APE_HOST_SEG_LEN_MAGIC)?;
            regs.wr32(APE_HOST_DRVR_STATE, APE_HOST_DRVR_STATE_START)?;
            APE_EVENT_STATUS_STATE_START
        }
        APE_HOST_DRVR_STATE_UNLOAD => {
            regs.wr32(APE_HOST_DRVR_STATE, APE_HOST_DRVR_STATE_UNLOAD)?;
            APE_EVENT_STATUS_STATE_UNLOAD
        }
        _ => return Ok(()),
    };

    let _ = with_ape_lock(regs, func, APE_LOCK_MEM, || {
        let full_event = event
            | APE_EVENT_STATUS_DRIVER_EVNT
            | APE_EVENT_STATUS_STATE_CHNGE
            | APE_EVENT_STATUS_EVENT_PENDING;
        regs.wr32(APE_EVENT_STATUS, full_event)?;
        regs.wr32(APE_EVENT, APE_EVENT_1)
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeRegs;

    #[test]
    fn ape_lock_acquire_and_release_flow() {
        let fake = FakeRegs::new();
        with_ape_lock(&fake, 0, APE_LOCK_GRC, || Ok(())).expect("APE lock succeeds");
        let off = 4 * APE_LOCK_GRC;
        assert_eq!(fake.get(APE_PER_LOCK_GRANT + off), 0);
    }

    #[test]
    fn ape_lock_released_on_error() {
        let fake = FakeRegs::new();
        let res: ViResult<()> = with_ape_lock(&fake, 0, APE_LOCK_GRC, || Err(ViError::IO));
        assert_eq!(res, Err(ViError::IO));
        let off = 4 * APE_LOCK_GRC;
        assert_eq!(fake.get(APE_PER_LOCK_GRANT + off), 0);
    }

    #[test]
    fn driver_state_change_writes_magic_and_state() {
        let fake = FakeRegs::new();
        notify_driver_state(&fake, 0, APE_HOST_DRVR_STATE_START).expect("notify start succeeds");
        assert_eq!(fake.get(APE_HOST_SEG_SIG), APE_HOST_SEG_SIG_MAGIC);
        assert_eq!(fake.get(APE_HOST_DRVR_STATE), APE_HOST_DRVR_STATE_START);

        notify_driver_state(&fake, 0, APE_HOST_DRVR_STATE_UNLOAD).expect("notify unload succeeds");
        assert_eq!(fake.get(APE_HOST_DRVR_STATE), APE_HOST_DRVR_STATE_UNLOAD);
    }
}
