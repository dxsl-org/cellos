use alloc::format;
use alloc::vec::Vec;
use api::ipc::{VfsRequest, VfsResponse};
use cellos_boot_config::{parse_cells, validate_plan, CellConfig, MAX_CONFIG_BYTES};
use ostd::clients::VfsClient;

const SERVICES_PATH: &str = "/etc/cellos/services.toml";
const AUTOLOAD_PATH: &str = "/etc/cellos/autoload.toml";

/// Registry publication is not VFS application readiness. Only an actual
/// successful, operation-correlated RPC proves the bootstrap server can serve.
pub(crate) fn wait_for_vfs(tid: usize, timeout: u64) -> bool {
    let started = crate::service_table::now_ticks();
    let mut send = [0u8; 512];
    let mut recv = [0u8; 512];
    for _ in 0..10_000 {
        if ostd::syscall::sys_lookup_service(api::syscall::service::VFS) != Some(tid)
            || crate::activation::task_alive(tid) == Some(false)
        {
            return false;
        }
        if matches!(ostd::ipc::service_call_typed_bounded::<_, VfsResponse<'_>>(
            tid, &VfsRequest::Stat("/"), &mut send, &mut recv, 50,
        ), Ok(VfsResponse::Stat { is_dir: true, .. })) {
            return true;
        }
        if crate::service_table::now_ticks().wrapping_sub(started) >= timeout {
            return false;
        }
        ostd::task::yield_now();
    }
    false
}

/// Read and validate both files before any configured cell can launch.
pub(crate) fn load() -> Result<(CellConfig, CellConfig), alloc::string::String> {
    let mut vfs = VfsClient::new();
    let services_bytes = read_required(&mut vfs, SERVICES_PATH)?;
    let services = parse_cells(&services_bytes)
        .map_err(|error| format!("{SERVICES_PATH}: {error}"))?;

    // Stat must precede the read: a file known to be present but unreadable,
    // oversized, empty or malformed must not become the optional empty plan.
    // The existing VFS protocol uses IO for absent paths as well as backend stat
    // failures; it cannot distinguish those two cases at this boundary.
    let mut send = [0u8; 512];
    let mut recv = [0u8; 512];
    let tid = ostd::syscall::sys_lookup_service(api::syscall::service::VFS)
        .ok_or_else(|| format!("{AUTOLOAD_PATH}: VFS unavailable"))?;
    let response = ostd::ipc::service_call_typed_bounded::<_, VfsResponse<'_>>(
        tid, &VfsRequest::Stat(AUTOLOAD_PATH), &mut send, &mut recv, 50,
    ).map_err(|error| format!("{AUTOLOAD_PATH}: stat RPC failed: {error:?}"))?;
    let autoload = match response {
        VfsResponse::Stat { size, is_dir: false } if size <= MAX_CONFIG_BYTES as u64 => {
            let bytes = read_required(&mut vfs, AUTOLOAD_PATH)?;
            parse_cells(&bytes).map_err(|error| format!("{AUTOLOAD_PATH}: {error}"))?
        }
        VfsResponse::Err(1) => {
            ostd::io::println("Init: autoload.toml absent — optional autoload is explicitly empty.");
            CellConfig::default()
        }
        other => return Err(format!("{AUTOLOAD_PATH}: invalid stat response: {other:?}")),
    };
    validate_plan(&services, &autoload).map_err(|error| format!("boot plan: {error}"))?;
    Ok((services, autoload))
}

fn read_required(vfs: &mut VfsClient, path: &str) -> Result<Vec<u8>, alloc::string::String> {
    let expected_size = match vfs.stat(path) {
        Ok((size, false)) if size <= MAX_CONFIG_BYTES as u64 => size as usize,
        Ok(_) => return Err(format!("{path}: not a bounded configuration file")),
        Err(error) => return Err(format!("{path}: required file unavailable: {error:?}")),
    };
    let bytes = vfs.read_file_bounded(path, MAX_CONFIG_BYTES)
        .map_err(|error| format!("{path}: bounded VFS read failed: {error:?}"))?;
    if bytes.len() != expected_size {
        return Err(format!("{path}: truncated or inconsistent configuration (expected {expected_size} bytes, read {})", bytes.len()));
    }
    Ok(bytes)
}
