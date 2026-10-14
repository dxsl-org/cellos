//! Private legacy VT-d software structures, not hardware publication or authority.
//! Routing references are independent of SLPT leaf mappings and never add aliases.
use crate::sync::Spinlock;
const NODES: usize = 128;
const EDGES: usize = 4096;
const SLOTS: usize = 256;
const ADDRESS: u64 = 0x000f_ffff_ffff_f000;
const UNSUPPORTED: u64 = 1;
const CAPACITY: u64 = 2;
const MALFORMED: u64 = 4;
const HISTORY: u64 = 8;
const ROOTS: u64 = 16;

/// Legacy 128-bit entries only: root->context or AW39 context->SLPT.
/// P=0 metadata is inert; unsupported present TT/AW/flags are never guessed.
pub(crate) fn descriptor(kind: u64, lo: u64, hi: u64) -> Result<Option<(u64, u64, u64)>, u64> {
    if !matches!(kind, 5 | 6) { return Err(UNSUPPORTED); }
    if lo & 1 == 0 { return Ok(None); }
    if lo & !(ADDRESS | 1) != 0 { return Err(UNSUPPORTED); }
    let target = lo & ADDRESS;
    if target == 0 { return Err(MALFORMED); }
    if kind == 5 {
        if hi != 0 { return Err(UNSUPPORTED); }
        Ok(Some((1, target, 0)))
    } else {
        if hi & !0x00ff_ff01 != 0 || hi & 0xff != 1 { return Err(UNSUPPORTED); }
        let did = (hi >> 8) & 0xffff;
        if did == 0 { return Err(MALFORMED); }
        Ok(Some((2, target, did)))
    }
}

pub(crate) struct Snapshot {
    pub(crate) source: [u64; 16],
    pub(crate) nodes: [[u64; 8]; NODES],
    pub(crate) edges: [[u64; 8]; EDGES],
}
impl Snapshot {
    pub(crate) const fn new() -> Self {
        Self { source: [0; 16], nodes: [[0; 8]; NODES], edges: [[0; 8]; EDGES] }
    }
    fn refuse(&mut self, reason: u64) {
        self.source[5] = 0; self.source[6] = 0; self.source[7] = 0;
        self.source[10] = 4; self.source[11] |= reason;
    }
}

fn identity(row: &[u64; 8], incarnation: &mut impl FnMut(u64) -> Option<u64>) -> Result<(), u64> {
    if row[0] == 0 || row[1] == 0 || row[1] & !ADDRESS != 0 || row[3] == 0 {
        return Err(MALFORMED);
    }
    if incarnation(row[1]) != Some(row[3]) { return Err(HISTORY); }
    Ok(())
}
fn target<'a>(roots: &'a [[u64; 8]], pa: u64, role: u64,
    incarnation: &mut impl FnMut(u64) -> Option<u64>) -> Result<&'a [u64; 8], u64> {
    let row = roots.iter().find(|row| row[1] == pa).ok_or(MALFORMED)?;
    if row[2] != if role == 1 { 6 } else { 4 } { return Err(MALFORMED); }
    identity(row, incarnation)?;
    Ok(row)
}

