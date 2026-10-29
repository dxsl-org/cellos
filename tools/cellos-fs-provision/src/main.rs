//! Offline development image preparation, not genesis, migration or durability admission.
use cellos_fs::{BlockDevice, CellosFs, FsError, BLOCK_SIZE};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const SECTOR_SIZE: u64 = 512;
const USAGE: &str = "cellos-fs-provision --image PATH --base LBA --sectors COUNT --intent fresh-blank|verify-existing --receipt PATH [--seed DIR]\nOffline only: stop all users of this disposable/staged image. fresh-blank refuses every nonzero selected byte; verify-existing never writes the image. P4: --base 800000 --sectors 131072; P5: --base 931072 --sectors 131072. Receipts are exclusive new outputs. --seed (fresh-blank only) copies a host directory tree (directories and regular files, no links) into the new volume for disposable test fixtures.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Intent { FreshBlank, VerifyExisting }
impl Intent {
    fn name(self) -> &'static str {
        match self { Self::FreshBlank => "fresh-blank", Self::VerifyExisting => "verify-existing" }
    }
}
struct Args { image: PathBuf, base: u64, sectors: u64, intent: Intent, receipt: PathBuf, seed: Option<PathBuf> }
fn args() -> Result<Args, String> {
    let mut image = None;
    let mut base = None;
    let mut sectors = None;
    let mut intent = None;
    let mut receipt = None;
    let mut seed = None;
    let mut it = std::env::args().skip(1);
    while let Some(key) = it.next() {
        let value = it.next().ok_or_else(|| format!("missing value for {key}\n{USAGE}"))?;
        match key.as_str() {
            "--image" if image.is_none() => image = Some(PathBuf::from(value)),
            "--base" if base.is_none() => base = Some(value.parse::<u64>().map_err(|_| "invalid base LBA")?),
            "--sectors" if sectors.is_none() => sectors = Some(value.parse::<u64>().map_err(|_| "invalid sector count")?),
            "--intent" if intent.is_none() => intent = Some(match value.as_str() {
                "fresh-blank" => Intent::FreshBlank,
                "verify-existing" => Intent::VerifyExisting,
                _ => return Err("intent must be fresh-blank or verify-existing".into()),
            }),
            "--receipt" if receipt.is_none() => receipt = Some(PathBuf::from(value)),
            "--seed" if seed.is_none() => seed = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown or repeated option {key}\n{USAGE}")),
        }
    }
    let intent = intent.ok_or(USAGE)?;
    if seed.is_some() && intent != Intent::FreshBlank {
        return Err("--seed is only valid with fresh-blank".into());
    }
    Ok(Args {
        image: image.ok_or(USAGE)?, base: base.ok_or(USAGE)?, sectors: sectors.ok_or(USAGE)?,
        intent, receipt: receipt.ok_or(USAGE)?, seed,
    })
}

