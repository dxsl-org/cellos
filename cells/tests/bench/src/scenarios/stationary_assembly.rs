//! ASSEMBLY-01 Stationary Coupling and Combined Operation Contract (Phase 08).
//!
//! Models:
//! 1. Three operational modes: StandaloneUpper, StandaloneBase, AssembledStationary.
//! 2. Stationary coupling lifecycle: Quiescent -> Coupled -> Verified -> Enabled.
//! 3. Decoupling lifecycle: Quiescent -> Decoupled -> Verified -> Standalone.
//! 4. Mutual exclusion: arm activity prohibits base motion; base motion prohibits arm activity.
//! 5. Fault handling: partial coupling, lock loss, or instability transitions to SafeInhibited.
//! 6. Native QEMU runner under `#[cfg(target_os = "none")]`.

use super::lab_transfer_contract::{Configuration, Principal};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssemblyMode {
    StandaloneUpper,
    StandaloneBase,
    AssembledStationary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CouplingState {
    Decoupled,
    Quiescent,
    CouplingPending,
    Coupled,
    Verified,
    Enabled,
    DecouplingPending,
    SafeInhibited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockObservation {
    pub mechanical_locked: bool,
    pub electrical_power_ok: bool,
    pub safety_circuit_closed: bool,
    pub sequence: u64,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssemblyError {
    InvalidTransition,
    NotQuiescent,
    LocksNotEngaged,
    PowerFault,
    SafetyCircuitOpen,
    ArmActiveDuringBaseMotion,
    BaseMovingDuringArmActivity,
    UnauthorizedReconcile,
    AlreadyInhibited,
    WrongConfiguration,
}

pub struct StationaryAssemblyContract {
    configuration: Configuration,
    coupling_state: CouplingState,
    active_mode: AssemblyMode,
    last_sequence: u64,
    arm_active: bool,
    base_moving: bool,
}
#[allow(dead_code)]
impl StationaryAssemblyContract {
    pub fn new(configuration: Configuration) -> Self {
        Self {
            configuration,
            coupling_state: CouplingState::Decoupled,
            active_mode: AssemblyMode::StandaloneUpper,
            last_sequence: 0,
            arm_active: false,
            base_moving: false,
        }
    }

    pub fn coupling_state(&self) -> CouplingState {
        self.coupling_state
    }

    pub fn active_mode(&self) -> AssemblyMode {
        self.active_mode
    }

    pub fn is_arm_active(&self) -> bool {
        self.arm_active
    }

    pub fn is_base_moving(&self) -> bool {
        self.base_moving
    }

    // ── Mode Selection ───────────────────────────────────────────────────────
    pub fn select_standalone_upper(&mut self) -> Result<(), AssemblyError> {
        if self.coupling_state != CouplingState::Decoupled {
            return Err(AssemblyError::InvalidTransition);
        }
        self.active_mode = AssemblyMode::StandaloneUpper;
        self.base_moving = false;
        Ok(())
    }

    pub fn select_standalone_base(&mut self) -> Result<(), AssemblyError> {
        if self.coupling_state != CouplingState::Decoupled {
            return Err(AssemblyError::InvalidTransition);
        }
        self.active_mode = AssemblyMode::StandaloneBase;
        self.arm_active = false;
        Ok(())
    }

    // ── Coupling Lifecycle ───────────────────────────────────────────────────
    pub fn quiesce_for_coupling(&mut self) -> Result<(), AssemblyError> {
        if self.coupling_state != CouplingState::Decoupled {
            return Err(AssemblyError::InvalidTransition);
        }
        self.arm_active = false;
        self.base_moving = false;
        self.coupling_state = CouplingState::Quiescent;
        Ok(())
    }

    pub fn begin_coupling(&mut self) -> Result<(), AssemblyError> {
        if self.coupling_state != CouplingState::Quiescent {
            return Err(AssemblyError::NotQuiescent);
        }
        self.coupling_state = CouplingState::CouplingPending;
        Ok(())
    }

    pub fn complete_physical_coupling(&mut self) -> Result<(), AssemblyError> {
        if self.coupling_state != CouplingState::CouplingPending {
            return Err(AssemblyError::InvalidTransition);
        }
        self.coupling_state = CouplingState::Coupled;
        Ok(())
    }

    pub fn verify_coupling(&mut self, obs: LockObservation) -> Result<(), AssemblyError> {
        if self.coupling_state != CouplingState::Coupled {
            return Err(AssemblyError::InvalidTransition);
        }
        if obs.sequence <= self.last_sequence {
            return Err(AssemblyError::InvalidTransition);
        }
        self.last_sequence = obs.sequence;

        if !obs.mechanical_locked {
            self.coupling_state = CouplingState::SafeInhibited;
            return Err(AssemblyError::LocksNotEngaged);
        }
        if !obs.electrical_power_ok {
            self.coupling_state = CouplingState::SafeInhibited;
            return Err(AssemblyError::PowerFault);
        }
        if !obs.safety_circuit_closed {
            self.coupling_state = CouplingState::SafeInhibited;
            return Err(AssemblyError::SafetyCircuitOpen);
        }

        self.coupling_state = CouplingState::Verified;
        Ok(())
    }

    pub fn local_re_enable(&mut self, authority: Principal) -> Result<(), AssemblyError> {
        if self.coupling_state != CouplingState::Verified {
            return Err(AssemblyError::InvalidTransition);
        }
        if authority != self.configuration.reconcile_authority {
            return Err(AssemblyError::UnauthorizedReconcile);
        }
        self.coupling_state = CouplingState::Enabled;
        self.active_mode = AssemblyMode::AssembledStationary;
        Ok(())
    }

    // ── Decoupling Lifecycle ─────────────────────────────────────────────────
    pub fn quiesce_for_decoupling(&mut self) -> Result<(), AssemblyError> {
        if self.coupling_state != CouplingState::Enabled {
            return Err(AssemblyError::InvalidTransition);
        }
        self.arm_active = false;
        self.base_moving = false;
        self.coupling_state = CouplingState::DecouplingPending;
        Ok(())
    }

    pub fn complete_decoupling(&mut self) -> Result<(), AssemblyError> {
        if self.coupling_state != CouplingState::DecouplingPending {
            return Err(AssemblyError::InvalidTransition);
        }
        self.coupling_state = CouplingState::Decoupled;
        self.active_mode = AssemblyMode::StandaloneUpper;
        Ok(())
    }

    // ── Mutual Exclusion & Safety Guardrails ──────────────────────────────────
    pub fn start_arm_activity(&mut self) -> Result<(), AssemblyError> {
        if self.coupling_state == CouplingState::SafeInhibited {
            return Err(AssemblyError::AlreadyInhibited);
        }
        if self.base_moving {
            self.coupling_state = CouplingState::SafeInhibited;
            return Err(AssemblyError::ArmActiveDuringBaseMotion);
        }
        self.arm_active = true;
        Ok(())
    }

    pub fn stop_arm_activity(&mut self) {
        self.arm_active = false;
    }

    pub fn start_base_motion(&mut self) -> Result<(), AssemblyError> {
        if self.active_mode == AssemblyMode::AssembledStationary {
            return Err(AssemblyError::BaseMovingDuringArmActivity);
        }
        if self.arm_active {
            self.coupling_state = CouplingState::SafeInhibited;
            return Err(AssemblyError::BaseMovingDuringArmActivity);
        }
        self.base_moving = true;
        Ok(())
    }

    pub fn stop_base_motion(&mut self) {
        self.base_moving = false;
    }

    pub fn handle_stationary_loss(&mut self) {
        self.arm_active = false;
        self.base_moving = false;
        self.coupling_state = CouplingState::SafeInhibited;
    }

    pub fn reconcile_and_reset(&mut self, authority: Principal) -> Result<(), AssemblyError> {
        if authority != self.configuration.reconcile_authority {
            return Err(AssemblyError::UnauthorizedReconcile);
        }
        self.arm_active = false;
        self.base_moving = false;
        self.coupling_state = CouplingState::Decoupled;
        self.active_mode = AssemblyMode::StandaloneUpper;
        Ok(())
    }
}

// ── Native QEMU Runner ───────────────────────────────────────────────────────
#[cfg(target_os = "none")]
#[allow(unused_imports)]
pub use runner::run;

#[cfg(target_os = "none")]
mod runner {
    use super::*;
    use alloc::format;

    const TRACE_LOG_PATH: &str = "/srv/assembly_trace.log";

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

    pub fn run() {
        ostd::io::println("[stationary-assembly] START: ASSEMBLY-01 native QEMU witness");
        ostd::syscall::sys_heartbeat(0);

        let Some(vfs_tid) = ostd::syscall::sys_lookup_service(api::syscall::service::VFS) else {
            fail("VFS service is not registered");
        };
        ostd::io::println(&format!(
            "[stationary-assembly] VFS service registered: tid={vfs_tid}"
        ));

        let mut vfs_client = ostd::clients::VfsClient::new();
        let _ = vfs_client.unlink(TRACE_LOG_PATH);

        let mut contract = StationaryAssemblyContract::new(CONFIG);

        // ── Use Case 1: Standalone Upper Mode ────────────────────────────────
        ostd::io::println("[stationary-assembly] Use Case 1: Standalone Upper Mode");
        contract
            .select_standalone_upper()
            .unwrap_or_else(|_| fail("select upper failed"));
        contract
            .start_arm_activity()
            .unwrap_or_else(|_| fail("start arm failed"));
        contract.stop_arm_activity();

        let trace_1 = format!("MODE:StandaloneUpper,task=LAB-01,status=Completed\n");
        if vfs_client
            .append_file(TRACE_LOG_PATH, trace_1.as_bytes())
            .is_err()
        {
            fail("failed to append trace 1 to VFS");
        }
        ostd::io::println("[stationary-assembly] standalone upper mode verified and logged");

        // ── Use Case 2: Standalone Base Mode ─────────────────────────────────
        ostd::io::println("[stationary-assembly] Use Case 2: Standalone Base Mode");
        contract
            .select_standalone_base()
            .unwrap_or_else(|_| fail("select base failed"));
        contract
            .start_base_motion()
            .unwrap_or_else(|_| fail("start base failed"));
        contract.stop_base_motion();

        let trace_2 = format!("MODE:StandaloneBase,task=BASE-01,status=Completed\n");
        if vfs_client
            .append_file(TRACE_LOG_PATH, trace_2.as_bytes())
            .is_err()
        {
            fail("failed to append trace 2 to VFS");
        }
        ostd::io::println("[stationary-assembly] standalone base mode verified and logged");

        // ── Use Case 3: Stationary Coupling Lifecycle ────────────────────────
        ostd::io::println("[stationary-assembly] Use Case 3: Stationary Coupling Lifecycle");
        contract
            .quiesce_for_coupling()
            .unwrap_or_else(|_| fail("quiesce failed"));
        contract
            .begin_coupling()
            .unwrap_or_else(|_| fail("begin coupling failed"));
        contract
            .complete_physical_coupling()
            .unwrap_or_else(|_| fail("physical coupling failed"));

        let lock_obs = LockObservation {
            mechanical_locked: true,
            electrical_power_ok: true,
            safety_circuit_closed: true,
            sequence: 1,
        };
        contract
            .verify_coupling(lock_obs)
            .unwrap_or_else(|_| fail("verify coupling failed"));
        contract
            .local_re_enable(RECONCILE_AUTHORITY)
            .unwrap_or_else(|_| fail("re-enable failed"));
        assert_eq!(contract.coupling_state(), CouplingState::Enabled);
        assert_eq!(contract.active_mode(), AssemblyMode::AssembledStationary);

        let trace_3 = format!("COUPLING:status=Enabled,mode=AssembledStationary,authority=60\n");
        if vfs_client
            .append_file(TRACE_LOG_PATH, trace_3.as_bytes())
            .is_err()
        {
            fail("failed to append trace 3 to VFS");
        }
        ostd::io::println("[stationary-assembly] stationary coupling enabled and logged");

        // ── Use Case 4: Assembled Stationary LAB-01 with Base Lock ───────────
        ostd::io::println("[stationary-assembly] Use Case 4: Assembled Stationary Operation");
        // Base motion must be forbidden while assembled and stationary
        assert!(contract.start_base_motion().is_err());
        contract
            .start_arm_activity()
            .unwrap_or_else(|_| fail("assembled arm start failed"));
        contract.stop_arm_activity();

        let trace_4 = format!("ASSEMBLED_LAB:status=Completed,base_immobilized=true\n");
        if vfs_client
            .append_file(TRACE_LOG_PATH, trace_4.as_bytes())
            .is_err()
        {
            fail("failed to append trace 4 to VFS");
        }
        ostd::io::println("[stationary-assembly] assembled stationary LAB operation verified");

        // ── Fault Injection: Loss of Stationary/Lock triggers Inhibition ─────
        ostd::io::println(
            "[stationary-assembly] Fault Injection: Loss of Lock / Motion during Assembled",
        );
        contract.handle_stationary_loss();
        assert_eq!(contract.coupling_state(), CouplingState::SafeInhibited);
        assert!(contract.start_arm_activity().is_err());

        // Reconcile and reset back to safe baseline
        contract
            .reconcile_and_reset(RECONCILE_AUTHORITY)
            .unwrap_or_else(|_| fail("reset failed"));
        assert_eq!(contract.coupling_state(), CouplingState::Decoupled);
        ostd::io::println("[stationary-assembly] fault inhibition and operator reset verified");

        // ── Use Case 5: Decoupling Lifecycle ─────────────────────────────────
        ostd::io::println("[stationary-assembly] Use Case 5: Decoupling Lifecycle");
        contract.quiesce_for_coupling().unwrap();
        contract.begin_coupling().unwrap();
        contract.complete_physical_coupling().unwrap();
        contract
            .verify_coupling(LockObservation {
                mechanical_locked: true,
                electrical_power_ok: true,
                safety_circuit_closed: true,
                sequence: 2,
            })
            .unwrap();
        contract.local_re_enable(RECONCILE_AUTHORITY).unwrap();

        // Now decouple safely
        contract
            .quiesce_for_decoupling()
            .unwrap_or_else(|_| fail("quiesce decoupling failed"));
        contract
            .complete_decoupling()
            .unwrap_or_else(|_| fail("complete decoupling failed"));
        assert_eq!(contract.coupling_state(), CouplingState::Decoupled);

        let trace_5 = format!("DECOUPLING:status=Decoupled,mode=StandaloneUpper\n");
        if vfs_client
            .append_file(TRACE_LOG_PATH, trace_5.as_bytes())
            .is_err()
        {
            fail("failed to append trace 5 to VFS");
        }
        ostd::io::println("[stationary-assembly] safe decoupling lifecycle verified and logged");

        // ── VFS Durability Check ─────────────────────────────────────────────
        ostd::io::println("[stationary-assembly] Verifying all trace records on VFS");
        let full_trace = vfs_client
            .read_file(TRACE_LOG_PATH)
            .unwrap_or_else(|_| fail("failed to read assembly trace from VFS"));

        assert!(full_trace
            .windows(trace_1.len())
            .any(|w| w == trace_1.as_bytes()));
        assert!(full_trace
            .windows(trace_2.len())
            .any(|w| w == trace_2.as_bytes()));
        assert!(full_trace
            .windows(trace_3.len())
            .any(|w| w == trace_3.as_bytes()));
        assert!(full_trace
            .windows(trace_4.len())
            .any(|w| w == trace_4.as_bytes()));
        assert!(full_trace
            .windows(trace_5.len())
            .any(|w| w == trace_5.as_bytes()));
        ostd::io::println("[stationary-assembly] all assembly traces verified on CellosFS Native");

        ostd::io::println("[stationary-assembly] Summary: 3 use cases, coupling/decoupling, mutual exclusion, fault inhibition verified");
        ostd::io::println("[stationary-assembly] ALL CRITERIA PASSED");
        ostd::syscall::sys_exit(0);
    }

    fn fail(message: &str) -> ! {
        ostd::io::println(&format!("[stationary-assembly] FAIL: {message}"));
        ostd::syscall::sys_exit(1)
    }
}
