use std::{env, path::PathBuf, process::Command};
use vicell_integration_tests::{qemu_binary_aarch64, QemuRunner};

const BOOT_TIMEOUT: u64 = 180;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn prerequisites() -> Option<(String, String)> {
    if env::var("TIER3B_GPU_E2E").ok().as_deref() != Some("1") {
        if env::var_os("CI").is_some() {
            panic!(
                "the dedicated Tier3b GPU lane must set TIER3B_GPU_E2E=1 and \
                 invoke this ignored test explicitly"
            );
        }
        eprintln!("SKIP tier3b GPU: set TIER3B_GPU_E2E=1 for the dedicated lane");
        return None;
    }
    let root = repo_root();
    let kernel = root.join("target/aarch64-unknown-none-softfloat/release/vicell-kernel");
    let disk = root.join("disk_hv_arm_gui.img");
    let qemu = Command::new(qemu_binary_aarch64())
        .arg("--version")
        .output()
        .is_ok();
    let ready = kernel.exists() && disk.exists() && qemu;
    vicell_integration_tests::ci_guard(ready);
    ready.then(|| {
        (
            kernel.to_string_lossy().into_owned(),
            disk.to_string_lossy().into_owned(),
        )
    })
}

#[test]
#[ignore = "requires ARM64 KVM/real hardware and TIER3B_GPU_E2E=1"]
fn linux_guest_exercises_virtio_gpu_track_a() {
    let Some((kernel, disk)) = prerequisites() else {
        return;
    };
    let qemu = QemuRunner::boot_tier3b_aarch64(&kernel, &disk);
    for token in [
        "TIER3B_T1_CARD0_OK",
        "TIER3B_T2_FB_LIFECYCLE_OK",
        "TIER3B_T12_XRGB_SCANOUT_OK",
        "[hv-gpu] scanout teardown ok",
    ] {
        qemu.wait_for(token, BOOT_TIMEOUT).unwrap_or_else(|error| {
            panic!("missing {token}: {error}\n--- output ---\n{}", qemu.dump())
        });
    }
}
