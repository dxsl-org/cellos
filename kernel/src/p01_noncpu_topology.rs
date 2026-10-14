//! Private software descriptor trees; never hardware publication or authority.
use crate::sync::Spinlock;
const NODES: usize = 1024;
const EDGES: usize = 16384;
const ADDRESS: u64 = 0x000f_ffff_ffff_f000;
const UNSUPPORTED: u64 = 1;
const CAPACITY: u64 = 2;
const MALFORMED: u64 = 4;
const HISTORY: u64 = 8;
const ROOTS: u64 = 16;

/// Return normalized descriptor, target PA, per-descriptor leaf bytes.
pub(crate) fn descriptor(kind: u64, level: u64, raw: u64) -> Result<Option<(u64, u64, u64)>, u64> {
    let max = if kind <= 2 { 4 } else { 3 };
    if !(1..=4).contains(&kind) || !(1..=max).contains(&level) { return Err(UNSUPPORTED); }
    if kind == 3 {
        if raw & 1 == 0 { return Ok(None); }
        if raw >> 54 != 0 || raw & 4 != 0 && raw & 2 == 0 { return Err(MALFORMED); }
        let leaf = raw & 14 != 0;
        if !leaf && level == 1 { return Err(MALFORMED); }
        let bytes = if leaf { 1 << (12 + 9 * (level - 1)) } else { 0 };
        let target = ((raw >> 10) << 12) & 0x00ff_ffff_ffff_f000;
        if target >= 1 << 52 || bytes != 0 && target & (bytes - 1) != 0
            || bytes == 0 && target == 0 { return Err(MALFORMED); }
        return Ok(Some((raw & !0xc0, target, bytes)));
    }
    if raw & (if kind == 1 { 1 } else if kind == 2 { 7 } else { 3 }) == 0 { return Ok(None); }
    if kind == 2 && raw & 2 != 0 && raw & 1 == 0 { return Err(MALFORMED); }
    let huge = raw & 128 != 0 && level > 1;
    if huge && level == 4 { return Err(MALFORMED); }
    let bytes = if level == 1 { 4096 } else if huge { 1 << (12 + 9 * (level - 1)) } else { 0 };
    let address = raw & ADDRESS;
    let target = if bytes > 4096 {
        let ignored = if kind == 1 { 4096 } else { 0 };
        if address & (bytes - 1) & !ignored != 0 { return Err(MALFORMED); }
        address & !(bytes - 1)
    } else { address };
    if bytes == 0 && target == 0 { return Err(MALFORMED); }
    Ok(Some((raw & !(if kind == 1 { 0x60 } else { 0x300 }), target, bytes)))
}
fn step(kind: u64) -> u64 { if kind == 3 { 1024 } else { 4096 } }

pub(crate) struct Snapshot {
    pub(crate) source: [u64; 16],
    pub(crate) nodes: [[u64; 5]; NODES],
    pub(crate) edges: [[u64; 7]; EDGES],
}
impl Snapshot {
    pub(crate) const fn new() -> Self { Self { source: [0; 16], nodes: [[0; 5]; NODES], edges: [[0; 7]; EDGES] } }
    fn refuse(&mut self, reason: u64) {
        self.source[5] = 0; self.source[6] = 0; self.source[10] = 4; self.source[11] |= reason;
    }
    fn node(&mut self, count: &mut usize, pa: u64, kind: u64, level: u64, incoming: bool,
        incarnation: &mut impl FnMut(u64) -> Option<u64>) -> Result<(), u64> {
        if pa == 0 || pa & !ADDRESS != 0 { return Err(MALFORMED); }
        if let Some(row) = self.nodes[..*count].iter_mut().find(|row| row[0] == pa) {
            if row[1] != kind || row[2] != level { return Err(MALFORMED); }
            if incoming { row[4] = row[4].checked_add(1).ok_or(CAPACITY)?; }
            return Ok(());
        }
        self.source[13] = (*count + 1) as u64;
        if *count == NODES { return Err(CAPACITY); }
        self.nodes[*count] = [pa, kind, level, incarnation(pa).filter(|id| *id != 0).ok_or(HISTORY)?, incoming as u64];
        *count += 1;
        Ok(())
    }
}

