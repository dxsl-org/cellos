//! The Cell's syscall half: PCIe probe, firmware and DMA through the kernel, and the
//! `XE_GPU` receive loop.

use crate::bring_up::{bring_up, Platform};
use crate::identity::VENDOR_INTEL;
use crate::owner_watch::WatchSys;
use crate::service::{Outcome, Service};
use intel_gpu_core::wire::{MAX_REPLY_LEN, MAX_REQUEST_LEN};
use ostd::grant::GrantHandle;
use ostd::io::print_fmt;
use ostd::mmio;
use ostd::syscall::{self, PcieDeviceInfo, SyscallResult};
use types::{ViError, ViResult};

/// BAR0 (`GTTMMADR`) span the driver maps: 8 MiB of registers, then the 8 MiB GSM.
const BAR0_LEN: usize = 0x100_0000;
const FIND_ATTEMPTS: usize = 200;
/// Largest GuC image accepted; the pinned ones are 365,376 (PTL) and 348,992 (LNL) bytes.
const FIRMWARE_MAX: usize = 512 * 1024;
/// GPU-visible client memory reported by `DEVICE_QUERY`: none until client buffers can be
/// exported to the driver (CP1 437/438, not yet in the kernel).
const CLIENT_SYSMEM_BYTES: u64 = 0;

// A request and the kernel's caller-identity trailer must share one receive buffer.
const _: () =
    assert!(MAX_REQUEST_LEN + api::caller_identity::CALLER_IDENTITY_LEN <= api::ipc::IPC_BUF_SIZE);

/// Device IDs from `include/drm/intel/pciids.h` (PTL, WCL and LNL lists).
pub const QUERY_DEVICE_IDS: &[u16] = &[
    0xB080, 0xB081, 0xB082, 0xB083, 0xB084, 0xB085, 0xB086, 0xB087, 0xB08F, 0xB090, 0xB0A0, 0xB0A1,
    0xB0B0, // PTL
    0xFD80, 0xFD81, // WCL
    0x6420, 0x64A0, 0x64B0, // LNL
];

/// Probe, bring the GPU up, then serve `XE_GPU`. Any bring-up failure exits the Cell.
pub fn cell_main() {
    let info = find_gpu().unwrap_or_else(|| fail("probe: no matching Intel GPU found on PCIe"));
    let bar0_len = match info.bar0_len as usize {
        0 => BAR0_LEN,
        len => len.min(BAR0_LEN),
    };
    if info.bar0_base == 0 {
        fail("probe: BAR0 is not a memory BAR");
    }
    let mmio = mmio::request_region(info.bar0_base as usize, bar0_len)
        .unwrap_or_else(|_| fail("probe: failed to map BAR0"));
    let mut platform = CellPlatform {
        bdf: info.bdf,
        firmware: None,
    };
    let ready = match bring_up(mmio, &mut platform) {
        Ok(ready) => ready,
        Err(error) => {
            log(format_args!("[intel-gpu] bring-up failed: {:?}\n", error));
            syscall::sys_exit(1);
        }
    };
    let identity = &ready.controller.identity;
    log(format_args!(
        "[intel-gpu] ready: {} (ver {}.{:02}.{}) clock={}MHz dss=0x{:x} guc@ggtt=0x{:x}\n",
        identity.name(),
        identity.ip_ver_major,
        identity.ip_ver_minor,
        identity.ip_ver_rev,
        identity.ref_clock_hz / 1_000_000,
        identity.dss_geometry_mask,
        ready.guc_ggtt,
    ));
    // `service::XE_GPU` (21) is below PRIVATE_ID_BASE: init registers this Cell for it from
    // its services.toml row (CP1 D-G8); the kernel refuses a self-registration.
    serve(Service::new(identity.device_info(CLIENT_SYSMEM_BYTES)));
}

fn find_gpu() -> Option<PcieDeviceInfo> {
    let mut info = PcieDeviceInfo::zeroed();
    for _ in 0..FIND_ATTEMPTS {
        for &dev_id in QUERY_DEVICE_IDS {
            if syscall::sys_find_pcie_device_by_vendor(VENDOR_INTEL, dev_id, &mut info).is_ok() {
                return Some(info);
            }
        }
        syscall::sys_yield();
    }
    None
}

