//! Phase 13 step 5: a supervisor-orchestrated hot-swap of `config` keeps its state.
//!
//! The ISO comes from `scripts/build-x86_64-config-hotswap-ci.sh`: boot services
//! with `config` and the supervisor running, plus `/bin/hotswap` and the private
//! `/bin/p03-filter config-probe` reader. From the serial shell the test writes a
//! key, swaps `config` onto `/bin/config`, and then requires:
//! - the key reads back after the swap (state crossed the kernel stash);
//! - `config` runs under a new tid (ps), and init adopted that tid;
//! - init neither restarted nor re-registered `config`.
//! `VICELL_CONFIG_HOTSWAP_ISO` pins the ISO (default: the builder's output path).

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary_x86, QemuRunner};

const BOOT_TIMEOUT: u64 = 90;
const COMMAND_TIMEOUT: u64 = 30;
/// The supervisor's bounded snapshot/freeze/spawn/restore/commit sequence.
const SWAP_TIMEOUT: u64 = 60;
const PROMPT: &str = "Cellos >";
const CONFIG_BANNER: &str = "[config] Config Service v0.4";
const KEY: &str = "P13KEY";
const VALUE: &str = "swapped-value";
const SWAPPED: &str = "hotswap: success: supervisor swapped service 'config' to /bin/config";
const SNAPSHOT: &str = "[config] state handed to the hot-swap replacement";
const RESTORED: &str = "[config] state restored from the previous incarnation";
/// ASCII only: the runner's serial dump decodes byte by byte, so a non-ASCII
/// marker would never match.
const ADOPTED: &str = "Init: config hot-swapped: adopted tid ";

fn iso_path() -> String {
    match std::env::var("VICELL_CONFIG_HOTSWAP_ISO") {
        Ok(path) if !path.is_empty() => path,
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../build/vicell-x86-config-hotswap.iso")
            .to_string_lossy()
            .into_owned(),
    }
}

fn prerequisites_ok() -> bool {
    let iso_exists = PathBuf::from(iso_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_x86())
        .arg("--version")
        .output()
        .is_ok();
    if !iso_exists {
        eprintln!(
            "SKIP config-hotswap-x86: ISO not built ({}); run scripts/build-x86_64-config-hotswap-ci.sh",
            iso_path()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP config-hotswap-x86: qemu-system-x86_64 not found");
    }
    vicell_integration_tests::ci_guard(iso_exists && qemu_ok)
}

/// Run `line` at the prompt and return everything it printed up to the next prompt.
fn run(qemu: &mut QemuRunner, line: &str, marker: &str, timeout: u64) -> String {
    let start = qemu.output_checkpoint();
    qemu.send_line(line);
    for pattern in [marker, PROMPT] {
        qemu.wait_for_after(pattern, start, timeout)
            .unwrap_or_else(|e| {
                panic!(
                    "`{line}`: {pattern:?} missing: {e}\n--- serial output ---\n{}",
                    qemu.dump()
                )
            });
    }
    // The prompt that closed the command, not the one echoed before it.
    let dump = qemu.dump();
    let fresh = &dump[start..];
    let after_marker = fresh.find(marker).expect("marker seen above");
    let end = fresh[after_marker..]
        .find(PROMPT)
        .map_or(fresh.len(), |prompt| after_marker + prompt);
    fresh[..end].to_string()
}

/// The tid of the one live `config` task in `/bin/ps` output.
fn config_tid(ps: &str) -> u64 {
    let live: Vec<u64> = ps
        .lines()
        .filter_map(|line| {
            // Serial console lines from a Cell carry a `USER:` prefix.
            let fields: Vec<&str> = line
                .split_whitespace()
                .skip_while(|field| *field == "USER:")
                .collect();
            match fields.as_slice() {
                [tid, state, "config"] if *state != "Dead" => tid.parse().ok(),
                _ => None,
            }
        })
        .collect();
    assert_eq!(live.len(), 1, "expected one live config task in:\n{ps}");
    live[0]
}

#[test]
fn a_config_hot_swap_keeps_the_store_and_init_adopts_the_new_tid() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_x86_bios(&iso_path());
    for marker in [CONFIG_BANNER, PROMPT] {
        qemu.wait_for(marker, BOOT_TIMEOUT).unwrap_or_else(|e| {
            panic!("boot: {marker:?} missing: {e}\n--- serial output ---\n{}", qemu.dump())
        });
    }
    std::thread::sleep(std::time::Duration::from_millis(500));

    let before = config_tid(&run(&mut qemu, "/bin/ps", "NAME", COMMAND_TIMEOUT));
    run(&mut qemu, &format!("export {KEY}={VALUE}"), PROMPT, COMMAND_TIMEOUT);
    run(&mut qemu, "echo exported=$?", "exported=0", COMMAND_TIMEOUT);
    let read = format!("/bin/p03-filter config-probe get {KEY}");
    let probe = format!("CFGPROBE get {KEY} value:{VALUE}");
    run(&mut qemu, &read, &probe, COMMAND_TIMEOUT);

    let swap_start = qemu.output_checkpoint();
    run(&mut qemu, "/bin/hotswap config /bin/config", SWAPPED, SWAP_TIMEOUT);
    qemu.wait_for_after(ADOPTED, swap_start, COMMAND_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!("init did not adopt the new config: {e}\n--- serial output ---\n{}", qemu.dump())
        });

    run(&mut qemu, &read, &probe, COMMAND_TIMEOUT);
    let after = config_tid(&run(&mut qemu, "/bin/ps", "NAME", COMMAND_TIMEOUT));
    assert_ne!(before, after, "config kept its tid across the swap");

    let dump = qemu.dump();
    let swap = &dump[swap_start..];
    let adopted = swap
        .lines()
        .find(|line| line.contains(ADOPTED))
        .expect("adoption line seen above");
    assert!(
        adopted.contains(&format!("adopted tid {after} (was {before})")),
        "init adopted another tid than ps reports: {adopted:?} (ps: {before} -> {after})"
    );
    for marker in [SNAPSHOT, RESTORED] {
        assert!(swap.contains(marker), "{marker:?} missing after the swap\n{dump}");
    }
    for refused in [
        "Init: config retirement",
        "Init: service died",
        "Init: service restarted.",
        "death unwatched",
        "denied for tid",
    ] {
        assert!(
            !swap.contains(refused),
            "{refused:?} after the swap: init restarted or a syscall was denied\n{dump}"
        );
    }
    assert_eq!(
        dump.matches(CONFIG_BANNER).count(),
        2,
        "config must start exactly twice: at boot and as the replacement\n{dump}"
    );
    let evidence: Vec<&str> = dump
        .lines()
        .filter(|line| {
            [CONFIG_BANNER, SNAPSHOT, RESTORED, SWAPPED, ADOPTED, "CFGPROBE"]
                .iter()
                .any(|needle| line.contains(needle))
        })
        .collect();
    println!(
        "config-hotswap-x86 evidence (config tid {before} -> {after}):\n{}",
        evidence.join("\n")
    );
}
