//! Export owner of the ADR-0033 CP1 §7 QEMU lanes (L1-L5): a SAS Cell that
//! exports its own Grant memory (`ExportDmaRange = 437`) to the `edu` holder
//! and checks what the device did to it.
//!
//! Init launches two instances; the holder hands out roles. Role 1 runs L5
//! (client-side refusals), L1 (device copy through the window), L2 (unexport,
//! then a device write to the stale window leaves memory unchanged) and L3
//! (dies holding a mapped lease). Role 2 runs L4 (the holder dies holding its
//! mapped lease; the range must come back). Every outcome is one
//! `[export-client]` serial line the lane script asserts.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

use api::completion::source;
use edu_export_proto::{
    Msg, MSG_LEN, OP_COPY, OP_DIE, OP_DMA_TO, OP_HELLO, OP_MAP, OP_STALE_AFTER_DEATH, SERVICE_ID,
    STATUS_OK,
};
use ostd::dma::{revoke_export, DmaBuf};
use ostd::io::{print_fmt, println};
use ostd::ipc::{self, PendingCall};
use ostd::syscall::{
    sys_exit, sys_export_dma_range, sys_grant_free, sys_lookup_service, sys_map_dma_export,
    DmaExportError,
};

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![
    Log,
    Exit,
    Send,
    Recv,
    GetTime,
    LookupService,
    GrantAlloc,
    GrantFree,
    ExportDmaRange,
    RevokeDmaExport,
    IpcSubmit,
    IpcTake,
    IpcWait,
    IpcCancel,
    WaitCompletion,
];

ostd::cell_main!(cell_main);

const PAGE: usize = 4096;

fn sleep(ticks: u64) {
    let _ = ipc::wait_any(source::TIMER, ticks);
}

/// One request to the holder; the reply's `(a, b)`, or `None` on any failure.
fn call(holder: usize, msg: Msg) -> Option<(u64, u64)> {
    let pending = PendingCall::submit(holder, &msg.encode()).ok()?;
    let mut reply = [0u8; MSG_LEN];
    let done = pending.wait_and_take(&mut reply, 100, 30).ok()??;
    if done.len != MSG_LEN {
        return None;
    }
    Msg::decode(&reply).map(|r| (r.a, r.b))
}

fn fill(buf: &DmaBuf, offset: usize, byte: impl Fn(usize) -> u8) -> bool {
    let mut page = [0u8; PAGE];
    for (i, b) in page.iter_mut().enumerate() {
        *b = byte(i);
    }
    buf.write_bytes(offset, &page)
}

fn page_matches(buf: &DmaBuf, offset: usize, byte: impl Fn(usize) -> u8) -> bool {
    let mut page = [0u8; PAGE];
    buf.read_bytes(offset, &mut page) && page.iter().enumerate().all(|(i, &b)| b == byte(i))
}

fn cell_main() {
    let mut holder = None;
    for _ in 0..500 {
        holder = sys_lookup_service(SERVICE_ID);
        if holder.is_some() {
            break;
        }
        sleep(2);
    }
    let Some(holder) = holder else {
        println("[export-client] FAIL holder service not registered");
        sys_exit(1);
    };
    let Some((role, _)) = call(holder, Msg::new(OP_HELLO, 0, 0, 0)) else {
        println("[export-client] FAIL hello");
        sys_exit(1);
    };
    print_fmt(format_args!("[export-client] role={role}\n"));
    if role == 2 {
        run_l4(holder);
    } else {
        run_role_one(holder);
    }
    sys_exit(0);
}

