use super::*;

const TOP: u64 = 0x7000_0000;

fn phdrs(n: usize) -> [u8; MAX_PHDRS * PHENT + PHENT] {
    let mut table = [0u8; MAX_PHDRS * PHENT + PHENT];
    for (i, byte) in table.iter_mut().enumerate().take(n * PHENT) {
        *byte = (i % 251) as u8;
    }
    table
}

fn word(block: &Block, addr: u64) -> u64 {
    let off = (addr - block.sp()) as usize;
    u64::from_le_bytes(block.bytes()[off..off + 8].try_into().unwrap())
}

/// Walk the block the way a libc start-up does and return the auxv entries.
fn auxv(block: &Block) -> [(u64, u64); MAX_AUX_PAIRS] {
    let mut at = block.sp();
    let argc = word(block, at);
    at += 8 * (1 + argc + 1);
    while word(block, at) != 0 {
        at += 8;
    }
    at += 8;
    let mut out = [(u64::MAX, 0); MAX_AUX_PAIRS];
    for slot in out.iter_mut() {
        *slot = (word(block, at), word(block, at + 8));
        at += 16;
        if slot.0 == AT_NULL {
            break;
        }
    }
    out
}

fn value(entries: &[(u64, u64)], key: u64) -> Option<u64> {
    entries.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn image(table: &[u8], random: Option<[u8; 16]>) -> Image<'_> {
    Image {
        phdrs: table,
        load_base: 0x4000_0000,
        entry: 0x4000_1234,
        random,
    }
}

#[test]
fn block_ends_at_top_with_aligned_sp_and_empty_argv_envp() {
    let table = phdrs(10);
    let block = build(TOP, &image(&table[..10 * PHENT], None)).unwrap();
    assert_eq!(block.sp() % 16, 0);
    assert_eq!(block.sp() + block.bytes().len() as u64, TOP);
    assert_eq!(word(&block, block.sp()), 0, "argc");
    assert_eq!(word(&block, block.sp() + 8), 0, "argv terminator");
    assert_eq!(word(&block, block.sp() + 16), 0, "envp terminator");
}

#[test]
fn auxv_points_at_an_exact_copy_of_the_program_headers() {
    let table = phdrs(7);
    let block = build(TOP, &image(&table[..7 * PHENT], None)).unwrap();
    let entries = auxv(&block);
    let phdr = value(&entries, AT_PHDR).unwrap();
    assert_eq!(phdr % 8, 0);
    assert_eq!(value(&entries, AT_PHENT), Some(PHENT as u64));
    assert_eq!(value(&entries, AT_PHNUM), Some(7));
    let off = (phdr - block.sp()) as usize;
    assert_eq!(&block.bytes()[off..off + 7 * PHENT], &table[..7 * PHENT]);
    assert!(phdr + (7 * PHENT) as u64 <= TOP);
}

#[test]
fn auxv_carries_page_size_load_base_and_entry() {
    let table = phdrs(3);
    let block = build(TOP, &image(&table[..3 * PHENT], None)).unwrap();
    let entries = auxv(&block);
    assert_eq!(value(&entries, AT_PAGESZ), Some(4096));
    assert_eq!(value(&entries, AT_BASE), Some(0x4000_0000));
    assert_eq!(value(&entries, AT_ENTRY), Some(0x4000_1234));
    assert_eq!(value(&entries, AT_RANDOM), None);
    assert!(entries.iter().any(|(k, _)| *k == AT_NULL), "terminated");
}

#[test]
fn random_bytes_are_present_only_with_entropy() {
    let table = phdrs(2);
    let seed = [0xA5u8; 16];
    let block = build(TOP, &image(&table[..2 * PHENT], Some(seed))).unwrap();
    let ptr = value(&auxv(&block), AT_RANDOM).unwrap();
    let off = (ptr - block.sp()) as usize;
    assert_eq!(&block.bytes()[off..off + 16], &seed);
    assert!(ptr + 16 <= TOP);
}

#[test]
fn the_largest_block_fits_its_bound() {
    let table = phdrs(MAX_PHDRS);
    let block = build(TOP, &image(&table[..MAX_PHDRS * PHENT], Some([1; 16]))).unwrap();
    assert!(block.bytes().len() <= MAX_BYTES);
    assert_eq!(value(&auxv(&block), AT_PHNUM), Some(MAX_PHDRS as u64));
}

#[test]
fn refuses_more_than_the_header_bound() {
    let table = phdrs(MAX_PHDRS + 1);
    let result = build(TOP, &image(&table[..(MAX_PHDRS + 1) * PHENT], None));
    assert_eq!(result.err(), Some(Error::TooManyPhdrs));
}

#[test]
fn refuses_a_partial_header_entry() {
    let table = phdrs(2);
    let result = build(TOP, &image(&table[..PHENT + 3], None));
    assert_eq!(result.err(), Some(Error::BadPhdrTable));
}

#[test]
fn refuses_an_unaligned_or_too_low_top() {
    let table = phdrs(1);
    assert_eq!(
        build(TOP - 8, &image(&table[..PHENT], None)).err(),
        Some(Error::BadTop)
    );
    assert_eq!(
        build(64, &image(&table[..PHENT], None)).err(),
        Some(Error::BadTop)
    );
}
