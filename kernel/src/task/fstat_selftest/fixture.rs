use super::super::tcb::Task;
use alloc::boxed::Box;
use alloc::vec::Vec;
use api::fs::{BoxFuture, FileResult, SeekFrom, ViFile};
use types::{CellId, ViError, ViResult};

pub(super) const TASK_A: usize = 0xF570;
pub(super) const TASK_B: usize = 0xF571;
pub(super) const TEST_FILE: &str = "/BIN/INIT";
pub(super) const TEST_DIR: &str = "/BIN";
pub(super) const FAILING_FD: usize = 90;
pub(super) const SENTINEL: u8 = 0xA5;

static BACKEND_SIZE_CALLS: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

struct FailingFile;

impl ViFile for FailingFile {
    fn read(&mut self, _buf: &mut [u8]) -> ViResult<usize> {
        Err(ViError::IO)
    }

    fn write(&mut self, _buf: &[u8]) -> ViResult<usize> {
        Err(ViError::IO)
    }

    fn seek(&mut self, _pos: SeekFrom) -> ViResult<u64> {
        Err(ViError::IO)
    }

    fn size(&mut self) -> ViResult<u64> {
        BACKEND_SIZE_CALLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        Err(ViError::IO)
    }

    fn read_async(
        self: Box<Self>,
        _buf_ptr: usize,
        _buf_len: usize,
    ) -> BoxFuture<'static, FileResult<usize>> {
        Box::pin(async move {
            let file: Box<dyn ViFile + Send + Sync> = self;
            (file, Err(ViError::IO))
        })
    }
}

pub(super) fn check(ok: &mut bool, passed: bool, label: &str) {
    if !passed {
        *ok = false;
        log::error!("[selftest] FSTAT: FAIL {}", label);
    }
}

pub(super) fn install_tasks(ok: &mut bool) -> bool {
    let mut guard = super::super::SCHEDULER.lock();
    let Some(scheduler) = guard.as_mut() else {
        check(ok, false, "scheduler initialized");
        return false;
    };
    if scheduler.tasks.contains_key(&TASK_A) || scheduler.tasks.contains_key(&TASK_B) {
        check(ok, false, "synthetic task IDs unused");
        return false;
    }
    scheduler.tasks.insert(
        TASK_A,
        Box::new(Task::new(TASK_A, CellId(0), "fstat-a", Vec::new())),
    );
    scheduler.tasks.insert(
        TASK_B,
        Box::new(Task::new(TASK_B, CellId(0), "fstat-b", Vec::new())),
    );
    true
}

pub(super) fn cleanup(installed: bool) {
    if installed {
        if let Some(scheduler) = super::super::SCHEDULER.lock().as_mut() {
            scheduler.tasks.remove(&TASK_A);
            scheduler.tasks.remove(&TASK_B);
        }
    }
}

pub(super) fn install_failing_file() {
    if let Some(scheduler) = super::super::SCHEDULER.lock().as_mut() {
        if let Some(task) = scheduler.tasks.get_mut(&TASK_A) {
            task.open_files
                .insert(FAILING_FD, api::fs::FileHandle::new(Box::new(FailingFile)));
        }
    }
}

pub(super) fn reset_backend_calls() {
    BACKEND_SIZE_CALLS.store(0, core::sync::atomic::Ordering::Relaxed);
}

pub(super) fn backend_calls() -> usize {
    BACKEND_SIZE_CALLS.load(core::sync::atomic::Ordering::Relaxed)
}
