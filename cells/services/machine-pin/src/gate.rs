//! Machine pin gate (ADR-0029 §3, Phase 13 amendment).
//!
//! Once trusted init has registered this Cell as `MACHINE_PIN`, the gate
//! compares the kernel's machine facts (syscall 433, answered only to the live
//! provider) with the record stored at `/srv/.cellos-machine/pin`:
//! - no record on a healthy store: first boot, record and continue;
//! - the same bytes: continue;
//! - anything else: print the field-level difference and decide safe mode.
//!
//! The pin carries no authority. It never changes a profile, driver gate or
//! DMA decision; safe mode only withholds services, and init does the
//! withholding after asking this Cell (`MachinePinRequest::Gate`).

use abi::machine_facts::{MachineFactsRecord, FLAG_HYPERVISOR, PROFILE_SOURCE_CMDLINE};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use ostd::io::println;
use ostd::syscall::SyscallError;
use wire::machine_pin::{PinState, Refusal};

use crate::diff;
use crate::store::{PinRead, PinStore};

/// The catalog's only profile that does not require DMA isolation. The kernel
/// binds it only when the command line names it and CPUID reports a
/// hypervisor; without a pin store it warns and continues (CP1 Q5).
const STORE_OPTIONAL_PROFILE: &[u8] = b"qemu-q35-x86_64";
/// Scheduler ticks to wait for init's `MACHINE_PIN` registration of this Cell,
/// which 433 requires; init registers it right after the spawn.
const REGISTRATION_TICKS: u64 = 500;
/// Poll backstop for a scheduler-tick clock that never advances.
const REGISTRATION_POLLS: usize = 5_000_000;

/// What the gate decided for this boot.
pub(crate) struct Gate {
    pub(crate) state: PinState,
    pub(crate) current: Option<MachineFactsRecord>,
    pub(crate) pinned: Option<[u8; 32]>,
    /// Console lines, without the `machine-pin: ` prefix.
    pub(crate) lines: Vec<String>,
}

pub(crate) fn short(digest: &[u8; 32]) -> String {
    diff::hex(&digest[..6])
}

/// The decision table, with the store abstracted. Writes only when the store
/// positively reports absence.
pub(crate) fn evaluate(current: Option<MachineFactsRecord>, store: &mut impl PinStore) -> Gate {
    let mut gate = Gate {
        state: PinState::FactsUnavailable,
        current,
        pinned: None,
        lines: Vec::new(),
    };
    let Some(current) = gate.current.as_ref() else {
        gate.lines.push(String::from("facts unavailable"));
        return gate;
    };
    let digest = current.digest();
    match store.read() {
        PinRead::Absent => {
            if store.write(current.bytes()) {
                gate.state = PinState::FirstBootRecorded;
                gate.pinned = Some(digest);
                gate.lines
                    .push(format!("first boot — recorded {}", short(&digest)));
            } else {
                gate.state = PinState::StoreUnavailable;
                gate.lines.push(String::from(
                    "store unavailable (first-boot write or read-back failed)",
                ));
            }
        }
        PinRead::Present(bytes) if bytes == current.bytes() => {
            gate.state = PinState::Match;
            gate.pinned = Some(digest);
            gate.lines.push(format!("match {}", short(&digest)));
        }
        PinRead::Present(bytes) => match MachineFactsRecord::decode(&bytes) {
            Ok(pinned) => {
                gate.state = PinState::Mismatch;
                gate.pinned = Some(pinned.digest());
                gate.lines.push(format!(
                    "mismatch — pinned {}, current {}",
                    short(&pinned.digest()),
                    short(&digest)
                ));
                gate.lines
                    .extend(diff::lines(pinned.facts(), current.facts()));
            }
            Err(error) => {
                gate.state = PinState::RecordUnreadable;
                gate.lines.push(format!("record unreadable ({error:?})"));
            }
        },
        PinRead::Oversized => {
            gate.state = PinState::RecordUnreadable;
            gate.lines
                .push(String::from("record unreadable (larger than any record)"));
        }
        PinRead::Unavailable(reason) => {
            gate.state = PinState::StoreUnavailable;
            gate.lines.push(format!("store unavailable ({reason})"));
        }
    }
    gate
}

impl Gate {
    /// Whether this outcome withholds non-boot services (CP1 Q4, Q5).
    pub(crate) fn safe_mode(&self) -> bool {
        match self.state {
            PinState::Match | PinState::FirstBootRecorded => false,
            PinState::StoreUnavailable => !self.store_optional(),
            PinState::Mismatch | PinState::RecordUnreadable | PinState::FactsUnavailable => true,
        }
    }

    /// The q35 profile, bound from the command line under a hypervisor.
    fn store_optional(&self) -> bool {
        self.current.as_ref().is_some_and(|record| {
            let facts = record.facts();
            facts.slug.as_bytes() == STORE_OPTIONAL_PROFILE
                && facts.profile_source == PROFILE_SOURCE_CMDLINE
                && facts.flags & FLAG_HYPERVISOR != 0
        })
    }

    fn report(&self) {
        for line in &self.lines {
            println(&format!("machine-pin: {line}"));
        }
        if self.state == PinState::StoreUnavailable && !self.safe_mode() {
            println("machine-pin: disabled — no store; continuing under qemu-q35-x86_64.");
        }
    }
}

/// This boot's gate and whether it withholds services, printing the outcome.
/// Off x86 there are no machine facts (433 answers "no facts"), and nothing is
/// withheld; init launches this Cell only on x86.
pub(crate) fn run(store: &mut impl PinStore) -> (Gate, bool) {
    if !cfg!(target_arch = "x86_64") {
        println("machine-pin: not applicable on this architecture.");
        let gate = Gate {
            state: PinState::FactsUnavailable,
            current: None,
            pinned: None,
            lines: Vec::new(),
        };
        return (gate, false);
    }
    let current = match registered_facts() {
        Ok(record) => Some(record),
        Err(SyscallError::PermissionDenied) => {
            println("machine-pin: endpoint registration not observed.");
            None
        }
        Err(_) => None,
    };
    let gate = evaluate(current, store);
    gate.report();
    let safe_mode = gate.safe_mode();
    (gate, safe_mode)
}

/// The facts, once init's registration makes this Cell the live provider:
/// 433 refuses every other caller with `PermissionDenied`, which is retried
/// until the bound. Any other answer is final.
fn registered_facts() -> Result<MachineFactsRecord, SyscallError> {
    let started = crate::now_ticks();
    let mut polls = 0usize;
    loop {
        let answer = ostd::syscall::sys_query_machine_facts();
        if !matches!(answer, Err(SyscallError::PermissionDenied))
            || crate::now_ticks().wrapping_sub(started) >= REGISTRATION_TICKS
            || polls >= REGISTRATION_POLLS
        {
            return answer;
        }
        polls += 1;
        ostd::task::yield_now();
    }
}

/// Confirm rules in order: safe mode, the console shell, the current digest.
pub(crate) fn authorize_confirm(
    safe_mode: bool,
    from_shell: bool,
    requested: [u8; 32],
    current: Option<[u8; 32]>,
) -> Result<(), Refusal> {
    if !safe_mode {
        return Err(Refusal::NotSafeMode);
    }
    if !from_shell {
        return Err(Refusal::NotConsoleShell);
    }
    if current != Some(requested) {
        return Err(Refusal::DigestStale);
    }
    Ok(())
}
