use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct Fixture {
    pub root: PathBuf,
    pub transcript: PathBuf,
    pub summary: PathBuf,
    pub key: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("vf2-root-stream-{}-{id}", std::process::id()));
        fs::create_dir(&root).unwrap();
        for (name, bytes) in [
            ("opensbi.bin", vec![1, 2, 3]),
            ("dtb.bin", vec![4, 5, 6, 7]),
            ("cellos.bin", vec![8, 9, 10, 11, 12]),
            ("vifs.bin", vec![13, 14, 15, 16, 17, 18]),
        ] {
            fs::write(root.join(name), bytes).unwrap();
        }
        let seed = [7u8; 32];
        fs::write(root.join("seed.bin"), seed).unwrap();
        let key = root.join("public-key.bin");
        fs::write(&key, manifest_core::public_key_from_seed(&seed).unwrap()).unwrap();
        Self {
            transcript: root.join("stream.xmodem"),
            summary: root.join("manifest-summary.txt"),
            key,
            root,
        }
    }

    pub fn bundler_args(&self) -> Vec<OsString> {
        let mut args = vec![OsString::from("vf2-root-stream-bundler")];
        for (name, file) in [
            ("--opensbi", "opensbi.bin"),
            ("--dtb", "dtb.bin"),
            ("--cellos", "cellos.bin"),
            ("--vifs", "vifs.bin"),
            ("--seed", "seed.bin"),
        ] {
            pair(&mut args, name, self.root.join(file));
        }
        pair(&mut args, "--transcript-out", &self.transcript);
        pair(&mut args, "--summary-out", &self.summary);
        args.extend(common_args());
        args
    }

    pub fn verifier_args(&self) -> Vec<OsString> {
        let mut args = vec![OsString::from("vf2-root-stream-verifier")];
        pair(&mut args, "--transcript", &self.transcript);
        pair(&mut args, "--public-key", &self.key);
        args.extend(common_args());
        args
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn common_args() -> Vec<OsString> {
    let hex_a = "11".repeat(32);
    let hex_b = "22".repeat(32);
    let hex_c = "33".repeat(32);
    let values = [
        ("--device-id", hex_a.as_str()),
        ("--authority-id", hex_b.as_str()),
        ("--approved-loader-sha256", hex_c.as_str()),
        ("--boot-epoch", "41"),
        ("--request-id", "9"),
        ("--entry-address", "0x80200000"),
        ("--opensbi-load-address", "0x80200000"),
        ("--opensbi-max-load-end", "0x80210000"),
        ("--opensbi-max-size", "65536"),
        ("--dtb-load-address", "0x80400000"),
        ("--dtb-max-load-end", "0x80410000"),
        ("--dtb-max-size", "65536"),
        ("--cellos-load-address", "0x80600000"),
        ("--cellos-max-load-end", "0x80610000"),
        ("--cellos-max-size", "65536"),
        ("--vifs-load-address", "0x80800000"),
        ("--vifs-max-load-end", "0x80810000"),
        ("--vifs-max-size", "65536"),
        ("--usable-dram-base", "0x80000000"),
        ("--usable-dram-end", "0x90000000"),
        ("--loader-range-base", "0x08000000"),
        ("--loader-range-end", "0x08008000"),
        ("--stack-range-base", "0x08010000"),
        ("--stack-range-end", "0x08011000"),
        ("--manifest-scratch-range-base", "0x08012000"),
        ("--manifest-scratch-range-end", "0x08013000"),
        ("--staging-base", "0x88000000"),
        ("--staging-size", "4096"),
        ("--max-transfer-blocks", "4"),
        ("--manifest-bound", "549"),
        ("--max-component-region-length", "1024"),
    ];
    let mut args = Vec::new();
    for (name, value) in values {
        args.push(name.into());
        args.push(value.into());
    }
    args
}

fn pair(args: &mut Vec<OsString>, name: &str, value: impl AsRef<Path>) {
    args.push(name.into());
    args.push(value.as_ref().as_os_str().to_owned());
}
