//! Top-level controller managing Intel iGPU hardware initialization.

use crate::forcewake::{forcewake_gt_acquire, forcewake_gt_release};
use crate::identity::{probe_gt, GtIdentity};
use crate::regs::RegisterFile;
use types::ViResult;

pub struct IntelGpuController<R: RegisterFile> {
    pub regs: R,
    pub identity: GtIdentity,
    pub forcewake_active: bool,
}

impl<R: RegisterFile> IntelGpuController<R> {
    pub fn probe(mut regs: R) -> ViResult<Self> {
        // 1. Wake GT domain before accessing GT registers
        forcewake_gt_acquire(&mut regs)?;

        // 2. Discover GT identity and topology
        let identity = match probe_gt(&regs) {
            Ok(id) => id,
            Err(e) => {
                let _ = forcewake_gt_release(&mut regs);
                return Err(e);
            }
        };

        Ok(Self {
            regs,
            identity,
            forcewake_active: true,
        })
    }

    pub fn shutdown(&mut self) -> ViResult<()> {
        if self.forcewake_active {
            forcewake_gt_release(&mut self.regs)?;
            self.forcewake_active = false;
        }
        Ok(())
    }
}
