//! Host tests: the gate's decision table, the diff, the confirm rules, the
//! endpoint's sender rules and the store's absence rule.

use crate::diff;
use crate::gate::*;
use crate::store::{self, PinRead, PinStore};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use api::machine_facts::{MachineFactsRecord, FLAG_HYPERVISOR, PROFILE_SOURCE_CMDLINE};
use api::services::machine_pin::{PinState, Refusal};
use api::machine_facts::{
    FactString, PciFunctionId, ViMachineFactsV1, FLAG_ACPI_OEM, FLAG_DMAR_PRESENT, FLAG_PCI_ECAM,
    FLAG_SMBIOS, FLAG_VTD_ACTIVE, PROFILE_SOURCE_DEFAULT,
};

/// A fake store recording every write; `read` answers `answer`.
struct FakeStore {
    answer: Option<PinRead>,
    writable: bool,
    written: Vec<Vec<u8>>,
}

impl FakeStore {
    fn new(answer: PinRead, writable: bool) -> Self {
        Self {
            answer: Some(answer),
            writable,
            written: Vec::new(),
        }
    }
}

impl PinStore for FakeStore {
    fn read(&mut self) -> PinRead {
        self.answer.take().expect("one read per gate")
    }
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.written.push(bytes.to_vec());
        self.writable
    }
}

fn pci(vendor: u16, device: u16, class: u8) -> PciFunctionId {
    PciFunctionId {
        vendor,
        device,
        class,
        subclass: 0,
        prog_if: 0,
    }
}

fn facts(slug: &[u8], source: u8, extra: &[PciFunctionId]) -> ViMachineFactsV1 {
    let mut devices = vec![pci(0x1b36, 0x0010, 1), pci(0x8086, 0x29c0, 6)];
    devices.extend_from_slice(extra);
    devices.sort_unstable();
    ViMachineFactsV1 {
        flags: FLAG_ACPI_OEM
            | FLAG_SMBIOS
            | FLAG_DMAR_PRESENT
            | FLAG_VTD_ACTIVE
            | FLAG_HYPERVISOR
            | FLAG_PCI_ECAM,
        profile_source: source,
        slug: FactString::new(slug).unwrap(),
        acpi_oem_revision: 1,
        acpi_oem_table_id: *b"BXPC    ",
        acpi_oem_id: *b"BOCHS ",
        smbios_uuid: [0; 16],
        smbios_manufacturer: FactString::new(b"QEMU").unwrap(),
        smbios_product: FactString::new(b"Standard PC").unwrap(),
        pci: devices,
    }
}

fn record(facts: &ViMachineFactsV1) -> MachineFactsRecord {
    let bytes = facts.encode(|body| [body.len() as u8; 32]).unwrap();
    MachineFactsRecord::decode(&bytes).unwrap()
}

fn pc() -> MachineFactsRecord {
    record(&facts(b"x86_64-pc", PROFILE_SOURCE_DEFAULT, &[]))
}

#[test]
fn absent_pin_on_a_healthy_store_is_recorded_once() {
    let mut store = FakeStore::new(PinRead::Absent, true);
    let gate = evaluate(Some(pc()), &mut store);
    assert_eq!(gate.state, PinState::FirstBootRecorded);
    assert_eq!(store.written, vec![pc().bytes().to_vec()]);
    assert!(!gate.safe_mode());
    assert!(gate.lines[0].starts_with("first boot — recorded "));
}

#[test]
fn a_failed_first_write_is_store_unavailable() {
    let mut store = FakeStore::new(PinRead::Absent, false);
    let gate = evaluate(Some(pc()), &mut store);
    assert_eq!(gate.state, PinState::StoreUnavailable);
    assert!(gate.safe_mode(), "PC profile");
}

#[test]
fn equal_bytes_match_without_writing() {
    let mut store = FakeStore::new(PinRead::Present(pc().bytes().to_vec()), true);
    let gate = evaluate(Some(pc()), &mut store);
    assert_eq!(gate.state, PinState::Match);
    assert!(store.written.is_empty());
    assert!(!gate.safe_mode());
}

#[test]
fn a_changed_machine_is_a_mismatch_with_a_field_diff_and_no_write() {
    let pinned = pc();
    let current = record(&facts(
        b"x86_64-pc",
        PROFILE_SOURCE_DEFAULT,
        &[pci(0x8086, 0x100e, 2)],
    ));
    let mut store = FakeStore::new(PinRead::Present(pinned.bytes().to_vec()), true);
    let gate = evaluate(Some(current), &mut store);
    assert_eq!(gate.state, PinState::Mismatch);
    assert!(store.written.is_empty(), "never re-pinned automatically");
    assert!(gate.safe_mode());
    assert!(
        gate.lines
            .iter()
            .any(|line| line == "pci +8086:100e 02:00:00"),
        "{:?}",
        gate.lines
    );
}

