// SPDX-License-Identifier: MPL-2.0
//! Fixture tests for the records added in plan 261103-2218 phase 01.

use super::super::dmar::parse_dmar;
use super::super::fadt::parse_fadt;
use super::super::madt::parse_madt;
use super::super::mcfg::parse_mcfg;
use super::super::spcr::parse_spcr;
use super::*;

fn rec(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut record = vec![kind, (payload.len() + 2) as u8];
    record.extend_from_slice(payload);
    record
}

fn lapic(uid: u8, apic_id: u8, flags: u32) -> Vec<u8> {
    let mut p = vec![uid, apic_id];
    p.extend_from_slice(&flags.to_le_bytes());
    rec(0, &p)
}

fn ioapic(id: u8, address: u32, gsi_base: u32) -> Vec<u8> {
    let mut p = vec![id, 0];
    p.extend_from_slice(&address.to_le_bytes());
    p.extend_from_slice(&gsi_base.to_le_bytes());
    rec(1, &p)
}

fn x2apic(apic_id: u32, flags: u32, uid: u32) -> Vec<u8> {
    let mut p = vec![0, 0];
    p.extend_from_slice(&apic_id.to_le_bytes());
    p.extend_from_slice(&flags.to_le_bytes());
    p.extend_from_slice(&uid.to_le_bytes());
    rec(9, &p)
}

fn madt_table(records: &[Vec<u8>]) -> Vec<u8> {
    let mut table = vec![0u8; 44];
    table[36..40].copy_from_slice(&0xFEE0_0000u32.to_le_bytes());
    for record in records {
        table.extend_from_slice(record);
    }
    table
}

fn madt(table: &[u8]) -> (AcpiInfo, Box<AcpiTopology>) {
    let mut info = AcpiInfo::default();
    let mut topo = topology();
    parse_madt(table.as_ptr() as usize, table.len(), &mut info, &mut topo);
    (info, topo)
}

#[test]
fn madt_keeps_every_ioapic_usable_cpu_override_flag_and_nmi() {
    let iso = rec(2, &[0, 0, 2, 0, 0, 0, 0x0F, 0x00]); // IRQ0 → GSI2, level/low
    let nmi = rec(4, &[0xFF, 0x05, 0x00, 1]); // all CPUs, LINT1
    let mut x2nmi = vec![0, 0];
    x2nmi.extend_from_slice(&7u32.to_le_bytes());
    x2nmi.extend_from_slice(&[0, 0, 0, 0]);
    x2nmi[6] = 1;
    let table = madt_table(&[
        lapic(0, 0, 1),
        lapic(1, 1, 0), // absent socket: not usable, not recorded
        x2apic(300, 2, 7),
        ioapic(8, 0xFEC0_0000, 0),
        ioapic(9, 0xFEC0_1000, 24),
        iso,
        nmi,
        rec(0xA, &x2nmi[..10]),
    ]);
    let (info, topo) = madt(&table);

    assert_eq!(info.lapic_base, 0xFEE0_0000);
    // Legacy field keeps its established last-entry meaning.
    assert_eq!((info.ioapic_base, info.ioapic_gsi_base), (0xFEC0_1000, 24));
    assert_eq!(
        (info.irq_overrides[0], info.irq_override_flags[0]),
        (2, 0x000F)
    );
    assert_eq!(info.irq_overrides[1], 1);

    let cpus = topo.cpus.as_slice();
    assert_eq!(cpus.len(), 2);
    assert_eq!((cpus[0].apic_id, cpus[0].x2apic), (0, false));
    assert_eq!(
        (cpus[1].apic_id, cpus[1].acpi_uid, cpus[1].x2apic),
        (300, 7, true)
    );
    let ioapics = topo.ioapics.as_slice();
    assert_eq!((ioapics[0].id, ioapics[0].gsi_base), (8, 0));
    assert_eq!((ioapics[1].id, ioapics[1].gsi_base), (9, 24));
    let nmis = topo.lapic_nmis.as_slice();
    assert_eq!(
        (nmis[0].acpi_uid, nmis[0].lint, nmis[0].flags),
        (u32::MAX, 1, 5)
    );
    assert_eq!((nmis[1].acpi_uid, nmis[1].lint), (7, 1));
}

