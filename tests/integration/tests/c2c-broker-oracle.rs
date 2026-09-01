use std::env;

use vicell_integration_tests::QemuRunner;

const BOOT_TIMEOUT_SECS: u64 = 120;
const ORACLE_TIMEOUT_SECS: u64 = 600;
const PREFIX: &str = "[c2c-broker-oracle]";

fn oracle_lines(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter_map(|line| line.find(PREFIX).map(|offset| &line[offset..]))
        .collect()
}

fn assert_line(lines: &[&str], required: &[&str]) {
    assert!(
        lines
            .iter()
            .any(|line| required.iter().all(|field| line.contains(field))),
        "missing oracle result with fields: {required:?}\nobserved oracle lines:\n{}",
        lines.join("\n")
    );
}

fn assert_positive_field(lines: &[&str], marker: &str, field: &str) {
    let value = lines
        .iter()
        .find(|line| line.contains(marker))
        .and_then(|line| {
            line.split_whitespace()
                .find_map(|token| token.strip_prefix(field)?.parse::<u64>().ok())
        })
        .unwrap_or_else(|| panic!("missing numeric {field} on {marker} line"));
    assert!(value > 0, "{field} must be positive on {marker} line");
}

#[test]
fn local_c2c_broker_oracle_meets_baseline_contract() {
    let kernel = env::var("CELLOS_C2C_ORACLE_KERNEL")
        .expect("CELLOS_C2C_ORACLE_KERNEL must name the isolated oracle kernel");
    let disk = env::var("CELLOS_C2C_ORACLE_DISK")
        .expect("CELLOS_C2C_ORACLE_DISK must name the isolated disk copy");

    let mut qemu = QemuRunner::boot_restricted(&kernel, &disk);
    qemu.wait_for("Cellos >", BOOT_TIMEOUT_SECS)
        .unwrap_or_else(|error| panic!("oracle shell did not boot: {error}\n{}", qemu.dump()));
    let command_checkpoint = qemu.output_checkpoint();
    qemu.send_line("bench c2c-broker-oracle");
    qemu.wait_for(
        "[c2c-broker-oracle] overflow status=PASS",
        ORACLE_TIMEOUT_SECS,
    )
    .unwrap_or_else(|error| {
        panic!(
            "oracle did not reach a passing overflow result: {error}\n{}",
            qemu.dump()
        )
    });
    qemu.wait_for_after("Cellos >", command_checkpoint, 30)
        .unwrap_or_else(|error| {
            panic!(
                "oracle process did not return to the shell: {error}\n{}",
                qemu.dump()
            )
        });

    let output = qemu.dump();
    assert!(
        output.contains("[net-broker] restart oracle drained runtime roles")
            && output.contains("[net-broker] restart oracle shutdown-after-admission exercised"),
        "restart passed without exercising and draining an admitted network IPC\n{output}"
    );
    assert!(
        output.contains("[net-rx-producer] irq->completion PASS"),
        "oracle passed without proving interrupt-driven NET_RX completion\n{output}"
    );
    for forbidden in [
        "[net-broker] restart oracle role drain timed out",
        "[net-broker] beacon IPC timed out; network disabled until restart",
        "[heartbeat] task ",
        "[watchdog] task ",
    ] {
        assert!(
            !output.contains(forbidden),
            "oracle observed runtime termination marker {forbidden:?}\n{output}"
        );
    }
    let lines = oracle_lines(&output);
    assert_line(&lines, &[" baseline ", "calibration=MEASURED"]);
    assert_line(&lines, &["role_gate=PASS"]);
    for sweep in [
        "sweep n=1 ",
        "sweep n=2 ",
        "sweep n=4 ",
        "sweep n=8 ",
        "sweep n=16 ",
    ] {
        assert_line(&lines, &[sweep]);
    }
    assert_line(
        &lines,
        &[
            "soak attempted=10000 ",
            "success=10000 ",
            "silent_drop=0 ",
            "network_progress_delta=",
            "heartbeat_miss_delta=0 ",
            "watchdog_expired_delta=0",
        ],
    );
    assert_positive_field(&lines, "soak attempted=10000 ", "network_progress_delta=");
    assert_line(&lines, &["overflow status=PASS"]);
    assert_line(&lines, &["restart status=PASS"]);
    assert!(
        lines.iter().all(|line| !line.contains("BLOCKED")),
        "oracle emitted a blocked result:\n{}",
        lines.join("\n")
    );

    for line in lines {
        println!("{line}");
    }
}
