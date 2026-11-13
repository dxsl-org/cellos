//! The `MACHINE_PIN` endpoint, served on [`ostd::service::serve`].
//!
//! `Status` and `Gate` are answered to every caller (they carry no secret).
//! `ConsoleShell` is accepted only from trusted init, whose tid init hands this
//! Cell on its command line (`init=<tid>`): only init has a launch edge to
//! `/bin/machine-pin`, and the kernel registers `MACHINE_PIN` only for init's
//! own child, so the command line of the live provider is init's. `Confirm` is
//! honoured only in safe mode, only from that live console shell, and only
//! for the current digest; a confirmed pin takes effect at the next boot.

use abi::machine_facts::MachineFactsRecord;
use alloc::format;
use ostd::io::println;
use ostd::service::{Handler, Request};
use wire::machine_pin::{MachinePinRequest, MachinePinResponse, Refusal};

use crate::gate::{authorize_confirm, short, Gate};
use crate::store::{PinStore, VfsPinStore};

pub(crate) struct Endpoint {
    gate: Gate,
    safe_mode: bool,
    /// Trusted init's tid, from the command line; `None` refuses every notice.
    init_tid: Option<usize>,
    /// The live console shell init started in safe mode.
    shell_tid: Option<usize>,
    store: VfsPinStore,
}

/// Init's tid from the command line init gave this Cell (`init=<tid>`).
pub(crate) fn init_tid(args: &[alloc::string::String]) -> Option<usize> {
    args.iter()
        .find_map(|arg| arg.strip_prefix("init="))
        .and_then(|tid| tid.parse::<usize>().ok())
        .filter(|tid| *tid != 0)
}

/// Whether a `ConsoleShell` notice from `sender` names the console shell:
/// only init may name it, and never tid 0.
pub(crate) fn notice_accepted(init_tid: Option<usize>, sender: usize, shell: u64) -> bool {
    init_tid == Some(sender) && shell != 0 && usize::try_from(shell).is_ok()
}

impl Endpoint {
    pub(crate) fn new(gate: Gate, safe_mode: bool, init_tid: Option<usize>) -> Self {
        Self {
            gate,
            safe_mode,
            init_tid,
            shell_tid: None,
            store: VfsPinStore::new(),
        }
    }

    fn confirm(&mut self, request: &Request<'_>, digest: [u8; 32]) -> MachinePinResponse {
        let sender = request.sender();
        let caller = request.caller();
        let from_shell = self.shell_tid == Some(sender)
            && caller.sender_tid == sender as u64
            && caller.cell_id != 0
            && caller.generation != 0
            && crate::task_alive(sender) == Some(true);
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
                    "machine-pin: re-confirmed {}; reboot to start services",
                    short(&recorded)
                ));
                MachinePinResponse::Recorded { digest: recorded }
            }
            Err(refusal) => {
                println(&format!(
                    "machine-pin: confirm refused — {}",
                    refusal.name()
                ));
                MachinePinResponse::Refused(refusal)
            }
        }
    }
}

impl Handler for Endpoint {
    fn on_request(&mut self, request: Request<'_>) {
        let payload = request.payload();
        if !MachinePinRequest::is_claimed(payload) {
            return;
        }
        let Some(decoded) = MachinePinRequest::decode(payload) else {
            println("machine-pin: malformed request dropped.");
            return;
        };
        let response = match decoded {
            MachinePinRequest::Status => MachinePinResponse::Status {
                state: self.gate.state,
                current: self
                    .gate
                    .current
                    .as_ref()
                    .map_or([0; 32], MachineFactsRecord::digest),
                pinned: self.gate.pinned.unwrap_or([0; 32]),
            },
            MachinePinRequest::Gate => MachinePinResponse::Gate {
                state: self.gate.state,
                safe_mode: self.safe_mode,
            },
            MachinePinRequest::Confirm { digest } => self.confirm(&request, digest),
            MachinePinRequest::ConsoleShell { tid } => {
                if notice_accepted(self.init_tid, request.sender(), tid) {
                    self.shell_tid = usize::try_from(tid).ok();
                } else {
                    println("machine-pin: console shell notice refused (not from init).");
                }
                return;
            }
        };
        if request.reply(&response.encode()).is_err() {
            println("machine-pin: reply undeliverable.");
        }
    }
}
