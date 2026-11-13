//! Raw disk fixture for the x86 storage lanes: a FAT32 volume at
//! `abi::disk::PART_FAT32_BASE_LBA` and no Cellos MBR, so the CellosFS, `/data`
//! and `/bin` partitions past the volume are simply absent. The same image
//! serves as a SATA (`ide-hd`) or NVMe namespace; the file is removed on drop.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// `abi::disk::PART_FAT32_BASE_LBA` in bytes (LBA 2048).
const FAT_BASE_OFFSET: u64 = 2_048 * 512;
/// `abi::disk::PART_FAT32_SECTORS`.
const FAT_SECTORS: u64 = 524_288;

/// The first working Python 3 interpreter on PATH (the FAT32 formatter is a
/// Python tool).
pub fn python_binary() -> Option<&'static str> {
    ["python", "python3"].into_iter().find(|binary| {
        std::process::Command::new(binary)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn temp_path(kind: &str) -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "vicell_{kind}_{}_{}.img",
        std::process::id(),
        CTR.fetch_add(1, Ordering::Relaxed)
    ))
}

/// A raw disk image holding one freshly formatted FAT32 volume.
pub struct FatDisk(PathBuf);

impl FatDisk {
    /// Format a volume with `tools/mkfat32_inplace.py` and splice it into an
    /// otherwise-empty sparse disk. `None` when the formatter fails.
    pub fn new(python: &str) -> Option<Self> {
        let volume = temp_path("fat32_volume");
        {
            let f = std::fs::File::create(&volume).expect("create FAT32 scratch image");
            f.set_len(FAT_SECTORS * 512).expect("size FAT32 scratch image");
        }
        let status = std::process::Command::new(python)
            .arg(repo_root().join("tools/mkfat32_inplace.py"))
            .arg(&volume)
            .arg(FAT_SECTORS.to_string())
            .status()
            .expect("run mkfat32_inplace.py");
        if !status.success() {
            let _ = std::fs::remove_file(&volume);
            return None;
        }

        let disk = Self(temp_path("fat_disk"));
        {
            let fat_bytes = std::fs::read(&volume).expect("read FAT32 volume");
            let mut d = std::fs::File::create(disk.path()).expect("create disk image");
            d.set_len(FAT_BASE_OFFSET + FAT_SECTORS * 512 + 1024 * 1024)
                .expect("size disk image");
            d.seek(SeekFrom::Start(FAT_BASE_OFFSET))
                .expect("seek to partition base");
            d.write_all(&fat_bytes).expect("write FAT32 volume");
        }
        let _ = std::fs::remove_file(&volume);
        Some(disk)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// The image path as QEMU's `-drive file=` argument.
    pub fn arg(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }

    /// Whether the raw medium holds `needle` anywhere (host-side readback,
    /// independent of the guest's caches).
    pub fn medium_contains(&self, needle: &[u8]) -> bool {
        const CHUNK: usize = 1 << 20;
        let mut file = std::fs::File::open(&self.0).expect("open disk image");
        let mut window = Vec::with_capacity(CHUNK + needle.len());
        let mut chunk = vec![0u8; CHUNK];
        loop {
            let n = file.read(&mut chunk).expect("read disk image");
            if n == 0 {
                return false;
            }
            window.extend_from_slice(&chunk[..n]);
            if window.windows(needle.len()).any(|w| w == needle) {
                return true;
            }
            let keep = window.len().min(needle.len().saturating_sub(1));
            window.drain(..window.len() - keep);
        }
    }
}

impl Drop for FatDisk {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
