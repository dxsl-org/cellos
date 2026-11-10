use super::*;

fn u32_at(blob: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(blob[offset..offset + 4].try_into().unwrap())
}

#[test]
fn layout_matches_the_upstream_struct_sizes() {
    assert_eq!(GUC_ADS_SIZE, 4572);
    assert_eq!(ADS_SCHEDULER_POLICIES, 4100);
    assert_eq!(ADS_GT_SYSTEM_INFO, 4104);
    assert_eq!(ADS_GOLDEN_CONTEXT_LRCA, 4116);
    assert_eq!(ADS_ENG_STATE_SIZE, 4180);
    assert_eq!(ADS_PRIVATE_DATA, 4244);
    assert_eq!(POLICIES_SIZE, 96);
    assert_eq!(SYSTEM_INFO_OFFSET, 4668);
    assert_eq!(SYSTEM_INFO_SIZE, 640);
    assert_eq!(REGSET_OFFSET, 21820);
    assert_eq!(GOLDEN_LRC_OFFSET, 24576);
    assert_eq!(PRIVATE_DATA_OFFSET, 40960);
    assert_eq!(RENDER_ENGINE_STATE_SIZE, 11904);
    assert_eq!(ads_size(0x2_1000), 40960 + 0x2_1000);
    assert_eq!(ads_size(1), 40960 + 4096);
}

#[test]
fn blob_fields_for_rcs0() {
    let ggtt = 0x0100_0000;
    let blob = build(ggtt, 0x8000, 1, 0x0003_0001);
    assert_eq!(blob.len(), 40960 + 0x8000);
    assert_eq!(u32_at(&blob, ADS_SCHEDULER_POLICIES), ggtt + 4572);
    assert_eq!(u32_at(&blob, ADS_GT_SYSTEM_INFO), ggtt + 4668);
    assert_eq!(u32_at(&blob, ADS_PRIVATE_DATA), ggtt + 40960);
    assert_eq!(u32_at(&blob, ADS_GOLDEN_CONTEXT_LRCA), ggtt + 24576);
    assert_eq!(u32_at(&blob, ADS_ENG_STATE_SIZE), 11904);
    // Other classes stay zero.
    assert_eq!(u32_at(&blob, ADS_GOLDEN_CONTEXT_LRCA + 4 * 4), 0);
    // Policies: dpc promote time, valid, max work items.
    assert_eq!(u32_at(&blob, POLICIES_OFFSET + 64), 500_000);
    assert_eq!(u32_at(&blob, POLICIES_OFFSET + 68), 1);
    assert_eq!(u32_at(&blob, POLICIES_OFFSET + 72), 15);
    // Mapping table: RCS0 maps to instance 0, every other slot invalid (32).
    assert_eq!(blob[SYSTEM_INFO_OFFSET], 0);
    assert_eq!(blob[SYSTEM_INFO_OFFSET + 1], 32);
    assert_eq!(blob[SYSTEM_INFO_OFFSET + 4 * 32], 32);
    // Engine masks then generic sysinfo (doorbells per SQIDI = 3 + 1).
    assert_eq!(u32_at(&blob, SYSTEM_INFO_OFFSET + 512), 1);
    assert_eq!(u32_at(&blob, SYSTEM_INFO_OFFSET + 512 + 4), 0);
    assert_eq!(u32_at(&blob, SYSTEM_INFO_OFFSET + 576 + 8), 4);
    // Register save/restore lists stay empty.
    assert!(blob[..4096].iter().all(|&b| b == 0));
}
