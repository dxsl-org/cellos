//! AArch64 CPU index ↔ `MPIDR_EL1` affinity map.
//!
//! The *CPU index* is the AArch64 physical id [`super::CPU_TOPOLOGY`] maps: the
//! position of the CPU's node under the firmware tree's `/cpus`, which is also
//! the numbering the spin-table release addresses and the boards' IPI targets
//! use. The hardware names a CPU by its full affinity (Aff3.Aff2.Aff1.Aff0):
//! `PSCI_CPU_ON` takes it, and `MPIDR_EL1` is the only way a running core can
//! tell which one it is. `Aff0` alone is not an index — on a GICv3 `virt`
//! machine CPU 16 is `0.0.1.0`, the same `Aff0` as CPU 0, and multi-cluster
//! SoCs restart `Aff0` in every cluster.
//!
//! The tree's `reg` of each `/cpus/cpu@…` node is that CPU's affinity. With no
//! tree (or a malformed one) the map stays *flat*: index `i` is affinity `i`,
//! the numbering every single-cluster machine this kernel ran on before had.
//!
//! Contract: [`MpidrTable::describe`] runs once, on the boot hart, before any
//! secondary is started; after that the table is read-only, so readers take no
//! lock.

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

/// The affinity fields of `MPIDR_EL1` (Aff3 at 39:32, Aff2..Aff0 at 23:0).
/// Bit 31 (RES1), U (30) and MT (24) say nothing about which CPU this is.
pub const AFFINITY_MASK: u64 = 0xFF_00FF_FFFF;

/// The affinity part of an `MPIDR_EL1` value (or of a `/cpus` `reg`).
pub const fn affinity(mpidr: u64) -> u64 {
    mpidr & AFFINITY_MASK
}

/// The affinity a `/cpus/cpu@…` `reg` property names.
///
/// `reg` is a list of `address_cells`-wide big-endian ids (more than one only
/// for SMT threads); the first one is the CPU's. `None` when the property is
/// too short, the cell count is not 1 or 2 (the only widths the Arm CPU
/// binding allows), or the id has bits outside the affinity fields — a tree
/// that says that is not describing an `MPIDR_EL1`.
pub fn reg_affinity(reg: &[u8], address_cells: usize) -> Option<u64> {
    let id = match address_cells {
        1 => u64::from(u32::from_be_bytes(reg.get(..4)?.try_into().ok()?)),
        2 => u64::from_be_bytes(reg.get(..8)?.try_into().ok()?),
        _ => return None,
    };
    (id == affinity(id)).then_some(id)
}

/// Why [`MpidrTable::describe`] kept the flat numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescribeError {
    /// The table was already described.
    AlreadyDescribed,
    /// The CPU at this index has no usable `reg`.
    MissingId(usize),
    /// The CPU at this index repeats an earlier CPU's affinity.
    Duplicate(usize),
}

/// The CPU-index → affinity table for up to `N` CPUs (see module docs).
pub struct MpidrTable<const N: usize> {
    affinity: [AtomicU64; N],
    /// CPUs described; 0 = flat numbering. Published last, with Release.
    count: AtomicUsize,
    /// Set by the one [`describe`](Self::describe) call allowed.
    claimed: AtomicBool,
}

impl<const N: usize> MpidrTable<N> {
    pub const fn new() -> Self {
        Self {
            affinity: [const { AtomicU64::new(0) }; N],
            count: AtomicUsize::new(0),
            claimed: AtomicBool::new(false),
        }
    }

    /// Record the firmware's CPUs, in `/cpus` order; `None` is a CPU whose
    /// `reg` gave no affinity.
    ///
    /// CPUs past the first `N` are not recorded: no logical hart can run them.
    /// Any missing or repeated id among the recorded ones refuses the whole
    /// description, so the map never names one core twice or skips one; the
    /// table then stays flat. Returns the number of CPUs recorded.
    pub fn describe(
        &self,
        cpus: impl IntoIterator<Item = Option<u64>>,
    ) -> Result<usize, DescribeError> {
        if self.claimed.swap(true, Ordering::AcqRel) {
            return Err(DescribeError::AlreadyDescribed);
        }
        let mut count = 0;
        for (index, id) in cpus.into_iter().take(N).enumerate() {
            let id = affinity(id.ok_or(DescribeError::MissingId(index))?);
            if self.affinity[..index]
                .iter()
                .any(|seen| seen.load(Ordering::Relaxed) == id)
            {
                return Err(DescribeError::Duplicate(index));
            }
            self.affinity[index].store(id, Ordering::Relaxed);
            count = index + 1;
        }
        self.count.store(count, Ordering::Release);
        Ok(count)
    }

