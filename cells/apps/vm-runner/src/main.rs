// SPDX-License-Identifier: MIT
//! Universal Linux MicroVM App Runner for CellOS Tier 3 workloads.
//!
//! Enforces the unified naming convention:
//!   - App Directory: /apps/<app>/
//!   - Static Config: /apps/<app>/<app>.toml
//!   - Dynamic Script: /apps/<app>/<app>.lua
//!   - Payload Disk:  /apps/<app>/<app>.ext4
//!
//! Enables zero-compilation app installation and launches on Intel VT-x hardware.

#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use alloc::format;
use alloc::string::String;
use ostd::io::println;
use ostd::syscall::sys_exit;

abi::declare_manifest!(block_io = true, network = true, spawn = true);
abi::declare_syscalls![
    Log,
    Send,
    SendGather,
    Recv,
    RecvTimeout,
    OpenCap,
    CloseCap,
    // `ostd::args()` reads the spawner's argv out of the state stash.
    StateRestore,
    LookupServiceBound,
    Spawn,
    Yield,
    Exit
];

ostd::cell_main!(cell_main);

struct AppConfig {
    #[allow(dead_code)]
    name: String,
    base_image: String,
    app_image: String,
    mode: String,
    ram_mb: usize,
    #[allow(dead_code)]
    entrypoint: String,
}

impl AppConfig {
    fn from_convention(app_name: &str) -> Self {
        Self {
            name: String::from(app_name),
            base_image: String::from("/sys/guests/linux-base.ext4"),
            app_image: format!("/apps/{}/{}.ext4", app_name, app_name),
            mode: String::from("gui"),
            ram_mb: 1024,
            entrypoint: format!("/opt/app/{}/{}", app_name, app_name),
        }
    }
}

fn extract_app_name(arg: &str) -> String {
    let trimmed = arg.trim_matches('/');
    if let Some(rest) = trimmed.strip_prefix("apps/") {
        let name = rest.split('/').next().unwrap_or(rest);
        let name = name.strip_suffix(".toml").unwrap_or(name);
        let name = name.strip_suffix(".lua").unwrap_or(name);
        String::from(name)
    } else {
        let name = trimmed.split('/').next_back().unwrap_or(trimmed);
        let name = name.strip_suffix(".toml").unwrap_or(name);
        let name = name.strip_suffix(".lua").unwrap_or(name);
        String::from(name)
    }
}

fn cell_main() {
    println("[vm-runner] CellOS Universal Linux App Runner v0.1");

    // `ostd::args()` carries no program-name entry: the app is the first argument.
    let args = ostd::args();
    let target_arg = match args.first() {
        Some(arg) if !arg.is_empty() => arg.as_str(),
        _ => "chrome",
    };

    let app_name = extract_app_name(target_arg);
    let config_path = format!("/apps/{}/{}.toml", app_name, app_name);
    let lua_path = format!("/apps/{}/{}.lua", app_name, app_name);
    let config = AppConfig::from_convention(&app_name);

    println(&format!("[vm-runner] Resolving app: '{}'", app_name));
    println(&format!("[vm-runner] Convention Config: {}", config_path));
    println(&format!("[vm-runner] Convention Script: {}", lua_path));
    println(&format!(
        "[vm-runner] Convention Disk:   {}",
        config.app_image
    ));

    // Check if dynamic Lua hook exists
    let has_lua = match ostd::syscall::sys_open_cap(&lua_path) {
        Ok(cap) => {
            ostd::syscall::sys_close_cap(cap);
            true
        }
        Err(_) => false,
    };
    if has_lua {
        println(&format!(
            "[vm-runner] Dynamic Lua hook active: {}",
            lua_path
        ));
    }

    // Connect to Hypervisor service (service ID 14)
    let hv_tid = ostd::service::provider_tid(abi::hypervisor::HYPERVISOR_SERVICE_ID).unwrap_or(0);
    if hv_tid == 0 {
        println("[vm-runner] ERROR: Hypervisor service (ID 14) unavailable.");
        sys_exit(1);
    }

    println(&format!(
        "[vm-runner] Connected to Hypervisor Service (tid={})",
        hv_tid
    ));
    println(&format!(
        "[vm-runner] Spawning MicroVM on Intel VT-x: base='{}' app='{}' mode={} ram={}M",
        config.base_image, config.app_image, config.mode, config.ram_mb
    ));

    let launch_req = wire::vmm_session::VmmLaunchRequest::new(&app_name, 1024, 768);
    let mut req_buf = [0u8; wire::vmm_session::VmmLaunchRequest::LEN];
    if launch_req.encode(&mut req_buf).is_err() {
        println("[vm-runner] ERROR: Failed to encode VmmLaunchRequest.");
        sys_exit(1);
    }

    // Posted: the hypervisor only polls between vCPU exits, and a blocking send
    // would hang here forever (instead of timing out below) if it never polls.
    if !matches!(
        ostd::syscall::sys_post(hv_tid, &req_buf),
        ostd::syscall::SyscallResult::Ok(0)
    ) {
        println("[vm-runner] ERROR: Failed to send VmmLaunchRequest to Hypervisor.");
        sys_exit(1);
    }

    let mut reply_buf = [0u8; wire::vmm_session::VmmLaunchReply::LEN];
    // RecvTimeout returns the sender tid (0 on timeout), not a byte count.
    match ostd::syscall::sys_recv_timeout(hv_tid, &mut reply_buf, 200) {
        ostd::syscall::SyscallResult::Ok(sender) if sender == hv_tid => {
            match wire::vmm_session::VmmLaunchReply::decode(&reply_buf) {
                Ok(reply) if reply.status == wire::vmm_session::VMM_STATUS_OK => {
                    println(&format!(
                        "[vm-runner] MicroVM active (session_id={}). Session ready.",
                        reply.session_id
                    ));
                    sys_exit(0);
                }
                Ok(reply) => {
                    println(&format!(
                        "[vm-runner] ERROR: Hypervisor rejected launch (status={})",
                        reply.status
                    ));
                    sys_exit(1);
                }
                Err(_) => {
                    println("[vm-runner] ERROR: Corrupt reply from Hypervisor.");
                    sys_exit(1);
                }
            }
        }
        _ => {
            println("[vm-runner] ERROR: Hypervisor reply timeout.");
            sys_exit(1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_app_name() {
        assert_eq!(extract_app_name("chrome"), "chrome");
        assert_eq!(extract_app_name("/apps/chrome"), "chrome");
        assert_eq!(extract_app_name("apps/nginx"), "nginx");
        assert_eq!(extract_app_name("python"), "python");
    }

    #[test]
    fn test_app_config_convention() {
        let cfg = AppConfig::from_convention("nginx");
        assert_eq!(cfg.name, "nginx");
        assert_eq!(cfg.base_image, "/sys/guests/linux-base.ext4");
        assert_eq!(cfg.app_image, "/apps/nginx/nginx.ext4");
        assert_eq!(cfg.mode, "gui");
        assert_eq!(cfg.ram_mb, 1024);
        assert_eq!(cfg.entrypoint, "/opt/app/nginx/nginx");
    }
}