#[test]
fn madt_stops_at_a_bad_length_and_counts_overflow() {
    let mut table = madt_table(&[ioapic(1, 0xFEC0_0000, 0)]);
    table.extend_from_slice(&[1, 0xFF]); // runs past the end
    let (info, topo) = madt(&table);
    assert_eq!(info.ioapic_base, 0xFEC0_0000);
    assert_eq!(topo.ioapics.len(), 1);

    let many: Vec<Vec<u8>> = (0..20u32)
        .map(|i| ioapic(i as u8, 0xFEC0_0000 + i * 0x1000, i * 24))
        .collect();
    let (_, topo) = madt(&madt_table(&many));
    assert_eq!((topo.ioapics.len(), topo.ioapics.dropped()), (16, 4));
}

#[test]
fn mcfg_records_every_allocation_including_other_segments() {
    let mut table = vec![0u8; 44];
    for (base, segment, start, end) in [
        (0xE000_0000u64, 0u16, 0u8, 0xFFu8),
        (0xC000_0000, 1, 0, 0x7F),
    ] {
        table.extend_from_slice(&base.to_le_bytes());
        table.extend_from_slice(&segment.to_le_bytes());
        table.extend_from_slice(&[start, end, 0, 0, 0, 0]);
    }
    let mut info = AcpiInfo::default();
    let mut topo = topology();
    parse_mcfg(table.as_ptr() as usize, table.len(), &mut info, &mut topo);
    assert_eq!(info.ecam_base, 0xE000_0000);
    let allocs = topo.mcfg.as_slice();
    assert_eq!(allocs.len(), 2);
    assert_eq!(
        (allocs[1].base, allocs[1].segment, allocs[1].bus_end),
        (0xC000_0000, 1, 0x7F)
    );
}

fn scope(kind: u8, bus: u8, path: &[[u8; 2]]) -> Vec<u8> {
    let mut s = vec![kind, (6 + path.len() * 2) as u8, 0, 0, 0, bus];
    for pair in path {
        s.extend_from_slice(pair);
    }
    s
}

fn with_scopes(mut record: Vec<u8>, scopes: &[Vec<u8>]) -> Vec<u8> {
    for s in scopes {
        record.extend_from_slice(s);
    }
    let len = record.len() as u16;
    record[2..4].copy_from_slice(&len.to_le_bytes());
    record
}

fn rmrr_record(base: u64, limit: u64, scopes: &[Vec<u8>]) -> Vec<u8> {
    let mut r = vec![0u8; 24];
    r[0..2].copy_from_slice(&1u16.to_le_bytes());
    r[8..16].copy_from_slice(&base.to_le_bytes());
    r[16..24].copy_from_slice(&limit.to_le_bytes());
    with_scopes(r, scopes)
}