    /// CPUs described by the firmware (0 = flat numbering).
    pub fn described(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    /// The CPU index of the core whose `MPIDR_EL1` is `mpidr`, if it has one.
    ///
    /// Called on every AArch64 `current_hart_id`, so the CPU at index
    /// `Aff0` — every CPU of a single-cluster machine — is checked first and
    /// the table is scanned only when that misses.
    #[inline]
    pub fn cpu_index(&self, mpidr: u64) -> Option<usize> {
        let id = affinity(mpidr);
        let count = self.described();
        if count == 0 {
            return (id < N as u64).then_some(id as usize);
        }
        let guess = (id & 0xFF) as usize;
        if guess < count && self.affinity[guess].load(Ordering::Relaxed) == id {
            return Some(guess);
        }
        self.affinity[..count]
            .iter()
            .position(|cpu| cpu.load(Ordering::Relaxed) == id)
    }

    /// The affinity (`PSCI_CPU_ON` target) of CPU `index`, if it exists.
    pub fn mpidr(&self, index: usize) -> Option<u64> {
        let count = self.described();
        if count == 0 {
            return (index < N).then_some(index as u64);
        }
        self.affinity[..count]
            .get(index)
            .map(|cpu| cpu.load(Ordering::Relaxed))
    }
}

impl<const N: usize> Default for MpidrTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_numbering_without_a_description() {
        let table = MpidrTable::<4>::new();
        // RES1 bit 31 and MT are not part of the id.
        assert_eq!(table.cpu_index(0x8000_0002), Some(2));
        assert_eq!(table.cpu_index(0x8100_0003), Some(3));
        assert_eq!(table.cpu_index(4), None);
        // Flat numbering has no CPU in another cluster; Aff0 alone would
        // alias CPU 0.
        assert_eq!(table.cpu_index(0x100), None);
        assert_eq!(table.mpidr(3), Some(3));
        assert_eq!(table.mpidr(4), None);
    }

    #[test]
    fn single_cluster_description_is_identity() {
        let table = MpidrTable::<32>::new();
        assert_eq!(table.describe((0..4).map(Some)), Ok(4));
        for cpu in 0..4 {
            assert_eq!(table.cpu_index(0x8000_0000 | cpu), Some(cpu as usize));
            assert_eq!(table.mpidr(cpu as usize), Some(cpu));
        }
        // A described machine has exactly the CPUs it describes.
        assert_eq!(table.cpu_index(4), None);
        assert_eq!(table.mpidr(4), None);
    }

    #[test]
    fn gicv3_virt_past_sixteen_cpus() {
        // QEMU virt with GICv3 packs 16 CPUs per Aff1 cluster.
        let ids = (0..20u64).map(|cpu| ((cpu / 16) << 8) | (cpu % 16));
        let table = MpidrTable::<32>::new();
        assert_eq!(table.describe(ids.map(Some)), Ok(20));
        assert_eq!(table.cpu_index(0x8000_0000), Some(0));
        assert_eq!(table.cpu_index(0x8000_0100), Some(16));
        assert_eq!(table.cpu_index(0x8000_0103), Some(19));
        assert_eq!(table.mpidr(17), Some(0x101));
    }

    #[test]
    fn multi_cluster_and_aff3() {
        // Apple t8103 shape: E-cores in cluster 0, P-cores in cluster 1.
        let ids = [0x0, 0x1, 0x2, 0x3, 0x1_0100, 0x1_0101, 0x1_0102, 0x1_0103];
        let table = MpidrTable::<32>::new();
        assert_eq!(table.describe(ids.into_iter().map(Some)), Ok(8));
        assert_eq!(table.cpu_index(0x8001_0100), Some(4));
        assert_eq!(table.cpu_index(0x8001_0103), Some(7));
        let wide = MpidrTable::<4>::new();
        assert_eq!(wide.describe([Some(0), Some(0x2_0000_0000)]), Ok(2));
        assert_eq!(wide.cpu_index(0x2_8000_0000), Some(1));
        assert_eq!(wide.mpidr(1), Some(0x2_0000_0000));
    }

    #[test]
    fn malformed_description_stays_flat() {
        let table = MpidrTable::<8>::new();
        assert_eq!(
            table.describe([Some(0), Some(0x100), Some(0x100)]),
            Err(DescribeError::Duplicate(2))
        );
        assert_eq!(table.described(), 0);
        assert_eq!(table.cpu_index(0x100), None);
        assert_eq!(table.cpu_index(1), Some(1));

        let missing = MpidrTable::<8>::new();
        assert_eq!(
            missing.describe([Some(0), None]),
            Err(DescribeError::MissingId(1))
        );
        assert_eq!(missing.described(), 0);
    }

    #[test]
    fn describe_is_write_once_and_bounded() {
        let table = MpidrTable::<2>::new();
        assert_eq!(table.describe((0..5).map(Some)), Ok(2));
        assert_eq!(
            table.describe([Some(7)]),
            Err(DescribeError::AlreadyDescribed)
        );
        assert_eq!(table.mpidr(0), Some(0));
        assert_eq!(table.cpu_index(2), None);
    }

    #[test]
    fn reg_parsing() {
        assert_eq!(reg_affinity(&[0, 0, 1, 3], 1), Some(0x103));
        assert_eq!(
            reg_affinity(&[0, 0, 0, 2, 0, 0, 0, 1], 2),
            Some(0x2_0000_0001)
        );
        // SMT: the first listed thread names the CPU.
        assert_eq!(reg_affinity(&[0, 0, 0, 4, 0, 0, 0, 5], 1), Some(4));
        assert_eq!(reg_affinity(&[0, 0, 1], 1), None);
        assert_eq!(reg_affinity(&[0, 0, 0, 0], 2), None);
        assert_eq!(reg_affinity(&[0, 0, 0, 0], 3), None);
        // RES1/MT bits are not an affinity.
        assert_eq!(reg_affinity(&[0x80, 0, 0, 0], 1), None);
    }
}