/// No publication until all identities, count/fill words, and registry joins agree.
/// The read callback uses a 128-bit slot index, not a u64 word index.
pub(crate) fn walk(snapshot: &mut Snapshot, roots: &[[u64; 8]], references: &[[u64; 5]],
    mut read: impl FnMut(u64, usize) -> Option<[u64; 2]>,
    mut incarnation: impl FnMut(u64) -> Option<u64>) {
    let result = (|| -> Result<(), u64> {
        let mut nodes = 0usize;
        let mut root_count = 0usize;
        let mut context_count = 0usize;
        for (index, row) in roots.iter().enumerate() {
            if row[0] == 0 || !(1..=6).contains(&row[2])
                || roots[..index].iter().any(|before| before[0] == row[0] || before[1] == row[1]) {
                return Err(ROOTS);
            }
            if !matches!(row[2], 5 | 6) { continue; }
            identity(row, &mut incarnation)?;
            if nodes == NODES { return Err(CAPACITY); }
            snapshot.nodes[nodes] = *row;
            nodes += 1;
            if row[2] == 5 { root_count += 1; } else { context_count += 1; }
        }
        let mut edges = 0usize;
        for row in snapshot.nodes.iter().take(nodes) {
            // Validate the actual active-frame incarnation before each structure read.
            identity(row, &mut incarnation)?;
            for index in 0..SLOTS {
                let [lo, hi] = read(row[1], index).ok_or(MALFORMED)?;
                let Some((role, pa, did)) = descriptor(row[2], lo, hi)? else { continue; };
                let target = target(roots, pa, role, &mut incarnation)?;
                edges = edges.checked_add(1).ok_or(CAPACITY)?;
                snapshot.source[13] = edges as u64;
                if edges <= EDGES {
                    snapshot.edges[edges - 1] = [row[1], index as u64, role, lo, hi, pa, target[0], did];
                }
            }
        }
        if edges > EDGES { return Err(CAPACITY); }
        let mut filled = 0usize;
        for row in snapshot.nodes.iter().take(nodes) {
            identity(row, &mut incarnation)?;
            for index in 0..SLOTS {
                let [lo, hi] = read(row[1], index).ok_or(MALFORMED)?;
                let Some((role, pa, did)) = descriptor(row[2], lo, hi)? else { continue; };
                let target = target(roots, pa, role, &mut incarnation)?;
                if filled >= edges || snapshot.edges[filled]
                    != [row[1], index as u64, role, lo, hi, pa, target[0], did] {
                    return Err(MALFORMED);
                }
                filled += 1;
            }
        }
        if filled != edges { return Err(MALFORMED); }
        for reference in references.iter().filter(|row| row[1] == 3) {
            let context = roots.iter().find(|row| row[0] == reference[0]).ok_or(ROOTS)?;
            if context[2] != 6 || reference[2] >= SLOTS as u64 || reference[3] != 0
                || !snapshot.edges[..edges].iter().any(|edge| edge[0] == reference[4]
                    && edge[1] == reference[2] && edge[2] == 1
                    && edge[5] == context[1] && edge[6] == context[0]) {
                return Err(ROOTS);
            }
        }
        snapshot.source[5] = root_count as u64;
        snapshot.source[6] = context_count as u64;
        snapshot.source[7] = edges as u64;
        snapshot.source[10] = 1;
        snapshot.source[11] = 0;
        Ok(())
    })();
    if let Err(reason) = result { snapshot.refuse(reason); }
}

static SNAPSHOTS: Spinlock<[Snapshot; 6]> = Spinlock::new([const { Snapshot::new() }; 6]);
pub(crate) fn static_bytes() -> usize { core::mem::size_of_val(&SNAPSHOTS) }
#[cfg(not(all(test, not(target_os = "none"))))]
pub(crate) fn capture(sample: usize, metrics: &[crate::p01_resources::Metric; 16]) {
    let mut storage = SNAPSHOTS.lock();
    let Some(snapshot) = sample.checked_sub(1).and_then(|index| storage.get_mut(index)) else { return; };
    if snapshot.source[0] != 0 {
        crate::p01_measurement::invalidate(crate::p01_measurement::PROTOCOL); return;
    }
    snapshot.source = [1, sample as u64, metrics[1].epoch_before, metrics[1].value,
        0, 0, 0, 0, NODES as u64, EDGES as u64, 4, ROOTS, 1, 0, 0, 0];
    crate::p01_noncpu::visit_structure_snapshot(sample, |source, roots, references| {
        snapshot.source[4] = source[2];
        snapshot.source[14] = source[6]; snapshot.source[15] = source[7];
        if source[0] != 2 || source[10] != 1 || source[11] != 0 { return; }
        snapshot.source[11] = 0;
        if !crate::p01_noncpu::heap_cutoff_matches(metrics) { snapshot.refuse(HISTORY); return; }
        let frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
        let Some(allocator) = frames.as_ref() else { snapshot.refuse(HISTORY); return; };
        if metrics[1].status != 1 || metrics[1].epoch_before != metrics[1].epoch_after
            || source[3] != snapshot.source[2] || source[4] != snapshot.source[3]
            || allocator.p01_snapshot().0 != source[3] || allocator.used_frames() as u64 != source[4] {
            snapshot.refuse(HISTORY); return;
        }
        walk(snapshot, roots, references, |pa, index| {
            // identity() checks active membership/incarnation before entering this callback.
            let ptr = crate::memory::frame::phys_to_virt(pa as usize) as *const u64;
            Some(unsafe { [ptr.add(index * 2).read_volatile(), ptr.add(index * 2 + 1).read_volatile()] })
        }, |pa| {
            if !allocator.p01_allocated(pa as usize) { return None; }
            crate::p01_retained::physical_incarnation_at(pa, source[3])
        });
    });
}
pub(crate) fn source(sample: usize) -> Option<[u64; 16]> {
    let storage = SNAPSHOTS.lock(); let snapshot = storage.get(sample.checked_sub(1)?)?;
    (snapshot.source[0] != 0).then_some(snapshot.source)
}
pub(crate) fn node(sample: usize, index: usize) -> Option<[u64; 8]> {
    let storage = SNAPSHOTS.lock(); let snapshot = storage.get(sample.checked_sub(1)?)?;
    (snapshot.source[10] == 1 && index < (snapshot.source[5] + snapshot.source[6]) as usize)
        .then(|| snapshot.nodes[index])
}
pub(crate) fn edge(sample: usize, index: usize) -> Option<[u64; 8]> {
    let storage = SNAPSHOTS.lock(); let snapshot = storage.get(sample.checked_sub(1)?)?;
    (snapshot.source[10] == 1 && index < snapshot.source[7] as usize).then(|| snapshot.edges[index])
}

