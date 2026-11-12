//! Client-side calls. Results are what a consumer observes: worker replies,
//! service lookups, the authority's status and the kernel verdict it relays.

use alloc::format;
use alloc::vec::Vec;
use api::retirement::{RetirementState, ViRetirementV1, RETIREMENT_RECORD_LEN};
use ostd::io::println;
use recovery_demo::wire::{Fault, Msg, FRAME_LEN};
use recovery_demo::{AUTHORITY_SERVICE, WORKER_SERVICE};

use crate::common;

/// One worker or authority round trip, admission included.
const REPLY_TICKS: u64 = 400;
/// Wait for a replacement to be published.
const PUBLISH_TICKS: usize = 3_000;

pub fn fail(what: &str) -> ! {
    println(&format!("[recovery-client] FAIL: {what}"));
    println("RECOVERY-CLIENT: FAIL");
    ostd::syscall::sys_exit(1)
}

pub struct Status {
    pub state: u8,
    pub worker: u64,
    pub ledger: u64,
    pub lock_word: u64,
    pub verdict: Option<ViRetirementV1>,
}

pub struct Client {
    pub authority: usize,
    pub worker: usize,
    /// `(cell, generation)` of every published worker this run observed.
    pub incarnations: Vec<(u64, u64)>,
}

impl Client {
    pub fn connect() -> Self {
        let Some(authority) = common::wait_lookup(AUTHORITY_SERVICE, PUBLISH_TICKS) else {
            fail("no authority")
        };
        let Some(worker) = common::wait_lookup(WORKER_SERVICE, PUBLISH_TICKS) else {
            fail("no published worker")
        };
        let mut client = Self {
            authority,
            worker,
            incarnations: Vec::new(),
        };
        client.note_incarnation();
        client
    }

    fn note_incarnation(&mut self) {
        let mut buf = [0u8; 64];
        if let Some(b) = ostd::syscall::sys_lookup_service_bound(WORKER_SERVICE, &mut buf) {
            if b.tid == self.worker as u64 {
                self.incarnations.push((b.cell_id, b.generation));
            }
        }
    }

    pub fn worker_call(&self, msg: Msg) -> Option<Msg> {
        common::call(self.worker, msg, REPLY_TICKS)
    }

    pub fn authority_call(&self, msg: Msg) -> Option<Msg> {
        common::call(self.authority, msg, REPLY_TICKS)
    }

    /// `(counter, revision, epoch)` from the published worker.
    pub fn get(&self) -> (u64, u64, u64) {
        match self.worker_call(Msg::Get) {
            Some(Msg::Value {
                counter,
                revision,
                epoch,
            }) => (counter, revision, epoch),
            other => fail(&format!("get: {other:?}")),
        }
    }

    pub fn mutate(&self, request_id: u64, method: u8, amount: u64, fault: Fault) -> Option<Msg> {
        self.worker_call(Msg::Mutate {
            request_id,
            method,
            amount,
            fault,
        })
    }

    pub fn status(&self) -> Status {
        let mut bytes = [0u8; 256];
        if !common::call_raw(self.authority, Msg::Report, REPLY_TICKS, &mut bytes) {
            fail("authority report");
        }
        let Some(Msg::Status {
            state,
            worker,
            ledger,
            lock_word,
            ..
        }) = Msg::decode(&bytes[..FRAME_LEN])
        else {
            fail("malformed status")
        };
        let record = &bytes[FRAME_LEN..FRAME_LEN + RETIREMENT_RECORD_LEN];
        let verdict = if record.iter().all(|b| *b == 0) {
            None
        } else {
            Some(
                ViRetirementV1::decode(record)
                    .unwrap_or_else(|e| fail(&format!("verdict record {e:?}"))),
            )
        };
        Status {
            state,
            worker,
            ledger,
            lock_word,
            verdict,
        }
    }

    /// Wait until a worker other than `old` is published; returns the verdict
    /// the authority recorded for `old`'s retirement.
    pub fn await_replacement(&mut self, old: usize) -> ViRetirementV1 {
        for _ in 0..PUBLISH_TICKS {
            if let Some(tid) = common::lookup(WORKER_SERVICE).filter(|tid| *tid != old) {
                self.worker = tid;
                self.note_incarnation();
                let status = self.status();
                return match status.verdict {
                    Some(v) if v.root_tid == old as u64 => v,
                    other => fail(&format!("verdict for {old} missing: {other:?}")),
                };
            }
            common::sleep(1);
        }
        fail(&format!("no replacement for worker {old}"))
    }

    /// After RecoveryFailed and Heal: wait for any newly published worker.
    pub fn await_replacement_after_failure(&mut self, old: usize) {
        for _ in 0..PUBLISH_TICKS {
            if let Some(tid) = common::lookup(WORKER_SERVICE).filter(|tid| *tid != old) {
                self.worker = tid;
                self.note_incarnation();
                return;
            }
            common::sleep(1);
        }
        fail("no replacement after heal");
    }

    /// Ask the worker to crash in `mode` and wait for its replacement.
    pub fn crash(&mut self, mode: u8, arg: u64) -> ViRetirementV1 {
        let old = self.worker;
        if self.worker_call(Msg::Crash { mode, arg }) != Some(Msg::Ack) {
            fail(&format!("crash mode {mode} not acknowledged"));
        }
        self.await_replacement(old)
    }

    /// Poll the authority until `pred` holds or the publish window ends.
    pub fn wait_status(&self, what: &str, pred: impl Fn(&Status) -> bool) -> Status {
        for _ in 0..PUBLISH_TICKS / 10 {
            let status = self.status();
            if pred(&status) {
                return status;
            }
            common::sleep(10);
        }
        fail(&format!("authority never reached: {what}"))
    }
}

pub fn state_name(v: &ViRetirementV1) -> &'static str {
    match v.retirement_state() {
        Some(RetirementState::Contained) => "Contained",
        Some(RetirementState::Quarantined) => "Quarantined",
        Some(RetirementState::Unproven) => "Unproven",
        Some(RetirementState::Pending) => "Pending",
        None => "invalid",
    }
}

pub fn free_frames() -> u64 {
    match ostd::syscall::sys_mem_info() {
        Ok(info) => info.free_frames,
        Err(_) => fail("MemInfo refused"),
    }
}
