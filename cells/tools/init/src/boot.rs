use api::syscall::service;
use ostd::syscall::{
    sys_lookup_service, sys_notify_on_exit, sys_register_service, sys_spawn_from_path,
    SyscallResult,
};

#[cfg(feature = "board-rpi3")]
const DISPLAY_DRIVER_PATH: &str = "/bin/bcm-display";
#[cfg(not(feature = "board-rpi3"))]
const DISPLAY_DRIVER_PATH: &str = "/bin/virtio-gpu";

pub(crate) fn start_block_drivers() {
    let _ = sys_spawn_from_path("/bin/block");
    let _ = sys_spawn_from_path("/bin/nvme");
    for _ in 0..400 {
        if sys_lookup_service(service::BLOCK_DRIVER).is_some() {
            break;
        }
        ostd::task::yield_now();
    }
}

pub(crate) fn prepare_service(path: &str) {
    if path == "/bin/net" {
        let _ = sys_spawn_from_path("/bin/virtio-net");
        let _ = sys_spawn_from_path("/bin/e1000");
        if sys_lookup_service(service::BLOCK_DRIVER).is_none() {
            let _ = sys_spawn_from_path("/bin/nvme");
        }
        for _ in 0..4 {
            ostd::task::yield_now();
        }
    }
    if path == "/bin/compositor" {
        let _ = sys_spawn_from_path(DISPLAY_DRIVER_PATH);
        for _ in 0..4 {
            ostd::task::yield_now();
        }
    }
}

pub(crate) fn spawn_optional_services() -> Option<usize> {
    #[cfg(not(feature = "hypervisor-min"))]
    match sys_spawn_from_path("/bin/fb-console") {
        SyscallResult::Ok(_) => ostd::io::println("Init: fb-console spawned."),
        SyscallResult::Err(_) => ostd::io::println("Init: fb-console spawn failed."),
    }

    let hypervisor_tid = match sys_spawn_from_path("/bin/hypervisor") {
        SyscallResult::Ok(tid) => {
            let _ = sys_notify_on_exit(tid);
            if let SyscallResult::Err(_) =
                sys_register_service(api::hypervisor::HYPERVISOR_SERVICE_ID, tid)
            {
                ostd::io::println("Init: hypervisor service registration failed.");
                None
            } else {
                Some(tid)
            }
        }
        _ => None,
    };

    #[cfg(not(feature = "hypervisor-min"))]
    {
        let _ = sys_spawn_from_path("/bin/silo-test");
        let _ = sys_spawn_from_path("/bin/vfs-test");
        let _ = sys_spawn_from_path("/bin/srv-test");
    }
    hypervisor_tid
}
