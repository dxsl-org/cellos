//! Export holder of the ADR-0033 CP1 §7 QEMU lanes (L1-L5): a Tier-1 PCIe
//! Driver Cell for QEMU's `edu` device (1234:11e8) under `-device intel-iommu`.
//!
//! Init registers it under the test-only holder service
//! [`edu_export_proto::SERVICE_ID`], which `test-hooks` kernels admit into
//! `DMA_EXPORT_SERVICES`. It maps client exports with `MapDmaExport` (438) and
//! drives the `edu` DMA engine at the returned window IOVA:
//!
//! * `edu` registers (QEMU `hw/misc/edu.c`): 0x80 source, 0x88 destination,
//!   0x90 count, 0x98 command (bit 0 run, bit 1 device→RAM, bit 2 raise IRQ
//!   0x100 when done), 0x24 interrupt status, 0x64 interrupt acknowledge; the
//!   device buffer lives at device address 0x40000 (4096 bytes; a transfer
//!   outside it aborts QEMU, so every transfer here is at most one page).
//! * The lane runs `edu` with `dma_mask=0x7fffffffff` so 39-bit window IOVAs
//!   reach the remapper unclamped.
//!
//! Every outcome is one `[edu-holder]` serial line the lane script asserts.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

use api::completion::source;
use api::pcie_function::{IRQ_MODE_ALLOW_MSI, IRQ_MODE_ALLOW_MSIX};
use edu_export_proto::{
    Msg, EDU_DEVICE, EDU_VENDOR, MSG_LEN, OP_COPY, OP_DIE, OP_DMA_TO, OP_HELLO, OP_MAP,
    OP_STALE_AFTER_DEATH, OP_STATUS, STATUS_BAD_REQUEST, STATUS_DMA_FAILED, STATUS_MAP_REFUSED,
    STATUS_OK,
};
use ostd::dma::{map_export, DmaBuf};
use ostd::io::{print_fmt, println};
use ostd::ipc::{self, Wake};
use ostd::mmio::{request_region, MmioRegion};
use ostd::syscall::{
    sys_alloc_pcie_irq, sys_exit, sys_find_pcie_device_by_vendor, sys_map_dma_export_into,
    sys_recv, sys_send, DmaExportError, PcieDeviceInfo, SyscallResult,
};

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![
    Log,
    Exit,
    Send,
    Recv,
    GetTime,
    FindPcieDeviceByVendor,
    RequestMmio,
    AllocPcieIrq,
    WaitCompletion,
    GrantAlloc,
    GrantDma,
    GrantFree,
    MapDmaExport,
    RevokeDmaExport,
    IpcCurrent,
    IpcReply,
];

ostd::cell_main!(cell_main);

const EDU_IDENT: usize = 0x00;
const EDU_INTR_STATUS: usize = 0x24;
const EDU_INTR_ACK: usize = 0x64;
const EDU_DMA_SRC: usize = 0x80;
const EDU_DMA_DST: usize = 0x88;
const EDU_DMA_CNT: usize = 0x90;
const EDU_DMA_CMD: usize = 0x98;
const EDU_DEV_BUF: u64 = 0x40000;
const CMD_RUN: u64 = 1;
const CMD_TO_RAM: u64 = 2;
const CMD_IRQ: u64 = 4;
const PAGE: usize = 4096;

/// The probed device, its MMIO window, its interrupt and a holder-owned page.
struct Edu {
    bdf: u32,
    mmio: MmioRegion,
    irq: bool,
    /// One holder-owned page of 0xEE, GrantDma-authorized (identity IOVA): the
    /// source of every "device writes garbage" transfer. `None` where GrantDma
    /// is refused (no remapper under a `Required` profile: the L5 boot).
    scratch: DmaBuf,
    scratch_iova: Option<u64>,
}

impl Edu {
    fn probe() -> Option<Self> {
        let mut info = PcieDeviceInfo::zeroed();
        let mut found = false;
        for _ in 0..200 {
            if sys_find_pcie_device_by_vendor(EDU_VENDOR, EDU_DEVICE, &mut info) == Ok(true) {
                found = true;
                break;
            }
            let _ = ipc::wait_any(source::TIMER, 5);
        }
        if !found {
            println("[edu-holder] FAIL edu device 1234:11e8 not found");
            return None;
        }
        let mmio = request_region(info.bar0_base as usize, info.bar0_len as usize).ok()?;
        let ident = mmio.read_u32(EDU_IDENT).ok()?;
        let irq = match sys_alloc_pcie_irq(info.bdf, 1, IRQ_MODE_ALLOW_MSIX | IRQ_MODE_ALLOW_MSI) {
            Ok(_) => true,
            Err(error) => {
                print_fmt(format_args!(
                    "[edu-holder] irq refused ({error:?}); polling\n"
                ));
                false
            }
        };
        let scratch = DmaBuf::alloc(1)?;
        if !scratch.write_bytes(0, &[0xEE; PAGE]) {
            return None;
        }
        let scratch_iova = scratch.authorize(info.bdf).ok();
        print_fmt(format_args!(
            "[edu-holder] ready bdf={:#06x} ident={ident:#010x} irq={} scratch-iova={scratch_iova:#x?}\n",
            info.bdf,
            if irq { "msi" } else { "polled" }
        ));
        Some(Self {
            bdf: info.bdf,
            mmio,
            irq,
            scratch,
            scratch_iova,
        })
    }

