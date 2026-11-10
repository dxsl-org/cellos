use super::*;

/// QEMU `virt` GICv3: distributor at 0x0800_0000, redistributors at
/// 0x080A_0000 (0xF6_0000 bytes), all inside the 16 MiB GIC map.
const QEMU_REG: [(usize, Option<usize>); 2] = [
    (0x0800_0000, Some(0x1_0000)),
    (0x080A_0000, Some(0xF6_0000)),
];

fn gic_map(base: usize, end: usize) -> bool {
    base >= 0x0800_0000 && end <= 0x0900_0000
}

fn qemu_windows() -> GicV3Windows {
    select_windows(&QEMU_REG, gic_map).unwrap()
}

/// A region whose frames report `typers[i]` at `base + i * 128 KiB`.
fn typer_reader(windows: GicV3Windows, typers: &[(usize, u64)]) -> impl Fn(usize) -> u64 + '_ {
    move |rd_base| {
        typers
            .iter()
            .find(|(offset, _)| windows.redistributors + offset == rd_base)
            .map(|&(_, typer)| typer)
            .expect("walk read a frame that holds no redistributor")
    }
}

#[test]
fn qemu_virt_windows_are_selected() {
    assert_eq!(
        qemu_windows(),
        GicV3Windows {
            distributor: 0x0800_0000,
            redistributors: 0x080A_0000,
            redistributors_size: 0xF6_0000,
        }
    );
}

#[test]
fn later_reg_entries_are_ignored() {
    let reg = [QEMU_REG[0], QEMU_REG[1], (0x4_0000_0000, Some(0x1000))];
    assert_eq!(select_windows(&reg, gic_map), Ok(qemu_windows()));
}

#[test]
fn missing_or_short_windows_are_refused() {
    assert_eq!(
        select_windows(&[], gic_map),
        Err(LayoutError::MissingDistributor)
    );
    assert_eq!(
        select_windows(&QEMU_REG[..1], gic_map),
        Err(LayoutError::MissingRedistributors)
    );
    assert_eq!(
        select_windows(&[(0x0800_0000, Some(0x1000)), QEMU_REG[1]], gic_map),
        Err(LayoutError::DistributorTooSmall)
    );
    assert_eq!(
        select_windows(&[QEMU_REG[0], (0x080A_0000, Some(0x1_0000))], gic_map),
        Err(LayoutError::RedistributorsTooSmall)
    );
    assert_eq!(
        select_windows(&[QEMU_REG[0], (0x080A_0000, None)], gic_map),
        Err(LayoutError::RedistributorsTooSmall)
    );
}

#[test]
fn unmapped_or_wrapping_windows_are_refused() {
    // QEMU places a second redistributor region in high memory beyond 123 CPUs;
    // used as the first region it is outside every kernel map.
    let high = [QEMU_REG[0], (0x40_1000_0000, Some(0x400_0000))];
    assert_eq!(
        select_windows(&high, gic_map),
        Err(LayoutError::Unmapped {
            base: 0x40_1000_0000,
            end: 0x40_1400_0000
        })
    );
    // Crossing the map end by one frame is refused, not truncated.
    let straddle = [QEMU_REG[0], (0x08FE_0000, Some(0x4_0000))];
    assert!(matches!(
        select_windows(&straddle, gic_map),
        Err(LayoutError::Unmapped { .. })
    ));
    let wraps = [(usize::MAX - 0xFFFF, Some(0x1_0000)), QEMU_REG[1]];
    assert_eq!(
        select_windows(&wraps, |_, _| true),
        Err(LayoutError::Unmapped {
            base: usize::MAX - 0xFFFF,
            end: usize::MAX
        })
    );
}

#[test]
fn affinity_packs_aff3_above_aff2_aff1_aff0() {
    // MPIDR: Aff3 = 0x12 (bits 39:32), U/MT bits 30/24 ignored outside Aff0-2.
    let mpidr = (0x12u64 << 32) | (1 << 31) | 0x00_03_02_01;
    assert_eq!(mpidr_affinity(mpidr), 0x1203_0201);
    assert_eq!(irouter_for(mpidr), (0x12u64 << 32) | 0x03_02_01);
    assert_eq!(irouter_for(1 << 31), 0);
}

#[test]
fn walk_finds_each_core_by_affinity() {
    let windows = qemu_windows();
    let typers = [(0, 0u64), (0x2_0000, (1u64 << 32) | GICR_TYPER_LAST)];
    let read = typer_reader(windows, &typers);
    assert_eq!(find_redistributor(&windows, 0, &read), Some(0x080A_0000));
    assert_eq!(find_redistributor(&windows, 1, &read), Some(0x080C_0000));
}

#[test]
fn walk_stops_at_last_frame() {
    let windows = qemu_windows();
    let typers = [(0, GICR_TYPER_LAST)];
    assert_eq!(
        find_redistributor(&windows, 1, typer_reader(windows, &typers)),
        None
    );
}

#[test]
fn walk_uses_vlpi_stride_for_gicv4_frames() {
    let windows = qemu_windows();
    let typers = [
        (0, GICR_TYPER_VLPIS),
        (0x4_0000, (1u64 << 32) | GICR_TYPER_VLPIS | GICR_TYPER_LAST),
    ];
    assert_eq!(
        find_redistributor(&windows, 1, typer_reader(windows, &typers)),
        Some(0x080E_0000)
    );
}

#[test]
fn walk_stays_inside_the_region() {
    // Two redistributors fit; neither matches and neither is marked Last.
    let windows = GicV3Windows {
        redistributors_size: 0x4_0000,
        ..qemu_windows()
    };
    let typers = [(0, 5u64 << 32), (0x2_0000, 6u64 << 32)];
    assert_eq!(
        find_redistributor(&windows, 7, typer_reader(windows, &typers)),
        None
    );
}
