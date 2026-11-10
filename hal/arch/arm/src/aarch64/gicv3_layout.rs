//! GICv3 window selection and redistributor lookup.
//!
//! The rules that decide *which* distributor and redistributor frames a core
//! uses live here, free of MMIO, so they are host-tested; `gicv3` feeds them
//! register reads.

/// Size of the distributor (`GICD_*`) register frame.
pub const GICD_SIZE: usize = 0x1_0000;
/// One 64 KiB redistributor frame (RD_base, then SGI_base).
pub const GICR_FRAME_SIZE: usize = 0x1_0000;
/// `GICR_TYPER` offset inside the RD_base frame (64-bit register).
pub const GICR_TYPER: usize = 0x0008;
/// `GICR_TYPER.Last`: this redistributor is the last one in its region.
pub const GICR_TYPER_LAST: u64 = 1 << 4;
/// `GICR_TYPER.VLPIS`: GICv4 VLPI frames follow, so a redistributor spans four
/// frames instead of two.
pub const GICR_TYPER_VLPIS: u64 = 1 << 1;

/// The GICv3 MMIO a board's kernel drives: one distributor and the first
/// redistributor region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GicV3Windows {
    /// Distributor base (`GICD_*`).
    pub distributor: usize,
    /// First redistributor region base.
    pub redistributors: usize,
    /// First redistributor region size in bytes.
    pub redistributors_size: usize,
}

/// Why a firmware-described GICv3 cannot be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// `reg` names no distributor window.
    MissingDistributor,
    /// `reg` names no redistributor region.
    MissingRedistributors,
    /// The distributor window is smaller than one register frame.
    DistributorTooSmall,
    /// The redistributor region cannot hold one redistributor (RD + SGI frame).
    RedistributorsTooSmall,
    /// `[base, end)` wraps or is not inside a kernel MMIO identity map, so the
    /// first access would fault.
    Unmapped {
        /// Window base.
        base: usize,
        /// Window end, exclusive (`usize::MAX` when the end wraps).
        end: usize,
    },
}

/// Pick the windows from a GICv3 node's `reg` entries (`(base, size)`, in tree
/// order: distributor, then the redistributor regions).
///
/// Only the first redistributor region is used: the kernel brings up at most
/// 32 cores and QEMU `virt` puts the first 123 in that region. `mapped(base,
/// end)` reports whether `[base, end)` lies inside a kernel MMIO identity map.
pub fn select_windows(
    reg: &[(usize, Option<usize>)],
    mapped: impl Fn(usize, usize) -> bool,
) -> Result<GicV3Windows, LayoutError> {
    let &(distributor, distributor_size) = reg.first().ok_or(LayoutError::MissingDistributor)?;
    let &(redistributors, redistributors_size) =
        reg.get(1).ok_or(LayoutError::MissingRedistributors)?;
    let distributor_size = distributor_size.unwrap_or(0);
    let redistributors_size = redistributors_size.unwrap_or(0);
    if distributor_size < GICD_SIZE {
        return Err(LayoutError::DistributorTooSmall);
    }
    if redistributors_size < 2 * GICR_FRAME_SIZE {
        return Err(LayoutError::RedistributorsTooSmall);
    }
    for (base, size) in [
        (distributor, GICD_SIZE),
        (redistributors, redistributors_size),
    ] {
        let end = base.checked_add(size);
        if !end.is_some_and(|end| mapped(base, end)) {
            return Err(LayoutError::Unmapped {
                base,
                end: end.unwrap_or(usize::MAX),
            });
        }
    }
    Ok(GicV3Windows {
        distributor,
        redistributors,
        redistributors_size,
    })
}

/// The affinity value `GICR_TYPER[63:32]` reports for the core with `mpidr`:
/// `Aff3:Aff2:Aff1:Aff0`.
pub const fn mpidr_affinity(mpidr: u64) -> u32 {
    ((((mpidr >> 32) & 0xFF) << 24) | (mpidr & 0x00FF_FFFF)) as u32
}

/// The `GICD_IROUTER` value that routes an SPI to the core with `mpidr`
/// (`Interrupt_Routing_Mode` = 0: that core only).
pub const fn irouter_for(mpidr: u64) -> u64 {
    mpidr & 0xFF_00FF_FFFF
}

/// Find the RD_base of the redistributor whose `GICR_TYPER` affinity is
/// `affinity`, walking the first region frame by frame (GICv3 §9.6).
///
/// `read_typer(rd_base)` reads that frame's `GICR_TYPER`. The walk stops at
/// `GICR_TYPER.Last` or the region end, whichever comes first.
pub fn find_redistributor(
    windows: &GicV3Windows,
    affinity: u32,
    read_typer: impl Fn(usize) -> u64,
) -> Option<usize> {
    let mut offset = 0usize;
    while offset
        .checked_add(2 * GICR_FRAME_SIZE)
        .is_some_and(|end| end <= windows.redistributors_size)
    {
        let rd_base = windows.redistributors + offset;
        let typer = read_typer(rd_base);
        if (typer >> 32) as u32 == affinity {
            return Some(rd_base);
        }
        if typer & GICR_TYPER_LAST != 0 {
            return None;
        }
        offset += if typer & GICR_TYPER_VLPIS != 0 {
            4 * GICR_FRAME_SIZE
        } else {
            2 * GICR_FRAME_SIZE
        };
    }
    None
}

#[cfg(test)]
#[path = "gicv3_layout_tests.rs"]
mod tests;