fn run_role_one(holder: usize) {
    let Some(probe) = DmaBuf::alloc(2) else {
        println("[export-client] FAIL no memory");
        return;
    };
    // L5: refusals the owner can provoke, each before any lease exists.
    let flags = sys_export_dma_range(probe.phys(), PAGE, SERVICE_ID, 2);
    print_fmt(format_args!(
        "[export-client] L5 unknown flags refused: {flags:?}\n"
    ));
    let service = sys_export_dma_range(probe.phys(), PAGE, 0x7A01, 0);
    print_fmt(format_args!(
        "[export-client] L5 service outside DMA_EXPORT_SERVICES refused: {service:?}\n"
    ));
    let stack_page = (&flags as *const _ as usize) & !(PAGE - 1);
    let not_owned = sys_export_dma_range(stack_page, PAGE, SERVICE_ID, 0);
    print_fmt(format_args!(
        "[export-client] L5 non-Grant range refused: {not_owned:?}\n"
    ));
    let unknown = revoke_export(u64::MAX >> 2);
    print_fmt(format_args!(
        "[export-client] L5 unknown export revoke refused: {unknown:?}\n"
    ));

    // L1: the device copies page 0 to page 1 through the export window.
    let pattern = |i: usize| (i as u8).wrapping_mul(7).wrapping_add(3);
    if !(fill(&probe, 0, pattern) && fill(&probe, PAGE, |_| 0)) {
        return;
    }
    let id = match probe.export(0, 2 * PAGE, SERVICE_ID, true) {
        Ok(id) => id,
        Err(DmaExportError::NotSupported) => {
            println("[export-client] L5 no translating remapper: export refused NotSupported");
            return;
        }
        Err(error) => {
            print_fmt(format_args!(
                "[export-client] L1 FAIL export refused: {error:?}\n"
            ));
            return;
        }
    };
    let non_holder = sys_map_dma_export(id, 0);
    print_fmt(format_args!(
        "[export-client] L5 non-holder map refused: {non_holder:?}\n"
    ));
    match call(holder, Msg::new(OP_COPY, id, 0, 0)) {
        Some((STATUS_OK, iova)) if page_matches(&probe, PAGE, pattern) => print_fmt(format_args!(
            "[export-client] L1 PASS device copied 4096 bytes through window iova={iova:#x}\n"
        )),
        other => print_fmt(format_args!(
            "[export-client] L1 FAIL copy reply={other:?}\n"
        )),
    };
    let revoked = revoke_export(id);
    let freed = revoked.is_ok() && sys_grant_free(probe.phys());
    print_fmt(format_args!(
        "[export-client] L1 unexport={revoked:?} grant-free-after={freed}\n"
    ));

    // L2: unexport, then the device writes to the old window.
    let Some(l2) = DmaBuf::alloc(1) else { return };
    let _ = fill(&l2, 0, |_| 0xA5);
    let Ok(id2) = l2.export(0, PAGE, SERVICE_ID, true) else {
        println("[export-client] L2 FAIL export");
        return;
    };
    let Some((STATUS_OK, iova2)) = call(holder, Msg::new(OP_MAP, id2, 0, 0)) else {
        println("[export-client] L2 FAIL map");
        return;
    };
    let unexport = revoke_export(id2);
    let _ = call(holder, Msg::new(OP_DMA_TO, iova2, PAGE as u64, 0));
    let unchanged = page_matches(&l2, 0, |_| 0xA5);
    print_fmt(format_args!(
        "[export-client] L2 {} unexport={unexport:?} then device write to iova={iova2:#x}: memory unchanged={unchanged}\n",
        if unexport.is_ok() && unchanged { "PASS" } else { "FAIL" }
    ));

    // L3: die holding a mapped lease.
    let Some(l3) = DmaBuf::alloc(1) else { return };
    let _ = fill(&l3, 0, |_| 0x3C);
    let Ok(id3) = l3.export(0, PAGE, SERVICE_ID, true) else {
        println("[export-client] L3 FAIL export");
        return;
    };
    let Some((STATUS_OK, iova3)) = call(holder, Msg::new(OP_MAP, id3, 0, 0)) else {
        println("[export-client] L3 FAIL map");
        return;
    };
    let _ = call(
        holder,
        Msg::new(OP_STALE_AFTER_DEATH, id3, iova3, l3.phys() as u64),
    );
    print_fmt(format_args!(
        "[export-client] L3 exiting with live lease id={id3} iova={iova3:#x}\n"
    ));
}

/// L4: the holder dies with this client's lease mapped; the owner range must be
/// unlocked (the Grant frees) once the kernel tore the holder's domain down.
fn run_l4(holder: usize) {
    let Some(buf) = DmaBuf::alloc(1) else { return };
    let Ok(id) = buf.export(0, PAGE, SERVICE_ID, false) else {
        println("[export-client] L4 FAIL export");
        return;
    };
    let Some((STATUS_OK, iova)) = call(holder, Msg::new(OP_MAP, id, 0, 0)) else {
        println("[export-client] L4 FAIL map");
        return;
    };
    let locked = !sys_grant_free(buf.phys());
    print_fmt(format_args!(
        "[export-client] L4 lease id={id} iova={iova:#x} range locked={locked}\n"
    ));
    let _ = call(holder, Msg::new(OP_DIE, 0, 0, 0));
    let mut freed = false;
    for _ in 0..300 {
        sleep(10);
        if sys_grant_free(buf.phys()) {
            freed = true;
            break;
        }
    }
    let after = revoke_export(id);
    print_fmt(format_args!(
        "[export-client] L4 {} holder death: range freed={freed} lease={after:?}\n",
        if locked && freed && after == Err(DmaExportError::NotFound) {
            "PASS"
        } else {
            "FAIL"
        }
    ));
}
