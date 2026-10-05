//! x86 16550 serial Driver Cell — Tier-1 Privileged Driver Cell.
//!
//! The kernel owns the 16550 mechanism: at boot it registers every port the
//! board profile declares and offers only the ones that answered a register-set
//! probe. This cell holds the `serial_port` capability and drives that surface,
//! so it can reach exactly the probed ports and nothing else — no PCIe, no MMIO,
//! no DMA.
//!
//! It:
//!   1. Enumerates the declared ports through `sys_serial_port_info`, naming each
//!      one that is declared but absent (the probe refused it) instead of
//!      assuming it exists.
//!   2. Writes a per-port marker to every usable port, proving the transmit path
//!      end to end from a cell.
//!   3. Polls the usable ports for a bounded window, echoing each received byte
//!      back, proving the receive path.
//!   4. Logs a summary naming the usable count, then idles.
//!
//! The console (index 0) is the kernel's; this cell may write to it, but the
//! console path itself is untouched — a serial cell that could break the log
//! would take the only x86 debug channel with it.
//!
//! RS485 direction control is **not** implemented or claimed here: the machine
//! records declare a DE/RE mechanism, and QEMU models no transceiver, so the
//! timing can only be measured on hardware (phase 07).
//!
//! `serial_port` is granted by init via the reviewed `/bin/serial` launch edge —
//! NOT via a manifest flag (all 8 flag bits in v1 are occupied).

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use ostd::app::{AppContext, AppEvent};
use ostd::io::print_fmt;
use ostd::syscall::{
    sys_get_time_ms, sys_serial_port_info, sys_serial_read, sys_serial_write, sys_yield,
    SerialPortInfo,
};

/// Ports the kernel mechanism can carry; the enumeration stops at the first
/// index the profile does not declare.
const MAX_PORTS: u32 = 8;

/// How long the cell keeps polling for received bytes. Long enough for a test
/// harness to attach and inject, short enough that the cell still reports.
const POLL_WINDOW_MS: u64 = 10_000;

api::declare_syscalls![
    // AppContext event-loop base set.
    Send,
    TrySend,
    Recv,
    TryRecv,
    Reply,
    Log,
    Heartbeat,
    LookupService,
    GetTime,
    RecvTimeout,
    // Serial-port surface (shares allowlist bit 50 with the driver-registration
    // calls, exactly as `RegisterUsbHidProducer` does; the `serial_port`
    // capability is the authority gate).
    SerialPortInfo,
    SerialWrite,
    SerialRead,
    SerialConfigure,
    // Clean shutdown returns through the exit path. `Exit`/`Yield` carry no
    // allowlist bit and are always permitted; naming them documents the intent.
    Exit,
];

/// Write a byte string to a port, returning how many bytes the kernel accepted.
fn write_line(index: u32, text: &str) -> usize {
    // The extra ports are raw byte pipes, so a line terminator is CRLF: a
    // terminal reading the far end of the chardev then shows it correctly.
    let mut buf = alloc::vec::Vec::with_capacity(text.len() + 2);
    for byte in text.bytes() {
        if byte == b'\n' {
            buf.push(b'\r');
        }
        buf.push(byte);
    }
    match sys_serial_write(index, &buf) {
        Ok(written) => written,
        Err(_) => 0,
    }
}

/// Enumerate, transmit a marker, poll for input, then report.
fn run_serial_probe() {
    let mut usable = alloc::vec::Vec::new();
    let mut declared = 0u32;

    for index in 0..MAX_PORTS {
        let mut info = SerialPortInfo::zeroed();
        match sys_serial_port_info(index, &mut info) {
            Ok(true) => {
                declared += 1;
                usable.push((index, info));
                print_fmt(format_args!(
                    "[serial] port {} base={:#06x} irq={} usable\n",
                    index, info.base, info.irq
                ));
            }
            Ok(false) => {
                if info.base == 0 {
                    // Not declared by this machine's profile: end of the list.
                    break;
                }
                declared += 1;
                print_fmt(format_args!(
                    "[serial] port {} base={:#06x} irq={} absent (kernel probe refused)\n",
                    index, info.base, info.irq
                ));
            }
            Err(_) => {
                print_fmt(format_args!(
                    "[serial] serial_port capability missing — no port is reachable\n"
                ));
                return;
            }
        }
    }

    if usable.is_empty() {
        print_fmt(format_args!(
            "[serial] no usable serial port (declared={})\n",
            declared
        ));
        return;
    }

    // Transmit proof: a marker per usable port, including the console index.
    for (index, info) in usable.iter() {
        let marker = alloc::format!("[serial] port {} marker base={:#06x}\n", index, info.base);
        let written = write_line(*index, &marker);
        print_fmt(format_args!(
            "[serial] port {} tx marker bytes={}\n",
            index, written
        ));
    }

    // Receive proof: poll the non-console ports and echo what arrives. The
    // console is excluded from echo — echoing the kernel's own log back into it
    // would corrupt the transcript this cell is reporting through.
    let polled: alloc::vec::Vec<u32> = usable
        .iter()
        .filter(|(index, _)| *index != 0)
        .map(|(index, _)| *index)
        .collect();
    let deadline = sys_get_time_ms().map(|now| now + POLL_WINDOW_MS);
    let mut echoed = 0usize;
    let mut buf = [0u8; 64];

    loop {
        let mut idle = true;
        for index in polled.iter() {
            match sys_serial_read(*index, &mut buf) {
                Ok(0) => {}
                Ok(read) => {
                    idle = false;
                    for byte in buf[..read].iter() {
                        // Echo the byte back, then report it in the log so the
                        // evidence shows the round trip without needing the far
                        // end of the chardev to be read.
                        let _ = sys_serial_write(*index, &[*byte]);
                        echoed += 1;
                        print_fmt(format_args!(
                            "[serial] port {} rx byte={:#04x} echoed\n",
                            index, byte
                        ));
                    }
                }
                Err(_) => {}
            }
        }
        if let Some(deadline) = deadline {
            if sys_get_time_ms().is_some_and(|now| now >= deadline) {
                break;
            }
        }
        if idle {
            sys_yield();
        }
    }

    print_fmt(format_args!(
        "[serial] serial cell ready: usable={} declared={} echoed={}\n",
        usable.len(),
        declared,
        echoed
    ));
}

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        // Bring-up runs in Init: enumerate, prove TX, then poll for a bounded
        // window to prove RX. A serial cell that reported before its ports were
        // driven would be claiming a data path it had not exercised.
        AppEvent::Init => run_serial_probe(),
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => ostd::syscall::sys_exit(0),
        _ => {}
    }
}

ostd::run_app!(handler);
