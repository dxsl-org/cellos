//! VirtIO GPU Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! Probes VirtIO MMIO slots for a GPU device (device type 16), claims exclusive
//! MMIO via `ostd::mmio::request_region`, initialises the virtqueues and
//! framebuffer via `virtio-drivers`, registers as the system GPU driver via
//! `sys_register_gpu_driver`, then serves flush + cursor IPC from the kernel.
//!
//! This cell replaces the kernel's `virtio_gpu.rs` driver per the Kernel
//! Boundary Law (2026-06-23).  The kernel's `GpuFlush`/`GpuCursor` syscall
//! handlers forward to this cell via fire-and-forget IPC when it is registered.
//!
//! # Fallback
//! If no VirtIO MMIO GPU is found the cell exits cleanly (code 0).  The kernel
//! GPU_CONTEXT path remains as a fallback until Phase 08 removes it.
//!
//! # Law 4 exception
//! `src/display.rs` uses `unsafe` for the `virtio_drivers::Hal` impl and MMIO
//! probe reads.  All other modules forbid unsafe code.
//!
//! # IPC message format (from kernel GpuFlush forward)
//! ```text
//! [0]      = 0x10  (OP_FLUSH)
//! [1..5]   = xy    (u32 LE: x<<16 | y)
//! [5..9]   = wh    (u32 LE: w<<16 | h)
//! [9..17]  = data_ptr (u64 LE: SAS pointer to compositor pixel buffer)
//! [17..21] = data_len (u32 LE: byte count, must be >= w*h*4)
//! ```
//!
//! # IPC message format (from kernel GpuCursor forward)
//! ```text
//! op=0 set sprite  [0]=0x11, [1..9]=data_ptr u64 LE, [9..13]=xy u32 LE, [13..17]=hot u32 LE
//! op=1 move cursor [0]=0x12, [1..5]=xy u32 LE
//! ```

#![no_std]
#![no_main]

extern crate alloc;

mod cursor;
mod display;

use ostd::app::{AppContext, AppEvent};
use ostd::sync::Mutex;
use ostd::syscall::sys_register_gpu_driver;
use display::GpuDevice;

// ─── Kernel→Cell IPC opcodes ──────────────────────────────────────────────────

const OP_FLUSH:    u8 = 0x10; // GpuFlush forward: flush rect
const OP_CUR_SET:  u8 = 0x11; // GpuCursor op=0: set sprite + initial position
const OP_CUR_MOVE: u8 = 0x12; // GpuCursor op=1: move cursor (no sprite re-upload)

// ─── Cell state ───────────────────────────────────────────────────────────────

static STATE: Mutex<Option<GpuDevice>> = Mutex::new(None);

// ─── Event handler ────────────────────────────────────────────────────────────

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            let Some(dev) = display::find_and_init_gpu() else {
                // No VirtIO MMIO GPU on this platform — exit cleanly.
                // The kernel virtio_gpu fallback remains until Phase 08.
                ostd::syscall::sys_exit(0);
            };

            if sys_register_gpu_driver().is_err() {
                ostd::io::println("[virtio-gpu] failed to register GPU driver");
                ostd::syscall::sys_exit(1);
            }

            ostd::io::println("[virtio-gpu] VirtIO GPU Driver Cell registered");
            *STATE.lock() = Some(dev);
        }

        AppEvent::Message { sender_tid: _, data } => {
            dispatch(data.as_ref());
        }

        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => {
            ostd::syscall::sys_exit(0);
        }

        _ => {}
    }
}

// ─── IPC dispatch ─────────────────────────────────────────────────────────────

fn dispatch(buf: &[u8]) {
    if buf.is_empty() { return; }
    match buf[0] {
        OP_FLUSH if buf.len() >= 21 => {
            let xy       = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
            let wh       = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
            let data_ptr = u64::from_le_bytes([
                buf[9],  buf[10], buf[11], buf[12],
                buf[13], buf[14], buf[15], buf[16],
            ]) as usize;
            let data_len = u32::from_le_bytes([buf[17], buf[18], buf[19], buf[20]]) as usize;
            if let Some(dev) = STATE.lock().as_mut() {
                dev.flush_rect(data_ptr, data_len, xy, wh);
            }
        }

        OP_CUR_SET if buf.len() >= 17 => {
            let data_ptr = u64::from_le_bytes([
                buf[1], buf[2], buf[3], buf[4],
                buf[5], buf[6], buf[7], buf[8],
            ]) as usize;
            let xy  = u32::from_le_bytes([buf[9],  buf[10], buf[11], buf[12]]);
            let hot = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
            if let Some(dev) = STATE.lock().as_mut() {
                cursor::set_sprite(dev, data_ptr, xy, hot);
            }
        }

        OP_CUR_MOVE if buf.len() >= 5 => {
            let xy = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
            if let Some(dev) = STATE.lock().as_mut() {
                cursor::move_to(dev, xy);
            }
        }

        _ => {} // unknown opcode — drop silently
    }
}

// ─── Entry point + manifest ───────────────────────────────────────────────────

ostd::run_app!(handler);

// PcieDriverCap is granted by loader.rs via path match (/bin/virtio-gpu).
// No manifest flags are needed; the cap is granted via direct TCB write at spawn.
api::declare_manifest!(
    block_io = false, network = false, spawn = false,
    gpio = false, uart = false, hypervisor = false
);
