//! GIC-400 (GICv2) driver, shared by the GICv2 board backends.
//!
//! Every function is generic over the board's [`GicV2Layout`], so each backend
//! supplies its distributor and CPU-interface bases from its `hal/soc` profile
//! (QEMU virt: 0x08000000 / 0x08010000).

/// One board's GICv2 wiring.
pub trait GicV2Layout {
    /// Distributor (`GICD_*`) MMIO base.
    const DISTRIBUTOR_BASE: usize;
    /// CPU interface (`GICC_*`) MMIO base.
    const CPU_INTERFACE_BASE: usize;
    /// GIC ID of the board's GPIO controller interrupt; `u32::MAX` when none
    /// is routed.
    const GPIO_GIC_ID: u32;

    /// Enable the board's device interrupts during distributor bring-up.
    fn enable_board_irqs();

    /// Dispatch a shared peripheral interrupt (GIC ID ≥ 32) that is neither
    /// the timer nor the GPIO controller.
    fn dispatch_spi(irq: u32);
}

fn gicd<G: GicV2Layout>(offset: usize) -> *mut u32 {
    (G::DISTRIBUTOR_BASE + offset) as *mut u32
}
fn gicc<G: GicV2Layout>(offset: usize) -> *mut u32 {
    (G::CPU_INTERFACE_BASE + offset) as *mut u32
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
pub fn init_cpu<G: GicV2Layout>() {
    wr(gicc::<G>(GICC_PMR), 0xFF);
    wr(gicc::<G>(GICC_CTLR), 1);
    enable_irq::<G>(SGI_IPI);
}

/// Send SGI `id` to the CPU at `target_cpu` (GICv2 `GICD_SGIR`, target list).
///
/// Only *remote* CPUs are addressed: the target list is relative to the writing
/// CPU's cluster, and every platform this kernel supports keeps its CPUs in one
/// cluster (Aff0 = CPU index).
pub fn send_sgi<G: GicV2Layout>(target_cpu: u32, id: u32) {
    if target_cpu >= 8 {
        return;
    }
    wr(gicd::<G>(GICD_SGIR), (1 << (16 + target_cpu)) | (id & 0xF));
}

/// Initialise GIC distributor and CPU interface.
pub fn init<G: GicV2Layout>() {
    // Disable distributor, configure, then enable.
    wr(gicd::<G>(GICD_CTLR), 0);

    // Set all SPIs to edge-triggered, targeting CPU 0, medium priority.
    let lines = (((rd(gicd::<G>(0x004)) & 0x1F) + 1) * 32) as usize; // GICD_TYPER
    for i in 0..(lines / 4) {
        wr(gicd::<G>(GICD_IPRIORITYR + i * 4), 0xA0A0_A0A0);
        wr(gicd::<G>(GICD_ITARGETSR + i * 4), 0x0101_0101); // CPU 0
    }
    for i in 0..(lines / 16) {
        wr(gicd::<G>(GICD_ICFGR + i * 4), 0); // level-triggered
    }

    // Enable distributor.
    wr(gicd::<G>(GICD_CTLR), 1);

    G::enable_board_irqs();

    // CPU interface: allow all priorities, enable.
    wr(gicc::<G>(GICC_PMR), 0xFF);
    wr(gicc::<G>(GICC_CTLR), 1);
}

/// Enable a specific IRQ in the distributor.
pub fn enable_irq<G: GicV2Layout>(irq: u32) {
    let reg = GICD_ISENABLER + (irq as usize / 32) * 4;
    wr(gicd::<G>(reg), 1 << (irq % 32));
}

/// Claim the highest-priority pending IRQ (acknowledge).
///
/// Returns the IRQ ID, or 0x3FF if no interrupt is pending.
pub fn claim<G: GicV2Layout>() -> u32 {
    rd(gicc::<G>(GICC_IAR)) & 0x3FF
}

/// Signal end-of-interrupt for `irq`.
pub fn complete<G: GicV2Layout>(irq: u32) {
    wr(gicc::<G>(GICC_EOIR), irq);
}