#[test]
fn dmar_records_units_scopes_rmrr_and_header_flags() {
    let deep: Vec<[u8; 2]> = (0..5).map(|i| [i, 0]).collect();
    let unit = with_scopes(
        drhd(0, 0, 0xFED9_1000),
        &[scope(1, 0, &[[0x1F, 0]]), scope(2, 0x80, &deep)],
    );
    let short_rmrr = vec![1u8, 0, 8, 0, 0, 0, 0, 0];
    let mut table = dmar_table(&[
        unit,
        rmrr_record(0x7A00_0000, 0x7A0F_FFFF, &[scope(1, 0, &[[0x14, 0]])]),
        short_rmrr,
        drhd(1, 0, 0xFED9_0000),
    ]);
    table[36] = 38;
    table[37] = 0b11;
    let mut info = AcpiInfo::default();
    let mut topo = topology();
    parse_dmar(table.as_ptr() as usize, table.len(), &mut info, &mut topo);

    // The DRHD after a short RMRR is still reached, and still wins.
    assert_eq!((info.dmar_base, info.dmar_units), (0xFED9_0000, 2));
    assert!(topo.dmar_intr_remap() && topo.dmar_x2apic_opt_out());
    assert_eq!(topo.dmar_host_address_width, 38);
    let units = topo.drhds.as_slice();
    assert_eq!(units.len(), 2);
    let scopes = units[0].scopes.as_slice();
    assert_eq!(
        (scopes[0].kind, scopes[0].path_len, scopes[0].path[0]),
        (1, 1, [0x1F, 0])
    );
    assert_eq!((scopes[1].start_bus, scopes[1].path_len), (0x80, 5));
    assert!(scopes[1].path_truncated());
    assert!(units[1].include_pci_all());
    let rmrrs = topo.rmrrs.as_slice();
    assert_eq!(rmrrs.len(), 1);
    assert_eq!((rmrrs[0].base, rmrrs[0].limit), (0x7A00_0000, 0x7A0F_FFFF));
    assert_eq!(rmrrs[0].scopes.as_slice()[0].path[0], [0x14, 0]);
}

fn gas(space: u8, width: u8, address: u64) -> [u8; 12] {
    let mut g = [0u8; 12];
    g[0] = space;
    g[1] = width;
    g[3] = 3;
    g[4..12].copy_from_slice(&address.to_le_bytes());
    g
}

/// A FADT of `length` bytes with legacy PM1a_CNT 0x604, PM timer 0x608
/// (len 4), SCI 9 and the given revision and flags.
fn fadt_table(revision: u8, length: usize, flags: u32) -> Vec<u8> {
    let mut t = vec![0u8; length];
    t[8] = revision;
    t[40..44].copy_from_slice(&0x7FE0_0000u32.to_le_bytes());
    t[46..48].copy_from_slice(&9u16.to_le_bytes());
    t[48..52].copy_from_slice(&0xB2u32.to_le_bytes());
    t[52] = 0xF1;
    t[64..68].copy_from_slice(&0x604u32.to_le_bytes());
    t[76..80].copy_from_slice(&0x608u32.to_le_bytes());
    t[89] = 2;
    t[91] = 4;
    t[112..116].copy_from_slice(&flags.to_le_bytes());
    t
}

fn fadt(table: &[u8]) -> Option<FadtInfo> {
    let mut info = AcpiInfo::default();
    parse_fadt(table.as_ptr() as usize, table.len(), &mut info);
    info.fadt
}

#[test]
fn fadt_rev1_uses_legacy_ports_and_offers_no_reset() {
    let f = fadt(&fadt_table(1, 116, FadtInfo::TMR_VAL_EXT)).expect("fadt");
    assert_eq!(
        (
            f.pm_timer.space_id,
            f.pm_timer.address,
            f.pm_timer.bit_width
        ),
        (1, 0x608, 32)
    );
    assert!(f.pm_timer_32bit());
    assert_eq!((f.pm1a_cnt.address, f.pm1a_cnt.bit_width), (0x604, 16));
    assert_eq!(
        (f.sci_int, f.smi_cmd, f.acpi_enable, f.dsdt),
        (9, 0xB2, 0xF1, 0x7FE0_0000)
    );
    assert!(!f.reset_supported());
    assert!(!f.msi_forbidden());
    assert_eq!(fadt(&vec![0u8; 100]), None);
}

