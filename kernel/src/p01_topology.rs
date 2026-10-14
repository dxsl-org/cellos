//! Private, current x86 CPU page-table graph. Never a physical-owner or workload-fit proof.
//! All mutation is excluded only on a boot-complete, single-online-CPU cutoff.
use crate::sync::Spinlock;

pub(crate) const ROOT_CAPACITY: usize = 64;
pub(crate) const NODE_CAPACITY: usize = 1024;
pub(crate) const EDGE_CAPACITY: usize = 16384;
const ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;
const UNSUPPORTED: u64 = 1;
const ROOT_MISSING: u64 = 2;
const CAPACITY: u64 = 4;
const MALFORMED: u64 = 8;
const HISTORY: u64 = 16;

pub(crate) struct Snapshot {
    pub(crate) source: [u64; 16],
    pub(crate) roots: [[u64; 8]; ROOT_CAPACITY],
    pub(crate) nodes: [[u64; 5]; NODE_CAPACITY],
    pub(crate) edges: [[u64; 7]; EDGE_CAPACITY],
}
impl Snapshot {
    pub(crate) const fn new() -> Self {
        Self { source: [0; 16], roots: [[0; 8]; ROOT_CAPACITY],
            nodes: [[0; 5]; NODE_CAPACITY], edges: [[0; 7]; EDGE_CAPACITY] }
    }
    fn begin(&mut self, sample: usize, epoch: u64, used: u64, online: u64) {
        self.source = [2, sample as u64, epoch, used, online, 0, 0, 0,
            ROOT_CAPACITY as u64, NODE_CAPACITY as u64, EDGE_CAPACITY as u64, 4, 0, 7, 0, 0];
    }
    fn refuse(&mut self, reason: u64) {
        self.source[5..8].fill(0);
        self.source[11] = 4;
        self.source[12] |= reason;
    }
}
static SNAPSHOTS: Spinlock<[Snapshot; 6]> = Spinlock::new([const { Snapshot::new() }; 6]);
pub(crate) fn static_bytes() -> usize { core::mem::size_of_val(&SNAPSHOTS) }

/// Descriptor geometry only: PT bit 7 is PAT; huge PAT bit 12 is not address.
/// Reject reserved low huge-address bits rather than rounding malformed leaves.
pub(crate) fn descriptor(level: u64, raw: u64) -> Result<Option<(u64, u64)>, u64> {
    if !(1..=4).contains(&level) { return Err(MALFORMED); }
    if raw & 1 == 0 { return Ok(None); }
    let address = raw & ADDRESS_MASK;
    let huge = raw & (1 << 7) != 0;
    if level == 4 && huge { return Err(MALFORMED); }
    let bytes = match (level, huge) {
        (1, _) => 4096,
        (2, true) => 2097152,
        (3, true) => 1073741824,
        _ => 0,
    };
    let target = if bytes > 4096 {
        if address & (bytes - 1) & !(1 << 12) != 0 { return Err(MALFORMED); }
        address & !(bytes - 1)
    } else { address };
    if bytes == 0 && target == 0 { return Err(MALFORMED); }
    Ok(Some((target, bytes)))
}

