//! Deterministic boot coverage for canonical task-relative paths and FAT stat.

use super::tcb::Task;
use alloc::boxed::Box;
use alloc::vec::Vec;
use types::{CellId, ViError};

const TASK_A: usize = 0xC0D0;
const TASK_B: usize = 0xC0D1;
const TEST_DIR: &str = "/BIN";
const TEST_FILE: &str = "/BIN/INIT";
const TEST_MISSING: &str = "/BIN/MISSING.BIN";
const SENTINEL: u8 = 0xA5;

fn check(ok: &mut bool, passed: bool, label: &str) {
    if !passed {
        *ok = false;
        log::error!("[selftest] CWD-PATH: FAIL {}", label);
    }
}

fn test_normalization(ok: &mut bool) {
    let cases = [
        ("/", "/", "/"),
        ("/", "a//./b/../c", "/a/c"),
        ("/base/dir", "../x", "/base/x"),
        ("/base", "../../../../x", "/x"),
        ("/ignored", "/a///b/./../c", "/a/c"),
        ("/base", "../../../..", "/"),
    ];
    for (cwd, input, expected) in cases {
        check(
            ok,
            matches!(super::resolve_path(cwd, input), Ok(path) if path == expected),
            input,
        );
    }
    check(ok, super::resolve_path("/", "").is_err(), "empty path");
}

fn test_fat_stat(ok: &mut bool) {
    let guard = crate::fs::VIFS1.lock();
    let Some(fs) = guard.as_ref() else {
        check(ok, false, "FAT mounted");
        return;
    };

    let root_ok = matches!(fs.stat("/"), Ok(stat) if stat.exists && stat.is_dir && stat.size == 0);
    let file_ok =
        matches!(fs.stat(TEST_FILE), Ok(stat) if stat.exists && !stat.is_dir && stat.size > 0);
    let dir_ok =
        matches!(fs.stat(TEST_DIR), Ok(stat) if stat.exists && stat.is_dir && stat.size == 0);
    let miss_ok = matches!(fs.stat(TEST_MISSING), Err(ViError::NotFound));
    check(ok, root_ok, "FAT root stat");
    check(ok, file_ok, "FAT file stat");
    check(ok, dir_ok, "FAT directory stat");
    check(ok, miss_ok, "FAT missing stat");
}

fn install_tasks(ok: &mut bool) -> bool {
    let mut guard = super::SCHEDULER.lock();
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
        Box::new(Task::new(TASK_A, CellId(0), "cwd-a", Vec::new())),
    );
    scheduler.tasks.insert(
        TASK_B,
        Box::new(Task::new(TASK_B, CellId(0), "cwd-b", Vec::new())),
    );
    true
}

fn cwd_is(tid: usize, expected: &str) -> bool {
    super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|scheduler| scheduler.tasks.get(&tid))
        .is_some_and(|task| task.cwd == expected)
}

fn test_task_paths(ok: &mut bool) {
    check(
        ok,
        super::file_chdir(TASK_A, "/BIN//./").is_ok(),
        "chdir directory",
    );
    check(ok, cwd_is(TASK_A, TEST_DIR), "caller cwd committed");
    check(ok, cwd_is(TASK_B, "/"), "second task cwd isolated");

    let file_failed = super::file_chdir(TASK_A, "INIT").is_err();
    let miss_failed = super::file_chdir(TASK_A, "MISSING.BIN").is_err();
    check(
        ok,
        file_failed && miss_failed && cwd_is(TASK_A, TEST_DIR),
        "failed chdir immutable",
    );

    let opened = super::file_open(TASK_A, "./INIT");
    let open_isolated = opened.is_ok()
        && super::SCHEDULER.lock().as_ref().is_some_and(|scheduler| {
            let fd = opened.unwrap_or(usize::MAX);
            scheduler
                .tasks
                .get(&TASK_A)
                .is_some_and(|task| task.open_files.contains_key(&fd))
                && scheduler
                    .tasks
                    .get(&TASK_B)
                    .is_some_and(|task| !task.open_files.contains_key(&fd))
        });
    check(ok, open_isolated, "open caller isolation");

    let mut exact = [SENTINEL; 4];
    let exact_ok = super::file_getcwd(TASK_A, &mut exact) == Ok(TEST_DIR.len())
        && exact.as_slice() == TEST_DIR.as_bytes();
    check(ok, exact_ok, "getcwd exact buffer");

    let mut oversized = [SENTINEL; 16];
    let oversize_ok = super::file_getcwd(TASK_A, &mut oversized) == Ok(TEST_DIR.len())
        && &oversized[..TEST_DIR.len()] == TEST_DIR.as_bytes()
        && oversized[TEST_DIR.len()..]
            .iter()
            .all(|byte| *byte == SENTINEL);
    check(ok, oversize_ok, "getcwd oversized sentinel");

    let mut undersized = [SENTINEL; 3];
    let undersize_ok = super::file_getcwd(TASK_A, &mut undersized).is_err()
        && undersized.iter().all(|byte| *byte == SENTINEL);
    check(ok, undersize_ok, "getcwd undersized immutable");

    let mut root = [SENTINEL; 4];
    let root_ok = super::file_getcwd(TASK_B, &mut root) == Ok(1)
        && root[0] == b'/'
        && root[1..].iter().all(|byte| *byte == SENTINEL);
    check(ok, root_ok, "second task getcwd isolation");

    let remove_denied = super::file_remove(TASK_A, "./INIT").is_err();
    let file_preserved = crate::fs::VIFS1
        .lock()
        .as_ref()
        .is_some_and(|fs| fs.stat(TEST_FILE).is_ok());
    check(
        ok,
        remove_denied && file_preserved,
        "relative remove read-only",
    );
}

fn cleanup(installed_tasks: bool) {
    if installed_tasks {
        if let Some(scheduler) = super::SCHEDULER.lock().as_mut() {
            scheduler.tasks.remove(&TASK_A);
            scheduler.tasks.remove(&TASK_B);
        }
    }
}

pub fn self_test() -> bool {
    let mut ok = true;
    test_normalization(&mut ok);
    test_fat_stat(&mut ok);
    let installed_tasks = install_tasks(&mut ok);
    if installed_tasks {
        test_task_paths(&mut ok);
    }
    cleanup(installed_tasks);
    ok
}
