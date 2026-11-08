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

api::declare_manifest!(block_io = true, network = true, spawn = true);
api::declare_syscalls![
    Log,
    Send,
    Recv,
    RecvTimeout,
    LookupService,
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

    let args = ostd::args();
    let target_arg = if args.len() > 1 && !args[1].is_empty() {
        &args[1]
    } else {
        "chrome"
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
    let hv_tid =
        ostd::syscall::sys_lookup_service(api::hypervisor::HYPERVISOR_SERVICE_ID).unwrap_or(0);
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

    let launch_req = api::services::vmm_session::VmmLaunchRequest::new(&app_name, 1024, 768);
    let mut req_buf = [0u8; api::services::vmm_session::VmmLaunchRequest::LEN];
    if launch_req.encode(&mut req_buf).is_err() {
        println("[vm-runner] ERROR: Failed to encode VmmLaunchRequest.");
        sys_exit(1);
    }

    if !matches!(
        ostd::syscall::sys_send(hv_tid, &req_buf),
        ostd::syscall::SyscallResult::Ok(_)
    ) {
        println("[vm-runner] ERROR: Failed to send VmmLaunchRequest to Hypervisor.");
        sys_exit(1);
    }

    let mut reply_buf = [0u8; 64];
    match ostd::syscall::sys_recv_timeout(hv_tid, &mut reply_buf, 200) {
        ostd::syscall::SyscallResult::Ok(n)
            if n >= api::services::vmm_session::VmmLaunchReply::LEN =>
        {
            match api::services::vmm_session::VmmLaunchReply::decode(&reply_buf[..n]) {
                Ok(reply) if reply.status == api::services::vmm_session::VMM_STATUS_OK => {
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
