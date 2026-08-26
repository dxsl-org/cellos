use std::time::Duration;
use vicell_integration_tests::QemuRunner;

fn main() {
    let root = "/home/dmin/cellos";
    let kernel = format!("{root}/target/riscv64gc-unknown-none-elf/release/vicell-kernel");
    let mode = std::env::args().nth(1);
    let probe_mode = matches!(mode.as_deref(), Some("probe" | "denial" | "inspect"));
    let disk = if probe_mode {
        "/tmp/cellos-a2a3-probe-0763e8a5.img".to_owned()
    } else {
        format!("{root}/disk_v3.img")
    };
    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel, &disk);
    qemu.wait_for("=== ViCell shell ready", 45)
        .unwrap_or_else(|error| panic!("shell: {error}\n{}", qemu.dump()));
    std::thread::sleep(Duration::from_secs(1));
    if mode.as_deref() == Some("shell") {
        qemu.send_line("echo A2A3_INPUT_OK");
        qemu.wait_for("USER: A2A3_INPUT_OK", 20)
            .unwrap_or_else(|error| panic!("shell input: {error}\n{}", qemu.dump()));
        println!("{}", qemu.dump());
        return;
    }
    if probe_mode {
        qemu.send_line(if mode.as_deref() == Some("inspect") {
            "ls /mnt/sd"
        } else {
            "exec /mnt/sd/OOMPROBE"
        });
        if mode.as_deref() == Some("inspect") {
            std::thread::sleep(Duration::from_secs(10));
            println!("{}", qemu.dump());
            return;
        }
        qemu.wait_for("[a2a3-probe] MEMINFO_DENIED", 30)
            .unwrap_or_else(|error| panic!("denial probe: {error}\n{}", qemu.dump()));
        if mode.as_deref() == Some("denial") {
            println!("{}", qemu.dump());
            return;
        }
        qemu.wait_for("[a2a3-probe] OOM_TYPED", 120)
            .unwrap_or_else(|error| panic!("OOM probe: {error}\n{}", qemu.dump()));
        qemu.send_line("echo A2A3_SHELL_OK_AFTER_OOM");
        qemu.wait_for("USER: A2A3_SHELL_OK_AFTER_OOM", 20)
            .unwrap_or_else(|error| panic!("shell recovery: {error}\n{}", qemu.dump()));
    } else {
        qemu.send_line("bench");
        qemu.wait_for("BENCHMARK SUITE COMPLETE", 240)
            .unwrap_or_else(|error| panic!("bench: {error}\n{}", qemu.dump()));
    }
    println!("{}", qemu.dump());
}
