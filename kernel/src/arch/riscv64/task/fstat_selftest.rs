//! Focused truthful-fstat boot coverage.

mod dispatch;
mod fixture;
mod metadata;

/// Run the focused truthful-fstat boot checks before user tasks start.
pub fn self_test() -> bool {
    let mut ok = true;
    let installed = fixture::install_tasks(&mut ok);
    if installed {
        metadata::test_stdio(&mut ok);
        if let Some((file_fd, _dir_fd)) = metadata::test_vifs(&mut ok) {
            dispatch::test_dispatch(&mut ok, file_fd);
        }
    }
    fixture::cleanup(installed);
    ok
}