fn serve(mut service: Service) -> ! {
    let mut buf = [0u8; api::ipc::IPC_BUF_SIZE];
    let mut reply = [0u8; MAX_REPLY_LEN];
    let mut unavailable_logged = false;
    loop {
        // Zeroed so a death record (no trailer) never shows an earlier sender's identity.
        buf.fill(0);
        let SyscallResult::Ok(sender) = syscall::sys_recv_attested(0, &mut buf) else {
            syscall::sys_yield();
            continue;
        };
        if sender == 0 {
            continue;
        }
        // Read before anything else can receive: the token of an `IpcSubmit` request.
        let operation = ostd::ipc::current();
        match service.handle(&buf, sender, &mut reply, &mut KernelWatch) {
            Outcome::Reply {
                len,
                watch_unavailable,
            } => {
                if watch_unavailable && !unavailable_logged {
                    unavailable_logged = true;
                    log(format_args!(
                        "[intel-gpu] client death watch unavailable; files are released on close\n"
                    ));
                }
                match operation {
                    Some(operation) => {
                        let _ = ostd::ipc::reply(operation, &reply[..len]);
                    }
                    // A plain `Send` client: never block the driver on one that is not receiving.
                    None => {
                        let _ = syscall::sys_try_send(sender, &reply[..len]);
                    }
                }
            }
            Outcome::Released { files } => {
                log(format_args!(
                    "[intel-gpu] client exit: released {} DRM files\n",
                    files
                ));
            }
            Outcome::Dropped => {}
        }
    }
}

/// The kernel half of [`WatchSys`], valid only inside the receive turn of the message
/// being served (the kernel binds the grant to it).
struct KernelWatch;

impl WatchSys for KernelWatch {
    fn watch(&mut self, cell_id: u64, generation: u64) -> Option<(usize, u64)> {
        syscall::sys_watch_cell_owner(cell_id, generation)
            .map(|(owner, token)| (owner.root_tid as usize, token))
    }

    fn cancel(&mut self, token: u64) {
        syscall::sys_cancel_cell_owner_watch(token);
    }
}

/// Firmware read from the VFS into a grant the GPU is then authorized to read.
struct CellPlatform {
    bdf: u32,
    firmware: Option<(GrantHandle<u8>, usize)>,
}

impl Platform for CellPlatform {
    fn read_firmware(&mut self, path: &str) -> Result<Option<usize>, ViError> {
        let Ok(mut file) = ostd::fs::File::open(path) else {
            return Ok(None);
        };
        let mut grant = GrantHandle::<u8>::alloc(FIRMWARE_MAX).ok_or(ViError::OutOfMemory)?;
        let len = grant.with_bytes_mut(|bytes| read_all(&mut file, bytes))?;
        let _ = file.close();
        self.firmware = Some((grant, len));
        Ok(Some(len))
    }

    fn with_firmware<T>(&mut self, f: impl FnOnce(&[u8]) -> T) -> T {
        match &mut self.firmware {
            Some((grant, len)) => {
                let len = *len;
                grant.with_bytes(|bytes| f(&bytes[..len]))
            }
            None => f(&[]),
        }
    }

    fn map_firmware(&mut self) -> Result<u64, ViError> {
        let (grant, _) = self.firmware.as_ref().ok_or(ViError::NotFound)?;
        syscall::sys_grant_dma(self.bdf, grant.id() as u64, grant.len())
            .map_err(|_| ViError::PermissionDenied)
    }
}

/// Fill `bytes` from `file`; a file that does not fit is refused, never truncated.
fn read_all(file: &mut ostd::fs::File, bytes: &mut [u8]) -> ViResult<usize> {
    let mut len = 0;
    loop {
        if len == bytes.len() {
            let mut more = [0u8; 1];
            return match file.read(&mut more)? {
                0 => Ok(len),
                _ => Err(ViError::OutOfMemory),
            };
        }
        match file.read(&mut bytes[len..])? {
            0 => return Ok(len),
            n => len += n,
        }
    }
}

fn log(args: core::fmt::Arguments<'_>) {
    let _ = print_fmt(args);
}

fn fail(what: &str) -> ! {
    log(format_args!("[intel-gpu] {}\n", what));
    syscall::sys_exit(1);
}
