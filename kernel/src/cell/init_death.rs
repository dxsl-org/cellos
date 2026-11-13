//! Kernel policy for the death of trusted init (Phase 13 step 4, Law-1 A5).
//!
//! Nothing in userspace can kill init (it is critical: `KillCell`, freeze and
//! `ForceExit` refuse it), so its root retiring means a fault, a watchdog or
//! heartbeat kill, a panic exit or a deliberate exit. `exit_task` calls
//! [`record`] once for the trusted init root, under `SCHEDULER`:
//!
//! - one line `[init-death] tid=… cell=… gen=… reason=… pc=… action=…`; the
//!   death path's own audit record (`CellFault` for a fault or watchdog kill,
//!   `CellHung` for a heartbeat kill, `CellExit` for an exit) is already written
//!   by the caller;
//! - an atomic flag naming the action.
//!
//! The action runs later, from [`act_if_pending`] in `yield_cpu`, which never
//! runs under `SCHEDULER` (it takes that lock itself). Exit code
//! [`INIT_BOOT_STOP`] halts, so a boot that cannot succeed does not reset into
//! the same failure forever; every other death resets the machine. Every hart
//! that reaches `yield_cpu` after the flag parks, so a halt stops the whole
//! machine and a reset runs on exactly one hart.
//!
//! Bounded: the record is a fixed-size `Copy` struct, the state is three
//! atomics, and nothing here allocates or takes a lock. Nothing survives the
//! reset (no persistent store exists).

use abi::retirement::INIT_BOOT_STOP;
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

/// What the kernel does once init is gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Reset the machine; init comes back with the next boot.
    Reset,
    /// Stop: init itself declared that boot cannot continue.
    Halt,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Reset => "reset",
            Self::Halt => "halt",
        }
    }
}

/// Why init's root retired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    /// A clean `Exit` with this code.
    Exit(usize),
    /// A fault or a kernel kill (`exit_task`'s `usize::MAX`).
    Fault,
}

impl Reason {
    /// Classify `exit_task`'s `exit_reason`: `usize::MAX` is the fault/kill
    /// sentinel, every other value an exit code.
    pub(crate) fn from_exit_reason(exit_reason: usize) -> Self {
        if exit_reason == usize::MAX {
            Self::Fault
        } else {
            Self::Exit(exit_reason)
        }
    }

    /// The policy: halt for init's own boot-stop exit, reset for anything else.
    pub(crate) fn action(self) -> Action {
        match self {
            Self::Exit(INIT_BOOT_STOP) => Action::Halt,
            Self::Exit(_) | Self::Fault => Action::Reset,
        }
    }
}

/// The A5 record: one per boot, formatted as the `[init-death]` line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InitDeath {
    pub tid: usize,
    pub cell_id: u64,
    pub generation: u64,
    pub reason: Reason,
    /// Faulting pc, when a trap captured one for init's generation.
    pub pc: Option<usize>,
}

impl InitDeath {
    pub(crate) fn action(&self) -> Action {
        self.reason.action()
    }
}

impl fmt::Display for InitDeath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[init-death] tid={} cell={} gen={} reason=",
            self.tid, self.cell_id, self.generation
        )?;
        match self.reason {
            Reason::Exit(code) => write!(f, "{code:#x}")?,
            Reason::Fault => f.write_str("fault")?,
        }
        match self.pc {
            Some(pc) => write!(f, " pc={pc:#x}")?,
            None => f.write_str(" pc=-")?,
        }
        write!(f, " action={}", self.action().name())
    }
}

const NONE: u8 = 0;
const RESET: u8 = 1;
const HALT: u8 = 2;
/// A hart took the reset; every other hart parks.
const RESET_CLAIMED: u8 = 3;

