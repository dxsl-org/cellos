//! Console selection policy (Phase 10: beyond COM1).
//!
//! Selection order (first match wins):
//! 1. `cellos.console=` command-line option (`ttyS0..3`, `mmio@0x...`, `fb`);
//! 2. ACPI SPCR table (16550 compatible, I/O port or MMIO);
//! 3. Probed COM1 (if probed present);
//! 4. Linear Framebuffer console fallback.

use crate::acpi::AcpiInfo;
use hal::x86_64::uart_16550::{
    probe_uart, set_console_config, ConsoleConfig, UartAddress, DEFAULT_CONSOLE_CONFIG,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleKind {
    Uart(ConsoleConfig),
    Framebuffer,
}

pub fn select_and_apply_console(cmdline: Option<&str>, acpi: &AcpiInfo) -> ConsoleKind {
    // 1. Command-line choice
    if let Some(cmd) = cmdline {
        if let Some(choice) = parse_console_cmdline(cmd) {
            apply_choice(choice, "command-line");
            return choice;
        }
    }

    // 2. SPCR from ACPI
    if let Some(spcr) = &acpi.spcr {
        if spcr.is_16550_family() && spcr.base.address != 0 {
            let baud = match spcr.baud_code {
                3 => 9600,
                4 => 19200,
                6 => 57600,
                7 => 115200,
                _ => 115200,
            };
            let (addr, is_gsi) = if spcr.base.space_id == 1 {
                // System I/O port
                (UartAddress::IoPort(spcr.base.address as u16), false)
            } else {
                // System memory (MMIO)
                let phys = spcr.base.address;
                let virt = crate::memory::frame::phys_to_virt(phys as usize);
                let stride = if spcr.base.access_size == 3 || spcr.base.bit_width == 32 {
                    4
                } else {
                    1
                };
                (
                    UartAddress::Mmio {
                        base_phys: phys,
                        base_virt: virt,
                        stride,
                    },
                    true,
                )
            };
            let cfg = ConsoleConfig {
                addr,
                irq: spcr.irq,
                gsi: spcr.gsi,
                is_gsi,
                baud,
            };
            let choice = ConsoleKind::Uart(cfg);
            apply_choice(choice, "SPCR");
            return choice;
        }
    }

    // 3. COM1 if probed present
    if probe_uart(UartAddress::IoPort(0x03F8)) {
        let choice = ConsoleKind::Uart(DEFAULT_CONSOLE_CONFIG);
        apply_choice(choice, "COM1 probed");
        return choice;
    }

    // 4. Framebuffer fallback
    if crate::resource_registry::display_framebuffer_info().is_some() {
        let choice = ConsoleKind::Framebuffer;
        apply_choice(choice, "framebuffer fallback");
        return choice;
    }

    // Final fallback: default COM1 anyway
    let choice = ConsoleKind::Uart(DEFAULT_CONSOLE_CONFIG);
    apply_choice(choice, "default fallback");
    choice
}

pub fn parse_console_cmdline(s: &str) -> Option<ConsoleKind> {
    match s {
        "ttyS0" => Some(ConsoleKind::Uart(ConsoleConfig {
            addr: UartAddress::IoPort(0x03F8),
            irq: 4,
            gsi: 4,
            is_gsi: false,
            baud: 115200,
        })),
        "ttyS1" => Some(ConsoleKind::Uart(ConsoleConfig {
            addr: UartAddress::IoPort(0x02F8),
            irq: 3,
            gsi: 3,
            is_gsi: false,
            baud: 115200,
        })),
        "ttyS2" => Some(ConsoleKind::Uart(ConsoleConfig {
            addr: UartAddress::IoPort(0x03E8),
            irq: 4,
            gsi: 4,
            is_gsi: false,
            baud: 115200,
        })),
        "ttyS3" => Some(ConsoleKind::Uart(ConsoleConfig {
            addr: UartAddress::IoPort(0x02E8),
            irq: 3,
            gsi: 3,
            is_gsi: false,
            baud: 115200,
        })),
        "fb" => Some(ConsoleKind::Framebuffer),
        other if other.starts_with("mmio@0x") || other.starts_with("mmio@0X") => {
            let hex_str = &other[7..];
            let phys = u64::from_str_radix(hex_str, 16).ok()?;
            let virt = crate::memory::frame::phys_to_virt(phys as usize);
            Some(ConsoleKind::Uart(ConsoleConfig {
                addr: UartAddress::Mmio {
                    base_phys: phys,
                    base_virt: virt,
                    stride: 4, // default 32-bit MMIO
                },
                irq: 0,
                gsi: 0,
                is_gsi: false,
                baud: 115200,
            }))
        }
        _ => None,
    }
}

fn apply_choice(choice: ConsoleKind, source: &str) {
    match choice {
        ConsoleKind::Uart(cfg) => {
            set_console_config(cfg);
            log::info!(
                "[console] active console: {:?} (source: {})",
                cfg.addr,
                source
            );
        }
        ConsoleKind::Framebuffer => {
            log::info!("[console] active console: Framebuffer (source: {})", source);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cmdline_options() {
        assert!(matches!(parse_console_cmdline("ttyS1"), Some(ConsoleKind::Uart(c)) if c.irq == 3));
        assert!(matches!(
            parse_console_cmdline("fb"),
            Some(ConsoleKind::Framebuffer)
        ));
        assert!(matches!(
            parse_console_cmdline("mmio@0xfe000000"),
            Some(ConsoleKind::Uart(c)) if matches!(c.addr, UartAddress::Mmio { base_phys: 0xfe000000, stride: 4, .. })
        ));
        assert_eq!(parse_console_cmdline("unknown"), None);
    }
}
