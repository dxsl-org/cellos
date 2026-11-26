//! virtio-sound Driver Cell — Tier-1 Privileged Driver Cell (x86_64).
//!
//! Claims the first virtio-sound function (modern `1af4:1059`, QEMU's
//! `virtio-sound-pci`), brings it up over the cell-side virtio PCI transport
//! with polled split virtqueues in authorized DMA memory, picks the first
//! output stream that plays 48 kHz S16 stereo, and serves `AUDIO`
//! (`wire::audio`), which init registers for this cell from its
//! `config/services.toml` row.
//!
//! Interrupts are not routed: one loop receives requests with a one-tick
//! (10 ms) timeout while the stream runs and feeds the device between them,
//! with up to 80 ms of PCM in the device and 200 ms queued here.
//!
//! Without a sound device the cell exits cleanly: `AUDIO` then has no live
//! provider, which clients treat like a refused stream (they play silence).
//!
//! No `unsafe`: DMA goes through `ostd::dma::DmaBuf`'s byte copies and
//! registers through the transport; the device configuration space is never
//! read (stream facts come from `VIRTIO_SND_R_PCM_INFO`).

// Hosted `cargo test` gets std and libtest's entry, so the pure modules run as
// host unit tests; Cell targets stay no_std.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]
extern crate alloc;

mod bars;
mod control;
mod device;
mod dma;
mod error;
mod pace;
mod pcm;
mod proto;
mod ring;
mod serve;
mod service;
mod tx;
mod virtq;

#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod virtq_tests;

use core::fmt;
use device::InitError;
use ostd::io::print_fmt;
use ostd::syscall::{sys_exit, sys_query_pcie_function};
use virtio_pci_transport::{InitFailure, TransportError};

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

/// One log record (`[virtio-snd] ...\n`), dropped when over the log line limit.
fn log(args: fmt::Arguments<'_>) {
    let _ = print_fmt(args);
}

/// Discovery through bring-up; exits the cell when there is nothing to drive.
fn cell_main() {
    let Some(info) = device::discover() else {
        log(format_args!("[virtio-snd] no sound device\n"));
        sys_exit(0)
    };
    let bdf = Bdf(info.bdf);
    let record = match sys_query_pcie_function(info.bdf) {
        Ok(record) => record,
        Err(error) => {
            log(format_args!("[virtio-snd] query {bdf} failed: {error:?}\n"));
            sys_exit(1)
        }
    };
    let device = match device::bring_up(&record) {
        Ok(device) => device,
        Err(
            InitError::Transport(TransportError::AccessPlatformMissing)
            | InitError::Negotiation(Some(InitFailure::AccessPlatformMissing)),
        ) => {
            log(format_args!(
                "[virtio-snd] refused {bdf}: VIRTIO_F_ACCESS_PLATFORM required (DMA remapped)\n"
            ));
            sys_exit(0)
        }
        Err(InitError::NoStream) => {
            log(format_args!(
                "[virtio-snd] {bdf}: {}\n",
                InitError::NoStream
            ));
            sys_exit(0)
        }
        Err(error) => {
            log(format_args!("[virtio-snd] init {bdf} failed: {error}\n"));
            sys_exit(1)
        }
    };
    log(format_args!(
        "[virtio-snd] ready: stream {}, 48000 Hz S16 stereo\n",
        device.stream
    ));
    serve::serve(device)
}

ostd::cell_main!(cell_main);

// PcieDriverCap is granted by the loader at spawn; this manifest declares NO
// privileged flags.
abi::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

// Exactly what the cell calls: PCIe discovery (424) and the 434 record, the BAR
// claim, the DMA grant (and its release when authorization fails), the clock
// for control budgets and pacing, the receive loop with its timed form, the
// reply bridge (`IpcCurrent`/`IpcReply`, `SendGather` for posted replies),
// owner-checked Grant reads (`GrantSlice`), and log.
abi::declare_syscalls![
    Recv,
    RecvTimeout,
    IpcCurrent,
    IpcReply,
    SendGather,
    GrantSlice,
    Log,
    GetTime,
    FindPcieDeviceByVendor,
    QueryPcieFunction,
    RequestMmio,
    GrantAlloc,
    GrantDma,
    GrantFree,
    Exit
];
