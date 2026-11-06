//! PVSCSI register window and the command handshake.
//!
//! Offsets, command numbers and interrupt bits follow FreeBSD's
//! `sys/dev/vmware/pvscsi/pvscsi.h` (BSD-2-Clause, attribution in `NOTICE`;
//! H-S3-14). The window is 32 KiB of memory space: BAR0 on QEMU's model, the
//! first memory BAR of that size on VMware's (whose BAR0 is I/O ports).
//!
//! A command is its number written to `COMMAND`, then its descriptor as
//! 32-bit words to `COMMAND_DATA`; the device runs it once the last word
//! arrives and reports 0 in `COMMAND_STATUS` on success. A request is
//! started by writing any value to a kick register.

use ostd::mmio::MmioRegion;
use types::ViResult;

pub(crate) const REG_COMMAND: usize = 0x0000;
pub(crate) const REG_COMMAND_DATA: usize = 0x0004;
pub(crate) const REG_COMMAND_STATUS: usize = 0x0008;
/// Write-1-to-clear interrupt causes.
pub(crate) const REG_INTR_STATUS: usize = 0x100C;
/// Interrupt causes allowed to raise the interrupt.
pub(crate) const REG_INTR_MASK: usize = 0x2010;
pub(crate) const REG_KICK_NON_RW_IO: usize = 0x3014;
pub(crate) const REG_KICK_RW_IO: usize = 0x4018;
/// Size of the register window.
pub(crate) const WINDOW_LEN: usize = 0x8000;
const _: () = assert!(REG_KICK_RW_IO + 4 <= WINDOW_LEN);

pub(crate) const CMD_ADAPTER_RESET: u32 = 1;
pub(crate) const CMD_SETUP_RINGS: u32 = 3;

/// Completion-ring causes (`CMPL_0 | CMPL_1`).
pub(crate) const INTR_CMPL_MASK: u32 = 0x3;

/// Register access seam.
pub(crate) trait RegisterFile {
    fn rd32(&self, offset: usize) -> ViResult<u32>;
    fn wr32(&self, offset: usize, value: u32) -> ViResult<()>;
}

impl RegisterFile for MmioRegion {
    #[inline]
    fn rd32(&self, offset: usize) -> ViResult<u32> {
        self.read_u32(offset)
    }

    #[inline]
    fn wr32(&self, offset: usize, value: u32) -> ViResult<()> {
        self.write_u32(offset, value)
    }
}

/// Run `command` with descriptor `words`; the device's status (0 = success).
pub(crate) fn command(regs: &impl RegisterFile, command: u32, words: &[u32]) -> ViResult<u32> {
    regs.wr32(REG_COMMAND, command)?;
    for &word in words {
        regs.wr32(REG_COMMAND_DATA, word)?;
    }
    regs.rd32(REG_COMMAND_STATUS)
}

/// Whether `opcode` is a READ or WRITE (6/10/12/16), which the device takes
/// through the read/write kick; everything else uses the other kick.
pub(crate) const fn is_rw(opcode: u8) -> bool {
    matches!(
        opcode,
        0x08 | 0x0A | 0x28 | 0x2A | 0xA8 | 0xAA | 0x88 | 0x8A
    )
}

/// Start the request just published for a command with `opcode`.
pub(crate) fn kick(regs: &impl RegisterFile, opcode: u8) -> ViResult<()> {
    let register = if is_rw(opcode) {
        REG_KICK_RW_IO
    } else {
        REG_KICK_NON_RW_IO
    };
    regs.wr32(register, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Fake {
        writes: RefCell<Vec<(usize, u32)>>,
        status: u32,
    }

    impl RegisterFile for Fake {
        fn rd32(&self, offset: usize) -> ViResult<u32> {
            assert_eq!(offset, REG_COMMAND_STATUS);
            Ok(self.status)
        }
        fn wr32(&self, offset: usize, value: u32) -> ViResult<()> {
            self.writes.borrow_mut().push((offset, value));
            Ok(())
        }
    }

    #[test]
    fn command_writes_number_then_words_then_reads_status() {
        let fake = Fake {
            writes: RefCell::new(Vec::new()),
            status: 0,
        };
        assert_eq!(command(&fake, CMD_SETUP_RINGS, &[7, 8]).unwrap(), 0);
        assert_eq!(
            *fake.writes.borrow(),
            [
                (REG_COMMAND, 3),
                (REG_COMMAND_DATA, 7),
                (REG_COMMAND_DATA, 8)
            ]
        );
        let failing = Fake {
            writes: RefCell::new(Vec::new()),
            status: u32::MAX,
        };
        assert_eq!(command(&failing, CMD_ADAPTER_RESET, &[]).unwrap(), u32::MAX);
        assert_eq!(*failing.writes.borrow(), [(REG_COMMAND, 1)]);
    }

    #[test]
    fn reads_and_writes_take_the_rw_kick() {
        for opcode in [0x28, 0x2A, 0x88, 0x8A] {
            assert!(is_rw(opcode), "{opcode:#x}");
        }
        // INQUIRY, TEST UNIT READY, REPORT LUNS, READ CAPACITY(16), SYNC CACHE.
        for opcode in [0x12, 0x00, 0xA0, 0x9E, 0x35] {
            assert!(!is_rw(opcode), "{opcode:#x}");
        }
        let fake = Fake {
            writes: RefCell::new(Vec::new()),
            status: 0,
        };
        kick(&fake, 0x88).unwrap();
        kick(&fake, 0x12).unwrap();
        assert_eq!(
            *fake.writes.borrow(),
            [(REG_KICK_RW_IO, 0), (REG_KICK_NON_RW_IO, 0)]
        );
    }
}