/// Copy `host` (directories and regular files, children in byte order) into `fs` under
/// `at`. Links and other kinds are refused: a fixture must not depend on host state.
fn seed_tree<D: BlockDevice>(fs: &mut CellosFs<D>, host: &Path, at: &str) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(host).map_err(|e| e.to_string())?
        .collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().into_string().map_err(|_| "seed names must be UTF-8")?;
        let path = format!("{at}/{name}");
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            fs.create_dir(&path).map_err(|e| format!("seed mkdir {path}: {e:?}"))?;
            seed_tree(fs, &entry.path(), &path)?;
        } else if kind.is_file() {
            let data = std::fs::read(entry.path()).map_err(|e| e.to_string())?;
            fs.create_file(&path).map_err(|e| format!("seed create {path}: {e:?}"))?;
            let written = fs.write_file(&path, 0, &data).map_err(|e| format!("seed write {path}: {e:?}"))?;
            if written != data.len() {
                return Err(format!("seed write {path}: short write {written}/{}", data.len()));
            }
        } else {
            return Err(format!("seed entry {path} is neither a directory nor a regular file"));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Window { start: u64, bytes: u64, blocks: u64 }
impl Window {
    fn new(base: u64, sectors: u64, image_bytes: u64) -> Result<Self, String> {
        // Only the admitted constant-addressed native windows, not guessed FAT layouts.
        if !matches!(base, 800_000 | 931_072) || sectors != 131_072 {
            return Err("selected window must be canonical P4 or P5 (131072 sectors)".into());
        }
        let start = base.checked_mul(SECTOR_SIZE).ok_or("base byte overflow")?;
        let bytes = sectors.checked_mul(SECTOR_SIZE).ok_or("length byte overflow")?;
        let end = start.checked_add(bytes).ok_or("partition end overflow")?;
        if start % BLOCK_SIZE as u64 != 0 || bytes % BLOCK_SIZE as u64 != 0 || end > image_bytes {
            return Err("unaligned or out-of-image partition window".into());
        }
        Ok(Self { start, bytes, blocks: bytes / BLOCK_SIZE as u64 })
    }
    fn offset(self, block: u64) -> Result<u64, FsError> {
        if block >= self.blocks { return Err(FsError::PartitionOverflow); }
        self.start.checked_add(block.checked_mul(BLOCK_SIZE as u64).ok_or(FsError::PartitionOverflow)?)
            .ok_or(FsError::PartitionOverflow)
    }
}
struct HostDisk { file: File, window: Window, writable: bool }
impl BlockDevice for HostDisk {
    fn read_block(&mut self, block: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        let offset = self.window.offset(block)?;
        self.file.seek(SeekFrom::Start(offset)).map_err(|_| FsError::IoError)?;
        self.file.read_exact(buf).map_err(|_| FsError::IoError)
    }
    fn write_block(&mut self, block: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if !self.writable { return Err(FsError::IoError); }
        let offset = self.window.offset(block)?;
        self.file.seek(SeekFrom::Start(offset)).map_err(|_| FsError::IoError)?;
        self.file.write_all(buf).map_err(|_| FsError::IoError)
    }
    fn flush(&mut self) -> Result<(), FsError> {
        if !self.writable { return Err(FsError::IoError); }
        self.file.sync_all().map_err(|_| FsError::IoError)
    }
    fn total_blocks(&self) -> u64 { self.window.blocks }
}
fn hex(bytes: impl AsRef<[u8]>) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.as_ref().len() * 2);
    for byte in bytes.as_ref() {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 15) as usize] as char);
    }
    result
}
fn hash_range(file: &mut File, start: u64, bytes: u64) -> Result<String, String> {
    file.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut remaining = bytes;
    let mut hash = Sha256::new();
    let mut buf = [0; 65536];
    while remaining != 0 {
        let count = remaining.min(buf.len() as u64) as usize;
        file.read_exact(&mut buf[..count]).map_err(|e| e.to_string())?;
        hash.update(&buf[..count]);
        remaining -= count as u64;
    }
    Ok(hex(hash.finalize()))
}
fn require_blank(file: &mut File, window: Window) -> Result<(), String> {
    file.seek(SeekFrom::Start(window.start)).map_err(|e| e.to_string())?;
    let mut remaining = window.bytes;
    let mut buf = [0; 65536];
    while remaining != 0 {
        let count = remaining.min(buf.len() as u64) as usize;
        file.read_exact(&mut buf[..count]).map_err(|e| e.to_string())?;
        if buf[..count].iter().any(|b| *b != 0) {
            return Err("selected partition is nonblank; formatting refused (use verify-existing for valid V1, otherwise explicit offline migration is required)".into());
        }
        remaining -= count as u64;
    }
    Ok(())
}
// Embedded exact library and formatter source identity, not an unverified git label.
fn source_identity() -> String {
    let mut hash = Sha256::new();
    for (name, bytes) in [
        ("formatter/Cargo.toml", include_bytes!("../Cargo.toml").as_slice()),
        ("formatter/Cargo.lock", include_bytes!("../Cargo.lock").as_slice()),
        ("formatter/main.rs", include_bytes!("main.rs").as_slice()),
        ("cellos-fs/Cargo.toml", include_bytes!("../../../libs/cellos-fs/Cargo.toml").as_slice()),
        ("cellos-fs/lib.rs", include_bytes!("../../../libs/cellos-fs/src/lib.rs").as_slice()),
        ("cellos-fs/engine.rs", include_bytes!("../../../libs/cellos-fs/src/engine.rs").as_slice()),
        ("cellos-fs/disk.rs", include_bytes!("../../../libs/cellos-fs/src/disk.rs").as_slice()),
        ("cellos-fs/format.rs", include_bytes!("../../../libs/cellos-fs/src/format.rs").as_slice()),
        ("cellos-fs/allocator.rs", include_bytes!("../../../libs/cellos-fs/src/allocator.rs").as_slice()),
        ("cellos-fs/inode.rs", include_bytes!("../../../libs/cellos-fs/src/inode.rs").as_slice()),
        ("cellos-fs/crc32.rs", include_bytes!("../../../libs/cellos-fs/src/crc32.rs").as_slice()),
    ] {
        hash.update((name.len() as u64).to_le_bytes()); hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes()); hash.update(bytes);
    }
    hex(hash.finalize())
}
fn verify(file: &File, window: Window) -> Result<(u64, u64), String> {
    // Duplicate the locked handle; a separate Windows open cannot read locked bytes.
    let disk = HostDisk { file: file.try_clone().map_err(|e| e.to_string())?, window, writable: false };
    let mut fs = CellosFs::open(disk).map_err(|e| format!("read-only reopen refused: {e:?}"))?;
    let root = fs.lookup("/").map_err(|e| format!("root verification failed: {e:?}"))?;
    if !root.is_dir() { return Err("root is not a directory".into()); }
    let sequence = fs.superblock().sequence;
    Ok((sequence, fs.free_blocks()))
}
fn run(args: Args) -> Result<(), String> {
    // Refuse existing receipts before any image mutation; create_new also closes the publication race.
    if args.receipt.try_exists().map_err(|e| e.to_string())? {
        return Err("receipt already exists; select a new exclusive receipt output".into());
    }
    let image = args.image.canonicalize().map_err(|e| e.to_string())?;
    let mut file = OpenOptions::new().read(true).write(args.intent == Intent::FreshBlank)
        .open(&image).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("only regular host image files are permitted, not devices".into());
    }
    // Held through format, host sync, read-only reopen and receipt publication. Offline
    // intent still requires excluding QEMU and other writers that do not honor locks.
    file.try_lock().map_err(|e| format!("exclusive image lock refused: {e}"))?;
    let image_bytes = file.metadata().map_err(|e| e.to_string())?.len();
    let window = Window::new(args.base, args.sectors, image_bytes)?;
    let formatter = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut executable = File::open(&formatter).map_err(|e| e.to_string())?;
    let executable_bytes = executable.metadata().map_err(|e| e.to_string())?.len();
    let formatter_sha256 = hash_range(&mut executable, 0, executable_bytes)?;
    let image_before = hash_range(&mut file, 0, image_bytes)?;
    if args.intent == Intent::FreshBlank {
        require_blank(&mut file, window)?;
        let disk = HostDisk { file: file.try_clone().map_err(|e| e.to_string())?, window, writable: true };
        // Any error ends mutation permanently on this disposable/staged image. No retry,
        // repair, alternate formatter or runtime fallback is permitted.
        let mut fs = CellosFs::format(disk, window.blocks)
            .map_err(|e| format!("format failed: {e:?}; preserve for read-only observation then discard image"))?;
        if let Some(seed) = &args.seed {
            seed_tree(&mut fs, seed, "").map_err(|e| format!("{e}; discard image"))?;
            fs.sync().map_err(|e| format!("seed sync failed: {e:?}; discard image"))?;
        }
        drop(fs);
        file.sync_all().map_err(|e| format!("host sync failed: {e}; discard image after read-only observation"))?;
    }
    let (sequence, free_blocks) = verify(&file, window)?;
    let partition_sha256 = hash_range(&mut file, window.start, window.bytes)?;
    let image_after = hash_range(&mut file, 0, image_bytes)?;
    if args.intent == Intent::VerifyExisting && image_before != image_after {
        return Err("image changed during read-only verification".into());
    }
    let receipt = serde_json::json!({
        "format": "cellos-fs-development-provisioning-v1", "intent": args.intent.name(),
        "image": image, "image_bytes": image_bytes, "image_before_sha256": image_before,
        "image_sha256": image_after, "source_sha256": source_identity(),
        "source_digest_encoding": "ordered length-prefixed names and bytes; u64 little-endian",
        "formatter": formatter, "formatter_version": env!("CARGO_PKG_VERSION"),
        "formatter_sha256": formatter_sha256,
        "partition": if args.base == 800000 { "P4" } else { "P5" },
        "base_lba": args.base, "sectors": args.sectors, "block_size": BLOCK_SIZE,
        "partition_sha256": partition_sha256, "sequence": sequence, "free_blocks": free_blocks,
        "reopen": "read-only V1 geometry and root verified",
        "seed": args.seed.as_ref().map(|s| s.display().to_string()),
        "authority": "development preparation only; no genesis, ledger migration, durability, protected storage or production admission"
    });
    let bytes = serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?;
    let mut output = OpenOptions::new().write(true).create_new(true).open(&args.receipt)
        .map_err(|e| format!("receipt publication failed: {e}"))?;
    output.write_all(&bytes).and_then(|_| output.write_all(b"\n")).and_then(|_| output.sync_all())
        .map_err(|e| format!("receipt publication failed: {e}"))?;
    println!("CellosFS {} {} verified; receipt {}", args.intent.name(), image.display(), args.receipt.display());
    Ok(())
}
fn main() {
    if std::env::args().nth(1).as_deref() == Some("--help") { println!("{USAGE}"); return; }
    if let Err(error) = args().and_then(run) { eprintln!("cellos-fs-provision: {error}"); std::process::exit(1); }
}

#[cfg(test)]
mod tests;
