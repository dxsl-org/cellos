//! Byte images of the hypervisor-syscall records the kernel copies to a cell.
//!
//! The hypervisor syscalls move these records through the caller's copy view,
//! which takes bytes. A typed `ViVmExit` cannot be viewed as bytes soundly — its
//! padding is uninitialised and would leak kernel stack — so the exit is
//! serialised field by field into a zeroed image of its `#[repr(C, u8)]`
//! layout: the `u8` tag at 0, the payload (a `repr(C)` struct per variant) at
//! 8, the union's alignment.

use abi::hypervisor::ViVmExit;

/// Size of the `ViVmExit` record in the cell's buffer (frozen ABI).
pub(crate) const EXIT_LEN: usize = core::mem::size_of::<ViVmExit>();

/// Size of the `VcpuRegs` buffer: 32 × u64.
pub(crate) const REGS_LEN: usize = 32 * 8;

const _: () = assert!(EXIT_LEN == 80);

/// Payload offset: the tag is padded to the payload union's 8-byte alignment.
const PAYLOAD: usize = 8;

struct Image([u8; EXIT_LEN]);

impl Image {
    fn put(&mut self, offset: usize, bytes: &[u8]) {
        self.0[PAYLOAD + offset..PAYLOAD + offset + bytes.len()].copy_from_slice(bytes);
    }
}

/// The exact bytes a cell reads as `ViVmExit`; every padding byte is zero.
pub(crate) fn encode_exit(exit: &ViVmExit) -> [u8; EXIT_LEN] {
    let mut image = Image([0; EXIT_LEN]);
    let tag = match *exit {
        ViVmExit::MmioRead { ipa, size, reg } => {
            image.put(0, &ipa.to_ne_bytes());
            image.put(8, &[size, reg]);
            0
        }
        ViVmExit::MmioWrite { ipa, size, val } => {
            image.put(0, &ipa.to_ne_bytes());
            image.put(8, &[size]);
            image.put(16, &val.to_ne_bytes());
            1
        }
        ViVmExit::Hvc { imm, regs } => {
            image.put(0, &imm.to_ne_bytes());
            for (index, reg) in regs.iter().enumerate() {
                image.put(8 + index * 8, &reg.to_ne_bytes());
            }
            2
        }
        ViVmExit::Wfi => 3,
        ViVmExit::SysReg {
            op0,
            op1,
            crn,
            crm,
            op2,
            rt,
            is_write,
        } => {
            image.put(0, &[op0, op1, crn, crm, op2, rt, is_write as u8]);
            4
        }
        ViVmExit::Preempted => 5,
        ViVmExit::Shutdown => 6,
        ViVmExit::Unknown { ec, iss } => {
            image.put(0, &ec.to_ne_bytes());
            image.put(4, &iss.to_ne_bytes());
            7
        }
        ViVmExit::PortIn { port, size, reg } => {
            image.put(0, &port.to_ne_bytes());
            image.put(2, &[size, reg]);
            8
        }
        ViVmExit::PortOut { port, size, val } => {
            image.put(0, &port.to_ne_bytes());
            image.put(2, &[size]);
            image.put(4, &val.to_ne_bytes());
            9
        }
        ViVmExit::Hlt => 10,
        ViVmExit::Msr {
            index,
            is_write,
            val,
        } => {
            image.put(0, &index.to_ne_bytes());
            image.put(4, &[is_write as u8]);
            image.put(8, &val.to_ne_bytes());
            11
        }
    };
    image.0[0] = tag;
    image.0
}

/// Decode a `VcpuRegs` buffer into its 32 words.
pub(crate) fn regs_from_bytes(bytes: &[u8; REGS_LEN]) -> [u64; 32] {
    let mut regs = [0u64; 32];
    for (word, chunk) in regs.iter_mut().zip(bytes.chunks_exact(8)) {
        let mut raw = [0u8; 8];
        raw.copy_from_slice(chunk);
        *word = u64::from_ne_bytes(raw);
    }
    regs
}

/// Encode 32 register words as the `VcpuRegs` buffer.
pub(crate) fn regs_to_bytes(regs: &[u64; 32]) -> [u8; REGS_LEN] {
    let mut bytes = [0u8; REGS_LEN];
    for (chunk, word) in bytes.chunks_exact_mut(8).zip(regs) {
        chunk.copy_from_slice(&word.to_ne_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::*;
    use alloc::format;

    /// Every variant, with distinct non-zero field values so a wrong offset
    /// shows up as a wrong field.
    fn samples() -> [ViVmExit; 12] {
        [
            ViVmExit::MmioRead {
                ipa: 0x1122_3344_5566_7788,
                size: 4,
                reg: 9,
            },
            ViVmExit::MmioWrite {
                ipa: 0xd000_0010,
                size: 2,
                val: 0xAABB_CCDD_EEFF_0011,
            },
            ViVmExit::Hvc {
                imm: 0x1234,
                regs: [1, 2, 3, 4, 5, 6, 7, u64::MAX],
            },
            ViVmExit::Wfi,
            ViVmExit::SysReg {
                op0: 3,
                op1: 1,
                crn: 14,
                crm: 3,
                op2: 2,
                rt: 30,
                is_write: true,
            },
            ViVmExit::Preempted,
            ViVmExit::Shutdown,
            ViVmExit::Unknown {
                ec: 0x400,
                iss: 0xDEAD_BEEF,
            },
            ViVmExit::PortIn {
                port: 0x3F8,
                size: 1,
                reg: 7,
            },
            ViVmExit::PortOut {
                port: 0xCF8,
                size: 4,
                val: 0x8000_0000,
            },
            ViVmExit::Hlt,
            ViVmExit::Msr {
                index: 0xC000_0080,
                is_write: true,
                val: 0x0123_4567_89AB_CDEF,
            },
        ]
    }

    #[test]
    fn every_exit_reads_back_as_the_same_record() {
        for exit in samples() {
            let image = encode_exit(&exit);
            // SAFETY: the image is fully initialised, 80 bytes, with a valid
            // tag and `bool` bytes of 0/1 — exactly what a cell reads.
            let read: ViVmExit = unsafe { core::ptr::read_unaligned(image.as_ptr().cast()) };
            assert_eq!(format!("{read:?}"), format!("{exit:?}"));
        }
    }

    #[test]
    fn padding_is_zero() {
        // Hlt carries no payload: every byte but the tag is padding.
        let image = encode_exit(&ViVmExit::Hlt);
        assert_eq!(image[0], 10);
        assert!(image[1..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn register_words_round_trip() {
        let regs: [u64; 32] = core::array::from_fn(|index| (index as u64) << 56 | index as u64);
        assert_eq!(regs_from_bytes(&regs_to_bytes(&regs)), regs);
    }
}
