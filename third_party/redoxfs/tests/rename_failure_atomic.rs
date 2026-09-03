mod support;

use redoxfs::{FileSystem, Node, TreePtr, HEADER_RING};
use support::FailingMemDisk;
use syscall::error::{EIO, ENOENT};

const SECTOR_BYTES: usize = 512;
const SENTINEL: &[u8] = b"atomic-rename-sentinel\0with-exact-bytes";

type Fs = FileSystem<FailingMemDisk>;

fn open(disk: &FailingMemDisk) -> Fs {
    FileSystem::open(disk.clone(), None, Some(0), false).expect("remount image")
}

fn assert_node(fs: &mut Fs, name: &str, mode: u16, bytes: Option<&[u8]>) {
    fs.tx(|tx| {
        let node = tx.find_node(TreePtr::root(), name)?;
        assert_eq!(node.data().mode() & Node::MODE_TYPE, mode);
        if let Some(expected) = bytes {
            assert_eq!(node.data().size(), expected.len() as u64);
            let mut actual = vec![0; expected.len()];
            let count = tx.read_node_inner(&node, 0, &mut actual)?;
            assert_eq!(count, expected.len());
            assert_eq!(actual, expected);
        }
        Ok(())
    })
    .expect("expected node and exact contents");
}

fn assert_missing(fs: &mut Fs, name: &str) {
    let error = fs
        .tx(|tx| tx.find_node(TreePtr::root(), name))
        .expect_err("node must be absent");
    assert_eq!(error.errno, ENOENT);
}

fn assert_base(fs: &mut Fs, generation: u64) {
    assert_eq!(fs.header.generation(), generation);
    assert_node(fs, "source", Node::MODE_FILE, Some(SENTINEL));
    assert_node(fs, "dir", Node::MODE_DIR, None);
    assert_node(fs, "occupied", Node::MODE_FILE, Some(&[]));
    assert_missing(fs, "target");
    assert_missing(fs, "moved-dir");
}

fn sweep(
    disk: &FailingMemDisk,
    image: &[u8],
    generation: u64,
    source: &str,
    target: &str,
    mode: u16,
    bytes: Option<&[u8]>,
) {
    disk.restore(image);
    let mut fs = open(disk);
    disk.arm(None);
    fs.tx(|tx| tx.rename_node_no_replace(TreePtr::root(), source, TreePtr::root(), target))
        .expect("calibration rename");
    let (calls, sector_writes) = disk.trace();
    assert!(!calls.is_empty());
    let observed = calls
        .iter()
        .map(|call| call.1.div_ceil(SECTOR_BYTES))
        .sum::<usize>();
    assert_eq!(observed, sector_writes);
    assert!(calls[..calls.len() - 1]
        .iter()
        .all(|call| call.0 >= HEADER_RING));
    assert!(calls.last().unwrap().0 < HEADER_RING, "header must commit last");
    assert_eq!(calls.iter().filter(|call| call.0 < HEADER_RING).count(), 1);
    drop(fs);

    let mut remounted = open(disk);
    assert!(remounted.header.generation() > generation);
    assert_missing(&mut remounted, source);
    assert_node(&mut remounted, target, mode, bytes);
    drop(remounted);

    for ordinal in 1..=sector_writes {
        disk.restore(image);
        let mut fs = open(disk);
        disk.arm(Some(ordinal));
        let error = fs
            .tx(|tx| tx.rename_node_no_replace(TreePtr::root(), source, TreePtr::root(), target))
            .expect_err("injected sector failure must abort rename");
        assert_eq!(error.errno, EIO);
        drop(fs);
        disk.arm(None);
        let mut remounted = open(disk);
        assert_eq!(remounted.header.generation(), generation);
        assert_node(&mut remounted, source, mode, bytes);
        assert_missing(&mut remounted, target);
    }
}

fn validate_rejection(
    disk: &FailingMemDisk,
    image: &[u8],
    generation: u64,
    source: &str,
    target: &str,
) {
    disk.restore(image);
    let mut fs = open(disk);
    disk.arm(None);
    let result = fs.tx(|tx| {
        tx.rename_node_no_replace(TreePtr::root(), source, TreePtr::root(), target)
    });
    drop(fs);
    let trace = disk.trace();
    let mut remounted = open(disk);
    assert_base(&mut remounted, generation);
    assert!(result.is_err(), "{source:?} -> {target:?} must be rejected");
    assert_eq!(trace, (Vec::new(), 0), "rejection must precede writes");
}

#[test]
fn rename_is_failure_atomic_after_every_sector_write() {
    let disk = FailingMemDisk::new();
    let mut fs = FileSystem::create(disk.clone(), None, 0, 0).expect("format disk");
    let root = TreePtr::root();
    let source = fs
        .tx(|tx| tx.create_node(root, "source", Node::MODE_FILE | 0o644, 0, 0))
        .expect("create source");
    fs.tx(|tx| tx.write_node(source.ptr(), 0, SENTINEL, 0, 0))
        .expect("write sentinel");
    fs.tx(|tx| tx.create_node(root, "occupied", Node::MODE_FILE | 0o644, 0, 0))
        .expect("create occupied destination");
    fs.tx(|tx| tx.create_node(root, "dir", Node::MODE_DIR | 0o755, 0, 0))
        .expect("create directory");
    let generation = fs.header.generation();
    drop(fs);
    let image = disk.image();

    sweep(
        &disk,
        &image,
        generation,
        "source",
        "target",
        Node::MODE_FILE,
        Some(SENTINEL),
    );
    sweep(
        &disk,
        &image,
        generation,
        "dir",
        "moved-dir",
        Node::MODE_DIR,
        None,
    );
    validate_rejection(&disk, &image, generation, "source", "occupied");
    validate_rejection(&disk, &image, generation, "source", "dir");
    validate_rejection(&disk, &image, generation, "missing", "target");
    validate_rejection(&disk, &image, generation, "", "target");
}