#[test]
fn fadt_rev6_prefers_x_fields_and_reads_reset_msi_and_hw_reduced() {
    let flags = FadtInfo::RESET_REG_SUP | FadtInfo::HW_REDUCED_ACPI;
    let mut t = fadt_table(6, 276, flags);
    t[109..111].copy_from_slice(&(FadtInfo::BOOT_ARCH_MSI_NOT_SUPPORTED | 2).to_le_bytes());
    t[116..128].copy_from_slice(&gas(1, 8, 0xCF9));
    t[128] = 0x06;
    t[140..148].copy_from_slice(&0x7FF0_0000u64.to_le_bytes());
    t[208..220].copy_from_slice(&gas(0, 32, 0xFED0_0808));
    t[244..256].copy_from_slice(&gas(1, 8, 0x404));
    let f = fadt(&t).expect("fadt");
    assert_eq!((f.pm_timer.space_id, f.pm_timer.address), (0, 0xFED0_0808));
    assert_eq!(
        f.pm1a_cnt.address, 0x604,
        "zero X_ block falls back to legacy"
    );
    assert!(f.reset_supported());
    assert_eq!((f.reset_reg.address, f.reset_value), (0xCF9, 6));
    assert_eq!(f.dsdt, 0x7FF0_0000);
    assert!(f.msi_forbidden() && f.hw_reduced());
    assert_eq!(f.sleep_control.address, 0x404);

    // The same boot-arch bits are reserved before revision 3.
    let mut old = fadt_table(2, 244, 0);
    old[109..111].copy_from_slice(&FadtInfo::BOOT_ARCH_MSI_NOT_SUPPORTED.to_le_bytes());
    assert!(!fadt(&old).expect("fadt").msi_forbidden());
}

fn spcr_table(revision: u8, base: [u8; 12], gsi: u32, clock: u32) -> Vec<u8> {
    let mut t = vec![0u8; 88];
    t[8] = revision;
    t[36] = 0;
    t[40..52].copy_from_slice(&base);
    t[52] = 0b10;
    t[54..58].copy_from_slice(&gsi.to_le_bytes());
    t[58] = 7;
    t[64..68].copy_from_slice(&[0xFF; 4]);
    t[76..80].copy_from_slice(&clock.to_le_bytes());
    t
}

#[test]
fn spcr_reads_io_and_mmio_ports() {
    let mut info = AcpiInfo::default();
    let io = spcr_table(2, gas(1, 8, 0x2F8), 3, 1_843_200);
    parse_spcr(io.as_ptr() as usize, io.len(), &mut info);
    let s = info.spcr.expect("spcr");
    assert_eq!(
        (s.base.space_id, s.base.address, s.gsi, s.baud_code),
        (1, 0x2F8, 3, 7)
    );
    assert_eq!(
        (s.uart_clock_hz, s.pci_vendor_id),
        (0, 0xFFFF),
        "clock is rev 3+ only"
    );
    assert!(s.is_16550_family());

    let mmio = spcr_table(4, gas(0, 32, 0xFE03_2000), 20, 1_843_200);
    parse_spcr(mmio.as_ptr() as usize, mmio.len(), &mut info);
    let s = info.spcr.expect("spcr");
    assert_eq!(
        (s.base.space_id, s.base.address, s.uart_clock_hz),
        (0, 0xFE03_2000, 1_843_200)
    );
}

#[test]
fn parse_walk_reaches_fadt_spcr_and_madt_and_drops_a_corrupt_fadt() {
    let madt = seal_sdt(
        madt_table(&[lapic(0, 0, 1), ioapic(0, 0xFEC0_0000, 0)]),
        b"APIC",
        b"BOCHS ",
    );
    let fadt = seal_sdt(fadt_table(6, 276, 0), b"FACP", b"BOCHS ");
    let spcr = seal_sdt(spcr_table(2, gas(1, 8, 0x3F8), 4, 0), b"SPCR", b"BOCHS ");
    let (info, topo) = parse_image(&firmware_image(
        2,
        b"BOCHS ",
        &[madt.clone(), fadt.clone(), spcr],
    ));
    assert_eq!(info.fadt.map(|f| f.pm_timer.address), Some(0x608));
    assert_eq!(info.spcr.map(|s| s.base.address), Some(0x3F8));
    assert_eq!((topo.cpus.len(), topo.ioapics.len()), (1, 1));

    let mut corrupt = fadt;
    corrupt[100] ^= 0xFF;
    let (info, _) = parse_image(&firmware_image(2, b"BOCHS ", &[madt, corrupt]));
    assert_eq!(info.fadt, None);
    assert_ne!(info.ioapic_base, 0);
}
