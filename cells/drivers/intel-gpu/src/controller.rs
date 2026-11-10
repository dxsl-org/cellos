//! Top-level controller managing Intel iGPU hardware initialization.

use crate::clock::Clock;
use crate::forcewake::{forcewake_gt_acquire, forcewake_gt_release};
use crate::identity::{probe_gt, GtIdentity};
use crate::regs::RegisterFile;
use types::ViResult;

/// The GT's registers and clock, its identity, and whether forcewake is held.
pub struct IntelGpuController<R: RegisterFile, C: Clock> {
    pub regs: R,
    pub clock: C,
    pub identity: GtIdentity,
    pub forcewake_active: bool,
}

impl<R: RegisterFile, C: Clock> IntelGpuController<R, C> {
    /// Wake the GT, then read its identity and topology. Forcewake is released again if
    /// the identity cannot be read.
    pub fn probe(mut regs: R, mut clock: C) -> ViResult<Self> {
        forcewake_gt_acquire(&mut regs, &mut clock)?;
        let identity = match probe_gt(&regs) {
            Ok(id) => id,
            Err(e) => {
                let _ = forcewake_gt_release(&mut regs, &mut clock);
                return Err(e);
            }
        };
        Ok(Self {
            regs,
            clock,
            identity,
            forcewake_active: true,
        })
    }

    /// Release forcewake if held.
    pub fn shutdown(&mut self) -> ViResult<()> {
        if self.forcewake_active {
            forcewake_gt_release(&mut self.regs, &mut self.clock)?;
            self.forcewake_active = false;
        }
        Ok(())
    }
}