#[test]
fn unreadable_records_and_unknown_versions_are_safe_mode_not_first_boot() {
    let mut unknown_version = pc().bytes().to_vec();
    unknown_version[0] = 2;
    for answer in [
        PinRead::Present(vec![0xAB; 40]),
        PinRead::Present(unknown_version),
        PinRead::Oversized,
    ] {
        let mut store = FakeStore::new(answer, true);
        let gate = evaluate(Some(pc()), &mut store);
        assert_eq!(gate.state, PinState::RecordUnreadable);
        assert!(store.written.is_empty());
        assert!(gate.safe_mode());
    }
}

#[test]
fn an_unavailable_store_is_never_written_and_only_q35_continues() {
    let q35 = record(&facts(b"qemu-q35-x86_64", PROFILE_SOURCE_CMDLINE, &[]));
    let mut store = FakeStore::new(PinRead::Unavailable("/srv not mounted"), true);
    let gate = evaluate(Some(q35), &mut store);
    assert_eq!(gate.state, PinState::StoreUnavailable);
    assert!(store.written.is_empty());
    assert!(!gate.safe_mode(), "q35 warns and continues");

    let mut store = FakeStore::new(PinRead::Unavailable("/srv not mounted"), true);
    assert!(
        evaluate(Some(pc()), &mut store).safe_mode(),
        "PC profile is strict"
    );

    let mut no_hv = facts(b"qemu-q35-x86_64", PROFILE_SOURCE_CMDLINE, &[]);
    no_hv.flags &= !FLAG_HYPERVISOR;
    let mut store = FakeStore::new(PinRead::Unavailable("x"), true);
    assert!(evaluate(Some(record(&no_hv)), &mut store).safe_mode());
}

#[test]
fn no_facts_is_safe_mode_and_touches_no_store() {
    let mut store = FakeStore::new(PinRead::Absent, true);
    let gate = evaluate(None, &mut store);
    assert_eq!(gate.state, PinState::FactsUnavailable);
    assert!(store.answer.is_some() && store.written.is_empty());
    assert!(gate.safe_mode());
}

#[test]
fn diff_names_each_changed_field_and_escapes_firmware_strings() {
    let old = facts(
        b"qemu-q35-x86_64",
        PROFILE_SOURCE_CMDLINE,
        &[pci(0x8086, 0x100e, 2)],
    );
    let mut new = old.clone();
    new.flags &= !(FLAG_DMAR_PRESENT | FLAG_VTD_ACTIVE);
    new.profile_source = PROFILE_SOURCE_DEFAULT;
    new.smbios_product = FactString::new(b"a\"b\n\xff").unwrap();
    new.pci.retain(|entry| entry.device != 0x100e);
    assert_eq!(
        diff::lines(&old, &new),
        vec![
            String::from("changed profile.source: cmdline -> default"),
            String::from("changed smbios.product: \"Standard PC\" -> \"a\\\"b\\x0a\\xff\""),
            String::from("changed dmar.present: 1 -> 0"),
            String::from("changed vtd.active: 1 -> 0"),
            String::from("pci -8086:100e 02:00:00"),
        ]
    );
}

#[test]
fn pci_lines_are_capped() {
    let old = facts(b"x86_64-pc", PROFILE_SOURCE_DEFAULT, &[]);
    let extra: Vec<_> = (0..20).map(|n| pci(0x8086, 0x1000 + n, 2)).collect();
    let new = facts(b"x86_64-pc", PROFILE_SOURCE_DEFAULT, &extra);
    let lines = diff::lines(&old, &new);
    assert_eq!(lines.len(), diff::PCI_LINES_MAX + 1);
    assert_eq!(lines.last().unwrap(), "pci …and 4 more");
    assert_eq!(lines[0], "pci +8086:1000 02:00:00");
}

#[test]
fn duplicate_functions_diff_as_a_multiset() {
    let old = facts(
        b"x86_64-pc",
        PROFILE_SOURCE_DEFAULT,
        &[pci(0x8086, 0x100e, 2)],
    );
    let new = facts(
        b"x86_64-pc",
        PROFILE_SOURCE_DEFAULT,
        &[pci(0x8086, 0x100e, 2), pci(0x8086, 0x100e, 2)],
    );
    assert_eq!(
        diff::lines(&old, &new),
        vec![String::from("pci +8086:100e 02:00:00")]
    );
}

#[test]
fn confirm_needs_safe_mode_then_the_shell_then_the_current_digest() {
    let digest = [7; 32];
    assert_eq!(
        authorize_confirm(false, true, digest, Some(digest)),
        Err(Refusal::NotSafeMode)
    );
    assert_eq!(
        authorize_confirm(true, false, digest, Some(digest)),
        Err(Refusal::NotConsoleShell)
    );
    assert_eq!(
        authorize_confirm(true, true, [8; 32], Some(digest)),
        Err(Refusal::DigestStale)
    );
    assert_eq!(
        authorize_confirm(true, true, digest, None),
        Err(Refusal::DigestStale)
    );
    assert_eq!(authorize_confirm(true, true, digest, Some(digest)), Ok(()));
}

