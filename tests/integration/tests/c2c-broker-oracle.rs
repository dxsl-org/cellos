use std::env;

use vicell_integration_tests::QemuRunner;

const BOOT_TIMEOUT_SECS: u64 = 120;
const ORACLE_TIMEOUT_SECS: u64 = 600;
const PREFIX: &str = "[c2c-broker-oracle]";
const IPC_PENDING_PASS: &str =
    "[selftest] IPC-PENDING: PASS (deferred, bounded, quota-safe, completion-wake)";
const IPC_PENDING_FAIL: &str = "[selftest] IPC-PENDING: FAIL";
const NET_RX_RESERVATION_PASS: &str =
    "[selftest] NET-RX-RESERVATION: PASS (fills, remembers, releases, IPC-safe)";
const NET_RX_RESERVATION_FAIL: &str = "[selftest] NET-RX-RESERVATION: FAIL";

fn oracle_lines(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter_map(|line| line.find(PREFIX).map(|offset| &line[offset..]))
        .collect()
}

fn has_token(line: &str, expected: &str) -> bool {
    line.split_whitespace().any(|token| token == expected)
}

fn has_field_value(line: &str, expected: &str) -> bool {
    line.split_whitespace().any(|token| {
        token
            .split_once('=')
            .is_some_and(|(_, value)| value == expected)
    })
}

fn numeric_field(line: &str, field: &str) -> Option<u64> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(field)?.parse().ok())
}

fn assert_tokens(lines: &[&str], required: &[&str]) {
    assert!(
        lines
            .iter()
            .any(|line| required.iter().all(|token| has_token(line, token))),
        "missing oracle result with tokens: {required:?}\nobserved oracle lines:\n{}",
        lines.join("\n")
    );
}

fn assert_positive_field(lines: &[&str], required: &[&str], field: &str) {
    let value = lines
        .iter()
        .find(|line| required.iter().all(|token| has_token(line, token)))
        .and_then(|line| numeric_field(line, field))
        .unwrap_or_else(|| panic!("missing numeric {field} on {required:?} line"));
    assert!(value > 0, "{field} must be positive on {required:?} line");
}

fn assert_no_oracle_failures(lines: &[&str]) {
    for line in lines {
        assert!(
            !["FAIL", "BLOCKED", "maintenance-timeout"]
                .iter()
                .any(|forbidden| {
                    has_token(line, forbidden) || has_field_value(line, forbidden)
                }),
            "retained output contains a rejected oracle result: {line}\n{}",
            lines.join("\n")
        );
    }
}

fn assert_kernel_completion_wake_proofs(output: &str) {
    for required in [IPC_PENDING_PASS, NET_RX_RESERVATION_PASS] {
        assert!(
            output.contains(required),
            "missing exact kernel completion-wake proof {required:?}\n{output}"
        );
    }
    for forbidden in [IPC_PENDING_FAIL, NET_RX_RESERVATION_FAIL] {
        assert!(
            !output.contains(forbidden),
            "kernel completion-wake proof emitted failure {forbidden:?}\n{output}"
        );
    }
}

fn assert_armed_wait(armed_line: &str) -> (u64, u64, u64) {
    let cycle =
        numeric_field(armed_line, "cycle=").expect("armed marker must carry a numeric cycle");
    let start_ticks = numeric_field(armed_line, "start_ticks=")
        .expect("armed marker must carry numeric start_ticks");
    let budget = numeric_field(armed_line, "budget_ticks=")
        .expect("armed marker must carry numeric budget_ticks");
    let proof_ceiling = numeric_field(armed_line, "proof_ceiling_ticks=")
        .expect("armed marker must carry numeric proof_ceiling_ticks");
    assert_eq!(
        numeric_field(armed_line, "raw_ret="),
        Some(0),
        "an idle IPC wake candidate must be armed only for exact raw zero"
    );
    assert!(cycle > 0, "armed wait cycle must be positive");
    assert!(start_ticks > 0, "armed wait start_ticks must be positive");
    assert_eq!(
        budget, 1_000_000,
        "armed wait must use the 100 ms smoltcp maintenance budget"
    );
    assert_eq!(
        proof_ceiling, 900_000,
        "armed wait proof must end before the earliest phase-aligned deadline wake"
    );
    assert!(
        proof_ceiling < budget,
        "proof ceiling must be stricter than the maintenance budget"
    );
    (cycle, budget, proof_ceiling)
}

