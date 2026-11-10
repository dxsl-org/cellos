use super::*;

#[test]
fn log_params_encode_the_non_debug_buffer_sizes() {
    // VALID | HALF_FULL | CAPTURE_ALLOC_UNITS, crash 16K/4K-1 = 3, event 64K/4K-1 = 15,
    // capture 1M/1M-1 = 0, page number of the log in 31:12.
    assert_eq!(log_params(0x0090_0000), 0x0090_03f7);
    assert_eq!(GUC_LOG_SIZE, 4096 + 64 * 1024 + 16 * 1024 + 1024 * 1024);
}

#[test]
fn parameter_block_dwords() {
    let p = params(0x0090_0000, 0x00b0_0000, 0xb080, 4);
    assert_eq!(p[GUC_CTL_LOG_PARAMS], 0x0090_03f7);
    assert_eq!(p[GUC_CTL_WA], 0);
    assert_eq!(p[GUC_CTL_FEATURE], 0x10);
    assert_eq!(p[GUC_CTL_DEBUG], 0x40);
    assert_eq!(p[GUC_CTL_ADS], (0xb00 << 1));
    assert_eq!(p[GUC_CTL_DEVID], 0xb080_0004);
    assert!(p[6..].iter().all(|&w| w == 0));
}

#[test]
fn unlocked_wopcm_partition_for_an_8_mib_wopcm() {
    let layout = compute_layout(LNL_WOPCM_SIZE, 365_376, 0).unwrap();
    // base = ALIGN(0 + 16K, 16K); size = (8M - 36K - 16K) & ~0xfff.
    assert_eq!(
        layout,
        WopcmLayout {
            base: 0x4000,
            size: 0x7f_3000
        }
    );
}

#[test]
fn wopcm_layout_checks_follow_check_layout() {
    // A GuC image that cannot fit beside its reservations.
    assert_eq!(
        compute_layout(LNL_WOPCM_SIZE, 8 << 20, 0),
        Err(WopcmError::NoSpaceForGuc)
    );
    // A locked base that leaves no room for the HuC reservation below it.
    let low = WopcmLayout {
        base: 0x2000,
        size: 0x10_0000,
    };
    assert_eq!(
        check_layout(MAX_WOPCM_SIZE, low, 0x1000, 0),
        Err(WopcmError::NoSpaceForHuc)
    );
    // A locked region that runs into the hardware-context reservation.
    let high = WopcmLayout {
        base: 0x4000,
        size: 0x80_0000,
    };
    assert_eq!(
        check_layout(MAX_WOPCM_SIZE, high, 0x1000, 0),
        Err(WopcmError::InvalidLayout)
    );
}

#[test]
fn locked_registers_need_both_lock_bits() {
    assert_eq!(
        locked_layout(0x4001, 0x7f_3001),
        Some(WopcmLayout {
            base: 0x4000,
            size: 0x7f_3000
        })
    );
    assert_eq!(locked_layout(0x4000, 0x7f_3001), None);
    assert_eq!(locked_layout(0x4001, 0x7f_3000), None);
}
