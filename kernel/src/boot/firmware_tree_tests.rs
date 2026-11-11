//! Real firmware trees through the aarch64 boot parser ([`firmware_cpus`],
//! [`fdt_nops`]) — the code `firmware_smp::note` runs on the board.
//!
//! Fixtures (`tests/fixtures/dtb/`, provenance in `SOURCES.txt` there):
//! - `t8103-j274.dtb`: Linux v7.3 `arch/arm64/boot/dts/apple/t8103-j274.dts`
//!   (Mac mini M1), `cpp` + `dtc` 1.7.0. `cpu-release-addr` is `<0 0>` until
//!   the loader fills it in.
//! - `t8103-j274-m1n1.dtb`: the same tree after an m1n1-style in-place fill of
//!   every `cpu-release-addr` (`fdtput`; 64-byte spin-table stride).
//! - `bcm2711-rpi-4-b.dtb`: Raspberry Pi firmware `boot/` (commit 12eeaa12).
//! - `bcm2711-rpi-4-b-qemu-raspi4b.dtb`: the tree QEMU 11.1.3 `raspi4b` hands
//!   the kernel for that DTB (`-M raspi4b,dumpdtb=`), PCIe/GENET/thermal nodes
//!   replaced by `FDT_NOP`.

use super::{fdt_nops, firmware_cpus};
use crate::sched::smp::mpidr::MpidrTable;

const T8103: &[u8] = include_bytes!("../../../tests/fixtures/dtb/t8103-j274.dtb");
const T8103_M1N1: &[u8] = include_bytes!("../../../tests/fixtures/dtb/t8103-j274-m1n1.dtb");
const RPI4: &[u8] = include_bytes!("../../../tests/fixtures/dtb/bcm2711-rpi-4-b.dtb");
const RPI4_QEMU: &[u8] =
    include_bytes!("../../../tests/fixtures/dtb/bcm2711-rpi-4-b-qemu-raspi4b.dtb");

fn cpus(blob: &[u8]) -> Vec<firmware_cpus::CpuNode> {
    let tree = fdt::Fdt::new(blob).expect("fixture is a valid FDT");
    firmware_cpus::cpus(&tree).collect()
}

fn psci(blob: &[u8]) -> Option<String> {
    let tree = fdt::Fdt::new(blob).expect("fixture is a valid FDT");
    firmware_cpus::psci_method(&tree).map(str::to_owned)
}

/// The MPIDR table `firmware_smp::note` builds from these CPUs.
fn table(cpus: &[firmware_cpus::CpuNode]) -> MpidrTable<32> {
    let table = MpidrTable::new();
    assert_eq!(
        table.describe(cpus.iter().map(|cpu| cpu.affinity)),
        Ok(cpus.len())
    );
    table
}

fn aff(mpidr: u64, level: u32) -> u64 {
    let shift = if level == 3 { 32 } else { 8 * level };
    (mpidr >> shift) & 0xFF
}

#[test]
fn t8103_describes_two_clusters_with_spin_table_and_no_psci() {
    let cpus = cpus(T8103);
    assert_eq!(
        psci(T8103),
        None,
        "m1n1 trees start CPUs by spin table only"
    );
    let ids: Vec<u64> = cpus.iter().map(|cpu| cpu.affinity.unwrap()).collect();
    assert_eq!(
        ids,
        [0x0, 0x1, 0x2, 0x3, 0x1_0100, 0x1_0101, 0x1_0102, 0x1_0103]
    );
    // Icestorm (E) cores: cluster 0. Firestorm (P) cores: Aff1 = 1 (die
    // Aff2 = 1), Aff0 restarting at 0 — not indexable by Aff0 alone.
    for &id in &ids[..4] {
        assert_eq!((aff(id, 2), aff(id, 1)), (0, 0));
    }
    for (n, &id) in ids[4..].iter().enumerate() {
        assert_eq!((aff(id, 2), aff(id, 1), aff(id, 0)), (1, 1, n as u64));
    }
    // The upstream tree leaves `cpu-release-addr = <0 0>` for the loader.
    assert!(cpus.iter().all(|cpu| cpu.release_addr.is_none()));

    let table = table(&cpus);
    assert_eq!(table.mpidr(4), Some(0x1_0100));
    // A P-core's live MPIDR_EL1 (RES1 bit 31, MT clear) maps to its index.
    assert_eq!(table.cpu_index(0x8001_0102), Some(6));
    assert_eq!(table.cpu_index(0x8000_0002), Some(2));
    // Nothing in the tree's unrelated nodes changes under compaction.
    let mut blob = T8103.to_vec();
    assert_eq!(fdt_nops::strip(&mut blob), Ok(0));
    assert_eq!(blob, T8103);
}

