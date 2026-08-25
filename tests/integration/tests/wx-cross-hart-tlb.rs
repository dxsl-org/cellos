//! RV64 W^X shootdown proof: two harts, physical-byte oracle, negative control.

use std::path::PathBuf;
use vicell_integration_tests::{ci_guard, qemu_binary, skip_notice, QemuRunner};

const BOOT_TIMEOUT: u64 = 25;

fn kernel_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel-test-hooks")
}

fn prerequisites_ok() -> bool {
    let kernel_ok = kernel_path().exists();
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel_ok {
        eprintln!(
            "SKIP wx-cross-hart-tlb: test-hooks kernel missing ({})",
            kernel_path().display()
        );
    }
    ci_guard(kernel_ok && qemu_ok)
}

#[test]
fn rv64_rfence_blocks_the_remote_stale_write() {
    if !prerequisites_ok() {
        return;
    }

    let kernel = kernel_path();
    for iteration in 1..=5 {
        let qemu = QemuRunner::boot_rv64_smp(kernel.to_str().expect("UTF-8 kernel path"), 2);
        if let Err(error) = qemu.wait_for("[smp] hart 1 online", BOOT_TIMEOUT) {
            skip_notice("RUNTIME-GATED: QEMU/OpenSBI did not provide an HSM-startable remote hart");
            if std::env::var_os("CI").is_some() {
                panic!(
                    "RV64 -smp 2 iteration {iteration} did not bring logical hart 1 online: {error}\n{}",
                    qemu.dump()
                );
            }
            return;
        }
        qemu.wait_for("[selftest] TLB-SHOOTDOWN: PASS", BOOT_TIMEOUT)
            .unwrap_or_else(|error| {
                panic!(
                    "RFENCE physical-byte oracle iteration {iteration} failed: {error}\n{}",
                    qemu.dump()
                )
            });

        let output = qemu.dump();
        let boot_zero = output.contains("[smp] physical 0 -> logical 0 boot")
            && output.contains("[smp] physical 1 -> logical 1");
        let boot_one = output.contains("[smp] physical 1 -> logical 0 boot")
            && output.contains("[smp] physical 0 -> logical 1");
        assert!(
            boot_zero || boot_one,
            "iteration {iteration} did not prove distinct physical/logical roles:\n{output}"
        );
        assert!(
            !output.contains("[selftest] TLB-SHOOTDOWN: FAIL"),
            "shootdown self-test iteration {iteration} emitted a failure:\n{output}"
        );
    }
}
