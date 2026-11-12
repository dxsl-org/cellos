//! Machine pin gate, safe mode and re-confirmation (ADR-0029 §3).
//!
//! Before any configured service starts, init compares the kernel's machine
//! facts (syscall 433) with the record stored at `/srv/.cellos-machine/pin`:
//! - no record on a healthy store: first boot, record and continue;
//! - the same bytes: continue;
//! - anything else: print the field-level difference and enter safe mode.
//!
//! The pin carries no authority. It never changes a profile, driver gate or
//! DMA decision; safe mode only withholds services. In safe mode init starts
//! the static safe set (the `input` service and the `cellbox` console shell)
//! and answers `MachinePinRequest::Confirm` only from that shell, only for the
//! current digest. A confirmed pin takes effect at the next boot.

mod diff;
mod store;
#[cfg(test)]
mod tests;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use api::machine_facts::{MachineFactsRecord, FLAG_HYPERVISOR, PROFILE_SOURCE_CMDLINE};
use api::services::machine_pin::{MachinePinRequest, MachinePinResponse, PinState, Refusal};
use ostd::io::println;

use crate::activation::Message;
use crate::service_table::Service;
pub(crate) use store::VfsPinStore;
use store::{PinRead, PinStore};

/// The catalog's only profile that does not require DMA isolation. The kernel
/// binds it only when the command line names it and CPUID reports a
/// hypervisor; without a pin store it warns and continues (CP1 Q5).
const STORE_OPTIONAL_PROFILE: &[u8] = b"qemu-q35-x86_64";
/// The safe set, by plan entry name (CP1 Q6).
const SAFE_SERVICE: &str = "input";
const SAFE_SHELL: &str = "cellbox";

/// What the gate decided for this boot.
pub(crate) struct Gate {
    pub(crate) state: PinState,
    current: Option<MachineFactsRecord>,
    pinned: Option<[u8; 32]>,
    /// Console lines, without the `Init: machine pin: ` prefix.
    lines: Vec<String>,
}

fn short(digest: &[u8; 32]) -> String {
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

    fn reason(&self) -> &'static str {
        match self.state {
            PinState::Mismatch => "machine pin mismatch",
            PinState::RecordUnreadable => "machine pin record unreadable",
            PinState::StoreUnavailable => "machine pin store unavailable",
            _ => "machine facts unavailable",
        }
    }

    fn report(&self) {
        for line in &self.lines {
            println(&format!("Init: machine pin: {line}"));
        }
        if self.state == PinState::StoreUnavailable && !self.safe_mode() {
            println("Init: machine pin disabled — no store; continuing under qemu-q35-x86_64.");
        }
    }
}

/// Register the endpoint, query the facts and decide, printing the outcome.
/// `init_tid` is init's own tid: the kernel accepts `MACHINE_PIN` only from
/// trusted init naming itself, and the VFS admits only that live provider to
/// the store.
pub(crate) fn gate(init_tid: usize) -> Endpoint {
    if !cfg!(target_arch = "x86_64") {
        // Machine facts are an x86 record (syscall 433 answers "no facts"
        // elsewhere); no endpoint is registered and nothing is withheld.
        println("Init: machine pin: not applicable on this architecture.");
        let gate = Gate {
            state: PinState::FactsUnavailable,
            current: None,
            pinned: None,
            lines: Vec::new(),
        };
        return Endpoint {
            gate,
            safe_mode: false,
            shell: None,
            store: VfsPinStore::new(),
        };
    }
    let registered = matches!(
        ostd::syscall::sys_register_service(api::syscall::service::MACHINE_PIN, init_tid, 0),
        ostd::syscall::SyscallResult::Ok(0)
    ) && ostd::syscall::sys_lookup_service(api::syscall::service::MACHINE_PIN)
        == Some(init_tid);
    let current = if registered {
        ostd::syscall::sys_query_machine_facts().ok()
    } else {
        println("Init: machine pin: endpoint registration failed.");
        None
    };
    let gate = evaluate(current, &mut VfsPinStore::new());
    gate.report();
    Endpoint {
        safe_mode: gate.safe_mode(),
        shell: None,
        gate,
        store: VfsPinStore::new(),
    }
}

