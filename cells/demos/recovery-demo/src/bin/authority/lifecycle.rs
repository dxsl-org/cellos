//! Worker lifecycle at the authority: spawn with a one-shot attach nonce, fence
//! on death, wait for the kernel's retirement verdict (K1, syscall 431), and
//! restart only a contained or explicitly quarantined incarnation.

use alloc::format;
use api::retirement::{reason, RetirementState, ViRetirementV1, RETIREMENT_RECORD_LEN};
use ostd::io::println;
use ostd::syscall::{self, SyscallResult};
use recovery_demo::ledger::Authority;
#[cfg(feature = "fault-inject")]
use recovery_demo::ledger::Committed;
use recovery_demo::wire::{Msg, FRAME_LEN};
use recovery_demo::{state, WORKER_PATH};

use crate::common;

/// Restart budget for one boot; the evidence plan needs about 30 cycles.
const MAX_RESTARTS: u64 = 64;
/// One-tick polls before a still-Pending retirement counts as unproven.
const VERDICT_POLLS: usize = 1_000;

pub struct Provider {
    pub auth: Authority,
    pub worker: Option<usize>,
    pub state: u8,
    pub restarts: u64,
    pub verdict: Option<ViRetirementV1>,
    /// Authority-owned page currently lent read-write to the worker.
    pub lent: Option<usize>,
    /// Word found in the last discarded lent page (never cleared or reused).
    pub lock_word: u64,
    #[cfg(feature = "fault-inject")]
    pub pristine: Option<Committed>,
}

impl Provider {
    pub fn new() -> Self {
        Self {
            auth: Authority::new(),
            worker: None,
            state: state::RECOVERING,
            restarts: 0,
            verdict: None,
            lent: None,
            lock_word: 0,
            #[cfg(feature = "fault-inject")]
            pristine: None,
        }
    }

    pub fn spawn_worker(&mut self) {
        let mut entropy = [0u8; 8];
        if syscall::sys_get_random(&mut entropy) != entropy.len() {
            return self.recovery_failed("no entropy for the attach nonce");
        }
        let nonce = u64::from_le_bytes(entropy) | 1;
        if !syscall::sys_set_spawn_args(&format!("nonce={nonce:016x}")) {
            return self.recovery_failed("attach nonce staging refused");
        }
        match syscall::sys_spawn_from_path_raw(WORKER_PATH) {
            SyscallResult::Ok(tid) => {
                let _ = syscall::sys_notify_on_exit(tid);
                self.auth.expect_worker(nonce, tid);
                self.worker = Some(tid);
                println(&format!("[recovery-authority] spawned worker tid={tid} restarts={}", self.restarts));
            }
            SyscallResult::Err(error) => self.recovery_failed(&format!("worker spawn failed: {error:?}")),
        }
    }

    pub fn recovery_failed(&mut self, why: &str) {
        self.state = state::RECOVERY_FAILED;
        println(&format!("[recovery-authority] RecoveryFailed: {why}; no Ready publication"));
    }

    /// Death of the current worker root: fence, read the verdict, then decide.
    pub fn on_death(&mut self, tid: usize, exit_reason: u64) {
        if self.worker != Some(tid) {
            return;
        }
        self.worker = None;
        let watermark = self.auth.fence();
        println(&format!(
            "[recovery-authority] worker {tid} died reason={exit_reason:#x}: writer fenced at revision {watermark}"
        ));
        self.verdict = wait_verdict(tid);
        match self.verdict {
            Some(v) => log_verdict(&v),
            None => println("[recovery-authority] verdict unavailable: treated as Unproven"),
        }
        self.discard_lent();
        let unproven = self.verdict.map_or(true, |v| v.retirement_state() == Some(RetirementState::Unproven));
        if unproven {
            self.state = state::NODE_RECOVERY_REQUIRED;
            println("[recovery-authority] node recovery required: containment unproven, no restart");
            return;
        }
        if self.state == state::RECOVERY_FAILED {
            println("[recovery-authority] no restart after RecoveryFailed");
            return;
        }
        if self.restarts >= MAX_RESTARTS {
            return self.recovery_failed("restart budget exhausted");
        }
        self.restarts += 1;
        self.state = state::RECOVERING;
        self.spawn_worker();
    }

    /// A page the dead writer could modify holds unproven invariants: read the
    /// lock word for the record, then free it. It is never unlocked or reused.
    fn discard_lent(&mut self) {
        let Some(grant) = self.lent.take() else { return };
        let mut word = [0u8; 8];
        let _ = syscall::sys_grant_copy_to_slice(grant, &mut word);
        self.lock_word = u64::from_le_bytes(word);
        let freed = syscall::sys_grant_free(grant);
        println(&format!(
            "[recovery-authority] lent page discarded with lock word={} (freed={freed}): never unlocked or reused",
            self.lock_word
        ));
    }

    /// `Status` frame followed by the last verdict record (zero when none).
    pub fn report(&self, to: usize) {
        let committed = self.auth.committed();
        let status = Msg::Status {
            state: self.state,
            worker: self.worker.unwrap_or(0) as u64,
            epoch: self.auth.epoch(),
            revision: committed.map_or(0, |c| c.revision),
            counter: committed.map_or(0, |c| c.counter),
            restarts: self.restarts,
            ledger: self.auth.ledger_len() as u64,
            lock_word: self.lock_word,
        };
        let mut out = [0u8; FRAME_LEN + RETIREMENT_RECORD_LEN];
        out[..FRAME_LEN].copy_from_slice(&status.encode());
        if let Some(verdict) = self.verdict {
            out[FRAME_LEN..].copy_from_slice(&verdict.encode());
        }
        let _ = syscall::sys_send(to, &out);
    }
}

fn wait_verdict(root_tid: usize) -> Option<ViRetirementV1> {
    for _ in 0..VERDICT_POLLS {
        match syscall::sys_query_retirement(root_tid) {
            Ok(v) if v.retirement_state() == Some(RetirementState::Pending) => common::sleep(1),
            Ok(v) => return Some(v),
            Err(error) => {
                println(&format!("[recovery-authority] QueryRetirement({root_tid}) refused: {error:?}"));
                return None;
            }
        }
    }
    None
}

fn log_verdict(v: &ViRetirementV1) {
    let label = match v.retirement_state() {
        Some(RetirementState::Contained) => "Contained",
        Some(RetirementState::Quarantined) => "Quarantined",
        Some(RetirementState::Unproven) => "Unproven",
        Some(RetirementState::Pending) | None => "Pending",
    };
    println(&format!(
        "[recovery-authority] verdict root={} cell={} gen={} state={label} reasons={:#x} threads={} remote_harts={:#x} \
grants={} borrows={} writable={} pins={} vfs={} shm={} dma={} qpages={} leaked={}{}",
        v.root_tid, v.cell_id, v.generation, v.reasons, v.member_threads, v.remote_hart_mask,
        v.grants_reclaimed, v.grantee_refs_cleared, v.writable_borrows, v.pins_quarantined,
        v.vfs_leases_pending, v.shm_frames_released, v.dma_deferred, v.quarantined_pages, v.leaked_pages,
        if v.reasons & reason::SHM_UNTRACKED != 0 { " (untracked SHM export)" } else { "" },
    ));
}
