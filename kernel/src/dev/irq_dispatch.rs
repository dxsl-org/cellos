use core::sync::atomic::{AtomicU8, Ordering};

const UNKNOWN_IRQ_WARN_LIMIT: u8 = 4;
static UNKNOWN_RV64_IRQ_WARNINGS: AtomicU8 = AtomicU8::new(0);

#[cfg(target_arch = "riscv64")]
const _: crate::hal::RiscvPlicContext = vi_riscv_plic_context;
#[cfg(target_arch = "riscv64")]
const _: crate::hal::HandleRiscvExternalIrq = vi_handle_riscv_external_irq;

enum ExternalIrqRoute {
    Uart,
    Virtio,
    Unknown,
}

#[no_mangle]
pub extern "Rust" fn vi_riscv_plic_context() -> usize {
    crate::platform::riscv_plic_context_for_current_hart().unwrap_or(usize::MAX)
}

fn warn_unknown_irq(irq: u32) {
    let seen = UNKNOWN_RV64_IRQ_WARNINGS.fetch_add(1, Ordering::Relaxed);
    if seen < UNKNOWN_IRQ_WARN_LIMIT {
        if seen + 1 == UNKNOWN_IRQ_WARN_LIMIT {
            log::warn!(
                "[plic] unhandled RV64 external IRQ {} (further warnings suppressed)",
                irq
            );
        } else {
            log::warn!("[plic] unhandled RV64 external IRQ {}", irq);
        }
    }
}

#[no_mangle]
pub extern "Rust" fn vi_handle_riscv_external_irq(irq: u32) {
    let route = crate::platform::with(|platform| {
        if platform.riscv_irq_owner_count(irq) != 1 {
            return ExternalIrqRoute::Unknown;
        }

        if irq == platform.uart_irq {
            return ExternalIrqRoute::Uart;
        }

        let mut index = 0;
        while index < platform.virtio_mmio.len() {
            if platform.virtio_mmio[index].is_some_and(|entry| entry.irq == irq) {
                return ExternalIrqRoute::Virtio;
            }
            index += 1;
        }

        ExternalIrqRoute::Unknown
    });

    match route {
        ExternalIrqRoute::Uart => {
            super::uart::vi_handle_uart_irq();
        }
        ExternalIrqRoute::Virtio => {
            super::virtio_common::vi_handle_virtio_irq(irq);
        }
        ExternalIrqRoute::Unknown => {
            warn_unknown_irq(irq);
        }
    }
}