    /// One `edu` transfer of `count` (≤ 4096) bytes; `to_ram` selects
    /// device buffer → `ram`, otherwise `ram` → device buffer. Waits for the
    /// command's run bit to clear, parked on the MSI when one is held.
    fn dma(&self, ram: u64, count: u64, to_ram: bool) -> bool {
        let (src, dst) = if to_ram {
            (EDU_DEV_BUF, ram)
        } else {
            (ram, EDU_DEV_BUF)
        };
        let irq_bit = if self.irq { CMD_IRQ } else { 0 };
        let started = self.mmio.write::<u64>(EDU_DMA_SRC, src).is_ok()
            && self.mmio.write::<u64>(EDU_DMA_DST, dst).is_ok()
            && self.mmio.write::<u64>(EDU_DMA_CNT, count).is_ok()
            && self
                .mmio
                .write::<u64>(
                    EDU_DMA_CMD,
                    CMD_RUN | if to_ram { CMD_TO_RAM } else { 0 } | irq_bit,
                )
                .is_ok();
        if !started {
            return false;
        }
        for _ in 0..600 {
            let sources = if self.irq {
                source::IRQ | source::TIMER
            } else {
                source::TIMER
            };
            let woke = ipc::wait_any(sources, 2);
            if matches!(woke, Ok(Wake::Irq(_))) {
                if let Ok(status) = self.mmio.read_u32(EDU_INTR_STATUS) {
                    let _ = self.mmio.write_u32(EDU_INTR_ACK, status);
                }
            }
            match self.mmio.read::<u64>(EDU_DMA_CMD) {
                Ok(cmd) if cmd & CMD_RUN == 0 => return true,
                Ok(_) => {}
                Err(_) => return false,
            }
        }
        false
    }

    /// Make the device write one page of 0xEE at `iova`: scratch → device
    /// buffer, then device buffer → `iova`.
    fn write_garbage(&self, iova: u64, len: u64) -> bool {
        let Some(source) = self.scratch_iova else {
            return false;
        };
        self.scratch.write_bytes(0, &[0xEE; PAGE])
            && self.dma(source, PAGE as u64, false)
            && self.dma(iova, len.min(PAGE as u64), true)
    }
}

fn status(code: u64, iova: u64) -> [u8; MSG_LEN] {
    Msg::new(OP_STATUS, code, iova, 0).encode()
}

/// Answer the request just received: `IpcReply` for a submitted operation,
/// plain `Send` to a synchronous sender.
fn answer(op: Option<usize>, sender: usize, bytes: &[u8; MSG_LEN]) {
    match op {
        Some(op) => {
            let _ = ipc::reply(op, bytes);
        }
        None => {
            let _ = sys_send(sender, bytes);
        }
    }
}

