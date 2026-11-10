//! BCM2837 (Raspberry Pi 3) backend: mini UART (AUX UART1) console with
//! interrupt-driven RX and single-byte boot probes, the BCM2836 local and
//! BCM2835 legacy interrupt controllers, the BCM2835 system-timer tick, and a
//! single hart.

pub mod bcm2835_legacy_irq;
pub mod bcm2835_systimer;
pub mod bcm2836_irq;
pub mod timer;
mod trap;
pub mod uart_bcm_mini;
mod vcpu_ext;

use hal_soc_bcm27xx::BCM2837;

use super::{
    ArmEl2Monitor, ArmIrqController, ArmPlatform, ArmResources, ArmSoc, ArmTimer, DisplayFirmware,
    DtbPolicy, GuestRunProbe, MmioMap, MmioSpan, MmioWindow, NoIommu, PlatformAnnounce,
    PlatformFacts, SdhciFacts, SecondaryStart, VgicWindows,
};
use crate::aarch64::trap::TrapFrame;
use crate::aarch64::vcpu::AArch64Vcpu;

pub use vcpu_ext::PiVcpuExt;

/// Raspberry Pi 3 (BCM2837).
pub struct Bcm2837;

impl ArmPlatform for Bcm2837 {
    type Iommu = NoIommu;

    /// The console is the mini UART, so no RPi3 path routes output through the
    /// PL011 driver; it keeps the QEMU `virt` address it has always had here.
    const PL011_BASE: usize = hal_soc_arm_virt::QEMU_ARM_VIRT.uart.mmio.base;
    /// The BCM2836 local controller has no software-interrupt path in this
    /// kernel and firmware parks the secondaries, so the board stays single-hart.
    const SECONDARY_START: SecondaryStart =
        SecondaryStart::None("BCM2836 has no SGI path in this kernel — keeping Cellos single-hart");
    const CONSOLE_RX_IRQ: bool = true;
    const PI_MONITOR_BOOT: bool = true;
    /// VideoCore firmware owns the GPU memory split, so a valid firmware DTB
    /// memory map is authoritative.
    const DTB_POLICY: DtbPolicy = DtbPolicy::Conservative;
    /// Four idle-loop `L` probes, then silence.
    const IDLE_PROBES: usize = 4;
    type VcpuExt = PiVcpuExt;
    type El2Monitor = PiEl2Monitor;

    #[inline(always)]
    fn console_init() {
        uart_bcm_mini::init();
    }

    #[inline(always)]
    fn early_putc(byte: u8) {
        uart_bcm_mini::putchar(byte);
    }

    #[inline(always)]
    fn early_puts(s: &str) {
        uart_bcm_mini::puts(s);
    }

    fn console_write_byte(byte: u8, service_rx: impl Fn() + Copy) {
        if byte == b'\n' {
            Self::console_write_byte(b'\r', service_rx);
        }
        loop {
            // Synchronous exceptions mask IRQs while `sys_log` prints. Keep draining
            // the eight-symbol RX FIFO into the 4096-byte kernel ring so full-duplex
            // terminal traffic cannot overrun while TX waits for space.
            service_rx();
            if uart_bcm_mini::try_putchar(byte) {
                break;
            }
            core::hint::spin_loop();
        }
    }

    #[inline(always)]
    fn console_poll_rx() -> Option<u8> {
        uart_bcm_mini::poll_rx()
    }

    #[inline(always)]
    fn console_enable_rx_irq() {
        uart_bcm_mini::enable_rx_interrupt();
    }

    #[inline(always)]
    fn diag_probe(byte: u8) {
        uart_bcm_mini::probe_put(byte);
    }

