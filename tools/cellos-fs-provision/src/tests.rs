use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture { dir: PathBuf, image: PathBuf }
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("cellos-fs-provision-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&dir).unwrap();
        let image = dir.join("disk.img");
        let file = OpenOptions::new().read(true).write(true).create_new(true).open(&image).unwrap();
        file.set_len(1_127_680 * 512).unwrap();
        Self { dir, image }
    }
    fn args(&self, base: u64, intent: Intent, receipt: &str) -> Args {
        Args { image: self.image.clone(), base, sectors: 131072, intent, receipt: self.dir.join(receipt) }
    }
    fn write_at(&self, offset: u64, bytes: &[u8]) {
        let mut file = OpenOptions::new().write(true).open(&self.image).unwrap();
        file.seek(SeekFrom::Start(offset)).unwrap();
        file.write_all(bytes).unwrap();
        file.sync_all().unwrap();
    }
    fn hash(&self) -> String {
        let mut file = File::open(&self.image).unwrap();
        let bytes = file.metadata().unwrap().len();
        hash_range(&mut file, 0, bytes).unwrap()
    }
}
impl Drop for Fixture { fn drop(&mut self) { std::fs::remove_dir_all(&self.dir).unwrap(); } }

#[test]
fn checked_windows_and_block_overflow_refuse() {
    assert!(Window::new(u64::MAX, 131072, u64::MAX).is_err());
    assert!(Window::new(800000, u64::MAX, u64::MAX).is_err());
    assert!(Window::new(2048, 131072, u64::MAX).is_err());
    assert!(Window::new(800000, 131072, 800001 * 512).is_err());
    let window = Window::new(800000, 131072, 1127680 * 512).unwrap();
    assert_eq!(window.offset(window.blocks), Err(FsError::PartitionOverflow));
    assert_eq!(window.offset(u64::MAX), Err(FsError::PartitionOverflow));
}

#[test]
fn fresh_p4_p5_reopen_and_existing_verify_preserve_bytes() {
    let f = Fixture::new();
    // Neighbor partitions/config remain byte-for-byte untouched.
    for offset in [0, 2048 * 512, 526336 * 512, 560000 * 512, 1062144 * 512] {
        f.write_at(offset, b"neighbor-config-sentinel");
    }
    run(f.args(800000, Intent::FreshBlank, "p4.json")).unwrap();
    run(f.args(931072, Intent::FreshBlank, "p5.json")).unwrap();
    let before = f.hash();
    run(f.args(800000, Intent::VerifyExisting, "p4-verify.json")).unwrap();
    run(f.args(931072, Intent::VerifyExisting, "p5-verify.json")).unwrap();
    assert_eq!(f.hash(), before);
    let mut file = File::open(&f.image).unwrap();
    for offset in [0, 2048 * 512, 526336 * 512, 560000 * 512, 1062144 * 512] {
        file.seek(SeekFrom::Start(offset)).unwrap();
        let mut bytes = [0; 24]; file.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"neighbor-config-sentinel");
    }
    assert!(run(f.args(931072, Intent::FreshBlank, "overwrite.json")).is_err());
    assert_eq!(f.hash(), before);
    let receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(f.dir.join("p5-verify.json")).unwrap()).unwrap();
    assert_eq!(receipt["image_sha256"], before);
    assert_eq!(receipt["intent"], "verify-existing");
    assert_eq!(receipt["partition"], "P5");
    for field in ["source_sha256", "formatter_sha256", "partition_sha256"] {
        assert_eq!(receipt[field].as_str().unwrap().len(), 64);
    }
}

#[test]
fn blank_corrupt_and_nonblank_tail_refuse_without_image_writes() {
    let f = Fixture::new();
    let blank = f.hash();
    assert!(run(f.args(931072, Intent::VerifyExisting, "blank.json")).is_err());
    assert_eq!(f.hash(), blank);
    // Blank detection scans the entire selected window, not just superblocks.
    f.write_at((931072 + 131072) * 512 - 1, &[1]);
    let nonblank = f.hash();
    assert!(run(f.args(931072, Intent::FreshBlank, "tail.json")).is_err());
    assert_eq!(f.hash(), nonblank);
    f.write_at((931072 + 131072) * 512 - 1, &[0]);
    run(f.args(931072, Intent::FreshBlank, "valid.json")).unwrap();
    f.write_at(931072 * 512, &[0; BLOCK_SIZE]); // A invalid, B is already empty.
    let corrupt = f.hash();
    assert!(run(f.args(931072, Intent::VerifyExisting, "corrupt.json")).is_err());
    assert_eq!(f.hash(), corrupt);
    assert!(!f.dir.join("corrupt.json").exists());
}

#[test]
fn existing_receipt_and_exclusive_image_lock_refuse_before_mutation() {
    let f = Fixture::new();
    std::fs::write(f.dir.join("taken.json"), b"existing-receipt").unwrap();
    let before = f.hash();
    assert!(run(f.args(800000, Intent::FreshBlank, "taken.json")).is_err());
    assert_eq!(f.hash(), before);
    let lock = OpenOptions::new().read(true).write(true).open(&f.image).unwrap();
    lock.try_lock().unwrap();
    assert!(run(f.args(800000, Intent::FreshBlank, "locked.json")).is_err());
    assert_eq!(f.hash(), before);
    drop(lock);
}
