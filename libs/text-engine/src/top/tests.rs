use super::*;
use api::syscall::ProcessInfoV2;

fn row(
    id: u64,
    state: u32,
    name: &str,
    sample_ticks: u64,
    cpu_run_ticks: u64,
    heap_bytes: u64,
    owned_bytes: u64,
) -> ProcessInfoV2 {
    let mut info = ProcessInfoV2 {
        id,
        state,
        reserved0: 0,
        name: [0u8; 32],
        sample_ticks,
        cpu_run_ticks,
        heap_bytes,
        owned_bytes,
    };
    let bytes = name.as_bytes();
    info.name[..bytes.len()].copy_from_slice(bytes);
    info
}

#[test]
fn parse_batch_defaults_to_single_sample_when_count_missing() {
    let options = parse_options(&["-b"]).expect("parse batch");
    assert!(options.batch);
    assert_eq!(options.count, None);
    assert_eq!(options.delay_ticks, TIMER_HZ);
    assert_eq!(options.sort, SortKey::Cpu);
}

#[test]
fn parse_supports_sort_count_delay_and_show_all() {
    let options =
        parse_options(&["-a", "-b", "-n", "2", "-d", "3", "-o", "mem"]).expect("parse full args");
    assert!(options.show_all);
    assert!(options.batch);
    assert_eq!(options.count, Some(2));
    assert_eq!(options.delay_ticks, 3 * TIMER_HZ);
    assert_eq!(options.sort, SortKey::Mem);
}

#[test]
fn cpu_sort_uses_delta_and_clamps_to_100_percent() {
    let previous = [
        row(7, 1, "hot", 100, 10, 4096, 8192),
        row(9, 2, "idle", 100, 1, 2048, 4096),
    ];
    let current = [
        row(7, 1, "hot", 120, 40, 4096, 8192),
        row(9, 2, "idle", 120, 2, 2048, 4096),
    ];
    let rows = build_rows(&previous, &current, false, SortKey::Cpu);
    assert_eq!(rows[0].pid, 7);
    assert_eq!(rows[0].cpu_tenths, 1000);
    assert_eq!(rows[1].cpu_tenths, 50);
}

#[test]
fn mem_sort_uses_owned_bytes_and_hides_dead_by_default() {
    let previous = [
        row(1, 0, "alive", 10, 1, 1024, 4096),
        row(2, 3, "dead", 10, 1, 1024, 16384),
    ];
    let current = [
        row(1, 0, "alive", 20, 2, 1024, 4096),
        row(2, 3, "dead", 20, 2, 1024, 16384),
    ];
    let rows = build_rows(&previous, &current, false, SortKey::Mem);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].mem_bytes, 4096);
}

#[test]
fn show_all_keeps_dead_rows_and_name_sort_is_ascending() {
    let previous = [
        row(1, 0, "zed", 10, 1, 1024, 4096),
        row(2, 3, "alpha", 10, 1, 1024, 2048),
    ];
    let current = [
        row(1, 0, "zed", 20, 2, 1024, 4096),
        row(2, 3, "alpha", 20, 2, 1024, 2048),
    ];
    let rows = build_rows(&previous, &current, true, SortKey::Name);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].name, "alpha");
    assert_eq!(rows[1].name, "zed");
}

#[test]
fn format_bytes_uses_binary_units() {
    assert_eq!(format_bytes(999), "999B");
    assert_eq!(format_bytes(1024), "1K");
    assert_eq!(format_bytes(2 * 1024 * 1024), "2M");
}