    /// `K<src_bit8><pend_bit1><src_nibble>`: CORE0_IRQ_SOURCE GPU bit, BCM2835
    /// systimer C1 pending (IRQ_PENDING1 bit 1), CORE0_IRQ_SOURCE low nibble.
    /// If G never fires but K shows pend=1 and src=0: QEMU does not route
    /// BCM2835→BCM2836.
    #[inline(always)]
    fn irq_snapshot_probe() {
        let soc = BCM2837;
        // SAFETY: both BCM controller apertures are identity-mapped before IRQs are enabled.
        let src_raw = unsafe {
            core::ptr::read_volatile((soc.mmio.local_controller_base + 0x60) as *const u32)
        };
        let pend =
            unsafe { core::ptr::read_volatile((soc.mmio.legacy_irq_base + 0x04) as *const u32) };
        let hex = |n: u32| -> u8 {
            if n < 10 {
                b'0' + n as u8
            } else {
                b'a' + n as u8 - 10
            }
        };
        uart_bcm_mini::probe_put(b'K');
        uart_bcm_mini::probe_put(if src_raw & soc.irq.local_gpu_mask != 0 {
            b'1'
        } else {
            b'0'
        });
        uart_bcm_mini::probe_put(if pend & (1 << soc.irq.system_timer_c1) != 0 {
            b'1'
        } else {
            b'0'
        });
        uart_bcm_mini::probe_put(hex(src_raw & 0xF));
    }

    #[inline(always)]
    fn probe_uncategorized_trap(frame: &TrapFrame, vector_kind: u8) {
        trap::probe_uncategorized_el2_fault(frame, vector_kind);
    }
}

impl ArmIrqController for Bcm2837 {
    /// No SGI path: the BCM2836 local controller has no software interrupt in
    /// this kernel, and the board starts no secondary.
    const HAS_IPI: bool = false;
    const USB_ONE_SHOT_IRQ: Option<u32> = Some(bcm2835_legacy_irq::USB_IRQ);

    #[inline(always)]
    fn init() {
        bcm2836_irq::init();
        bcm2835_legacy_irq::init();
    }

    #[inline(always)]
    fn init_cpu() {
        // BCM2836 local controller + BCM2835 legacy bank: the RPi3 has no GIC.
        bcm2836_irq::init();
        bcm2835_legacy_irq::init();
    }

    /// Never called: [`HAS_IPI`](Self::HAS_IPI) is `false`.
    #[inline(always)]
    fn send_ipi(_target_cpu: u32, _target_mpidr: u64) {}

    #[inline(always)]
    fn unmask_usb_irq() {
        // One-shot unmask: re-enable USB legacy IRQ 9 after driver has cleared GINTSTS
        bcm2835_legacy_irq::enable_irq(bcm2835_legacy_irq::USB_IRQ);
    }

    #[inline(always)]
    fn handle_irq() {
        trap::handle_irq();
    }
}

impl ArmTimer for Bcm2837 {
    #[inline(always)]
    fn init() {
        timer::init();
    }

    #[inline(always)]
    fn reset() {
        timer::reset();
    }
}

impl ArmResources for Bcm2837 {
    const SOC: ArmSoc = ArmSoc::Bcm2837;

    /// Peripherals at 0x3F000000, no GIC, no VirtIO, no Goldfish/PL031 RTC
    /// (uptime only via the ARM counter, epoch = 0). The console register is
    /// the mini UART IO register at 0x3F215040 (AUX_MU_IO).
    const PLATFORM: PlatformFacts = PlatformFacts {
        uart_base: BCM2837.mmio.mini_uart_io,
        uart_irq: 0,
        rtc_base: None,
        published_virtio_slots: 0,
        announce: Some(PlatformAnnounce {
            label: "RPi 3 BCM2837",
            peripheral_base: BCM2837.mmio.peripheral_base,
        }),
    };