#[cfg(test)]
mod tests {
    use super::*;
    static TEST_SNAPSHOT: Spinlock<Snapshot> = Spinlock::new(Snapshot::new());
    fn row(token: u64, pa: u64, kind: u64) -> [u64; 8] { [token, pa, kind, 9, 2, pa + 0x100000, 7, 0] }
    fn reset(snapshot: &mut Snapshot) { snapshot.source = [0; 16]; }
    #[test]
    fn geometry_rejects_unsupported_tt_aw_and_flags_without_guessing() {
        assert_eq!(descriptor(5, 8193, 0), Ok(Some((1, 8192, 0))));
        assert_eq!(descriptor(6, 12289, (65535 << 8) | 1), Ok(Some((2, 12288, 65535))));
        for (kind, lo, hi) in [(5, 8193, 1), (5, 8195, 0), (6, 12293, 257),
            (6, 12289, 258), (6, 12289, 257 | (1 << 24)), (6, 12289 | (1 << 52), 257)] {
            assert_eq!(descriptor(kind, lo, hi), Err(UNSUPPORTED));
        }
        assert_eq!(descriptor(6, 12289, 1), Err(MALFORMED));
        assert_eq!(descriptor(5, 1, 0), Err(MALFORMED));
        assert_eq!(descriptor(6, u64::MAX - 1, u64::MAX), Ok(None));
    }
    #[test]
    fn exact_root_rows_and_uncompressed_last_slot_edges_join_registry_births() {
        let mut snapshot = TEST_SNAPSHOT.lock(); reset(&mut snapshot);
        let roots = [row(1, 4096, 5), row(2, 8192, 6), row(3, 12288, 4)];
        let references = [[2, 3, 255, 0, 4096], [3, 4, 42, 1, 4096]];
        walk(&mut snapshot, &roots, &references, |pa, index| Some(match (pa, index) {
            (4096, 255) => [8193, 0], (8192, 254 | 255) => [12289, 257], _ => [0, 0],
        }), |_| Some(9));
        assert_eq!(snapshot.source[10], 1);
        assert_eq!(&snapshot.source[5..8], &[1, 1, 3]);
        assert_eq!(&snapshot.nodes[..2], &roots[..2]);
        assert_eq!(snapshot.edges[0], [4096, 255, 1, 8193, 0, 8192, 2, 0]);
        assert_eq!(snapshot.edges[2], [8192, 255, 2, 12289, 257, 12288, 3, 1]);
    }
    #[test]
    fn count_fill_checks_both_present_words_and_last_absent_slot_transitions() {
        let roots = [row(1, 4096, 5), row(2, 8192, 6), row(3, 12288, 4)];
        for mutation in 0..5 {
            let mut snapshot = TEST_SNAPSHOT.lock(); reset(&mut snapshot);
            let mut context_pass = 0;
            walk(&mut snapshot, &roots, &[], |pa, index| {
                if pa == 8192 && index == 0 { context_pass += 1; }
                Some(match (pa, index) {
                    (4096, 0) => [8193, 0],
                    (8192, 255) => match (mutation, context_pass == 2) {
                        (0, true) => [12289, 513], // DID-only high-word mutation.
                        (1, true) => [12289 | 4, 257],
                        (2, true) | (3, false) => [0, 0], // disappearing/new last entry.
                        _ => [12289, 257],
                    },
                    _ => [0, if mutation == 4 && context_pass == 2 { 123 } else { 0 }],
                })
            }, |_| Some(9));
            assert_eq!(snapshot.source[10], if mutation == 4 { 1 } else { 4 });
            if mutation != 4 { assert_eq!(&snapshot.source[5..8], &[0, 0, 0]); }
        }
    }
    #[test]
    fn changed_or_inactive_physical_identity_is_rejected_before_structure_read() {
        let roots = [row(1, 4096, 5), row(2, 8192, 6)];
        let reads = core::cell::Cell::new(0usize);
        let mut snapshot = TEST_SNAPSHOT.lock(); reset(&mut snapshot);
        walk(&mut snapshot, &roots, &[], |pa, index| {
            reads.set(reads.get() + 1);
            Some(if pa == 4096 && index == 0 { [8193, 0] } else { [0, 0] })
        }, |pa| if pa == 8192 && reads.get() >= 512 { None } else { Some(9) });
        assert_eq!(snapshot.source[10], 4);
        assert_eq!(snapshot.source[11], HISTORY);
        assert_eq!(&snapshot.source[5..8], &[0, 0, 0]);
        assert!(reads.get() < 1024);
        reset(&mut snapshot);
        walk(&mut snapshot, &roots, &[], |_, _| panic!("inactive structure must never be read"), |_| None);
        assert_eq!(snapshot.source[11], HISTORY);
    }
    #[test]
    fn wrong_target_family_identity_and_registry_route_refuse_without_rows() {
        for fault in 0..4 {
            let mut snapshot = TEST_SNAPSHOT.lock(); reset(&mut snapshot);
            let mut roots = [row(1, 4096, 5), row(2, 8192, 6), row(3, 12288, 4)];
            if fault == 0 { roots[1][2] = 4; }
            let references = [[2, 3, if fault == 3 { 1 } else { 0 }, 0, 4096]];
            walk(&mut snapshot, &roots, &references, |pa, index| Some(match (pa, index) {
                (4096, 0) => [8193, 0],
                (8192, 0) => [if fault == 1 { 4097 } else { 12289 }, 257], _ => [0, 0],
            }), |pa| if fault == 2 && pa == 8192 { Some(10) } else { Some(9) });
            assert_eq!(snapshot.source[10], 4);
            assert_eq!(&snapshot.source[5..8], &[0, 0, 0]);
            assert_eq!(snapshot.source[11], match fault { 2 => HISTORY, 3 => ROOTS, _ => MALFORMED });
        }
    }
    #[test]
    fn exact_edge_ceiling_and_one_more_structure_fail_closed() {
        for contexts in [16, 17] {
            let mut snapshot = TEST_SNAPSHOT.lock(); reset(&mut snapshot);
            let mut roots = [[0; 8]; 18];
            for index in 0..contexts { roots[index] = row(index as u64 + 1, (index as u64 + 1) * 4096, 6); }
            roots[contexts] = row(100, 0x100000, 4);
            walk(&mut snapshot, &roots[..=contexts], &[], |_, _| Some([0x100001, 257]), |_| Some(9));
            assert_eq!(snapshot.source[13], contexts as u64 * 256);
            assert_eq!(snapshot.source[10], if contexts == 16 { 1 } else { 4 });
            if contexts == 17 { assert_eq!(snapshot.source[11], CAPACITY); assert_eq!(&snapshot.source[5..8], &[0, 0, 0]); }
        }
    }
    #[test]
    fn node_ceiling_empty_census_and_missing_reads_are_exact() {
        let mut roots = [[0; 8]; NODES + 1];
        for (index, root) in roots.iter_mut().enumerate() { *root = row(index as u64 + 1, (index as u64 + 1) * 4096, 5); }
        for count in [0, NODES, NODES + 1] {
            let mut snapshot = TEST_SNAPSHOT.lock(); reset(&mut snapshot);
            walk(&mut snapshot, &roots[..count], &[], |_, _| Some([0, 0]), |_| Some(9));
            assert_eq!(snapshot.source[10], if count <= NODES { 1 } else { 4 });
            if count <= NODES { assert_eq!(snapshot.source[5], count as u64); }
            else { assert_eq!(snapshot.source[11], CAPACITY); }
        }
        let mut snapshot = TEST_SNAPSHOT.lock(); reset(&mut snapshot);
        walk(&mut snapshot, &roots[..1], &[], |_, index| if index == 255 { None } else { Some([0, 0]) }, |_| Some(9));
        assert_eq!(snapshot.source[11], MALFORMED);
        assert_eq!(&snapshot.source[5..8], &[0, 0, 0]);
    }
}
