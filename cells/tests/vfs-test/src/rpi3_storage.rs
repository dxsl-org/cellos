use super::{fail, pass, vfs_req};

const MARKER: &[u8] = b"cellos-rpi3-storage-pass-20260826";

fn read_marker(path: &'static str) -> Option<bool> {
    let handle = match vfs_req(&api::ipc::VfsRequest::ReadAsync { path }) {
        api::ipc::VfsResponse::PendingHandle(handle) => handle,
        _ => return None,
    };
    match vfs_req(&api::ipc::VfsRequest::Poll { handle }) {
        api::ipc::VfsResponse::Data(data) => Some(data == MARKER),
        _ => None,
    }
}

fn verify_existing(path: &'static str, label: &str) {
    match vfs_req(&api::ipc::VfsRequest::Stat(path)) {
        api::ipc::VfsResponse::Err(_) => {
            ostd::io::print("[rpi3-storage] arming first-boot marker on ");
            ostd::io::println(label);
            fail("RPi3 marker exists from prior boot");
        }
        api::ipc::VfsResponse::Stat { is_dir: false, .. } => match read_marker(path) {
            Some(true) => {
                ostd::io::print("[rpi3-storage] persisted ");
                ostd::io::println(label);
                pass("RPi3 marker persisted across reboot");
            }
            _ => fail("RPi3 persisted marker content matches"),
        },
        _ => fail("RPi3 persisted marker is a file"),
    }
}

fn write_and_read(path: &'static str, label: &str) {
    match vfs_req(&api::ipc::VfsRequest::Write {
        path,
        content: MARKER,
    }) {
        api::ipc::VfsResponse::Ok => {}
        _ => {
            fail("RPi3 storage marker write");
            return;
        }
    }

    match read_marker(path) {
        Some(true) => {
            ostd::io::print("[rpi3-storage] same-boot readback ");
            ostd::io::println(label);
            pass("RPi3 storage same-boot write/readback");
        }
        _ => fail("RPi3 storage same-boot write/readback"),
    }
}

fn exercise(path: &'static str, label: &str) {
    verify_existing(path, label);
    write_and_read(path, label);
}

pub(crate) fn run() {
    ostd::io::println("[rpi3-storage] physical persistence gate starting");
    exercise("/mnt/sd/rpi3-storage-marker.txt", "FAT");
    exercise("/data/rpi3-storage-marker.txt", "littlefs");
    exercise("/srv/rpi3-storage-marker.txt", "RedoxFS");
}