/// Actual source-including walker, shared with host synthetic descriptor tests.
/// Callers keep roots, descriptor bytes, bitmap and incarnation history immutable
/// throughout both passes. `classify` returns bitmap-owned=1/external-or-mixed=2/free=3.
/// Count writes private scratch only; publication occurs after an exact second fill.
/// On an aborted count, required_* are observed LOWER BOUNDS, never a fit certificate.
pub(crate) fn capture_graph(
    snapshot: &mut Snapshot, root_count: usize,
    mut read: impl FnMut(u64, usize) -> Option<u64>,
    mut classify: impl FnMut(u64, u64) -> u64,
    mut incarnation: impl FnMut(u64) -> Option<u64>,
) {
    let result = (|| -> Result<(), u64> {
        if root_count == 0 { return Err(ROOT_MISSING); }
        if root_count > ROOT_CAPACITY { return Err(CAPACITY); }
        let mut nodes = 0usize;
        let mut edges = 0usize;
        let mut last_edge = [0u64; 7];
        for index in 0..root_count {
            let pa = snapshot.roots[index][0];
            add_node(snapshot, &mut nodes, pa, 4, false, &mut classify, &mut incarnation)?;
        }
        let mut cursor = 0usize;
        while cursor < nodes {
            let [pa, level, _, _, _] = snapshot.nodes[cursor];
            for index in 0..512 {
                let raw = read(pa, index).ok_or(MALFORMED)?;
                let Some((target, bytes)) = descriptor(level, raw)? else { continue; };
                let class = classify(target, if bytes == 0 { 4096 } else { bytes });
                if !(1..=3).contains(&class) { return Err(MALFORMED); }
                if bytes == 0 {
                    if class == 3 { return Err(MALFORMED); }
                    add_node(snapshot, &mut nodes, target, level - 1, true,
                        &mut classify, &mut incarnation)?;
                }
                let raw = raw & !0x60; // Hardware accessed/dirty are not graph facts.
                if bytes == 4096 && edges != 0 {
                    let previous = &mut last_edge;
                    if previous[0] == pa && previous[1] + previous[6] == index as u64
                        && previous[4] == bytes && previous[5] == class
                        && previous[2] & !ADDRESS_MASK == raw & !ADDRESS_MASK
                        && previous[3].checked_add(previous[6] * 4096) == Some(target) {
                        previous[6] += 1;
                        if edges <= EDGE_CAPACITY { snapshot.edges[edges - 1] = last_edge; }
                        continue;
                    }
                }
                edges = edges.checked_add(1).ok_or(CAPACITY)?;
                snapshot.source[15] = edges as u64;
                last_edge = [pa, index as u64, raw, target, bytes, class, 1];
                if edges <= EDGE_CAPACITY {
                    snapshot.edges[edges - 1] = last_edge;
                }
            }
            cursor += 1;
        }
        if edges > EDGE_CAPACITY { return Err(CAPACITY); }
        // Check mapping references, including absent slots. CPU hardware may set
        // accessed/dirty while the walker reads HHDM or writes snapshot backing.
        let mut filled = 0usize;
        let mut offset = 0u64;
        for node in snapshot.nodes.iter().take(nodes) {
            let [pa, level, identity, class, _] = *node;
            if classify(pa, 4096) != class { return Err(MALFORMED); }
            if class == 1 && incarnation(pa) != Some(identity) { return Err(HISTORY); }
            for index in 0..512 {
                let raw = read(pa, index).ok_or(MALFORMED)?;
                let Some((target, bytes)) = descriptor(level, raw)? else { continue; };
                if filled >= edges { return Err(MALFORMED); }
                let before = snapshot.edges[filled];
                let displacement = offset.checked_mul(4096).ok_or(MALFORMED)?;
                let expected = [before[0], before[1] + offset,
                    before[2].checked_add(displacement).ok_or(MALFORMED)?,
                    before[3].checked_add(displacement).ok_or(MALFORMED)?, before[4], before[5]];
                let row = [pa, index as u64, raw & !0x60, target, bytes,
                    classify(target, if bytes == 0 { 4096 } else { bytes })];
                if expected != row { return Err(MALFORMED); }
                offset += 1;
                if offset == before[6] { offset = 0; filled += 1; }
            }
        }
        if filled != edges || offset != 0 { return Err(MALFORMED); }
        snapshot.source[5] = root_count as u64;
        snapshot.source[6] = nodes as u64;
        snapshot.source[7] = filled as u64;
        snapshot.source[11] = 1;
        Ok(())
    })();
    if let Err(reason) = result { snapshot.refuse(reason); }
}
fn add_node(
    snapshot: &mut Snapshot, count: &mut usize, pa: u64, level: u64, incoming: bool,
    classify: &mut impl FnMut(u64, u64) -> u64,
    incarnation: &mut impl FnMut(u64) -> Option<u64>,
) -> Result<(), u64> {
    if pa == 0 || pa & !ADDRESS_MASK != 0 || !(1..=4).contains(&level) { return Err(MALFORMED); }
    if let Some(node) = snapshot.nodes[..*count].iter_mut().find(|node| node[0] == pa) {
        if node[1] != level { return Err(MALFORMED); }
        if incoming { node[4] = node[4].checked_add(1).ok_or(CAPACITY)?; }
        return Ok(());
    }
    snapshot.source[14] = (*count as u64).checked_add(1).ok_or(CAPACITY)?;
    if *count == NODE_CAPACITY { return Err(CAPACITY); }
    let class = classify(pa, 4096);
    let identity = match class {
        1 => incarnation(pa).filter(|id| *id != 0).ok_or(HISTORY)?,
        2 => 0,
        _ => return Err(MALFORMED),
    };
    snapshot.nodes[*count] = [pa, level, identity, class, incoming as u64];
    *count += 1;
    Ok(())
}