/// The safe set's plan indices and the enabled required entries it withholds.
/// The shell is looked up among the autoload entries (`autoload_offset..`),
/// the service among the configured services (`1..autoload_offset`).
pub(crate) fn safe_set(
    services: &[Service],
    autoload_offset: usize,
) -> (Option<usize>, Option<usize>, Vec<String>) {
    let enabled = |index: &usize, name: &str| {
        services[*index].spec.enabled && services[*index].spec.name == name
    };
    let service =
        (1..autoload_offset.min(services.len())).find(|index| enabled(index, SAFE_SERVICE));
    let shell = (autoload_offset..services.len()).find(|index| enabled(index, SAFE_SHELL));
    let withheld = services
        .iter()
        .enumerate()
        .filter(|(index, entry)| {
            (1..services.len()).contains(index)
                && Some(*index) != service
                && Some(*index) != shell
                && entry.spec.enabled
                && entry.spec.required
                && entry.tid.is_none()
        })
        .map(|(_, entry)| entry.spec.name.clone())
        .collect();
    (service, shell, withheld)
}

/// Init's `MACHINE_PIN` endpoint for the rest of the boot.
pub(crate) struct Endpoint {
    gate: Gate,
    safe_mode: bool,
    /// Plan index of the safe-mode console shell.
    shell: Option<usize>,
    store: VfsPinStore,
}

impl Endpoint {
    pub(crate) fn safe_mode(&self) -> bool {
        self.safe_mode
    }

    /// Start only the safe set, printing why. Boot-critical drivers and the
    /// VFS started before the gate keep running under supervision.
    pub(crate) fn enter_safe_mode(&mut self, services: &mut [Service], autoload_offset: usize) {
        println(&format!(
            "Init: SAFE MODE — {}; non-boot services withheld. At the console run 'machine pin --confirm' then reboot.",
            self.gate.reason()
        ));
        let (service, shell, withheld) = safe_set(services, autoload_offset);
        if !withheld.is_empty() {
            println(&format!(
                "Init: SAFE MODE — required services withheld: {}",
                withheld.join(", ")
            ));
        }
        for index in [service, shell].into_iter().flatten() {
            match crate::service_table::spawn(&mut services[index]) {
                Some(_) => println(&format!("Init: {} launched.", services[index].spec.name)),
                None => println(&format!(
                    "Init: {} spawn/registration failed.",
                    services[index].spec.name
                )),
            }
        }
        self.shell = shell;
        if shell.is_none() {
            println("Init: SAFE MODE — no console in plan; supervising storage only.");
        }
    }

    /// Answer a machine pin frame. `true` when `message` claimed this protocol
    /// (answered or dropped as malformed), so no other handler sees it.
    pub(crate) fn handle(&mut self, message: &Message, services: &[Service]) -> bool {
        let payload = &message.bytes[..crate::activation::PAYLOAD_BYTES];
        if !MachinePinRequest::is_claimed(payload) {
            return false;
        }
        let Some(request) = MachinePinRequest::decode(payload) else {
            println("Init: machine pin: malformed request dropped.");
            return true;
        };
        let response = match request {
            MachinePinRequest::Status => MachinePinResponse::Status {
                state: self.gate.state,
                current: self
                    .gate
                    .current
                    .as_ref()
                    .map_or([0; 32], MachineFactsRecord::digest),
                pinned: self.gate.pinned.unwrap_or([0; 32]),
            },
            MachinePinRequest::Confirm { digest } => self.confirm(message, services, digest),
        };
        if !crate::activation::deliver_frame(message, &response.encode()) {
            println("Init: machine pin: reply undeliverable.");
        }
        true
    }

    fn confirm(
        &mut self,
        message: &Message,
        services: &[Service],
        digest: [u8; 32],
    ) -> MachinePinResponse {
        let shell_tid = self.shell.and_then(|index| services[index].tid);
        let from_shell = shell_tid == Some(message.sender)
            && message.identity.is_some_and(|identity| {
                identity.sender_tid == message.sender as u64
                    && identity.cell_id != 0
                    && identity.generation != 0
            })
            && crate::activation::task_alive(message.sender) == Some(true);
        let current = ostd::syscall::sys_query_machine_facts().ok();
        let result = authorize_confirm(
            self.safe_mode,
            from_shell,
            digest,
            current.as_ref().map(MachineFactsRecord::digest),
        )
        .and_then(|()| {
            let record = current.as_ref().ok_or(Refusal::DigestStale)?;
            if self.store.write(record.bytes()) {
                Ok(record.digest())
            } else {
                Err(Refusal::StoreWriteFailed)
            }
        });
        match result {
            Ok(recorded) => {
                self.gate.pinned = Some(recorded);
                println(&format!(
                    "Init: machine pin: re-confirmed {}; reboot to start services",
                    short(&recorded)
                ));
                MachinePinResponse::Recorded { digest: recorded }
            }
            Err(refusal) => {
                println(&format!(
                    "Init: machine pin: confirm refused — {}",
                    refusal.name()
                ));
                MachinePinResponse::Refused(refusal)
            }
        }
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
