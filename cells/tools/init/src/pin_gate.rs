//! The machine-pin gate as seen from init (ADR-0029 §3, Phase 13 amendment).
//!
//! Init launches the `machine-pin` Cell right after the VFS (x86 only),
//! registers it as `MACHINE_PIN`, and asks it `MachinePinRequest::Gate` as a
//! client. Init answers no request itself. Safe mode is init's to apply: it
//! starts only the static safe set (the `input` service and the `cellbox`
//! console shell) and tells the pin Cell the shell's tid one way
//! (`MachinePinRequest::ConsoleShell`), the only sender whose `Confirm` the
//! Cell honours. Without an answer the gate fails closed: safe mode.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use ostd::io::println;
use ostd::ipc::{IpcTerminal, PendingCall};
use wire::machine_pin::{MachinePinRequest, MachinePinResponse, PinState};

use crate::service_table::{self, Service};

/// The safe set, by plan entry name (CP1 Q6).
const SAFE_SERVICE: &str = "input";
const SAFE_SHELL: &str = "cellbox";
/// One `Gate` call: 31 rounds of 100 scheduler ticks, just past the kernel's
/// 30 s operation deadline, so an expired submission is resubmitted.
const GATE_ROUND_TICKS: u64 = 100;
const GATE_ROUNDS: usize = 31;
/// Submissions before the gate fails closed (about two minutes in all): the
/// Cell answers only after its first VFS reads and writes of the pin store.
const GATE_ATTEMPTS: usize = 4;

/// What the pin Cell decided for this boot.
pub(crate) struct Decision {
    /// `None` off x86, or when the Cell could not be started or gave no answer.
    pub(crate) state: Option<PinState>,
    pub(crate) safe_mode: bool,
}

/// The safe-mode banner's reason for `state`.
fn reason(state: Option<PinState>) -> &'static str {
    match state {
        Some(PinState::Mismatch) => "machine pin mismatch",
        Some(PinState::RecordUnreadable) => "machine pin record unreadable",
        Some(PinState::StoreUnavailable) => "machine pin store unavailable",
        Some(_) => "machine facts unavailable",
        None => "machine pin unavailable",
    }
}

/// Launch the pin Cell at `services[index]` and ask it for this boot's gate.
/// `None` index: not x86, no pin and nothing withheld.
pub(crate) fn gate(services: &mut [Service], index: Option<usize>) -> Decision {
    let Some(index) = index else {
        println("Init: machine pin not applicable on this architecture.");
        return Decision {
            state: None,
            safe_mode: false,
        };
    };
    let unavailable = Decision {
        state: None,
        safe_mode: true,
    };
    let Some(pin) = service_table::spawn(&mut services[index]) else {
        println("Init: machine-pin spawn/registration failed — failing closed.");
        return unavailable;
    };
    println("Init: machine-pin launched.");
    match ask_gate(pin) {
        Some((state, safe_mode)) => Decision {
            state: Some(state),
            safe_mode,
        },
        None => {
            println("Init: machine-pin gave no gate answer — failing closed.");
            unavailable
        }
    }
}

/// `MachinePinRequest::Gate`, resubmitted while the Cell lives and the kernel
/// expired an earlier submission before it was answered.
fn ask_gate(pin: usize) -> Option<(PinState, bool)> {
    let request = MachinePinRequest::Gate.encode();
    let mut reply = [0u8; wire::ipc::IPC_BUF_SIZE];
    for _ in 0..GATE_ATTEMPTS {
        let pending = PendingCall::submit(pin, &request).ok()?;
        if let Ok(Some(completion)) =
            pending.wait_and_take(&mut reply, GATE_ROUND_TICKS, GATE_ROUNDS)
        {
            if completion.terminal != IpcTerminal::Reply {
                if service_table::task_alive(pin) != Some(true) {
                    return None;
                }
                continue;
            }
            return match MachinePinResponse::decode(&reply[..completion.len]) {
                Some(MachinePinResponse::Gate { state, safe_mode }) => Some((state, safe_mode)),
                _ => None,
            };
        }
        // Still outstanding: dropping the handle cancels it before resubmitting.
        if service_table::task_alive(pin) != Some(true) {
            return None;
        }
    }
    None
}

