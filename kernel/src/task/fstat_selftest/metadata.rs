use super::fixture::{check, TASK_A, TASK_B, TEST_DIR, TEST_FILE};
use api::fs::SeekFrom;
use api::syscall::{
    VI_FSTAT_ACCESS_READ, VI_FSTAT_ACCESS_WRITE, VI_FSTAT_KIND_CHARACTER, VI_FSTAT_KIND_DIRECTORY,
    VI_FSTAT_KIND_REGULAR,
};

pub(super) fn test_stdio(ok: &mut bool) {
    for (fd, access) in [
        (0, VI_FSTAT_ACCESS_READ),
        (1, VI_FSTAT_ACCESS_WRITE),
        (2, VI_FSTAT_ACCESS_WRITE),
    ] {
        check(
            ok,
            matches!(
                super::super::file_fstat(TASK_A, fd),
                Ok(metadata)
                    if metadata.kind == VI_FSTAT_KIND_CHARACTER
                        && metadata.access == access
                        && metadata.size == 0
                        && metadata.reserved == [0; 2]
            ),
            "stdio kind/access/zero fields",
        );
    }
    check(
        ok,
        super::super::file_fstat(TASK_A, usize::MAX).is_err()
            && super::super::file_fstat(TASK_A, 89).is_err(),
        "negative and nonexistent descriptors",
    );
}

pub(super) fn test_vifs(ok: &mut bool) -> Option<(usize, usize)> {
    let expected_size = crate::fs::VIFS1
        .lock()
        .as_ref()
        .and_then(|fs| fs.stat(TEST_FILE).ok())
        .map(|stat| stat.size);
    let file_fd = super::super::file_open(TASK_A, TEST_FILE).ok();
    let dir_fd = super::super::file_open(TASK_A, TEST_DIR).ok();
    let (Some(expected_size), Some(file_fd), Some(dir_fd)) = (expected_size, file_fd, dir_fd)
    else {
        check(ok, false, "VIFS fixtures open");
        return None;
    };

    let positioned = super::super::SCHEDULER
        .lock()
        .as_mut()
        .is_some_and(|scheduler| {
            scheduler
                .tasks
                .get_mut(&TASK_A)
                .and_then(|task| task.open_files.get_mut(&file_fd))
                .is_some_and(|handle| handle.seek(SeekFrom::Start(7)) == Ok(7))
        });
    check(ok, positioned, "file cursor positioned");

    let file_metadata = super::super::file_fstat(TASK_A, file_fd);
    check(
        ok,
        matches!(
            file_metadata,
            Ok(metadata)
                if metadata.kind == VI_FSTAT_KIND_REGULAR
                    && metadata.access == VI_FSTAT_ACCESS_READ
                    && metadata.size == expected_size
                    && metadata.reserved == [0; 2]
        ),
        "regular file truth",
    );
    let cursor_preserved = super::super::SCHEDULER
        .lock()
        .as_mut()
        .is_some_and(|scheduler| {
            scheduler
                .tasks
                .get_mut(&TASK_A)
                .and_then(|task| task.open_files.get_mut(&file_fd))
                .is_some_and(|handle| handle.seek(SeekFrom::Current(0)) == Ok(7))
        });
    check(ok, cursor_preserved, "regular file cursor preserved");

    let directory_positioned = super::super::SCHEDULER
        .lock()
        .as_mut()
        .is_some_and(|scheduler| {
            scheduler
                .tasks
                .get_mut(&TASK_A)
                .and_then(|task| task.open_files.get_mut(&dir_fd))
                .is_some_and(|handle| handle.seek(SeekFrom::Start(1)) == Ok(1))
        });
    check(ok, directory_positioned, "directory cursor positioned");

    check(
        ok,
        matches!(
            super::super::file_fstat(TASK_A, dir_fd),
            Ok(metadata)
                if metadata.kind == VI_FSTAT_KIND_DIRECTORY
                    && metadata.access == VI_FSTAT_ACCESS_READ
                    && metadata.size == 0
                    && metadata.reserved == [0; 2]
        ),
        "directory truth",
    );
    let directory_cursor_preserved =
        super::super::SCHEDULER
            .lock()
            .as_mut()
            .is_some_and(|scheduler| {
                scheduler
                    .tasks
                    .get_mut(&TASK_A)
                    .and_then(|task| task.open_files.get_mut(&dir_fd))
                    .is_some_and(|handle| handle.seek(SeekFrom::Current(0)) == Ok(1))
            });
    check(ok, directory_cursor_preserved, "directory cursor preserved");
    check(
        ok,
        super::super::file_fstat(TASK_B, file_fd).is_err(),
        "caller descriptor isolation",
    );

    Some((file_fd, dir_fd))
}