    const KERNEL_MMIO_MAPS: &'static [MmioMap] = &[
        // BCM2837 peripheral space 0x3F000000–0x3FFFFFFF (16 MiB): kernel-only.
        // It holds the DMA engine (no IOMMU: a channel writes any RAM), the
        // legacy IRQ controller and system timer behind the kernel tick, EMMC
        // and DWC2. A Cell reaches only the allowlisted windows it claims:
        // `RequestMmio` sets USER on exactly the granted whole pages and
        // retirement clears it; the sub-page mailbox is kernel-mediated.
        MmioMap {
            base: BCM2837.mmio.peripheral_base,
            end: match BCM2837.mmio.peripheral_end() {
                Some(end) => end,
                None => panic!("BCM2837 peripheral span must not overflow"),
            },
        },
        // BCM2836 local interrupt controller 0x40000000–0x40001000 (kernel-only).
        MmioMap {
            base: BCM2837.mmio.local_controller_base,
            end: match BCM2837.mmio.local_controller_end() {
                Some(end) => end,
                None => panic!("BCM2837 local-controller span must not overflow"),
            },
        },
    ];

    const VIRTIO_MMIO: Option<hal_soc_arm_virt::VirtioMmioLayout> = None;
    const VGIC: Option<VgicWindows> = None;
    const GPIO_MMIO_BASE: usize = BCM2837.mmio.gpio_base;
    const DWC2_WINDOW: Option<MmioWindow> = Some(MmioWindow {
        base: BCM2837.mmio.dwc2_base,
        size: BCM2837.mmio.dwc2_grant_size,
    });
    /// VideoCore mailbox; framebuffers live below the peripheral aperture.
    const DISPLAY: Option<DisplayFirmware> = Some(DisplayFirmware {
        mailbox: MmioWindow {
            base: BCM2837.mmio.mailbox_base,
            size: BCM2837.mmio.mailbox_grant_size,
        },
        framebuffer_limit: BCM2837.mmio.peripheral_base,
    });
    const SDHCI: Option<SdhciFacts> = Some(SdhciFacts {
        base: BCM2837.mmio.sdhci_base,
        word_access_only: BCM2837.sdhci.word_access_only,
        minimum_write_spacing_us: BCM2837.sdhci.minimum_write_spacing_us,
        pinmux_gpio_base: Some(BCM2837.mmio.gpio_base),
    });
    const BOARD_PINMUX_GPIO_BASE: Option<usize> = Some(BCM2837.mmio.gpio_base);
    /// Peripheral aperture and BCM2836 local controller.
    const SUPERVISOR_DEVICE_MMIO: &'static [MmioSpan] = &[
        MmioSpan {
            base: BCM2837.mmio.peripheral_base,
            end: match BCM2837.mmio.peripheral_end() {
                Some(end) => end,
                None => panic!("BCM2837 peripheral span must not overflow"),
            },
        },
        MmioSpan {
            base: BCM2837.mmio.local_controller_base,
            end: match BCM2837.mmio.local_controller_end() {
                Some(end) => end,
                None => panic!("BCM2837 local-controller span must not overflow"),
            },
        },
    ];
}

/// The Pi non-VHE EL2 monitor ([`super::monitor`]): the host and its Cells
/// stay at EL1/EL0, and Stage-2 and guest runs go through its HVC gateway.
pub struct PiEl2Monitor;

impl ArmEl2Monitor for PiEl2Monitor {
    const PRESENT: bool = true;

    #[inline(always)]
    fn init(root_pa: u64) {
        super::monitor::init(root_pa);
    }

    #[inline(always)]
    fn is_ready() -> bool {
        super::monitor::is_ready()
    }

    #[inline(always)]
    fn is_verified() -> bool {
        super::monitor::is_verified()
    }

    #[inline(always)]
    fn mark_verified() {
        super::monitor::mark_verified();
    }

    #[inline(always)]
    fn entry_el() -> u8 {
        super::monitor::entry_el()
    }

    #[inline(always)]
    fn probe_status() -> (bool, u64, bool) {
        super::monitor::probe_status()
    }

    #[inline(always)]
    fn gateway(op: u64, a: u64, b: u64, c: u64, d: u64) -> u64 {
        super::monitor::gateway(op, a, b, c, d)
    }

    #[inline(always)]
    unsafe fn run(vcpu: &mut AArch64Vcpu, vmid: u16, root_pa: u64, vi: bool) {
        // SAFETY: forwarded caller contract.
        unsafe { super::monitor::run(vcpu, vmid, root_pa, vi) }
    }

    #[inline(always)]
    unsafe fn run_probed(
        vcpu: &mut AArch64Vcpu,
        vmid: u16,
        root_pa: u64,
        vi: bool,
        guest_ipa: u64,
        guest_pa: u64,
    ) -> GuestRunProbe {
        // SAFETY: forwarded caller contract.
        unsafe { super::monitor::run_probed(vcpu, vmid, root_pa, vi, guest_ipa, guest_pa) }
    }

    #[inline(always)]
    fn prepare_smoke_window() {
        timer::prepare_monitor_smoke_window();
    }

    #[inline(always)]
    fn arm_smoke_preemption() {
        timer::arm_monitor_smoke_preemption();
    }

    #[inline(always)]
    fn restore_host_tick() {
        bcm2835_systimer::init();
        crate::aarch64::timer::reset();
    }
}