/// The safe set's plan indices and the enabled required entries it withholds.
/// The shell is looked up among the autoload entries (`autoload_offset..`),
/// the service among the configured services (`1..autoload_offset`).
pub(crate) fn safe_set(
    services: &[Service],
    autoload_offset: usize,
    autoload_end: usize,
) -> (Option<usize>, Option<usize>, Vec<String>) {
    let enabled = |index: &usize, name: &str| {
        services[*index].spec.enabled && services[*index].spec.name == name
    };
    let service =
        (1..autoload_offset.min(services.len())).find(|index| enabled(index, SAFE_SERVICE));
    let shell = (autoload_offset..autoload_end.min(services.len()))
        .find(|index| enabled(index, SAFE_SHELL));
    let withheld = services
        .iter()
        .enumerate()
        .filter(|(index, entry)| {
            (1..autoload_end.min(services.len())).contains(index)
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

/// The safe-mode console init tells the pin Cell about, and re-tells after a
/// restart of either.
pub(crate) struct SafeConsole {
    pub(crate) pin: usize,
    pub(crate) shell: usize,
}

impl SafeConsole {
    /// One-way `ConsoleShell` to the live pin Cell naming the live shell.
    pub(crate) fn announce(&self, services: &[Service]) {
        let (Some(pin), Some(shell)) = (services[self.pin].tid, services[self.shell].tid) else {
            return;
        };
        let notice = MachinePinRequest::ConsoleShell { tid: shell as u64 }.encode();
        if ostd::ipc::post(pin, &notice).is_err() {
            println("Init: machine pin: console shell notice undeliverable.");
        }
    }
}

/// Start only the safe set, printing why. Boot-critical drivers and the VFS
/// started before the gate keep running under supervision.
pub(crate) fn enter_safe_mode(
    services: &mut [Service],
    autoload_offset: usize,
    autoload_end: usize,
    decision: &Decision,
    pin: Option<usize>,
) -> Option<SafeConsole> {
    println(&format!(
        "Init: SAFE MODE — {}; non-boot services withheld. At the console run 'machine pin --confirm' then reboot.",
        reason(decision.state)
    ));
    let (service, shell, withheld) = safe_set(services, autoload_offset, autoload_end);
    if !withheld.is_empty() {
        println(&format!(
            "Init: SAFE MODE — required services withheld: {}",
            withheld.join(", ")
        ));
    }
    for index in [service, shell].into_iter().flatten() {
        match service_table::spawn(&mut services[index]) {
            Some(_) => println(&format!("Init: {} launched.", services[index].spec.name)),
            None => println(&format!(
                "Init: {} spawn/registration failed.",
                services[index].spec.name
            )),
        }
    }
    let Some(shell) = shell else {
        println("Init: SAFE MODE — no console in plan; supervising storage only.");
        return None;
    };
    let console = SafeConsole { pin: pin?, shell };
    console.announce(services);
    Some(console)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn safe_set_is_input_and_the_cellbox_shell_from_the_shipped_plan() {
        let plan =
            cellos_boot_config::parse_cells(include_bytes!("../../../../config/services.toml"))
                .unwrap();
        let autoload =
            cellos_boot_config::parse_cells(include_bytes!("../../../../config/autoload.toml"))
                .unwrap();
        let autoload_offset = 1 + plan.cells.len();
        let mut services = vec![Service::bootstrap_vfs()];
        services.extend(plan.cells.into_iter().map(Service::new));
        services.extend(autoload.cells.into_iter().map(Service::new));
        let autoload_end = services.len();
        // Init's fixed rows after the plan are never part of the safe set.
        services.push(Service::machine_pin(2));
        services.push(Service::activator());
        let (service, shell, withheld) = safe_set(&services, autoload_offset, autoload_end);
        let service = service.expect("input in the plan");
        let shell = shell.expect("cellbox in autoload");
        assert_eq!(services[service].spec.name, "input");
        assert_eq!(
            (
                services[shell].spec.name.as_str(),
                services[shell].spec.args.as_slice()
            ),
            ("cellbox", &[String::from("sh")][..])
        );
        assert!(shell >= autoload_offset);
        for name in ["net", "silo"] {
            assert!(
                withheld.iter().any(|entry| entry == name),
                "{name} withheld: {withheld:?}"
            );
        }
        assert!(!withheld.iter().any(|entry| {
            ["input", "cellbox", "vfs", "machine-pin", "activator"].contains(&entry.as_str())
        }));
    }

    #[test]
    fn the_banner_names_the_gate_outcome_and_fails_closed_without_one() {
        assert_eq!(reason(Some(PinState::Mismatch)), "machine pin mismatch");
        assert_eq!(
            reason(Some(PinState::StoreUnavailable)),
            "machine pin store unavailable"
        );
        assert_eq!(
            reason(Some(PinState::FactsUnavailable)),
            "machine facts unavailable"
        );
        assert_eq!(reason(None), "machine pin unavailable");
    }
}