#[test]
fn t8103_release_addresses_come_from_the_loader_filled_tree() {
    let cpus = cpus(T8103_M1N1);
    let release: Vec<u64> = cpus.iter().map(|cpu| cpu.release_addr.unwrap()).collect();
    let expected: Vec<u64> = (0..8).map(|i| 0x8_03a0_0010 + 64 * i).collect();
    assert_eq!(release, expected);
    // Same topology as the unfilled tree.
    let ids: Vec<_> = cpus.iter().map(|cpu| cpu.affinity).collect();
    let upstream: Vec<_> = self::cpus(T8103).iter().map(|cpu| cpu.affinity).collect();
    assert_eq!(ids, upstream);
}

#[test]
fn rpi4_firmware_tree_is_spin_table_at_d8_without_psci() {
    let cpus = cpus(RPI4);
    assert_eq!(psci(RPI4), None, "VideoCore armstub8 has no PSCI");
    assert_eq!(
        cpus.iter().map(|cpu| cpu.affinity).collect::<Vec<_>>(),
        [Some(0), Some(1), Some(2), Some(3)],
        "/cpus #address-cells = <1>"
    );
    assert_eq!(
        cpus.iter().map(|cpu| cpu.release_addr).collect::<Vec<_>>(),
        [Some(0xd8), Some(0xe0), Some(0xe8), Some(0xf0)]
    );
    assert_eq!(table(&cpus).cpu_index(0x8000_0003), Some(3));
}

/// Why the boot path compacts: the `fdt` crate cannot step over QEMU's
/// NOP'd-out nodes, so the `/cpus` walk itself panics on the raw tree.
#[test]
#[should_panic(expected = "bad node")]
fn qemu_raspi4b_tree_with_nops_breaks_the_fdt_crate() {
    let tree = fdt::Fdt::new(RPI4_QEMU).expect("header is valid");
    let _ = firmware_cpus::cpus(&tree).count();
}

#[test]
fn qemu_raspi4b_tree_parses_after_nop_compaction() {
    let mut blob = RPI4_QEMU.to_vec();
    let removed = fdt_nops::strip(&mut blob).expect("well-formed tree");
    assert!(removed > 0, "QEMU NOPs out PCIe/GENET");
    // Idempotent: nothing left to remove.
    assert_eq!(fdt_nops::strip(&mut blob.clone()), Ok(0));

    let tree = fdt::Fdt::new(&blob).expect("compacted tree is valid");
    assert!(tree.all_nodes().count() > 0);
    assert!(tree.find_compatible(&["brcm,bcm2711-genet-v5"]).is_none());
    assert!(tree.memory().regions().next().is_some(), "QEMU's /memory");
    assert_eq!(cpus(&blob), cpus(RPI4), "same CPUs as the firmware tree");
}

#[test]
fn malformed_structure_is_left_untouched() {
    let mut blob = RPI4_QEMU.to_vec();
    let off = u32::from_be_bytes(blob[8..12].try_into().unwrap()) as usize;
    let size = u32::from_be_bytes(blob[36..40].try_into().unwrap()) as usize;
    // Turn the final FDT_END into an unknown token.
    blob[off + size - 4..off + size].copy_from_slice(&7u32.to_be_bytes());
    let before = blob.clone();
    assert_eq!(
        fdt_nops::strip(&mut blob),
        Err(fdt_nops::StripError::Structure)
    );
    assert_eq!(blob, before);
    assert_eq!(
        fdt_nops::strip(&mut [0u8; 8]),
        Err(fdt_nops::StripError::Header)
    );
    assert_eq!(fdt_nops::total_size(&RPI4[..8]), Some(RPI4.len()));
}