fn cell_main() {
    let Some(edu) = Edu::probe() else {
        sys_exit(1);
    };
    let mut buf = [0u8; 256];
    let mut roles = 0u64;
    let mut l3_done = false;
    let mut deferred_hello: Option<(Option<usize>, usize)> = None;

    loop {
        let sender = match sys_recv(0, &mut buf) {
            SyscallResult::Ok(sender) => sender,
            SyscallResult::Err(_) => continue,
        };
        let op = ipc::current();
        let Some(msg) = Msg::decode(&buf) else {
            answer(op, sender, &status(STATUS_BAD_REQUEST, 0));
            continue;
        };
        match msg.op {
            OP_HELLO => {
                if roles == 0 {
                    roles = 1;
                    answer(op, sender, &Msg::new(OP_STATUS, 1, 0, 0).encode());
                } else if l3_done {
                    roles = 2;
                    answer(op, sender, &Msg::new(OP_STATUS, 2, 0, 0).encode());
                } else {
                    // The second client runs L4, which kills this holder: it
                    // must wait until L3 is over.
                    deferred_hello = Some((op, sender));
                }
            }
            OP_COPY => {
                // L5 (holder side): a record buffer of the wrong length is
                // refused before any effect.
                let mut short = [0u8; 47];
                let refused = sys_map_dma_export_into(msg.a, edu.bdf, &mut short);
                print_fmt(format_args!(
                    "[edu-holder] L5 out_len=47 refused: {refused:?}\n"
                ));
                let reply = match map_export(msg.a, edu.bdf) {
                    Ok(rec) => {
                        print_fmt(format_args!(
                            "[edu-holder] L1 mapped id={} iova={:#x} len={} owner={}/{}\n",
                            rec.export_id,
                            rec.iova,
                            rec.len,
                            rec.owner_cell_id,
                            rec.owner_generation
                        ));
                        let copied = edu.dma(rec.iova, PAGE as u64, false)
                            && edu.dma(rec.iova + PAGE as u64, PAGE as u64, true);
                        print_fmt(format_args!(
                            "[edu-holder] L1 edu copy window->device->window+4096 done={copied}\n"
                        ));
                        status(if copied { STATUS_OK } else { STATUS_DMA_FAILED }, rec.iova)
                    }
                    Err(error) => {
                        print_fmt(format_args!(
                            "[edu-holder] L1 FAIL map refused: {error:?}\n"
                        ));
                        status(STATUS_MAP_REFUSED, 0)
                    }
                };
                answer(op, sender, &reply);
            }
            OP_MAP => {
                let reply = match map_export(msg.a, edu.bdf) {
                    Ok(rec) => {
                        print_fmt(format_args!(
                            "[edu-holder] mapped id={} iova={:#x}\n",
                            rec.export_id, rec.iova
                        ));
                        status(STATUS_OK, rec.iova)
                    }
                    Err(error) => {
                        print_fmt(format_args!(
                            "[edu-holder] FAIL map id={} refused: {error:?}\n",
                            msg.a
                        ));
                        status(STATUS_MAP_REFUSED, 0)
                    }
                };
                answer(op, sender, &reply);
            }
            OP_DMA_TO => {
                let done = edu.write_garbage(msg.a, msg.b);
                print_fmt(format_args!(
                    "[edu-holder] L2 device write to revoked iova={:#x} issued done={done}\n",
                    msg.a
                ));
                answer(
                    op,
                    sender,
                    &status(if done { STATUS_OK } else { STATUS_DMA_FAILED }, msg.a),
                );
            }
            OP_STALE_AFTER_DEATH => {
                answer(op, sender, &status(STATUS_OK, msg.b));
                run_l3(&edu, msg.a, msg.b, msg.c);
                l3_done = true;
                if let Some((op, sender)) = deferred_hello.take() {
                    roles = 2;
                    answer(op, sender, &Msg::new(OP_STATUS, 2, 0, 0).encode());
                }
            }
            OP_DIE => {
                answer(op, sender, &status(STATUS_OK, 0));
                println("[edu-holder] L4 exiting with a mapped lease");
                sys_exit(0);
            }
            _ => answer(op, sender, &status(STATUS_BAD_REQUEST, 0)),
        }
    }
}

/// L3: the owner of export `id` (window `iova`, frames at `owner_va`) is dying.
/// Wait until the kernel has revoked the lease, take a fresh page (often the
/// owner's freed frame), and make the device write to the stale window.
fn run_l3(edu: &Edu, id: u64, iova: u64, owner_va: u64) {
    let mut gone = false;
    for _ in 0..2000 {
        match map_export(id, edu.bdf) {
            Err(DmaExportError::NotFound) => {
                gone = true;
                break;
            }
            _ => {
                let _ = ipc::wait_any(source::TIMER, 1);
            }
        }
    }
    if !gone {
        println("[edu-holder] L3 FAIL lease still mapped after owner death");
        return;
    }
    println("[edu-holder] L3 lease revoked after owner death");
    let Some(fresh) = DmaBuf::alloc(1) else {
        println("[edu-holder] L3 FAIL no page");
        return;
    };
    let reused = fresh.phys() as u64 == owner_va;
    let _ = fresh.write_bytes(0, &[0x77; PAGE]);
    let done = edu.write_garbage(iova, PAGE as u64);
    let mut page = [0u8; PAGE];
    let intact = fresh.read_bytes(0, &mut page) && page.iter().all(|&b| b == 0x77);
    print_fmt(format_args!(
        "[edu-holder] L3 {} stale DMA after owner death: reused_frame={reused} dma_done={done} pattern_intact={intact}\n",
        if intact { "PASS" } else { "FAIL" }
    ));
    fresh.free();
}