/// The outer private lock masks local interrupts before root discovery or metrics.
/// Never hold SCHEDULER with frame/root locks, never acquire private table_frames
/// beneath FRAME_ALLOCATOR, and never clone/drop Arcs in this capture.
#[cfg(not(all(test, not(target_os = "none"))))]
pub(crate) fn capture(sample: usize) -> [crate::p01_resources::Metric; 16] {
    let mut storage = SNAPSHOTS.lock();
    let metrics = crate::p01_resources::snapshot();
    crate::p01_noncpu::capture(sample, &metrics);
    crate::p01_noncpu_topology::capture(sample, &metrics);
    crate::p01_vtd_topology::capture(sample, &metrics);
    let Some(snapshot) = sample.checked_sub(1).and_then(|index| storage.get_mut(index)) else {
        crate::p01_measurement::invalidate(crate::p01_measurement::PROTOCOL);
        return metrics;
    };
    if snapshot.source[0] != 0 {
        crate::p01_measurement::invalidate(crate::p01_measurement::PROTOCOL);
        return metrics;
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        snapshot.begin(sample, metrics[1].epoch_before, metrics[1].value, 0);
        snapshot.refuse(UNSUPPORTED);
    }
    #[cfg(target_arch = "x86_64")]
    {
        let online = crate::task::smp::online_hart_count();
        snapshot.begin(sample, metrics[1].epoch_before, metrics[1].value, online as u64);
        if online != 1 || !crate::task::smp::x86_scheduler_ready() {
            snapshot.refuse(UNSUPPORTED);
            return metrics;
        }
        // Root row zero is the explicit kernel root; resolve SAS rows later,
        // after releasing the scheduler and taking FRAME_ALLOCATOR -> KERNEL_ROOT.
        let mut roots = 1usize;
        {
            let scheduler = crate::task::SCHEDULER.lock();
            if let Some(scheduler) = scheduler.as_ref() {
                for task in scheduler.tasks.values() {
                    let owner = crate::p01_measurement::Owner::of(task);
                    let mut row = [0, owner.tid, owner.root, owner.cell, owner.generation, 0, 0, 1];
                    #[cfg(feature = "native-domains")]
                    if let crate::task::tcb::TaskAddressSpace::Domain(space) = &task.address_space {
                        row[0] = (space.root_ppn() as u64) << 12;
                        row[5] = space.identity().raw();
                        row[6] = space.generation();
                        row[7] = 2;
                    }
                    if roots < ROOT_CAPACITY { snapshot.roots[roots] = row; }
                    roots += 1;
                }
            } else {
                snapshot.refuse(ROOT_MISSING);
                return metrics;
            }
        }
        if roots > ROOT_CAPACITY { snapshot.refuse(CAPACITY); return metrics; }
        let frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
        let kernel_root = crate::memory::paging::KERNEL_ROOT.lock();
        let (Some(allocator), Some(root)) = (frames.as_ref(), *kernel_root) else {
            snapshot.refuse(ROOT_MISSING);
            return metrics;
        };
        if metrics[1].status != 1 || metrics[1].epoch_before != metrics[1].epoch_after
            || metrics[1].epoch_before != allocator.p01_snapshot().0
            || metrics[1].value != allocator.used_frames() as u64 {
            snapshot.refuse(HISTORY);
            return metrics;
        }
        snapshot.roots[0] = [root as u64, 0, 0, 0, 0, 0, 0, 1];
        for row in snapshot.roots.iter_mut().take(roots) {
            if row[7] == 1 { row[0] = root as u64; }
        }
        let cr3: u64;
        unsafe { core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags)); }
        if !snapshot.roots[..roots].iter().any(|row| row[0] == cr3 & ADDRESS_MASK) {
            snapshot.refuse(ROOT_MISSING);
            return metrics;
        }
        let history_before = crate::p01_retained::physical_snapshot_before();
        let epoch = allocator.p01_snapshot().0;
        capture_graph(snapshot, roots,
            |pa, index| {
                let physical = usize::try_from(pa).ok()?;
                let alias = crate::memory::frame::phys_to_virt(physical);
                if alias < physical { return None; }
                // SAFETY: boot-complete UP, IRQ-masked cutoff; no mapping/reaper
                // can run, and only decoded table pointers are followed via HHDM.
                Some(unsafe { core::ptr::read_volatile((alias as *const u64).add(index)) })
            },
            |pa, bytes| {
                let Some(end) = pa.checked_add(bytes) else { return 2; };
                let mut frame = pa;
                while frame < end {
                    if !allocator.manages(frame as usize) { return 2; }
                    if !allocator.p01_allocated(frame as usize) {
                        // A huge leaf spanning allocated/free inventory is mixed,
                        // not a claimed owner of the whole physical extent.
                        return if bytes == 4096 { 3 } else { 2 };
                    }
                    frame += 4096;
                }
                1
            },
            |pa| crate::p01_retained::physical_incarnation_at(pa, epoch));
        let history = crate::p01_retained::physical_snapshot_after(history_before);
        if history.status != 1 || history.epoch_before != history.epoch_after {
            snapshot.refuse(HISTORY);
        }
    }
    metrics
}