/// Scripted VFS: `stat`/`list_dir` answers per path; reads return `file`.
struct FakeVfs {
    stats: Vec<(&'static str, Result<bool, ostd::ViError>)>,
    lists: Vec<(&'static str, Result<&'static [u8], ostd::ViError>)>,
    file: Result<Vec<u8>, ostd::ViError>,
}

impl store::VfsOps for FakeVfs {
    fn stat(&mut self, path: &str) -> ostd::ViResult<(u64, bool)> {
        let (_, answer) = self
            .stats
            .iter()
            .find(|(p, _)| *p == path)
            .expect("scripted stat");
        answer.clone().map(|is_dir| (0, is_dir))
    }
    fn list_dir(&mut self, path: &str) -> ostd::ViResult<Vec<u8>> {
        let (_, answer) = self
            .lists
            .iter()
            .find(|(p, _)| *p == path)
            .expect("scripted list");
        answer.clone().map(<[u8]>::to_vec)
    }
    fn read_file(&mut self, _: &str, _: usize) -> ostd::ViResult<Vec<u8>> {
        self.file.clone()
    }
    fn mkdir(&mut self, _: &str) -> ostd::ViResult<()> {
        panic!("reading never creates")
    }
    fn write_file(&mut self, _: &str, _: &[u8]) -> ostd::ViResult<()> {
        panic!("reading never writes")
    }
}

fn vfs(dir: Result<bool, ostd::ViError>) -> FakeVfs {
    FakeVfs {
        stats: vec![("/srv", Ok(true)), ("/srv/.cellos-machine", dir)],
        lists: Vec::new(),
        file: Ok(vec![1, 2, 3]),
    }
}

/// The VFS answers IO for an absent path and for a failing backend alike, so
/// a failed stat of the pin directory is absence only when a complete `/srv`
/// listing lacks it; anything else is `Unavailable`, which never re-pins.
#[test]
fn only_a_complete_listing_without_the_entry_proves_absence() {
    use ostd::ViError::IO;
    use store::{read_pin, PinRead};

    let mut absent = vfs(Err(IO));
    absent.lists.push(("/srv", Ok(b"d:cellos\nf:notes\n")));
    assert_eq!(read_pin(&mut absent), PinRead::Absent);

    let mut transient = vfs(Err(IO));
    transient.lists.push(("/srv", Ok(b"d:.cellos-machine\n")));
    assert!(
        matches!(read_pin(&mut transient), PinRead::Unavailable(_)),
        "listed but stat failed"
    );

    let mut unlistable = vfs(Err(IO));
    unlistable.lists.push(("/srv", Err(IO)));
    assert!(matches!(read_pin(&mut unlistable), PinRead::Unavailable(_)));

    let long: &'static [u8] = Vec::leak(vec![b'x'; 3_800]);
    let mut truncated = vfs(Err(IO));
    truncated.lists.push(("/srv", Ok(long)));
    assert!(
        matches!(read_pin(&mut truncated), PinRead::Unavailable(_)),
        "possibly truncated listing"
    );

    let mut no_file = vfs(Ok(true));
    no_file
        .lists
        .push(("/srv/.cellos-machine", Ok(b"f:pin.tmp\n")));
    assert_eq!(read_pin(&mut no_file), PinRead::Absent);

    let mut dir_unlistable = vfs(Ok(true));
    dir_unlistable.lists.push(("/srv/.cellos-machine", Err(IO)));
    assert!(matches!(
        read_pin(&mut dir_unlistable),
        PinRead::Unavailable(_)
    ));

    let mut present = vfs(Ok(true));
    present.lists.push(("/srv/.cellos-machine", Ok(b"f:pin\n")));
    assert_eq!(read_pin(&mut present), PinRead::Present(vec![1, 2, 3]));

    let mut unreadable = vfs(Ok(true));
    unreadable
        .lists
        .push(("/srv/.cellos-machine", Ok(b"f:pin\n")));
    unreadable.file = Err(IO);
    assert!(matches!(read_pin(&mut unreadable), PinRead::Unavailable(_)));

    let mut unmounted = vfs(Ok(true));
    unmounted.stats[0].1 = Err(IO);
    assert_eq!(
        read_pin(&mut unmounted),
        PinRead::Unavailable("/srv not mounted")
    );
}

#[test]
fn init_tid_comes_from_the_command_line_and_never_zero() {
    use crate::endpoint::init_tid;
    let args = |list: &[&str]| list.iter().map(|s| String::from(*s)).collect::<Vec<_>>();
    assert_eq!(init_tid(&args(&["init=2"])), Some(2));
    assert_eq!(init_tid(&args(&["x", "init=17"])), Some(17));
    for bad in [&[][..], &["init=0"], &["init="], &["init=x"], &["tid=2"]] {
        assert_eq!(init_tid(&args(bad)), None, "{bad:?}");
    }
}

#[test]
fn only_init_names_the_console_shell() {
    use crate::endpoint::notice_accepted;
    assert!(notice_accepted(Some(2), 2, 40));
    assert!(!notice_accepted(Some(2), 3, 40), "another sender");
    assert!(!notice_accepted(None, 2, 40), "init unknown");
    assert!(!notice_accepted(Some(2), 2, 0), "no shell");
}
