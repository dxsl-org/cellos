//! VMware PVSCSI Driver Cell — Tier-1 Privileged Driver Cell (S3 phase 09).
//!
//! PVSCSI (`15AD:07C0`) is the paravirtual SCSI host VMware recommends; QEMU
//! models it as `-device pvscsi`. The cell claims the first adapter by exact
//! ID (424), takes its 32 KiB register window from the 434 function record,
//! resets it, hands it one-page request and completion rings in one
//! `GrantDma`-authorized allocation (`SETUP_RINGS`), finds the first
//! direct-access disk through `scsi-block`, publishes the block route of its
//! BDF (`BlockDevice::Pci`, 416) and serves the `DrvRequest` sector-I/O
//! protocol to the registered VFS only — the virtio-scsi pattern.
//!
//! Completions are interrupt-driven (H-S3-5): 435 grants MSI-X or MSI and
//! every wait parks on `WaitCompletion(IRQ)` inside the command's time budget;
//! without a vector the cell polls and says why (`irq=polled reason=…`).
//!
//! Law 4 exception: `unsafe` is confined to the DMA area (`dma.rs`); MMIO goes
//! through the bounds-checked `ostd::mmio::MmioRegion`.

// Hosted `cargo test` gets std and libtest's entry, so the pure modules run as
// host unit tests; Cell targets stay no_std.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

mod dispatch;
mod dma;
mod error;
mod irq;
mod proto;
mod queue;
mod regs;
mod rings;
mod scan;

use api::pcie_function::{ViPcieFunctionV1, BAR_KIND_MEM32, BAR_KIND_MEM64};
use ostd::app::{AppContext, AppEvent};
use ostd::io::print_fmt;
use ostd::sync::Mutex;
use ostd::syscall::{
    sys_exit, sys_find_pcie_device_by_vendor, sys_query_pcie_function, sys_register_block_device,
    BlockDevice, PcieDeviceInfo,
};
use scan::Disk;
use types::block_wire::SECTOR_SIZE;

/// VMware's PCI vendor ID and PVSCSI's device ID.
const VENDOR_VMWARE: u16 = 0x15AD;
const DEVICE_PVSCSI: u16 = 0x07C0;
/// Bounded wait for the PCI scan to publish the device table.
const DISCOVERY_ATTEMPTS: usize = 200;

static STATE: Mutex<Option<Disk>> = Mutex::new(None);

fn fail(what: core::fmt::Arguments<'_>) -> ! {
    let _ = print_fmt(format_args!("[pvscsi] {what}\n"));
    sys_exit(1)
}

/// The register window: the first memory BAR of at least 32 KiB (BAR0 on
/// QEMU; VMware's BAR0 is I/O ports).
fn window(record: &ViPcieFunctionV1) -> Option<(usize, u8)> {
    record.bars.iter().enumerate().find_map(|(index, bar)| {
        let memory = bar.kind == BAR_KIND_MEM32 || bar.kind == BAR_KIND_MEM64;
        (memory && bar.base != 0 && bar.len >= regs::WINDOW_LEN as u64)
            .then_some((bar.base as usize, index as u8))
    })
}

