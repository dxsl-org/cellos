//! The worker's request loop. Every mutation is looked up and then committed at
//! the authority under the worker's epoch before the client sees a reply, so a
//! crash at any point leaves either no effect or a committed, reconcilable one.

use alloc::format;
use ostd::io::println;
use recovery_demo::ledger::apply;
use recovery_demo::wire::{Binding, Fault, Msg, Refusal};
use recovery_demo::EXIT_FENCED;

use crate::common;

/// Ticks to wait for an authority reply before failing the request closed.
const AUTHORITY_TICKS: u64 = 500;
/// Ticks a stalled mutation waits for the client's `Go`.
const STALL_TICKS: u64 = 2_000;

pub struct Worker {
    pub authority: usize,
    pub epoch: u64,
    pub revision: u64,
    pub counter: u64,
}

pub fn run(mut worker: Worker) -> ! {
    loop {
        let Some(rx) = common::recv_attested(0) else { continue };
        if rx.sender == worker.authority {
            continue;
        }
        let Some((cell, generation)) = rx.principal() else { continue };
        let rights = rx.identity.map_or(0, |id| id.flags);
        match rx.msg {
            Some(Msg::Get) => {
                common::send(rx.sender, Msg::Value {
                    counter: worker.counter,
                    revision: worker.revision,
                    epoch: worker.epoch,
                });
            }
            Some(Msg::Mutate { request_id, method, amount, fault }) => {
                let binding = Binding {
                    caller_cell: cell,
                    caller_generation: generation,
                    request_id,
                    method,
                    rights,
                    payload: amount,
                };
                worker.mutate(rx.sender, binding, fault);
            }
            #[cfg(feature = "fault-inject")]
            Some(Msg::Crash { mode, arg }) => {
                common::send(rx.sender, Msg::Ack);
                crate::fault::scenario(mode, arg);
            }
            _ => {}
        }
    }
}

impl Worker {
    fn call(&self, msg: Msg) -> Option<Msg> {
        if !common::send(self.authority, msg) {
            return None;
        }
        common::recv_from(self.authority, AUTHORITY_TICKS)
    }

    fn mutate(&mut self, client: usize, binding: Binding, fault: Fault) {
        let request_id = binding.request_id;
        match self.call(Msg::Lookup { epoch: self.epoch, binding }) {
            Some(Msg::Present { revision, result }) => {
                common::send(client, Msg::Done { request_id, result, revision, replayed: true });
                return;
            }
            Some(Msg::Absent) => {}
            Some(Msg::Refused(refusal)) => return self.refuse(client, request_id, refusal),
            _ => return self.refuse(client, request_id, Refusal::RecoveryFailed),
        }
        let Some(new_state) = apply(binding.method, self.counter, binding.payload) else {
            return self.refuse(client, request_id, Refusal::InvalidMutation);
        };
        self.fault_point(fault, Fault::BeforeCommit);
        if cfg!(feature = "fault-inject") && fault == Fault::Stall {
            common::send(client, Msg::Staged { request_id });
            if common::recv_from(client, STALL_TICKS) != Some(Msg::Go) {
                return self.refuse(client, request_id, Refusal::Denied);
            }
        }
        let commit = Msg::Commit { epoch: self.epoch, expected_revision: self.revision, binding, new_state };
        match self.call(commit) {
            Some(Msg::Committed { revision, result }) => {
                self.revision = revision;
                self.counter = result;
                self.fault_point(fault, Fault::AfterCommit);
                common::send(client, Msg::Done { request_id, result, revision, replayed: false });
            }
            Some(Msg::Present { revision, result }) => {
                common::send(client, Msg::Done { request_id, result, revision, replayed: true });
            }
            Some(Msg::Refused(refusal)) => self.refuse(client, request_id, refusal),
            _ => self.refuse(client, request_id, Refusal::RecoveryFailed),
        }
    }

    /// A fenced writer reports the refusal and exits: it can never commit again.
    fn refuse(&self, client: usize, request_id: u64, refusal: Refusal) {
        common::send(client, Msg::Failed { request_id, refusal });
        if refusal == Refusal::StaleEpoch {
            println(&format!("[recovery-worker] epoch {} fenced: exiting", self.epoch));
            ostd::syscall::sys_exit(EXIT_FENCED);
        }
    }

    #[cfg(feature = "fault-inject")]
    fn fault_point(&self, requested: Fault, here: Fault) {
        if requested == here {
            println(&format!("[recovery-worker] injected fault {here:?}"));
            crate::fault::crash();
        }
    }

    #[cfg(not(feature = "fault-inject"))]
    fn fault_point(&self, _requested: Fault, _here: Fault) {}
}
