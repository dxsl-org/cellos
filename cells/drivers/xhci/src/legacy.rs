//! xHCI USB Legacy Support (USBLEGSUP) BIOS handoff (xHCI 1.2 §4.22.1).

use ostd::io::print_fmt;
use types::ViResult;

pub const CAP_ID_USB_LEGACY: u8 = 1;
pub const BIOS_OWNED_BIT: u32 = 1 << 16;
pub const OS_OWNED_BIT: u32 = 1 << 24;

/// Minimal MMIO accessor trait for testability.
pub trait XhciMmio {
    fn read_u32(&self, offset: usize) -> ViResult<u32>;
    fn write_u32(&self, offset: usize, val: u32) -> ViResult<()>;
}

impl XhciMmio for ostd::mmio::MmioRegion {
    #[inline]
    fn read_u32(&self, offset: usize) -> ViResult<u32> {
        self.read_u32(offset)
    }
    #[inline]
    fn write_u32(&self, offset: usize, val: u32) -> ViResult<()> {
        self.write_u32(offset, val)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LegacyHandoffResult {
    pub found: bool,
    pub bios_cleared: bool,
}

/// Perform USBLEGSUP BIOS-to-OS ownership handoff on the controller.
pub fn bios_handoff<M: XhciMmio>(mmio: &M, hccparams1: u32) -> ViResult<LegacyHandoffResult> {
    let mut xecp = (((hccparams1 >> 16) & 0xFFFF) as usize) * 4;
    while xecp != 0 {
        let cap = mmio.read_u32(xecp)?;
        let cap_id = (cap & 0xFF) as u8;
        let next_dwords = ((cap >> 8) & 0xFF) as usize;

        if cap_id == CAP_ID_USB_LEGACY {
            let legsup = cap;
            let mut bios_cleared = (legsup & BIOS_OWNED_BIT) == 0;

            if !bios_cleared {
                // Assert OS-owned semaphore
                mmio.write_u32(xecp, legsup | OS_OWNED_BIT)?;

                // Wait up to 1 second for BIOS-owned semaphore to clear
                for _ in 0..100 {
                    let cur = mmio.read_u32(xecp)?;
                    if (cur & BIOS_OWNED_BIT) == 0 {
                        bios_cleared = true;
                        break;
                    }
                    ostd::task::yield_now();
                }

                if !bios_cleared {
                    let _ = print_fmt(format_args!(
                        "[xhci] BIOS handoff timed out waiting for BIOS semaphore — forcing OS ownership\n"
                    ));
                    let cur = mmio.read_u32(xecp)?;
                    let _ = mmio.write_u32(xecp, (cur & !BIOS_OWNED_BIT) | OS_OWNED_BIT);
                }
            } else {
                let _ = mmio.write_u32(xecp, legsup | OS_OWNED_BIT);
            }

            // Disable SMIs in USBLEGCTLSTS (offset + 4)
            let _ = mmio.write_u32(xecp + 4, 0);

            return Ok(LegacyHandoffResult {
                found: true,
                bios_cleared,
            });
        }

        if next_dwords == 0 {
            break;
        }
        xecp += next_dwords * 4;
    }

    Ok(LegacyHandoffResult {
        found: false,
        bios_cleared: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;
    use core::cell::RefCell;

    struct MockMmio {
        regs: RefCell<BTreeMap<usize, u32>>,
        auto_clear_bios: bool,
    }

    impl MockMmio {
        fn new(regs: BTreeMap<usize, u32>, auto_clear_bios: bool) -> Self {
            Self {
                regs: RefCell::new(regs),
                auto_clear_bios,
            }
        }
    }

    impl XhciMmio for MockMmio {
        fn read_u32(&self, offset: usize) -> ViResult<u32> {
            let mut regs = self.regs.borrow_mut();
            let val = regs.get(&offset).copied().unwrap_or(0);
            if offset == 0x100 && (val & OS_OWNED_BIT != 0) && self.auto_clear_bios {
                regs.insert(offset, val & !BIOS_OWNED_BIT);
            }
            Ok(val)
        }

        fn write_u32(&self, offset: usize, val: u32) -> ViResult<()> {
            self.regs.borrow_mut().insert(offset, val);
            Ok(())
        }
    }

    #[test]
    fn handoff_succeeds_when_bios_clears() {
        let mut regs = BTreeMap::new();
        regs.insert(0x100, 1 | BIOS_OWNED_BIT);
        let mock = MockMmio::new(regs, true);

        let res = bios_handoff(&mock, 0x0040_0000).expect("handoff");
        assert!(res.found);
        assert!(res.bios_cleared);
        assert_eq!(mock.read_u32(0x104).unwrap(), 0);
    }

    #[test]
    fn handoff_forces_ownership_when_bios_never_clears() {
        let mut regs = BTreeMap::new();
        regs.insert(0x100, 1 | BIOS_OWNED_BIT);
        let mock = MockMmio::new(regs, false);

        let res = bios_handoff(&mock, 0x0040_0000).expect("handoff");
        assert!(res.found);
        assert!(!res.bios_cleared);
    }

    #[test]
    fn handoff_skips_when_no_xecp() {
        let regs = BTreeMap::new();
        let mock = MockMmio::new(regs, false);

        let res = bios_handoff(&mock, 0).expect("handoff");
        assert!(!res.found);
        assert!(res.bios_cleared);
    }
}
