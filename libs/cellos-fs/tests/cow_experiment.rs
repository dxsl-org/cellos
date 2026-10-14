#![cfg(feature = "g2-cow-experiment")]

//! Direct-host algorithm evidence only: disposable images, atomic 512-byte sectors,
//! ordered writes and truthful flushes. Recovered fault images are observed and discarded.

use cellos_fs::format::{INODES_PER_BLOCK, INODE_PAYLOAD_SIZE};
use cellos_fs::{
    BlockDevice, CellosFs, DirEntryRecord, Extent, FsError, InodeRecord, Superblock,
    BLOCK_SIZE, INODE_MODE_DIR, INODE_SIZE,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

const SECTOR: usize = 512;
const SECTORS: usize = BLOCK_SIZE / SECTOR;
const MAX_IMAGE_BLOCKS: usize = 64 * 1024 * 1024 / BLOCK_SIZE;
type Image = Vec<[u8; BLOCK_SIZE]>;
type Fs = CellosFs<SectorDisk>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Persistence {
    Cached,
    WriteThrough,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
    Sector { write: usize, block: u64, sector: usize, after: bool },
    Flush { barrier: usize, after: bool },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Calls {
    reads: usize,
    writes: usize,
    flushes: usize,
    sectors: usize,
}

struct Medium {
    volatile: Image,
    durable: Image,
    persistence: Persistence,
    calls: Calls,
    trace: Vec<Edge>,
    cut: Option<usize>,
    fired: bool,
    stopped: bool,
    unreadable: Option<u64>,
}

#[derive(Clone)]
struct SectorDisk(Rc<RefCell<Medium>>);

impl SectorDisk {
    fn blank(blocks: usize, persistence: Persistence) -> Self {
        assert!((16..=MAX_IMAGE_BLOCKS).contains(&blocks));
        Self::from_image(vec![[0; BLOCK_SIZE]; blocks], persistence)
    }

    fn from_image(image: Image, persistence: Persistence) -> Self {
        assert!((16..=MAX_IMAGE_BLOCKS).contains(&image.len()));
        Self(Rc::new(RefCell::new(Medium {
            volatile: image.clone(), durable: image, persistence,
            calls: Calls::default(), trace: Vec::new(), cut: None,
            fired: false, stopped: false, unreadable: None,
        })))
    }

    fn arm(&self, cut: Option<usize>) {
        let mut m = self.0.borrow_mut();
        m.calls = Calls::default();
        m.trace.clear();
        m.cut = cut;
        m.fired = false;
        m.stopped = false;
    }

    fn image(&self) -> Image { self.0.borrow().durable.clone() }
    fn calls(&self) -> Calls { self.0.borrow().calls }
    fn trace(&self) -> Vec<Edge> { self.0.borrow().trace.clone() }

    fn crash(&self) {
        let mut m = self.0.borrow_mut();
        m.volatile = m.durable.clone();
        m.stopped = false;
        m.cut = None;
        m.unreadable = None;
    }
}

impl Medium {
    fn edge(&mut self, edge: Edge) -> Result<(), FsError> {
        let index = self.trace.len();
        self.trace.push(edge);
        if self.cut == Some(index) {
            self.fired = true;
            self.stopped = true;
            return Err(FsError::IoError);
        }
        Ok(())
    }
}

impl BlockDevice for SectorDisk {
    fn read_block(&mut self, block: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        let mut m = self.0.borrow_mut();
        m.calls.reads += 1;
        if m.stopped || m.unreadable == Some(block) { return Err(FsError::IoError); }
        let source = m.volatile.get(block as usize).ok_or(FsError::PartitionOverflow)?;
        buf.copy_from_slice(source);
        Ok(())
    }

    fn write_block(&mut self, block: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        let mut m = self.0.borrow_mut();
        m.calls.writes += 1;
        if m.stopped { return Err(FsError::IoError); }
        if block >= m.volatile.len() as u64 { return Err(FsError::PartitionOverflow); }
        let write = m.calls.writes;
        for sector in 0..SECTORS {
            m.edge(Edge::Sector { write, block, sector, after: false })?;
            let range = sector * SECTOR..(sector + 1) * SECTOR;
            m.volatile[block as usize][range.clone()].copy_from_slice(&buf[range.clone()]);
            if m.persistence == Persistence::WriteThrough {
                m.durable[block as usize][range.clone()].copy_from_slice(&buf[range]);
            }
            m.calls.sectors += 1;
            m.edge(Edge::Sector { write, block, sector, after: true })?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), FsError> {
        let mut m = self.0.borrow_mut();
        m.calls.flushes += 1;
        if m.stopped { return Err(FsError::IoError); }
        let barrier = m.calls.flushes;
        m.edge(Edge::Flush { barrier, after: false })?;
        m.durable = m.volatile.clone();
        m.edge(Edge::Flush { barrier, after: true })?;
        Ok(())
    }

    fn total_blocks(&self) -> u64 { self.0.borrow().volatile.len() as u64 }
}

#[derive(Debug, PartialEq, Eq)]
struct Node {
    path: String,
    inode: [u8; INODE_SIZE],
    bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
struct Graph {
    sequence: u64,
    next_inode: u64,
    nodes: Vec<Node>,
}

fn observe(fs: &mut Fs) -> Graph {
    fn visit(fs: &mut Fs, path: String, nodes: &mut Vec<Node>) {
        let inode = fs.lookup(&path).expect("complete reachable inode");
        let mut bytes = Vec::new();
        if inode.is_file() {
            bytes.resize(inode.size as usize, 0);
            assert_eq!(fs.read_file(&path, 0, &mut bytes).unwrap(), bytes.len());
            let mut eof = [0xA5; 1];
            assert_eq!(fs.read_file(&path, inode.size, &mut eof).unwrap(), 0);
        }
        nodes.push(Node { path: path.clone(), inode: inode.to_bytes(), bytes });
        if inode.is_dir() {
            let mut children = fs.list_dir(&path).expect("complete namespace enumeration");
            children.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, _, _) in children {
                let child = if path == "/" { format!("/{name}") } else { format!("{path}/{name}") };
                visit(fs, child, nodes);
            }
        }
    }
    let mut nodes = Vec::new();
    visit(fs, "/".into(), &mut nodes);
    Graph { sequence: fs.superblock().sequence, next_inode: fs.superblock().next_inode_num, nodes }
}

fn roots(image: &Image) -> Vec<(usize, Superblock)> {
    (0..2).filter_map(|slot| Superblock::from_bytes(&image[slot]).map(|sb| (slot, sb))).collect()
}

fn inode_from(image: &Image, sb: &Superblock, num: u64) -> InodeRecord {
    let slot = (num as usize - 1) / INODES_PER_BLOCK;
    let index = (num as usize - 1) % INODES_PER_BLOCK;
    let block = &image[sb.inode_blocks[slot] as usize];
    InodeRecord::from_bytes(block[index * INODE_SIZE..(index + 1) * INODE_SIZE].try_into().unwrap())
        .expect("fixture inode checksum")
}

fn put_inode(image: &mut Image, sb: &Superblock, mut inode: InodeRecord) {
    let slot = (inode.inode_num as usize - 1) / INODES_PER_BLOCK;
    let index = (inode.inode_num as usize - 1) % INODES_PER_BLOCK;
    inode.update_checksum();
    image[sb.inode_blocks[slot] as usize][index * INODE_SIZE..(index + 1) * INODE_SIZE]
        .copy_from_slice(&inode.to_bytes());
}

fn put_root(image: &mut Image, slot: usize, mut sb: Superblock) {
    sb.update_checksum();
    image[slot] = sb.to_bytes();
}

fn reachable(image: &Image, sb: &Superblock) -> BTreeSet<u64> {
    fn visit(image: &Image, sb: &Superblock, num: u64, seen: &mut BTreeSet<u64>, blocks: &mut BTreeSet<u64>) {
        assert!(seen.insert(num), "fixture contains directory alias or cycle");
        let inode = inode_from(image, sb, num);
        if inode.is_dir() {
            for entry in inode.payload.chunks_exact(DirEntryRecord::SIZE).take(inode.extent_count as usize) {
                let entry = DirEntryRecord::from_bytes(entry.try_into().unwrap()).unwrap();
                visit(image, sb, entry.inode_num, seen, blocks);
            }
        } else if !inode.is_inline() {
            for extent in inode.payload.chunks_exact(Extent::SIZE).take(inode.extent_count as usize) {
                let e = Extent::from_bytes(extent.try_into().unwrap());
                blocks.extend(e.physical_block..e.physical_block + e.block_count as u64);
            }
        }
    }
    let mut blocks: BTreeSet<_> = sb.inode_blocks.into_iter().filter(|b| *b != 0).collect();
    visit(image, sb, 1, &mut BTreeSet::new(), &mut blocks);
    blocks
}

fn union(image: &Image) -> BTreeSet<u64> {
    let mut blocks = BTreeSet::from([0, 1, 2]);
    for (_, sb) in roots(image) { blocks.extend(reachable(image, &sb)); }
    blocks
}

fn protected_bytes(image: &Image) -> BTreeMap<u64, [u8; BLOCK_SIZE]> {
    union(image).into_iter().filter(|b| *b >= 3).map(|b| (b, image[b as usize])).collect()
}

fn assert_protected(before: &BTreeMap<u64, [u8; BLOCK_SIZE]>, after: &Image, label: &str) {
    for (&block, bytes) in before { assert_eq!(&after[block as usize], bytes, "{label}: protected block {block}"); }
}

fn assert_poisoned(fs: &mut Fs, disk: &SectorDisk) {
    let calls = disk.calls();
    assert_eq!(fs.create_file("/denied").err(), Some(FsError::Corrupted));
    assert_eq!(fs.create_dir("/denied").err(), Some(FsError::Corrupted));
    assert_eq!(fs.write_file("/file", 0, b"denied"), Err(FsError::Corrupted));
    assert_eq!(fs.replace_file("/file", b"denied"), Err(FsError::Corrupted));
    assert_eq!(fs.unlink("/file"), Err(FsError::Corrupted));
    assert_eq!(fs.rmdir("/empty"), Err(FsError::Corrupted));
    assert_eq!(fs.rename("/file", "/denied"), Err(FsError::Corrupted));
    assert_eq!(fs.sync(), Err(FsError::Corrupted));
    assert_eq!(disk.calls(), calls, "poison must reject before even a device read");
}

#[derive(Clone, Copy, Debug)]
enum Scenario { FirstPublication, InlineToExtent, ExtentReplacement, ExtentToInline, Positional, Namespace, TableBoundary }

fn pattern(length: usize, salt: usize) -> Vec<u8> {
    (0..length).map(|i| ((i * 17 + salt) % 251) as u8).collect()
}

fn prepare(scenario: Scenario, persistence: Persistence) -> (Fs, SectorDisk) {
    let disk = SectorDisk::blank(128, persistence);
    let mut fs = CellosFs::format(disk.clone(), 128).unwrap();
    if !matches!(scenario, Scenario::FirstPublication) {
        fs.create_file("/file").unwrap();
        fs.replace_file("/file", b"previous-inline-acknowledged").unwrap();
        fs.create_file("/keep").unwrap();
        fs.replace_file("/keep", b"untouched-acknowledged").unwrap();
        fs.create_dir("/dir").unwrap();
        fs.create_file("/dir/old").unwrap();
        fs.replace_file("/dir/old", b"namespace-acknowledged").unwrap();
        fs.create_dir("/empty").unwrap();
        if matches!(scenario, Scenario::TableBoundary) {
            for index in 0..3 {
                fs.create_file(&format!("/dir/extra{index}")).unwrap();
                fs.replace_file(&format!("/dir/extra{index}"), b"second-table-prior").unwrap();
            }
            assert_eq!(fs.lookup("/dir/extra2").unwrap().inode_num, 9);
        }
        fs.sync().unwrap();
        if matches!(scenario, Scenario::ExtentReplacement | Scenario::ExtentToInline | Scenario::Positional) {
            fs.replace_file("/file", &pattern(2 * BLOCK_SIZE + 731, 11)).unwrap();
        } else {
            fs.replace_file("/keep", b"second-root-acknowledged").unwrap();
        }
        fs.sync().unwrap();
        assert_eq!(roots(&disk.image()).len(), 2);
    } else {
        assert_eq!(roots(&disk.image()).len(), 1);
    }
    (fs, disk)
}

fn transaction(fs: &mut Fs, scenario: Scenario) -> Result<(), FsError> {
    match scenario {
        Scenario::FirstPublication => {
            fs.create_file("/file")?;
            fs.replace_file("/file", b"first-durable-content")?;
        }
        Scenario::InlineToExtent => { fs.replace_file("/file", &pattern(2 * BLOCK_SIZE + 319, 29))?; }
        Scenario::ExtentReplacement => { fs.replace_file("/file", &pattern(3 * BLOCK_SIZE + 113, 71))?; }
        Scenario::ExtentToInline => { fs.replace_file("/file", b"replacement-inline-exact")?; }
        Scenario::Positional => { fs.write_file("/file", (BLOCK_SIZE - 37) as u64, &pattern(BLOCK_SIZE + 79, 101))?; }
        Scenario::TableBoundary => {
            fs.replace_file("/file", &pattern(BLOCK_SIZE + 107, 67))?;
            fs.replace_file("/dir/extra2", b"second-table-successor")?;
            fs.rename("/dir/old", "/moved")?;
        }
        Scenario::Namespace => {
            fs.rename("/dir/old", "/moved")?;
            fs.create_dir("/newdir")?;
            fs.create_file("/newdir/child")?;
            fs.replace_file("/newdir/child", b"new-namespace-bytes")?;
            fs.unlink("/keep")?;
            fs.rmdir("/empty")?;
        }
    }
    fs.sync()
}

fn validate_golden(trace: &[Edge], calls: Calls, bootstrap: bool) {
    let mut cursor = 0;
    let mut write = 0;
    let mut barrier = 0;
    while cursor < trace.len() {
        match trace[cursor] {
            Edge::Sector { block, sector: 0, after: false, .. } => {
                write += 1;
                for sector in 0..SECTORS {
                    for after in [false, true] {
                        assert_eq!(trace[cursor], Edge::Sector { write, block, sector, after });
                        cursor += 1;
                    }
                }
                if bootstrap && block == 1 {
                    assert_eq!(barrier, 0, "empty B initialized before FS1");
                } else if block <= 1 {
                    assert_eq!(barrier, 1, "root publication must follow FS1");
                } else {
                    assert_eq!(barrier, 0, "no payload writes after FS1");
                }
            }
            Edge::Flush { after: false, .. } => {
                barrier += 1;
                assert_eq!(trace[cursor], Edge::Flush { barrier, after: false });
                assert_eq!(trace[cursor + 1], Edge::Flush { barrier, after: true });
                cursor += 2;
            }
            edge => panic!("invalid golden edge {edge:?}"),
        }
    }
    assert_eq!(barrier, 2, "exactly FS1 and FS2");
    assert_eq!(write, calls.writes);
    assert_eq!(calls.sectors, write * SECTORS);
    assert_eq!(trace.len(), write * SECTORS * 2 + 4);
    assert!(trace.iter().any(|e| matches!(e, Edge::Sector { block: 2, .. })), "advisory bitmap exercised");
}

#[test]
fn every_sector_and_barrier_edge_preserves_exact_acknowledgement_or_refuses() {
    let scenarios = [Scenario::FirstPublication, Scenario::InlineToExtent, Scenario::ExtentReplacement,
        Scenario::ExtentToInline, Scenario::Positional, Scenario::Namespace, Scenario::TableBoundary];
    let mut cuts = 0;
    let mut prior_observations = 0;
    let mut successor_observations = 0;
    let mut refusals = 0;
    for persistence in [Persistence::Cached, Persistence::WriteThrough] {
        for scenario in scenarios {
            let (mut golden, disk) = prepare(scenario, persistence);
            let prior = observe(&mut golden);
            let protected = protected_bytes(&disk.image());
            disk.arm(None);
            transaction(&mut golden, scenario).expect("golden direct-engine transaction");
            let trace = disk.trace();
            let calls = disk.calls();
            validate_golden(&trace, calls, false);
            let candidate_writes: Vec<_> = trace.iter().filter_map(|edge| match edge {
                Edge::Sector { block, sector: 0, after: false, .. } if *block >= 3 => Some(*block),
                _ => None,
            }).collect();
            assert_eq!(candidate_writes.iter().copied().collect::<BTreeSet<_>>().len(), candidate_writes.len(),
                "no duplicate assignment of candidate data/table blocks");
            for edge in &trace {
                if let Edge::Sector { block, .. } = edge {
                    assert!(*block <= 2 || !protected.contains_key(block), "candidate allocated protected block {block}");
                }
            }
            let successor = observe(&mut golden);
            assert_eq!(successor.sequence, prior.sequence + 1);
            assert_ne!(successor.nodes, prior.nodes);
            drop(golden);
            for (cut, edge) in trace.iter().enumerate() {
                let label = format!("{scenario:?}/{persistence:?}/cut={cut}/{edge:?}");
                let (mut fs, fault) = prepare(scenario, persistence);
                assert_eq!(observe(&mut fs), prior, "deterministic preparation {label}");
                fault.arm(Some(cut));
                assert_eq!(transaction(&mut fs, scenario), Err(FsError::IoError), "{label}");
                assert!(fault.0.borrow().fired, "fault edge not reached: {label}");
                assert_eq!(fault.trace(), trace[..=cut], "golden prefix: {label}");
                assert_poisoned(&mut fs, &fault);
                assert_protected(&protected, &fault.image(), &label);
                drop(fs);
                fault.crash();
                let durable = fault.image();
                let newest = roots(&durable).into_iter().map(|(_, sb)| sb.sequence).max().unwrap();
                let before_open = fault.calls();
                match CellosFs::open(fault.clone()) {
                    Ok(mut recovered) => {
                        let observed = observe(&mut recovered);
                        if newest == successor.sequence {
                            assert_eq!(observed, successor, "newest valid successor/lost acknowledgement: {label}");
                            successor_observations += 1;
                        } else {
                            assert_eq!(newest, prior.sequence, "never an arbitrary older graph: {label}");
                            assert_eq!(observed, prior, "exact last acknowledged graph: {label}");
                            prior_observations += 1;
                        }
                        drop(recovered); // Read-only observation; no retry, repair or recovered mutation.
                    }
                    Err(error) => {
                        assert_eq!(error, FsError::Corrupted, "explicit refusal: {label}");
                        assert_ne!(newest, successor.sequence, "valid durable successor cannot be ignored: {label}");
                        refusals += 1;
                    }
                }
                assert_eq!(fault.calls().writes, before_open.writes, "reopen writes: {label}");
                assert_eq!(fault.calls().flushes, before_open.flushes, "reopen flushes: {label}");
                cuts += 1;
            }
            println!("cow-fault golden scenario={scenario:?} persistence={persistence:?} writes={} sectors={} barriers={} cut_edges={}",
                calls.writes, calls.sectors, calls.flushes, trace.len());
        }
    }
    assert!(prior_observations > 0 && successor_observations > 0);
    assert_eq!(cuts, prior_observations + successor_observations + refusals);
    println!("cow-fault summary cuts={cuts} exact_prior={prior_observations} exact_successor={successor_observations} explicit_refusal={refusals}");
}

#[test]
fn bootstrap_cuts_have_no_invented_graph_and_lost_fs2_ack_recovers_genesis() {
    for persistence in [Persistence::Cached, Persistence::WriteThrough] {
        let golden_disk = SectorDisk::blank(64, persistence);
        let mut fs = CellosFs::format(golden_disk.clone(), 64).unwrap();
        let trace = golden_disk.trace();
        let expected = observe(&mut fs);
        assert_eq!(expected.sequence, 1);
        assert_eq!(expected.nodes.len(), 1);
        assert_eq!(roots(&golden_disk.image()).len(), 1);
        validate_golden(&trace, golden_disk.calls(), true);
        for (cut, edge) in trace.iter().enumerate() {
            let disk = SectorDisk::blank(64, persistence);
            disk.arm(Some(cut));
            assert_eq!(CellosFs::format(disk.clone(), 64).err(), Some(FsError::IoError), "bootstrap {persistence:?} {edge:?}");
            assert!(disk.0.borrow().fired);
            assert_eq!(disk.trace(), trace[..=cut]);
            disk.crash();
            let valid = !roots(&disk.image()).is_empty();
            let calls = disk.calls();
            match CellosFs::open(disk.clone()) {
                Ok(mut fs) => { assert!(valid); assert_eq!(observe(&mut fs), expected); }
                Err(error) => { assert_eq!(error, FsError::Corrupted); assert!(!valid, "durable valid genesis must recover"); }
            }
            assert_eq!(disk.calls().writes, calls.writes);
            assert_eq!(disk.calls().flushes, calls.flushes);
        }
        println!("cow-bootstrap persistence={persistence:?} sectors={} cut_edges={}", golden_disk.calls().sectors, trace.len());
    }
}

#[test]
fn direct_engine_smoke_executes_namespace_extent_and_root_rotation() {
    let disk = SectorDisk::blank(256, Persistence::Cached);
    let mut fs = CellosFs::format(disk.clone(), 256).unwrap();
    fs.create_dir("/tx").unwrap();
    fs.create_file("/tx/payload").unwrap();
    fs.replace_file("/tx/payload", b"inline-payload").unwrap();
    fs.sync().unwrap();
    let payload = pattern(3 * BLOCK_SIZE + 37, 211);
    fs.replace_file("/tx/payload", &payload).unwrap();
    fs.create_file("/tx/commit").unwrap();
    fs.replace_file("/tx/commit", b"commit-binds-exact-payload").unwrap();
    fs.sync().unwrap();
    fs.write_file("/tx/payload", 23, b"positional-copy-on-write").unwrap();
    let mut expected = payload;
    expected[23..23 + b"positional-copy-on-write".len()].copy_from_slice(b"positional-copy-on-write");
    fs.rename("/tx/commit", "/commit").unwrap();
    fs.sync().unwrap();
    let mut read = vec![0; expected.len()];
    assert_eq!(fs.read_file("/tx/payload", 0, &mut read).unwrap(), expected.len());
    assert_eq!(read, expected);
    fs.replace_file("/tx/payload", b"inline-again").unwrap();
    fs.unlink("/commit").unwrap();
    fs.sync().unwrap();
    let expected_graph = observe(&mut fs);
    assert_eq!(expected_graph.sequence, 5);
    assert!(fs.lookup("/tx/payload").unwrap().is_inline());
    assert_eq!(fs.list_dir("/").unwrap(), vec![("tx".into(), true, 0)]);
    assert_eq!(fs.list_dir("/tx").unwrap(), vec![("payload".into(), false, 12)]);
    assert_eq!(roots(&disk.image()).iter().map(|(_, sb)| sb.sequence).collect::<BTreeSet<_>>(), BTreeSet::from([4, 5]));
    drop(fs);
    disk.crash();
    let calls = disk.calls();
    let mut recovered = CellosFs::open(disk.clone()).unwrap();
    assert_eq!(observe(&mut recovered), expected_graph);
    assert_eq!(disk.calls().writes, calls.writes);
    assert_eq!(disk.calls().flushes, calls.flushes);
    println!("cow-direct-smoke sequence=5 root_slots=2 exact_nodes={} writes={} sectors={} flushes={}",
        expected_graph.nodes.len(), calls.writes, calls.sectors, calls.flushes);
}

fn corruption_fixture() -> (Image, Superblock, u64, u64) {
    let disk = SectorDisk::blank(128, Persistence::Cached);
    let mut fs = CellosFs::format(disk.clone(), 128).unwrap();
    let a = fs.create_file("/a").unwrap().inode_num;
    fs.replace_file("/a", &pattern(2 * BLOCK_SIZE, 7)).unwrap();
    let b = fs.create_file("/b").unwrap().inode_num;
    fs.replace_file("/b", &pattern(BLOCK_SIZE, 31)).unwrap();
    fs.create_dir("/dir").unwrap();
    fs.sync().unwrap();
    let sb = *fs.superblock();
    (disk.image(), sb, a, b)
}

fn assert_open_refuses(image: Image, error: FsError, label: &str) {
    let disk = SectorDisk::from_image(image.clone(), Persistence::Cached);
    assert_eq!(CellosFs::open(disk.clone()).err(), Some(error), "{label}");
    assert_eq!(disk.calls().writes, 0, "refusal must not repair: {label}");
    assert_eq!(disk.calls().flushes, 0, "refusal must not flush: {label}");
    assert_eq!(disk.image(), image, "refusal altered durable bytes: {label}");
}

#[test]
fn checksum_valid_malformed_alias_cycle_and_overlap_roots_refuse_without_repair() {
    let (base, sb, a, b) = corruption_fixture();
    let cases = ["inactive-root", "inode-identity", "unsupported-mode", "duplicate-table", "namespace-alias",
        "namespace-cycle", "dangling-target", "duplicate-name", "metadata-data-overlap", "reserved-data-overlap",
        "extent-alias", "cross-file-alias", "cross-root-incompatible"];
    for case in cases {
        let mut image = base.clone();
        let mut root = inode_from(&image, &sb, 1);
        let mut file = inode_from(&image, &sb, a);
        match case {
            "inactive-root" => {
                let older = Superblock::from_bytes(&image[0]).unwrap();
                let mut inode = inode_from(&image, &older, 1);
                inode.mode = cellos_fs::INODE_MODE_FILE;
                put_inode(&mut image, &older, inode);
            }
            "inode-identity" => {
                file.inode_num = 99;
                file.update_checksum();
                let offset = ((a as usize - 1) % INODES_PER_BLOCK) * INODE_SIZE;
                image[sb.inode_blocks[0] as usize][offset..offset + INODE_SIZE].copy_from_slice(&file.to_bytes());
            }
            "unsupported-mode" => { file.mode = 99; put_inode(&mut image, &sb, file); }
            "duplicate-table" => { let mut malformed = sb; malformed.inode_blocks[1] = sb.inode_blocks[0]; put_root(&mut image, 1, malformed); }
            "namespace-alias" => {
                let e = DirEntryRecord::new(a, 1, "alias").unwrap();
                let offset = root.extent_count as usize * DirEntryRecord::SIZE;
                root.payload[offset..offset + DirEntryRecord::SIZE].copy_from_slice(&e.to_bytes());
                root.extent_count += 1;
                put_inode(&mut image, &sb, root);
            }
            "namespace-cycle" => {
                let e = DirEntryRecord::new(1, 2, "cycle").unwrap();
                root.payload[..DirEntryRecord::SIZE].copy_from_slice(&e.to_bytes());
                put_inode(&mut image, &sb, root);
            }
            "dangling-target" => {
                let e = DirEntryRecord::new(250, 1, "missing").unwrap();
                root.payload[..DirEntryRecord::SIZE].copy_from_slice(&e.to_bytes());
                put_inode(&mut image, &sb, root);
            }
            "duplicate-name" => {
                let e = DirEntryRecord::new(b, 1, "a").unwrap();
                root.payload[DirEntryRecord::SIZE..2 * DirEntryRecord::SIZE].copy_from_slice(&e.to_bytes());
                put_inode(&mut image, &sb, root);
            }
            "metadata-data-overlap" | "reserved-data-overlap" => {
                let e = Extent { logical_block: 0, block_count: 2,
                    physical_block: if case == "metadata-data-overlap" { sb.inode_blocks[0] } else { 1 } };
                file.payload[..Extent::SIZE].copy_from_slice(&e.to_bytes());
                file.extent_count = 1;
                put_inode(&mut image, &sb, file);
            }
            "extent-alias" => {
                let physical = Extent::from_bytes(file.payload[..Extent::SIZE].try_into().unwrap()).physical_block;
                file.payload[..Extent::SIZE].copy_from_slice(&Extent { logical_block: 0, block_count: 1, physical_block: physical }.to_bytes());
                file.payload[Extent::SIZE..2 * Extent::SIZE].copy_from_slice(&Extent { logical_block: 1, block_count: 1, physical_block: physical }.to_bytes());
                file.extent_count = 2;
                put_inode(&mut image, &sb, file);
            }
            "cross-file-alias" => {
                let mut other = inode_from(&image, &sb, b);
                other.payload[..Extent::SIZE].copy_from_slice(&file.payload[..Extent::SIZE]);
                let mut e = Extent::from_bytes(other.payload[..Extent::SIZE].try_into().unwrap());
                e.block_count = 1;
                other.payload[..Extent::SIZE].copy_from_slice(&e.to_bytes());
                other.extent_count = 1;
                put_inode(&mut image, &sb, other);
            }
            "cross-root-incompatible" => {
                let physical = Extent::from_bytes(file.payload[..Extent::SIZE].try_into().unwrap()).physical_block;
                let mut older = Superblock::from_bytes(&image[0]).unwrap();
                older.root_inode_block = physical;
                older.inode_blocks[0] = physical;
                // Give the old root a locally valid directory interpretation of the new root's data.
                image[physical as usize] = [0; BLOCK_SIZE];
                let inode = InodeRecord::new(1, INODE_MODE_DIR);
                image[physical as usize][..INODE_SIZE].copy_from_slice(&inode.to_bytes());
                put_root(&mut image, 0, older);
            }
            _ => unreachable!(),
        }
        assert_open_refuses(image, FsError::Corrupted, case);
    }
    println!("cow-malformed cases={} aliases=directory/extent/cross-file/cross-root cycles=directory inactive_root=validated", cases.len());
}

#[test]
fn unreadable_roots_and_geometry_refuse_before_any_write() {
    let (base, sb, _, _) = corruption_fixture();
    for block in [0, 1, sb.root_inode_block] {
        let disk = SectorDisk::from_image(base.clone(), Persistence::Cached);
        disk.0.borrow_mut().unreadable = Some(block);
        assert_eq!(CellosFs::open(disk.clone()).err(), Some(FsError::IoError));
        assert_eq!(disk.calls().writes, 0);
        assert_eq!(disk.calls().flushes, 0);
    }
    for case in 0..5 {
        let mut image = base.clone();
        let mut malformed = sb;
        match case {
            0 => malformed.total_blocks += 1,
            1 => malformed.root_inode_block = 2,
            2 => malformed.free_bitmap_block = 3,
            3 => malformed.free_bitmap_blocks = 2,
            _ => malformed.version += 1,
        }
        put_root(&mut image, 1, malformed);
        assert_open_refuses(image, FsError::Corrupted, &format!("geometry-{case}"));
    }
}

#[test]
fn torn_bitmap_is_advisory_and_shared_immutable_roots_are_accepted_read_only() {
    let (base, sb, _, _) = corruption_fixture();
    let normal = SectorDisk::from_image(base.clone(), Persistence::Cached);
    let mut fs = CellosFs::open(normal).unwrap();
    let expected = observe(&mut fs);
    let expected_free = base.len() as u64 - union(&base).len() as u64;
    for sectors in 0..=SECTORS {
        let mut image = base.clone();
        image[2][..sectors * SECTOR].fill(0);
        image[2][sectors * SECTOR..].fill(0xA5);
        let disk = SectorDisk::from_image(image.clone(), Persistence::Cached);
        let mut fs = CellosFs::open(disk.clone()).expect("torn advisory bitmap cannot erase root reachability");
        assert_eq!(fs.free_blocks(), expected_free);
        assert_eq!(observe(&mut fs), expected);
        assert_eq!(disk.image(), image);
        assert_eq!(disk.calls().writes, 0);
        assert_eq!(disk.calls().flushes, 0);
    }
    let mut shared = base;
    let mut older = sb;
    older.sequence -= 1;
    put_root(&mut shared, 0, older);
    let disk = SectorDisk::from_image(shared.clone(), Persistence::Cached);
    let mut fs = CellosFs::open(disk.clone()).expect("two roots may share immutable table and data");
    assert_eq!(observe(&mut fs), expected);
    assert_eq!(fs.free_blocks(), shared.len() as u64 - union(&shared).len() as u64);
    assert_eq!(disk.calls().writes, 0);
    assert_eq!(disk.calls().flushes, 0);
    println!("cow-recovery advisory_bitmap_variants={} shared_roots=2 reconstructed_free={expected_free}", SECTORS + 1);
}

#[test]
fn sequence_inode_and_space_exhaustion_refuse_before_effect_and_poison() {
    let (base, sb, _, _) = corruption_fixture();
    for operation in 0..8 {
        let mut image = base.clone();
        let mut exhausted = sb;
        exhausted.sequence = u64::MAX;
        put_root(&mut image, 1, exhausted);
        let disk = SectorDisk::from_image(image.clone(), Persistence::Cached);
        let mut fs = CellosFs::open(disk.clone()).unwrap();
        disk.arm(None);
        let result = match operation {
            0 => fs.create_file("/new").map(|_| ()),
            1 => fs.create_dir("/new").map(|_| ()),
            2 => fs.write_file("/a", 0, b"x").map(|_| ()),
            3 => fs.replace_file("/a", b"x").map(|_| ()),
            4 => fs.unlink("/a"),
            5 => fs.rmdir("/dir"),
            6 => fs.rename("/a", "/new"),
            _ => fs.sync(),
        };
        assert_eq!(result, Err(FsError::NoSpace), "sequence operation={operation}");
        assert_eq!(disk.calls().writes, 0);
        assert_eq!(disk.calls().flushes, 0);
        assert_eq!(disk.image(), image);
        assert_poisoned(&mut fs, &disk);
    }
    let mut image = base;
    let mut exhausted = sb;
    exhausted.next_inode_num = 257;
    put_root(&mut image, 1, exhausted);
    let disk = SectorDisk::from_image(image.clone(), Persistence::Cached);
    let mut fs = CellosFs::open(disk.clone()).unwrap();
    disk.arm(None);
    assert_eq!(fs.create_file("/last").err(), Some(FsError::NoSpace));
    assert_eq!(disk.calls().writes, 0);
    assert_eq!(disk.image(), image);
    assert_poisoned(&mut fs, &disk);

    let disk = SectorDisk::blank(16, Persistence::Cached);
    let mut fs = CellosFs::format(disk.clone(), 16).unwrap();
    fs.create_file("/file").unwrap();
    fs.sync().unwrap();
    let image = disk.image();
    let prior = observe(&mut fs);
    disk.arm(None);
    // The exact free union complement cannot also pay for the required CoW table.
    let bytes = pattern(fs.free_blocks() as usize * BLOCK_SIZE, 113);
    assert_eq!(fs.replace_file("/file", &bytes), Err(FsError::NoSpace));
    assert_eq!(disk.calls().writes, 0, "reserve complete data+table cost before dispatch");
    assert_eq!(disk.calls().flushes, 0);
    assert_eq!(disk.image(), image);
    assert_poisoned(&mut fs, &disk);
    drop(fs);
    disk.crash();
    let mut recovered = CellosFs::open(disk.clone()).unwrap();
    assert_eq!(observe(&mut recovered), prior);
    println!("cow-exhaustion sequence_mutators=8 inode_limit=256 full_space_pre_effect=true");
}

#[test]
fn unpublished_reservations_are_disjoint_and_reuse_waits_for_durable_displacement() {
    let disk = SectorDisk::blank(32, Persistence::Cached);
    let mut fs = CellosFs::format(disk.clone(), 32).unwrap();
    fs.create_file("/file").unwrap();
    fs.create_file("/other").unwrap();
    fs.replace_file("/file", &pattern(5 * BLOCK_SIZE, 3)).unwrap();
    fs.sync().unwrap();
    let old = disk.image();
    let old_blocks: BTreeSet<_> = reachable(&old, fs.superblock()).into_iter()
        .filter(|b| !fs.superblock().inode_blocks.contains(b)).collect();
    let protected = protected_bytes(&old);
    let free_before = fs.free_blocks();
    disk.arm(None);
    fs.replace_file("/file", &pattern(5 * BLOCK_SIZE, 19)).unwrap();
    let after_first = disk.calls().writes;
    fs.replace_file("/other", &pattern(3 * BLOCK_SIZE, 41)).unwrap();
    let staged = disk.trace();
    let first: BTreeSet<_> = staged.iter().filter_map(|edge| match edge {
        Edge::Sector { write, block, sector: 0, after: false } if *write <= after_first => Some(*block), _ => None,
    }).collect();
    let second: BTreeSet<_> = staged.iter().filter_map(|edge| match edge {
        Edge::Sector { write, block, sector: 0, after: false } if *write > after_first => Some(*block), _ => None,
    }).collect();
    assert_eq!(first.len(), 5);
    assert_eq!(second.len(), 3);
    assert!(first.is_disjoint(&second), "second candidate must exclude unpublished first reservations");
    assert!(first.iter().chain(second.iter()).all(|b| !protected.contains_key(b)));
    assert!(fs.free_blocks() <= free_before - 8, "outstanding reservations charged");
    fs.sync().unwrap(); // Sequence 3 retains sequence 2's five old data blocks.
    let retained = disk.image();
    assert_protected(&protected, &retained, "first replacement durable");
    assert!(old_blocks.is_subset(&union(&retained)));
    assert_eq!(fs.free_blocks(), 32 - union(&retained).len() as u64);
    fs.replace_file("/file", b"inline-retirement").unwrap();
    let before_displacement = disk.image();
    assert!(old_blocks.is_subset(&union(&before_displacement)));
    fs.sync().unwrap(); // Sequence 4 displaces sequence 2 only after FS2 success.
    let displaced = disk.image();
    let live = union(&displaced);
    assert!(old_blocks.is_disjoint(&live));
    assert_eq!(fs.free_blocks(), 32 - live.len() as u64);
    assert!(fs.free_blocks() >= old_blocks.len() as u64 + 2);
    disk.arm(None);
    // Consume all but one free block: retired data cannot all be avoided by allocation.
    let consume = fs.free_blocks() as usize - 1;
    assert!(consume <= INODE_PAYLOAD_SIZE / Extent::SIZE);
    fs.replace_file("/file", &pattern(consume * BLOCK_SIZE, 139)).unwrap();
    let allocated: BTreeSet<_> = disk.trace().iter().filter_map(|edge| match edge {
        Edge::Sector { block, sector: 0, after: false, .. } => Some(*block), _ => None,
    }).collect();
    assert_eq!(allocated.len(), consume);
    assert!(!allocated.is_disjoint(&old_blocks), "retired blocks become usable after durable displacement");
    assert!(allocated.is_disjoint(&live));
    fs.sync().unwrap();
    println!("cow-allocation unpublished_first={} unpublished_second={} retired={} reused={} durable_sequence={}",
        first.len(), second.len(), old_blocks.len(), allocated.intersection(&old_blocks).count(), fs.superblock().sequence);
}

fn prepare_displacement(persistence: Persistence) -> (Fs, SectorDisk, BTreeSet<u64>) {
    let disk = SectorDisk::blank(32, persistence);
    let mut fs = CellosFs::format(disk.clone(), 32).unwrap();
    fs.create_file("/file").unwrap();
    fs.replace_file("/file", &pattern(5 * BLOCK_SIZE, 17)).unwrap();
    fs.sync().unwrap();
    let image = disk.image();
    let retired: BTreeSet<_> = reachable(&image, fs.superblock()).into_iter()
        .filter(|b| !fs.superblock().inode_blocks.contains(b)).collect();
    fs.replace_file("/file", &pattern(5 * BLOCK_SIZE, 53)).unwrap();
    fs.sync().unwrap();
    assert!(retired.is_subset(&union(&disk.image())));
    (fs, disk, retired)
}

#[test]
fn failed_displacement_retains_instance_reservations_even_after_persisted_lost_ack() {
    for persistence in [Persistence::Cached, Persistence::WriteThrough] {
        let (mut golden, control, _) = prepare_displacement(persistence);
        let previous = observe(&mut golden);
        golden.replace_file("/file", b"inline-displacement").unwrap();
        control.arm(None);
        golden.sync().unwrap();
        let trace = control.trace();
        validate_golden(&trace, control.calls(), false);
        let successor = observe(&mut golden);
        for after in [false, true] {
            let cut = trace.iter().position(|edge| *edge == Edge::Flush { barrier: 2, after }).unwrap();
            let (mut fs, disk, retired) = prepare_displacement(persistence);
            let protected = protected_bytes(&disk.image());
            let before = fs.free_blocks();
            fs.replace_file("/file", b"inline-displacement").unwrap();
            disk.arm(Some(cut));
            assert_eq!(fs.sync(), Err(FsError::IoError));
            assert!(disk.0.borrow().fired);
            assert_eq!(disk.trace(), trace[..=cut]);
            assert!(fs.free_blocks() <= before, "ambiguous FS2 cannot release prior union or candidate reservations");
            assert_protected(&protected, &disk.image(), "failed displacement");
            if persistence == Persistence::Cached && !after {
                assert!(retired.is_subset(&union(&disk.image())), "pre-effect FS2 retains old durable root");
            } else {
                assert!(retired.is_disjoint(&union(&disk.image())), "durable displaced root no longer references retired data");
            }
            assert_poisoned(&mut fs, &disk);
            drop(fs);
            disk.crash();
            let calls = disk.calls();
            let mut recovered = CellosFs::open(disk.clone()).unwrap();
            assert_eq!(&observe(&mut recovered),
                if persistence == Persistence::Cached && !after { &previous } else { &successor });
            assert_eq!(disk.calls().writes, calls.writes);
            assert_eq!(disk.calls().flushes, calls.flushes);
            drop(recovered); // No allocation/retry on recovered fault media.
        }
    }
    println!("cow-delayed-reuse fs2_failure_edges=4 poisoned_reservations_retained=true read_only_discard=true");
}
