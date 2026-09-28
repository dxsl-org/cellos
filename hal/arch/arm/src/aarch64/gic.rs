//! GIC-400 (GICv2) driver for QEMU virt machine.
//!
//! Distributor: 0x08000000  CPU interface: 0x08010000
//! Use  in QEMU to select GICv2.

#[cfg(feature = "board-rpi4")]
const GICD_BASE: usize = hal_soc_bcm27xx::BCM2711.mmio.gic_distributor_base;
#[cfg(not(feature = "board-rpi4"))]
const GICD_BASE: usize = hal_soc_arm_virt::QEMU_ARM_VIRT.gic_distributor.base;
#[cfg(feature = "board-rpi4")]
const GICC_BASE: usize = hal_soc_bcm27xx::BCM2711.mmio.gic_cpu_base;
#[cfg(not(feature = "board-rpi4"))]
const GICC_BASE: usize = hal_soc_arm_virt::QEMU_ARM_VIRT.gic_cpu.base;

fn gicd(offset: usize) -> *mut u32 {
    (GICD_BASE + offset) as *mut u32
}
fn gicc(offset: usize) -> *mut u32 {
    (GICC_BASE + offset) as *mut u32
}

fn wr(ptr: *mut u32, val: u32) {
    unsafe { core::ptr::write_volatile(ptr, val) }
}
fn rd(ptr: *mut u32) -> u32 {
    unsafe { core::ptr::read_volatile(ptr) }
}

const GICD_CTLR: usize = 0x000;
const GICD_ISENABLER: usize = 0x100; // +4*n
const GICD_IPRIORITYR: usize = 0x400;
const GICD_ITARGETSR: usize = 0x800;
const GICD_ICFGR: usize = 0xC00;
const GICC_CTLR: usize = 0x000;
const GICC_PMR: usize = 0x004;
const GICC_IAR: usize = 0x00C;
const GICC_EOIR: usize = 0x010;
const GICD_SGIR: usize = 0xF00;

/// Software-generated interrupt used as the kernel's cross-hart IPI.
///
/// SGIs are private to each CPU and never routed through the distributor's
/// configuration, which is exactly what a "flush your local TLB and publish the
/// epoch" request needs: it must reach a hart parked in WFI, and it must not be
/// maskable by the sender's own priority state.
pub const SGI_IPI: u32 = 0;

/// Enable this CPU's banked interface and its private interrupts.
///
/// The distributor (`init()`) is global, but `GICC_*` and the enable bits for
/// SGIs/PPIs are banked per CPU: a secondary hart that skips this never takes a
/// timer tick or an IPI, and its pending SGIs stay unacknowledged forever.
pub fn init_cpu() {
    wr(gicc(GICC_PMR), 0xFF);
    wr(gicc(GICC_CTLR), 1);
    enable_irq(SGI_IPI);
}

/// Send SGI `id` to the CPU at `target_cpu` (GICv2 `GICD_SGIR`, target list).
///
/// Only *remote* CPUs are addressed: the target list is relative to the writing
/// CPU's cluster, and every platform this kernel supports keeps its CPUs in one
/// cluster (Aff0 = CPU index).
pub fn send_sgi(target_cpu: u32, id: u32) {
    if target_cpu >= 8 {
        return;
    }
    wr(gicd(GICD_SGIR), (1 << (16 + target_cpu)) | (id & 0xF));
}

/// Initialise GIC distributor and CPU interface.
pub fn init() {
    // Disable distributor, configure, then enable.
    wr(gicd(GICD_CTLR), 0);

    // Set all SPIs to edge-triggered, targeting CPU 0, medium priority.
    let lines = (((rd(gicd(0x004)) & 0x1F) + 1) * 32) as usize; // GICD_TYPER
    for i in 0..(lines / 4) {
        wr(gicd(GICD_IPRIORITYR + i * 4), 0xA0A0_A0A0);
        wr(gicd(GICD_ITARGETSR + i * 4), 0x0101_0101); // CPU 0
    }
    for i in 0..(lines / 16) {
        wr(gicd(GICD_ICFGR + i * 4), 0); // level-triggered
    }

    // Enable distributor.
    wr(gicd(GICD_CTLR), 1);

    // Enable VirtIO MMIO IRQs: QEMU virt assigns SPI 16..47 (GIC IDs 48..79)
    // to the 32 VirtIO MMIO slots.  Without this, GICD_ISENABLER bit is 0 and
    // the GIC never delivers VirtIO interrupts even after claim/complete.
    // NIC is at slot 30 (SPI 46, GIC ID 78); Block at slot 31 (SPI 47, GIC ID 79).
    #[cfg(not(feature = "board-rpi4"))]
    {
        let virtio = hal_soc_arm_virt::QEMU_ARM_VIRT.virtio;
        let first = hal_soc_arm_virt::ArmVirtProfile::gic_id_for_spi(virtio.first_spi);
        for i in first..first + virtio.count as u32 {
            enable_irq(i);
        }
        enable_irq(hal_soc_arm_virt::ArmVirtProfile::gic_id_for_spi(
            hal_soc_arm_virt::QEMU_ARM_VIRT.gpio.spi,
        ));
    }

    // CPU interface: allow all priorities, enable.
    wr(gicc(GICC_PMR), 0xFF);
    wr(gicc(GICC_CTLR), 1);
}

/// Enable a specific IRQ in the distributor.
pub fn enable_irq(irq: u32) {
    let reg = GICD_ISENABLER + (irq as usize / 32) * 4;
    wr(gicd(reg), 1 << (irq % 32));
}

/// Claim the highest-priority pending IRQ (acknowledge).
///
/// Returns the IRQ ID, or 0x3FF if no interrupt is pending.
pub fn claim() -> u32 {
    rd(gicc(GICC_IAR)) & 0x3FF
}

/// Signal end-of-interrupt for .
pub fn complete(irq: u32) {
    wr(gicc(GICC_EOIR), irq);
}