pub(crate) fn walk(snapshot: &mut Snapshot, roots: &[[u64; 8]],
    mut read: impl FnMut(u64, usize) -> Option<u64>, mut incarnation: impl FnMut(u64) -> Option<u64>) {
    let result = (|| -> Result<(), u64> {
        let mut nodes = 0usize;
        let mut edges = 0usize;
        let mut last = [0u64; 7];
        for root in roots {
            if matches!(root[2], 5 | 6) { continue; }
            if !(1..=4).contains(&root[2]) { return Err(UNSUPPORTED); }
            snapshot.node(&mut nodes, root[1], root[2], if root[2] <= 2 { 4 } else { 3 }, false, &mut incarnation)?;
        }
        let mut cursor = 0usize;
        while cursor < nodes {
            let [pa, kind, level, _, _] = snapshot.nodes[cursor];
            for index in 0..512 {
                let Some((raw, target, bytes)) = descriptor(kind, level, read(pa, index).ok_or(MALFORMED)?)? else { continue; };
                if bytes == 0 { snapshot.node(&mut nodes, target, kind, level - 1, true, &mut incarnation)?; }
                if bytes == 4096 && edges != 0 && last[0] == pa && last[1] + last[5] == index as u64
                    && last[4] == bytes && last[6] == kind
                    && last[2].checked_add(last[5] * step(kind)) == Some(raw)
                    && last[3].checked_add(last[5] * 4096) == Some(target) {
                    last[5] += 1;
                    if edges <= EDGES { snapshot.edges[edges - 1] = last; }
                    continue;
                }
                edges = edges.checked_add(1).ok_or(CAPACITY)?;
                snapshot.source[14] = edges as u64;
                last = [pa, index as u64, raw, target, bytes, 1, kind];
                if edges <= EDGES { snapshot.edges[edges - 1] = last; }
            }
            cursor += 1;
        }
        if edges > EDGES { return Err(CAPACITY); }
        let mut filled = 0usize;
        let mut offset = 0u64;
        for node in snapshot.nodes.iter().take(nodes) {
            let [pa, kind, level, identity, _] = *node;
            if incarnation(pa) != Some(identity) { return Err(HISTORY); }
            for index in 0..512 {
                let Some((raw, target, bytes)) = descriptor(kind, level, read(pa, index).ok_or(MALFORMED)?)? else { continue; };
                if filled >= edges { return Err(MALFORMED); }
                let before = snapshot.edges[filled];
                if [pa, index as u64, raw, target, bytes, kind] != [before[0], before[1] + offset,
                    before[2].checked_add(offset * step(kind)).ok_or(MALFORMED)?,
                    before[3].checked_add(offset * 4096).ok_or(MALFORMED)?, before[4], before[6]] { return Err(MALFORMED); }
                offset += 1;
                if offset == before[5] { offset = 0; filled += 1; }
            }
        }
        if filled != edges || offset != 0 { return Err(MALFORMED); }
        snapshot.source[5] = nodes as u64; snapshot.source[6] = edges as u64; snapshot.source[10] = 1;
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
    snapshot.source = [2, sample as u64, metrics[1].epoch_before, metrics[1].value, 0, 0, 0,
        128, NODES as u64, EDGES as u64, 4, ROOTS, 15, 0, 0, 0];
    crate::p01_noncpu::visit_snapshot(sample, |source, roots| {
        snapshot.source[4] = roots.iter().filter(|root| (1..=4).contains(&root[2])).count() as u64;
        snapshot.source[15] = source[2];
        if source[10] != 1 { return; }
        snapshot.source[11] = 0;
        if !crate::p01_noncpu::heap_cutoff_matches(metrics) { snapshot.refuse(HISTORY); return; }
        let frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
        let Some(allocator) = frames.as_ref() else { snapshot.refuse(HISTORY); return; };
        if metrics[1].status != 1 || metrics[1].epoch_before != metrics[1].epoch_after
            || allocator.p01_snapshot().0 != source[3] || source[3] != snapshot.source[2]
            || source[4] != snapshot.source[3] || allocator.used_frames() as u64 != source[4] {
            snapshot.refuse(HISTORY); return;
        }
        walk(snapshot, roots, |pa, index| Some(unsafe {
            ((crate::memory::frame::phys_to_virt(pa as usize) as *const u64).add(index)).read_volatile()
        }), |pa| crate::p01_retained::physical_incarnation_at(pa, source[3]));
    });
}
pub(crate) fn source(sample: usize) -> Option<[u64; 16]> {
    let storage = SNAPSHOTS.lock(); let snapshot = storage.get(sample.checked_sub(1)?)?;
    (snapshot.source[0] != 0).then_some(snapshot.source)
}
pub(crate) fn node(sample: usize, index: usize) -> Option<[u64; 5]> {
    let storage = SNAPSHOTS.lock(); let snapshot = storage.get(sample.checked_sub(1)?)?;
    (snapshot.source[10] == 1 && index < snapshot.source[5] as usize).then(|| snapshot.nodes[index])
}
pub(crate) fn edge(sample: usize, index: usize) -> Option<[u64; 7]> {
    let storage = SNAPSHOTS.lock(); let snapshot = storage.get(sample.checked_sub(1)?)?;
    (snapshot.source[10] == 1 && index < snapshot.source[6] as usize).then(|| snapshot.edges[index])
}

#[cfg(test)]
mod tests {
    use super::*;
    static TEST_SNAPSHOT: Spinlock<Snapshot> = Spinlock::new(Snapshot::new());
    #[test]
    fn independent_vtd_structure_roots_never_seed_or_multiply_slpt_trees() {
        let mut snapshot = TEST_SNAPSHOT.lock();
        snapshot.source = [0; 16];
        let roots = [[1, 4096, 5, 1, 2, 0, 0, 0], [2, 8192, 6, 1, 2, 0, 0, 0],
            [3, 12288, 4, 1, 2, 0, 0, 0]];
        walk(&mut snapshot, &roots, |pa, _| { assert_eq!(pa, 12288); Some(0) }, |_| Some(1));
        assert_eq!(snapshot.source[10], 1);
        assert_eq!(snapshot.source[5], 1);
        assert_eq!(snapshot.nodes[0], [12288, 4, 3, 1, 0]);
    }
    #[test]
    fn geometry_preserves_ept_memory_type_and_sv39_permissions_not_cpu_pat() {
        assert_eq!(descriptor(2, 1, 0x8037), Ok(Some((0x8037, 0x8000, 4096))));
        assert_eq!(descriptor(3, 1, (8 << 10) | 0xc7), Ok(Some(((8 << 10) | 7, 0x8000, 4096))));
        assert!(descriptor(3, 1, (8 << 10) | 5).is_err());
        assert!(descriptor(2, 2, 0x2010b7).is_err());
        assert_eq!(descriptor(1, 2, 0x201081), Ok(Some((0x201081, 0x200000, 2097152))));
    }
    #[test]
    fn exact_sv39_run_checks_every_middle_descriptor_and_normalizes_ad() {
        for mutate in [false, true] {
            let mut snapshot = TEST_SNAPSHOT.lock();
            snapshot.source = [0; 16];
            let roots = [[1, 4096, 3, 1, 2, 0, 0, 0]];
            let mut pass = 0;
            walk(&mut snapshot, &roots, |pa, index| {
                if pa == 12288 && index == 0 { pass += 1; }
                Some(match pa {
                    4096 => if index == 0 { (2 << 10) | 1 } else { 0 },
                    8192 => if index == 0 { (3 << 10) | 1 } else { 0 },
                    _ => ((100 + index as u64 + if mutate && pass == 2 && index == 257 { 1 } else { 0 }) << 10) | 7 | if pass == 2 { 0xc0 } else { 0 },
                })
            }, |_| Some(1));
            assert_eq!(snapshot.source[10], if mutate { 4 } else { 1 });
            if !mutate { assert_eq!(snapshot.edges[2][5], 512); }
        }
    }
}
