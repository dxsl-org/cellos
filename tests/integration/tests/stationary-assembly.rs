#[path = "../../../cells/tests/bench/src/scenarios/stationary_assembly.rs"]
mod assembly;
#[allow(dead_code)]
#[path = "../../../cells/tests/bench/src/scenarios/lab_transfer_contract.rs"]
mod lab_transfer_contract;

use assembly::{
    AssemblyError, AssemblyMode, CouplingState, LockObservation, StationaryAssemblyContract,
};
use lab_transfer_contract::{Configuration, Principal};
use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

const OBSERVER: Principal = Principal {
    id: 50,
    generation: 1,
};

const RECONCILE_AUTHORITY: Principal = Principal {
    id: 60,
    generation: 1,
};

const CONFIG: Configuration = Configuration {
    id: 301,
    observer: OBSERVER,
    reconcile_authority: RECONCILE_AUTHORITY,
    observation_max_age_ticks: 1000,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn kernel_path() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel-native-workload")
        .to_string_lossy()
        .into_owned()
}

fn disk_path() -> String {
    repo_root()
        .join("build/disk_srv.img")
        .to_string_lossy()
        .into_owned()
}

fn qemu_ok() -> bool {
    std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok()
}

// ── 08A Host Unit Tests ──────────────────────────────────────────────────────

#[test]
fn test_standalone_upper_mode() {
    let mut contract = StationaryAssemblyContract::new(CONFIG);
    assert_eq!(contract.active_mode(), AssemblyMode::StandaloneUpper);
    assert_eq!(contract.coupling_state(), CouplingState::Decoupled);

    assert!(contract.start_arm_activity().is_ok());
    assert!(contract.is_arm_active());
    // Arm active -> base motion must be rejected
    assert_eq!(
        contract.start_base_motion(),
        Err(AssemblyError::BaseMovingDuringArmActivity)
    );
    contract.stop_arm_activity();
    assert!(!contract.is_arm_active());
}

#[test]
fn test_standalone_base_mode() {
    let mut contract = StationaryAssemblyContract::new(CONFIG);
    contract.select_standalone_base().unwrap();
    assert_eq!(contract.active_mode(), AssemblyMode::StandaloneBase);

    assert!(contract.start_base_motion().is_ok());
    assert!(contract.is_base_moving());
    // Base moving -> arm activity must be rejected
    assert_eq!(
        contract.start_arm_activity(),
        Err(AssemblyError::ArmActiveDuringBaseMotion)
    );
    contract.stop_base_motion();
    assert!(!contract.is_base_moving());
}

#[test]
fn test_stationary_coupling_lifecycle() {
    let mut contract = StationaryAssemblyContract::new(CONFIG);
    assert_eq!(contract.coupling_state(), CouplingState::Decoupled);

    // 1. Quiesce
    contract.quiesce_for_coupling().unwrap();
    assert_eq!(contract.coupling_state(), CouplingState::Quiescent);

    // 2. Begin coupling
    contract.begin_coupling().unwrap();
    assert_eq!(contract.coupling_state(), CouplingState::CouplingPending);

    // 3. Complete physical coupling
    contract.complete_physical_coupling().unwrap();
    assert_eq!(contract.coupling_state(), CouplingState::Coupled);

    // 4. Verify coupling with locks, power, and safety
    let valid_obs = LockObservation {
        mechanical_locked: true,
        electrical_power_ok: true,
        safety_circuit_closed: true,
        sequence: 1,
    };
    contract.verify_coupling(valid_obs).unwrap();
    assert_eq!(contract.coupling_state(), CouplingState::Verified);

    // 5. Local re-enable requires authoritative principal
    let unauthorized = Principal { id: 99, generation: 1 };
    assert_eq!(
        contract.local_re_enable(unauthorized),
        Err(AssemblyError::UnauthorizedReconcile)
    );

    contract.local_re_enable(RECONCILE_AUTHORITY).unwrap();
    assert_eq!(contract.coupling_state(), CouplingState::Enabled);
    assert_eq!(contract.active_mode(), AssemblyMode::AssembledStationary);

    // In AssembledStationary mode, base motion is strictly forbidden
    assert_eq!(
        contract.start_base_motion(),
        Err(AssemblyError::BaseMovingDuringArmActivity)
    );
}

#[test]
fn test_coupling_rejection_on_faults() {
    let mut contract = StationaryAssemblyContract::new(CONFIG);
    contract.quiesce_for_coupling().unwrap();
    contract.begin_coupling().unwrap();
    contract.complete_physical_coupling().unwrap();

    // Lock failure
    let lock_fail = LockObservation {
        mechanical_locked: false,
        electrical_power_ok: true,
        safety_circuit_closed: true,
        sequence: 1,
    };
    assert_eq!(
        contract.verify_coupling(lock_fail),
        Err(AssemblyError::LocksNotEngaged)
    );
    assert_eq!(contract.coupling_state(), CouplingState::SafeInhibited);
}

#[test]
fn test_decoupling_lifecycle() {
    let mut contract = StationaryAssemblyContract::new(CONFIG);
    contract.quiesce_for_coupling().unwrap();
    contract.begin_coupling().unwrap();
    contract.complete_physical_coupling().unwrap();
    contract.verify_coupling(LockObservation {
        mechanical_locked: true,
        electrical_power_ok: true,
        safety_circuit_closed: true,
        sequence: 1,
    }).unwrap();
    contract.local_re_enable(RECONCILE_AUTHORITY).unwrap();

    // Decouple
    contract.quiesce_for_decoupling().unwrap();
    assert_eq!(contract.coupling_state(), CouplingState::DecouplingPending);

    contract.complete_decoupling().unwrap();
    assert_eq!(contract.coupling_state(), CouplingState::Decoupled);
    assert_eq!(contract.active_mode(), AssemblyMode::StandaloneUpper);
}

// ── 08B Native QEMU Test ─────────────────────────────────────────────────────

#[test]
fn riscv64_stationary_assembly_qemu() {
    let kernel = PathBuf::from(kernel_path());
    let disk = PathBuf::from(disk_path());
    if !kernel.exists() || !disk.exists() || !qemu_ok() {
        return;
    }

    let tmp = tempfile::Builder::new()
        .suffix(".img")
        .tempfile()
        .expect("create temp disk");
    std::fs::copy(disk_path(), tmp.path()).expect("copy srv disk");

    let mut qemu = QemuRunner::boot_rv64_with_disk(&kernel_path(), tmp.path().to_str().unwrap());

    qemu.wait_for("Cellos >", 60)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));

    // Run the native ASSEMBLY-01 stationary integration bench
    qemu.send_line("bench stationary-assembly");

    qemu.wait_for("[stationary-assembly] ALL CRITERIA PASSED", 60)
        .unwrap_or_else(|e| {
            panic!(
                "ASSEMBLY-01 stationary integration failed or timed out: {e}\n--- serial output ---\n{}",
                qemu.dump()
            )
        });

    let serial = qemu.dump();
    assert!(serial.contains("[stationary-assembly] standalone upper mode verified and logged"));
    assert!(serial.contains("[stationary-assembly] standalone base mode verified and logged"));
    assert!(serial.contains("[stationary-assembly] stationary coupling enabled and logged"));
    assert!(serial.contains("[stationary-assembly] assembled stationary LAB operation verified"));
    assert!(serial.contains("[stationary-assembly] fault inhibition and operator reset verified"));
    assert!(serial.contains("[stationary-assembly] safe decoupling lifecycle verified and logged"));
    assert!(serial.contains("[stationary-assembly] all assembly traces verified on CellosFS Native"));
}
