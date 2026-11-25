//! How often the guest came back to this Cell, by the device it wanted, and
//! how long the Cell took before it ran the guest again.
//!
//! The kernel counts exits by cause (`[hv-x86] vm … stats`); only the Cell
//! knows whose port or register an exit was for. Printed once when the run
//! loop ends and read by the bench lane (`scripts/tier3_chrome_bench.py`).

extern crate alloc;

use crate::{
    acpi_pm::AcpiPm, cmos_rtc::CmosRtc, pic_8259::Pic8259, pit_8253::Pit8253,
    uart_16550::Uart16550, virtio_mmio,
};
use abi::hypervisor::ViVmExit;
use ostd::io::println;

const NAMES: [&str; 13] = [
    "uart",
    "pic",
    "pit",
    "rtc",
    "acpi-pm",
    "other-port",
    "blk",
    "net",
    "gpu",
    "input",
    "other-mmio",
    "hlt",
    "preempted",
];
const UART: usize = 0;
const PIC: usize = 1;
const PIT: usize = 2;
const RTC: usize = 3;
const ACPI_PM: usize = 4;
const OTHER_PORT: usize = 5;
const BLK: usize = 6;
const NET: usize = 7;
const GPU: usize = 8;
const INPUT: usize = 9;
const OTHER_MMIO: usize = 10;
const HLT: usize = 11;
const PREEMPTED: usize = 12;

pub struct ExitCounts {
    count: [u64; NAMES.len()],
    /// Time-stamp-counter ticks from an exit of the class to the next entry.
    ticks: [u64; NAMES.len()],
    /// The exit being served: its class and the counter when it arrived.
    serving: Option<(usize, u64)>,
    /// Counter and clock at creation, to turn ticks into milliseconds.
    started: (u64, u64),
}

impl ExitCounts {
    pub fn new() -> Self {
        Self {
            count: [0; NAMES.len()],
            ticks: [0; NAMES.len()],
            serving: None,
            started: (ostd::vmm::read_tsc(), ostd::syscall::sys_get_time()),
        }
    }

    /// The guest is about to run again: the exit being served is done.
    pub fn entering(&mut self) {
        if let Some((class, arrived)) = self.serving.take() {
            self.ticks[class] += ostd::vmm::read_tsc().wrapping_sub(arrived);
        }
    }

    pub fn record(&mut self, exit: &ViVmExit) {
        let class = match exit {
            ViVmExit::PortOut { port, .. } | ViVmExit::PortIn { port, .. } => port_class(*port),
            ViVmExit::MmioRead { ipa, .. } | ViVmExit::MmioWrite { ipa, .. } => mmio_class(*ipa),
            ViVmExit::Hlt => HLT,
            ViVmExit::Preempted => PREEMPTED,
            _ => return,
        };
        self.count[class] += 1;
        self.serving = Some((class, ostd::vmm::read_tsc()));
    }

    /// One line: per class, the exits and the milliseconds spent serving them.
    pub fn report(&self) {
        let ticks = ostd::vmm::read_tsc().wrapping_sub(self.started.0).max(1);
        let ns = ostd::syscall::sys_get_time().saturating_sub(self.started.1);
        let mut line = alloc::string::String::from("[hv-x86] exits to the Cell:");
        for (class, name) in NAMES.iter().enumerate() {
            let ms = u128::from(self.ticks[class]) * u128::from(ns) / u128::from(ticks) / 1_000_000;
            line.push_str(&alloc::format!(
                " {name}={} {name}_ms={ms}",
                self.count[class]
            ));
        }
        println(&line);
    }
}

fn port_class(port: u16) -> usize {
    if Uart16550::owns(port) {
        UART
    } else if Pic8259::owns(port) {
        PIC
    } else if Pit8253::owns(port) {
        PIT
    } else if CmosRtc::owns(port) {
        RTC
    } else if AcpiPm::owns(port) {
        ACPI_PM
    } else {
        OTHER_PORT
    }
}

/// The virtio-mmio slot table of `x86-mmio-dispatch.rs`.
fn mmio_class(ipa: u64) -> usize {
    if !virtio_mmio::owns(ipa) {
        return OTHER_MMIO;
    }
    match virtio_mmio::slot_and_offset(ipa).0 {
        0 => BLK,
        1 => NET,
        3 => GPU,
        4 => INPUT,
        _ => OTHER_MMIO,
    }
}
