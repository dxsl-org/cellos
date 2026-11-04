//! virtio-scsi Driver Cell — Tier-1 Privileged Driver Cell (x86_64).
//!
//! Claims the first QEMU `virtio-scsi-pci` controller (modern 1af4:1048,
//! then transitional 1af4:1004), brings it up over the cell-side virtio PCI
//! transport with a polled request virtqueue in authorized DMA memory, finds
//! the first direct-access disk LUN through `scsi-block`, publishes the block
//! route of its BDF via `sys_register_block_device(BlockDevice::Pci { .. })`
//! and serves the `DrvRequest` sector-I/O protocol (same wire format as the
//! virtio-blk/NVMe cells) to the registered VFS only.
//!
//! Interrupts are not routed (phase 04b deferred): completion is polled with
//! a time budget.
//!
//! # Law 4 exception
//! `src/dma.rs` (DMA memory) and `src/config.rs` (device configuration
//! reads) use `unsafe`; every block carries a `// SAFETY:` comment.

// Test builds only: a hosted `cargo test` gets std and libtest's entry, so the
// pure modules (`proto`, `ring`, `config`, `wait`) run as host unit tests.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

mod bars;
mod config;
mod dispatch;
mod dma;
mod init;
mod proto;
mod queue;
mod ring;
mod scan;
mod wait;

use core::fmt;
use core::sync::atomic::{AtomicUsize, Ordering};
use init::InitError;
use ostd::app::{AppContext, AppEvent};
use ostd::io::print_fmt;
use ostd::sync::Mutex;
use ostd::syscall::{
    sys_exit, sys_lookup_service, sys_query_pcie_function, sys_register_block_device, sys_send,
    BlockDevice,
};
use scan::Disk;
use types::block_wire::{BlockOutcome, SECTOR_SIZE};
use virtio_pci_transport::{InitFailure, TransportError};

static STATE: Mutex<Option<Disk>> = Mutex::new(None);

/// Tid of the registered VFS once a request from it was accepted. Tids are
/// never reissued, so a cached match stays valid for the life of that VFS.
static VFS_TID: AtomicUsize = AtomicUsize::new(0);

/// Whether `sender_tid` is the registered VFS, the only accepted block client.
fn sender_is_vfs(sender_tid: usize) -> bool {
    let cached = VFS_TID.load(Ordering::Relaxed);
    if cached != 0 && cached == sender_tid {
        return true;
    }
    if sys_lookup_service(api::syscall::service::VFS) == Some(sender_tid) {
        VFS_TID.store(sender_tid, Ordering::Relaxed);
        return true;
    }
    false
}

/// PCI requester ID printed as `bb:dd.f`.
struct Bdf(u32);

impl fmt::Display for Bdf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bdf = self.0;
        write!(
            f,
            "{:02x}:{:02x}.{:x}",
            (bdf >> 8) & 0xFF,
            (bdf >> 3) & 0x1F,
            bdf & 0x7
        )
    }
}

/// Discovery through registration; exits the cell on any failure.
fn start() {
    let Some(info) = init::discover() else {
        // Nothing to claim: no controller, or no PcieDriverCap.
        ostd::io::println("[virtio-scsi] no controller - exiting");
        sys_exit(0);
    };
    let bdf = Bdf(info.bdf);
    let record = match sys_query_pcie_function(info.bdf) {
        Ok(record) => record,
        Err(error) => {
            let _ = print_fmt(format_args!(
                "[virtio-scsi] query {bdf} failed: {error:?}\n"
            ));
            sys_exit(1)
        }
    };
    let (queue, limits) = match init::bring_up(&record) {
        Ok(up) => up,
        // Refused at construction, or at negotiation when the offer changed
        // across the reset.
        Err(
            InitError::Transport(TransportError::AccessPlatformMissing)
            | InitError::Negotiation(Some(InitFailure::AccessPlatformMissing)),
        ) => {
            let _ = print_fmt(format_args!(
                "[virtio-scsi] refused {bdf}: VIRTIO_F_ACCESS_PLATFORM required (DMA remapped)\n"
            ));
            sys_exit(0)
        }
        Err(error) => {
            let _ = print_fmt(format_args!("[virtio-scsi] init {bdf} failed: {error}\n"));
            sys_exit(1)
        }
    };
    let Some(disk) = scan::first_disk(queue, &limits) else {
        let _ = print_fmt(format_args!("[virtio-scsi] no disk on {bdf} - exiting\n"));
        sys_exit(0)
    };
    if disk.block_size() as usize != SECTOR_SIZE {
        let _ = print_fmt(format_args!(
            "[virtio-scsi] unsupported block size {} on {bdf} (block protocol needs {SECTOR_SIZE}) - exiting\n",
            disk.block_size()
        ));
        drop(disk); // resets the device before the cell goes away
        sys_exit(0)
    }
    let target = disk.target();
    let lun = proto::lun_number(disk.lun()).unwrap_or_default();
    let blocks = disk.block_count();

    // Publish state before registration: VFS may route a request as soon as
    // the syscall succeeds.
    *STATE.lock() = Some(disk);
    if let Err(error) = sys_register_block_device(BlockDevice::Pci {
        bdf: info.bdf as u16,
    }) {
        let disk = STATE.lock().take();
        drop(disk);
        let _ = print_fmt(format_args!(
            "[virtio-scsi] block driver registration failed: {error:?} ({bdf})\n"
        ));
        sys_exit(1);
    }
    let _ = print_fmt(format_args!(
        "[virtio-scsi] pci {bdf} {:04x}:{:04x} target={target} lun={lun} blocks={blocks} irq=polled reason=04b-deferred\n",
        record.vendor_id, record.device_id
    ));
    ostd::io::println("[virtio-scsi] ready");
}

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => start(),

        // VFS speaks the raw DrvRequest protocol (no 0xAC App-SDK envelope),
        // so requests may arrive as RawMessage; accept Message too — layout is
        // identical. Only the registered VFS may issue block requests; any
        // other sender gets `NotIssued` without touching the device.
        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
            let mut reply = [0u8; dispatch::REPLY_SIZE];
            let len = if !sender_is_vfs(sender_tid) {
                let _ = print_fmt(format_args!(
                    "[virtio-scsi] refused block request from tid={sender_tid}: sender is not the registered VFS\n"
                ));
                reply[0] = BlockOutcome::NotIssued.to_byte();
                1
            } else if let Some(disk) = STATE.lock().as_mut() {
                dispatch::handle(disk, data.as_ref(), &mut reply)
            } else {
                reply[0] = BlockOutcome::NotIssued.to_byte(); // not initialised
                1
            };
            // Blocking send: the VFS client issues sys_send + a blocking
            // sys_recv(tid), so it is waiting for this reply.
            let _ = sys_send(sender_tid, &reply[..len]);
        }

        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => sys_exit(0),

        _ => {}
    }
}

ostd::run_app!(handler);

// PcieDriverCap is granted by the loader at spawn; this manifest declares NO
// privileged flags.
api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);