/// Only the STOP/drained exporter calls this. Rows never change after capture.
/// Individual scalar copies avoid holding a lock while formatting/logging.
pub(crate) fn source(sample: usize) -> Option<[u64; 16]> {
    let storage = SNAPSHOTS.lock();
    let snapshot = storage.get(sample.checked_sub(1)?)?;
    (snapshot.source[0] != 0).then_some(snapshot.source)
}
pub(crate) fn root(sample: usize, index: usize) -> Option<[u64; 8]> {
    let storage = SNAPSHOTS.lock();
    let snapshot = storage.get(sample.checked_sub(1)?)?;
    (snapshot.source[11] == 1 && index < snapshot.source[5] as usize)
        .then(|| snapshot.roots[index])
}
pub(crate) fn node(sample: usize, index: usize) -> Option<[u64; 5]> {
    let storage = SNAPSHOTS.lock();
    let snapshot = storage.get(sample.checked_sub(1)?)?;
    (snapshot.source[11] == 1 && index < snapshot.source[6] as usize)
        .then(|| snapshot.nodes[index])
}
pub(crate) fn edge(sample: usize, index: usize) -> Option<[u64; 7]> {
    let storage = SNAPSHOTS.lock();
    let snapshot = storage.get(sample.checked_sub(1)?)?;
    (snapshot.source[11] == 1 && index < snapshot.source[7] as usize)
        .then(|| snapshot.edges[index])
}
