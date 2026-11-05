use super::*;

const PTL_DMA_BITS: u32 = 46;

#[test]
fn wb_writable_4k_leaf_matches_xe_encoding() {
    // PAT 2 = PAT1 only; PRESENT | RW; no page-size bit at level 0.
    let pte = encode_leaf(0x1234_5000, 0, PatIndex::WB, true, PTL_DMA_BITS).unwrap();
    assert_eq!(pte, 0x1234_5000 | PAGE_PRESENT | PAGE_RW | PPGTT_PTE_PAT1);
}

#[test]
fn read_only_leaf_has_no_rw_bit() {
    let pte = encode_leaf(0x2000, 0, PatIndex::UC, false, PTL_DMA_BITS).unwrap();
    assert_eq!(pte, 0x2000 | PAGE_PRESENT | PPGTT_PTE_PAT0 | PPGTT_PTE_PAT1);
}

#[test]
fn pat_bit2_moves_between_leaf_levels() {
    let pat = PatIndex::new(4).unwrap();
    let l0 = encode_leaf(0, 0, pat, true, PTL_DMA_BITS).unwrap();
    let l1 = encode_leaf(0, 1, pat, true, PTL_DMA_BITS).unwrap();
    assert_ne!(l0 & PPGTT_PTE_PAT2, 0);
    assert_eq!(l1 & PPGTT_PDE_PDPE_PAT2, PPGTT_PDE_PDPE_PAT2);
    // At level 1 bit 7 is the 2 MiB page-size bit, not PAT2.
    assert_eq!(l1 & PDE_PS_LEAF, PDE_PS_LEAF);
}

#[test]
fn wt_index_15_uses_pat3_and_low_bits() {
    let pte = encode_leaf(0, 0, PatIndex::WT, true, PTL_DMA_BITS).unwrap();
    let expect = PPGTT_PTE_PAT0 | PPGTT_PTE_PAT1 | PPGTT_PTE_PAT2 | XELPG_PPGTT_PTE_PAT3;
    assert_eq!(pte & !(PAGE_PRESENT | PAGE_RW), expect);
}

#[test]
fn pat_index_16_sets_pat4() {
    let pte = encode_leaf(0, 0, PatIndex::new(16).unwrap(), true, PTL_DMA_BITS).unwrap();
    assert_eq!(pte & XE2_PPGTT_PTE_PAT4, XE2_PPGTT_PTE_PAT4);
    assert_eq!(PatIndex::new(32), None);
}

#[test]
fn huge_leaves_require_their_alignment() {
    assert_eq!(
        encode_leaf(0x1000, 1, PatIndex::WB, true, PTL_DMA_BITS),
        Err(EncodeError::Misaligned)
    );
    assert!(encode_leaf(2 << 20, 1, PatIndex::WB, true, PTL_DMA_BITS).is_ok());
    assert_eq!(
        encode_leaf(0, 3, PatIndex::WB, true, PTL_DMA_BITS),
        Err(EncodeError::LevelHasNoLeaf)
    );
}

#[test]
fn addresses_beyond_the_dma_width_are_refused() {
    assert_eq!(
        encode_leaf(1 << 46, 0, PatIndex::WB, true, PTL_DMA_BITS),
        Err(EncodeError::AddressTooWide)
    );
    assert!(encode_leaf((1 << 46) - 0x1000, 0, PatIndex::WB, true, PTL_DMA_BITS).is_ok());
    assert_eq!(
        encode_leaf(1 << 52, 0, PatIndex::WB, true, 64),
        Err(EncodeError::AddressTooWide)
    );
}

#[test]
fn table_entries_carry_only_two_pat_bits() {
    let pde = encode_table(0x8000, PatIndex::UC, PTL_DMA_BITS).unwrap();
    assert_eq!(
        pde,
        0x8000 | PAGE_PRESENT | PAGE_RW | PPGTT_PTE_PAT0 | PPGTT_PTE_PAT1
    );
    assert_eq!(
        encode_table(0x8000, PatIndex::WT, PTL_DMA_BITS),
        Err(EncodeError::PatTooWideForPde)
    );
    assert_eq!(
        encode_table(0x8001, PatIndex::WB, PTL_DMA_BITS),
        Err(EncodeError::Misaligned)
    );
}

#[test]
fn integrated_gpu_entries_never_set_device_memory() {
    let pte = encode_leaf(0x4000, 0, PatIndex::WB, true, PTL_DMA_BITS).unwrap();
    assert_eq!(pte & PPGTT_PTE_DM, 0);
}