fn assert_runtime_wake_evidence(lines: &[&str]) {
    assert_no_oracle_failures(lines);
    let mut saw_pass = false;

    for (result_index, result_line) in lines.iter().copied().enumerate() {
        if !has_token(result_line, "idle_ipc_wake") {
            continue;
        }
        let is_pass = has_token(result_line, "status=PASS");
        let is_inconclusive = has_token(result_line, "status=INCONCLUSIVE");
        if !is_pass && !is_inconclusive {
            continue;
        }

        let result_cycle = numeric_field(result_line, "cycle=")
            .expect("idle IPC wake result must carry a numeric cycle");
        let armed_line = lines[..result_index]
            .iter()
            .copied()
            .rev()
            .find(|line| {
                has_token(line, "idle_ipc_wake")
                    && has_token(line, "status=ARMED")
                    && numeric_field(line, "cycle=") == Some(result_cycle)
            })
            .unwrap_or_else(|| {
                panic!(
                    "idle IPC wake result has no preceding same-cycle ARMED marker: {result_line}"
                )
            });
        let (_, armed_budget, armed_proof_ceiling) = assert_armed_wait(armed_line);

        assert!(
            has_token(result_line, "wake=recordless"),
            "idle IPC wake result was not caused by a raw recordless return: {result_line}"
        );
        assert_eq!(
            numeric_field(result_line, "raw_ret="),
            Some(0),
            "idle IPC wake result must preserve the exact raw zero observation"
        );
        let elapsed = numeric_field(result_line, "elapsed_ticks=")
            .expect("idle IPC wake result must carry numeric elapsed_ticks");
        let budget = numeric_field(result_line, "budget_ticks=")
            .expect("idle IPC wake result must carry numeric budget_ticks");
        let proof_ceiling = numeric_field(result_line, "proof_ceiling_ticks=")
            .expect("idle IPC wake result must carry numeric proof_ceiling_ticks");
        assert_eq!(
            budget, armed_budget,
            "armed and drained wait markers must carry the same maintenance budget"
        );
        assert_eq!(
            proof_ceiling, armed_proof_ceiling,
            "armed and drained wait markers must carry the same proof ceiling"
        );

        if is_pass {
            assert!(
                elapsed < proof_ceiling,
                "idle IPC wake PASS was not strictly below the proof ceiling: {result_line}"
            );
            saw_pass = true;
        } else {
            assert!(
                has_token(result_line, "reason=late-drain"),
                "inconclusive idle IPC wake must identify a late drain: {result_line}"
            );
            assert!(
                elapsed >= proof_ceiling,
                "idle IPC wake became INCONCLUSIVE before the proof ceiling: {result_line}"
            );
        }
    }
    assert!(
        saw_pass,
        "retained output contained no exact sub-ceiling runtime wake PASS\n{}",
        lines.join("\n")
    );
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
    let startup_output = qemu.dump();
    assert_kernel_completion_wake_proofs(&startup_output);

    let command_checkpoint = qemu.output_checkpoint();
    qemu.send_line("bench c2c-broker-oracle");
    qemu.wait_for_after(
        "[c2c-broker-oracle] START",
        command_checkpoint,
        ORACLE_TIMEOUT_SECS,
    )
    .unwrap_or_else(|error| {
        panic!(
            "oracle benchmark did not start after the command checkpoint: {error}\n{}",
            qemu.dump()
        )
    });
    qemu.wait_for_after(
        "[c2c-broker-oracle] overflow status=PASS",
        command_checkpoint,
        ORACLE_TIMEOUT_SECS,
    )
    .unwrap_or_else(|error| {
        panic!(
            "oracle did not reach a passing post-command overflow result: {error}\n{}",
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
    let output_after_command = &output[command_checkpoint..];
    let all_oracle_lines = oracle_lines(&output);
    let command_lines = oracle_lines(output_after_command);
    assert_kernel_completion_wake_proofs(&output);
    assert_runtime_wake_evidence(&all_oracle_lines);
    assert_tokens(&command_lines, &["START"]);

    let shutdown_race_exercised = output_after_command
        .contains("[net-broker] restart oracle shutdown-before-admission exercised")
        || output_after_command
            .contains("[net-broker] restart oracle shutdown-after-admission exercised");
    assert!(
        output_after_command.contains("[net-broker] restart oracle drained runtime roles")
            && shutdown_race_exercised,
        "restart passed without exercising and draining a network IPC race\n{output_after_command}"
    );
    assert!(
        output.contains("[net-rx-producer] irq->completion PASS"),
        "oracle passed without proving interrupt-driven NET_RX completion\n{output}"
    );
    for forbidden in [
        "[net-broker] restart oracle role drain timed out",
        "[net-broker] restart oracle IPC admission timed out",
        "[net-broker] beacon IPC timed out; network disabled until restart",
        "[heartbeat] task ",
        "[watchdog] task ",
    ] {
        assert!(
            !output.contains(forbidden),
            "oracle observed runtime termination marker {forbidden:?}\n{output}"
        );
    }

    assert_tokens(&command_lines, &["baseline", "calibration=MEASURED"]);
    assert_tokens(&command_lines, &["role_gate=PASS"]);
    for n in ["n=1", "n=2", "n=4", "n=8", "n=16"] {
        assert_tokens(&command_lines, &["sweep", n]);
    }
    assert_tokens(
        &command_lines,
        &[
            "soak",
            "attempted=10000",
            "success=10000",
            "silent_drop=0",
            "heartbeat_miss_delta=0",
            "watchdog_expired_delta=0",
        ],
    );
    assert_positive_field(
        &command_lines,
        &["soak", "attempted=10000"],
        "network_progress_delta=",
    );
    assert_tokens(&command_lines, &["overflow", "status=PASS"]);
    assert_tokens(&command_lines, &["restart", "status=PASS"]);

    for line in command_lines {
        println!("{line}");
    }
}