fn start() {
    let mut info = PcieDeviceInfo::zeroed();
    let found = (0..DISCOVERY_ATTEMPTS).any(|_| {
        let hit = matches!(
            sys_find_pcie_device_by_vendor(VENDOR_VMWARE, DEVICE_PVSCSI, &mut info),
            Ok(true)
        );
        if !hit {
            ostd::task::yield_now();
        }
        hit
    });
    if !found {
        ostd::io::println("[pvscsi] no controller - exiting");
        sys_exit(0);
    }
    let bdf = info.bdf;
    let (b, d, f) = ((bdf >> 8) & 0xFF, (bdf >> 3) & 0x1F, bdf & 7);
    let record = match sys_query_pcie_function(bdf) {
        Ok(record) => record,
        Err(error) => fail(format_args!(
            "query {b:02x}:{d:02x}.{f:x} failed: {error:?}"
        )),
    };
    let Some((base, bar)) = window(&record) else {
        fail(format_args!(
            "{b:02x}:{d:02x}.{f:x} has no 32 KiB memory BAR"
        ));
    };
    let regs = match ostd::mmio::request_region(base, regs::WINDOW_LEN) {
        Ok(region) => region,
        Err(error) => fail(format_args!("BAR{bar} claim failed: {error:?}")),
    };
    let adapter = match queue::Adapter::start(regs, bdf) {
        Ok(adapter) => adapter,
        Err(error) => fail(format_args!("init {b:02x}:{d:02x}.{f:x} failed: {error}")),
    };
    let (granted, reason) = (adapter.irq.mode(), adapter.irq.reason);
    let Some(disk) = scan::first_disk(adapter) else {
        let _ = print_fmt(format_args!(
            "[pvscsi] no disk on {b:02x}:{d:02x}.{f:x} - exiting\n"
        ));
        sys_exit(0)
    };
    if disk.block_size() as usize != SECTOR_SIZE {
        let _ = print_fmt(format_args!(
            "[pvscsi] unsupported block size {} (block protocol needs {SECTOR_SIZE}) - exiting\n",
            disk.block_size()
        ));
        drop(disk); // resets the adapter before the cell goes away
        sys_exit(0)
    }
    let (target, lun, blocks) = (disk.target(), disk.lun()[1], disk.block_count());
    let evidence = irq::evidence();
    let (mode, why, vectors) = match granted {
        Some((mode, vectors)) if !evidence.refused => (mode, "", vectors),
        Some(_) => ("polled reason=", "wait-refused", 0),
        None => ("polled reason=", reason, 0),
    };
    let irq::Evidence {
        wakes,
        expiries,
        expiry_ready,
        expiry_empty,
        ..
    } = evidence;

    // Publish state before registration: VFS may route a request as soon as
    // the syscall succeeds.
    *STATE.lock() = Some(disk);
    if let Err(error) = sys_register_block_device(BlockDevice::Pci { bdf: bdf as u16 }) {
        let disk = STATE.lock().take();
        drop(disk);
        fail(format_args!("block driver registration failed: {error:?}"));
    }
    let _ = print_fmt(format_args!(
        "[pvscsi] pci {b:02x}:{d:02x}.{f:x} {:04x}:{:04x} target={target} lun={lun} blocks={blocks} irq={mode}{why} vectors={vectors} irq_wakes={wakes} park_expiries={expiries} expiry_ready={expiry_ready} expiry_empty={expiry_empty}\n",
        record.vendor_id, record.device_id
    ));
    ostd::io::println("[pvscsi] ready");
}

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => start(),

        AppEvent::Message { sender_tid, data } | AppEvent::RawMessage { sender_tid, data } => {
            dispatch::serve(&STATE, sender_tid, data.as_ref());
        }

        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => sys_exit(0),

        _ => {}
    }
}

ostd::run_app!(handler);

// PcieDriverCap is granted by init via the reviewed `/bin/pvscsi` launch
// edge; this manifest declares no privileged flags.
api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

// Exactly what the cell calls: the AppContext base set (`LookupService` also
// resolves the VFS sender), the exact-ID PCIe query and 434 record, the BAR
// claim, block-route registration, the per-command clock budget, exit, the
// DMA grant (and its release when authorization fails), and the completion
// interrupt (435 at start, `WaitCompletion(IRQ)` parks).
api::declare_syscalls![
    Send,
    Recv,
    TryRecv,
    Reply,
    Log,
    Heartbeat,
    LookupService,
    GetTime,
    RecvTimeout,
    FindPcieDeviceByVendor,
    QueryPcieFunction,
    RequestMmio,
    RegisterBlockDriver,
    Exit,
    GrantAlloc,
    GrantDma,
    GrantFree,
    AllocPcieIrq,
    WaitCompletion
];