/// Pending action, published after the record line.
static PENDING: AtomicU8 = AtomicU8::new(NONE);
/// The record is written once per boot.
static RECORDED: AtomicBool = AtomicBool::new(false);
/// Root tid and pc of the last trap-captured fault in init's generation;
/// written under `SCHEDULER` just before that fault enters `exit_task`.
static FAULT_ROOT: AtomicUsize = AtomicUsize::new(0);
static FAULT_PC: AtomicUsize = AtomicUsize::new(0);

/// Remember the pc of a fault in init's generation (`root_tid` is init's
/// root), for the record the root's retirement writes. Called under SCHEDULER.
pub(crate) fn stage_fault(root_tid: usize, pc: usize) {
    FAULT_PC.store(pc, Ordering::Relaxed);
    FAULT_ROOT.store(root_tid, Ordering::Release);
}

/// Write the record for init's root `tid` and publish the action. Called by
/// `exit_task` under SCHEDULER; a second call (same boot) is ignored.
pub(crate) fn record(tid: usize, cell_id: u64, generation: u64, exit_reason: usize) {
    if RECORDED.swap(true, Ordering::AcqRel) {
        return;
    }
    let reason = Reason::from_exit_reason(exit_reason);
    let pc = (reason == Reason::Fault && FAULT_ROOT.load(Ordering::Acquire) == tid)
        .then(|| FAULT_PC.load(Ordering::Relaxed));
    let death = InitDeath {
        tid,
        cell_id,
        generation,
        reason,
        pc,
    };
    log::error!("{death}");
    let action = match death.action() {
        Action::Reset => RESET,
        Action::Halt => HALT,
    };
    PENDING.store(action, Ordering::Release);
}

/// Carry out a published action. Called by `yield_cpu` with SCHEDULER not held
/// (it is that function's own lock); returns at once when init is alive.
pub(crate) fn act_if_pending() {
    match PENDING.load(Ordering::Acquire) {
        NONE => {}
        RESET => {
            if PENDING
                .compare_exchange(RESET, RESET_CLAIMED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                reset();
            }
            park();
        }
        _ => park(),
    }
}

/// Reset the machine. Returns only when the platform refused.
#[cfg(not(test))]
fn reset() {
    #[cfg(target_arch = "x86_64")]
    crate::arch::acpi::power::reboot();
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    {
        crate::hal::sbi::system_reset(crate::hal::sbi::SBI_RESET_COLD_REBOOT, 0);
        log::error!("[init-death] SBI cold reset refused; halting");
    }
    #[cfg(target_arch = "aarch64")]
    match crate::sched::smp::usable_psci_conduit() {
        // The conduit the firmware declares, under the same usability check
        // the secondary-core start applies (`sched/smp.rs`).
        Ok(conduit) => {
            let status = hal::aarch64::psci::system_reset(conduit);
            log::error!(
                "[init-death] PSCI SYSTEM_RESET refused: {}; halting",
                hal::aarch64::psci::status_name(status)
            );
        }
        Err(why) => log::error!("[init-death] no usable PSCI conduit ({why}); halting"),
    }
    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "riscv64",
        target_arch = "riscv32",
        target_arch = "aarch64"
    )))]
    log::error!("[init-death] no reset path on this architecture; halting");
}

/// Host tests never reach `yield_cpu`; the policy is tested through [`Reason`].
#[cfg(test)]
fn reset() {}

/// Stop this hart for good: the panic handler's halt for this architecture,
/// with interrupts masked so no trap re-enters the scheduler.
#[cfg(not(test))]
fn park() -> ! {
    use crate::hal::Arch;
    crate::hal::ARCH.disable_interrupts();
    loop {
        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        // SAFETY: `cli; hlt` stops this hart; nothing runs afterwards.
        unsafe {
            core::arch::asm!("cli; hlt", options(nomem, nostack));
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
        // SAFETY: `wfi` idles this hart with interrupts masked.
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack));
        }
    }
}

#[cfg(test)]
fn park() {}

#[cfg(test)]
#[path = "init_death_tests.rs"]
mod tests;
