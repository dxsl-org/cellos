//! Kernel-internal capability tokens.
//!
//! Each token is a zero-sized type (ZST).  Constructors are `pub(crate)` so
//! only kernel code can create them — Cell crates are separate Rust
//! compilation units and cannot call `pub(crate)` items from this crate.
//!
//! `Option<ZST>` uses Rust's niche optimization: exactly 1 byte on the wire.
//! Three caps together are 3 bytes, smaller than the previous `KernelPerms(u32)`.

/// Permits raw block-device syscalls (BlkRead, BlkWrite, BlkFlush).
/// Granted to `/bin/vfs` at spawn.
#[derive(Copy, Clone, Debug)]
pub struct BlockIoCap(());

/// Permits network transmit and receive syscalls (NetTx, NetRx).
/// Granted to `/bin/net` at spawn.
#[derive(Copy, Clone, Debug)]
pub struct NetworkCap(());

/// Permits spawning new Cells (SpawnFromPath, SpawnPinned) and hot-swapping (HotSwap).
/// Granted to `/bin/init` and `/bin/shell` at spawn.
#[derive(Copy, Clone, Debug)]
pub struct SpawnCap(());

impl BlockIoCap {
    /// Create a `BlockIoCap` token.  Only callable within the kernel crate.
    pub(crate) fn new() -> Self {
        Self(())
    }
}

impl NetworkCap {
    /// Create a `NetworkCap` token.  Only callable within the kernel crate.
    pub(crate) fn new() -> Self {
        Self(())
    }
}

impl SpawnCap {
    /// Create a `SpawnCap` token.  Only callable within the kernel crate.
    pub(crate) fn new() -> Self {
        Self(())
    }
}

/// Permits use of RISC-V H-extension CSRs (`hstatus`, `hgatp`, `vsatp`, etc.).
///
/// Granted only when BOTH the ELF manifest declares `hypervisor = true` AND
/// `cpu_features::has_h_ext()` confirms the firmware reported H-ext at boot.
/// Always absent on non-riscv64 targets.
#[derive(Copy, Clone, Debug)]
pub struct HypervisorCap(());

impl HypervisorCap {
    /// Create a `HypervisorCap` token.  Only callable within the kernel crate.
    pub(crate) fn new() -> Self {
        Self(())
    }
}

/// Permits `sys_freeze_cell`, `sys_resume_cell`, `sys_kill_cell`.
///
/// Carried in `CapSet` (P-TRUST) and gated by the spawn-time ceiling: the
/// `/bin/supervisor` install path *requests* it (`with_path_caps`), and the
/// request is intersected against the spawner's ceiling like every other cap, so
/// a cell can only receive it if its spawner (ultimately init) holds it. A
/// supervisor cell still cannot forge it into a child beyond its own authority
/// (monotonic downgrade). init also holds it directly (root authority) so it can
/// unfreeze orphaned targets if the Supervisor Cell crashes.
#[derive(Copy, Clone, Debug)]
pub struct SupervisorCap(());

impl SupervisorCap {
    /// Create a `SupervisorCap` token.  Only callable within the kernel crate.
    pub(crate) fn new() -> Self {
        Self(())
    }
}

/// Permits claiming PCIe BAR MMIO ranges and authorising DMA via `GrantDma`.
///
/// Granted by exact path match in `loader.rs` (`/bin/nvme`, `/bin/e1000`).
/// The v1 manifest has no free flag bits for this cap — it is NOT manifest-based.
/// Required before `RequestMmio` can claim a PCIe BAR range.
#[derive(Copy, Clone, Debug)]
pub struct PcieDriverCap(());

impl PcieDriverCap {
    /// Create a `PcieDriverCap` token.  Only callable within the kernel crate.
    pub(crate) fn new() -> Self {
        Self(())
    }
}

/// Permits PCIe ECAM enumeration and BAR registration via `sys_register_pcie_bar`.
///
/// Granted by exact path match in `loader.rs` to `/bin/platform` ONLY, and is a
/// singleton — the kernel refuses to grant it a second time (second `/bin/platform`
/// spawn is rejected). This prevents any cell other than the one trusted Platform
/// Cell from declaring fake BARs in the allowlist.
#[derive(Copy, Clone, Debug)]
pub struct PlatformCap(());

use core::sync::atomic::{AtomicBool, Ordering};

/// Singleton latch: set when PlatformCap has been granted to any task.
/// Once true, `try_grant_platform()` returns `None` for all future callers.
static PLATFORM_CAP_GRANTED: AtomicBool = AtomicBool::new(false);

impl PlatformCap {
    /// Create a `PlatformCap` token.  Only callable within the kernel crate.
    pub(crate) fn new() -> Self {
        Self(())
    }
}

/// Attempt to grant `PlatformCap`.
///
/// Returns `Some(PlatformCap)` on the first call, `None` on all subsequent calls
/// (singleton enforcement). The compare_exchange is sequentially consistent to
/// avoid races on SMP (even though Cellos is currently UP, the invariant must hold
/// under future SMP enablement).
pub(crate) fn try_grant_platform() -> Option<PlatformCap> {
    PLATFORM_CAP_GRANTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .ok()
        .map(|_| PlatformCap::new())
}

// ─── Capability set + spawn-delegation (P2 — monotonic downgrade) ────────────

/// A plain-data snapshot of a Task's capabilities, used to enforce spawn-time
/// **intersection**: a child is granted `manifest ∩ spawner`, so no cell can
/// hand a child a capability it does not itself hold (Fuchsia/Genode monotonic
/// downgrade). Single source of truth for "what caps does X hold".
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CapSet {
    pub block_io: bool,
    pub network: bool,
    pub spawn: bool,
    pub hypervisor: bool,
    pub mmio_devices: u8,  // bitmask of resource_registry::DEV_*
    pub block_regions: u8, // P03 partition bitmask
    // P-TRUST: the privileged path-triggered caps now live in the CapSet so the
    // SAME spawn-time intersection that bounds every other cap also bounds them.
    // Before this, they were minted by a raw `path ==` match AFTER (and blind to)
    // the ceiling intersection — reachable via sys_spawn_from_elf to hand any
    // SpawnCap holder PcieDriverCap → DMA-anywhere (LBI bypass). They have no
    // manifest flag bit (v1 manifest is full); the install path is the request
    // signal, but the request is now `∩ ceiling` like everything else.
    pub pcie_driver: bool,
    pub platform: bool,
    pub supervisor: bool,
}

impl CapSet {
    /// No capabilities (used for an unknown spawner — fail-safe).
    pub const EMPTY: CapSet = CapSet {
        block_io: false,
        network: false,
        spawn: false,
        hypervisor: false,
        mmio_devices: 0,
        block_regions: 0,
        pcie_driver: false,
        platform: false,
        supervisor: false,
    };

    /// Full capability authority — granted ONLY to `init` (the root authority,
    /// like seL4's initial task holds the root CNode). Direct-write in `main.rs`;
    /// never reached via the manifest path. `hypervisor` is set unconditionally
    /// here (init never exercises H-ext CSRs; a child's H-ext gate lives in
    /// `from_manifest`, and intersection preserves it).
    pub const ALL: CapSet = CapSet {
        block_io: true,
        network: true,
        spawn: true,
        hypervisor: true,
        mmio_devices: crate::resource_registry::DEV_GPIO | crate::resource_registry::DEV_UART,
        block_regions: 0b111,
        // init is root authority, so its ceiling permits delegating the privileged
        // path-caps to the driver/supervisor cells it spawns. `platform` is inert
        // in practice — the Platform Cell is Root-spawned by the kernel, and
        // `apply_to` never writes `platform_cap` (the singleton latch owns it).
        pcie_driver: true,
        platform: true,
        supervisor: true,
    };

    /// Snapshot a (running) Task's current capabilities.
    pub fn of_task(t: &super::tcb::Task) -> CapSet {
        CapSet {
            block_io: t.block_io_cap.is_some(),
            network: t.network_cap.is_some(),
            spawn: t.spawn_cap.is_some(),
            hypervisor: t.hypervisor_cap.is_some(),
            mmio_devices: t.mmio_devices,
            block_regions: t.block_regions,
            pcie_driver: t.pcie_driver_cap.is_some(),
            platform: t.platform_cap.is_some(),
            supervisor: t.supervisor_cap.is_some(),
        }
    }

    /// Derive the caps a manifest *requests*. Mirrors the historical loader grant
    /// logic exactly — in particular `block_regions` replicates the SRV-bit
    /// co-grant `data | (lfs<<1) | (lfs<<2)` (NOT a 1:1 copy) so the VFS service
    /// keeps its P5 range after intersection. The H-ext gate is baked in here so
    /// `hypervisor` can never be held on a CPU lacking H-ext.
    pub fn from_manifest(m: &api::manifest::CellManifest) -> CapSet {
        let hv = m.has_hypervisor()
            && (crate::cpu_features::has_h_ext() || crate::cpu_features::has_el2());
        let mut mmio = 0u8;
        if m.has_gpio() {
            mmio |= crate::resource_registry::DEV_GPIO;
        }
        if m.has_uart() {
            mmio |= crate::resource_registry::DEV_UART;
        }
        if m.has_can() {
            mmio |= crate::resource_registry::DEV_CAN;
        }
        if m.has_adc() {
            mmio |= crate::resource_registry::DEV_ADC;
        }
        CapSet {
            block_io: m.has_block_io(),
            network: m.has_network(),
            spawn: m.has_spawn(),
            hypervisor: hv,
            mmio_devices: mmio,
            block_regions: (m.has_part_data() as u8)
                | ((m.has_part_lfs() as u8) << 1)
                | ((m.has_part_lfs() as u8) << 2),
            // The manifest never requests the privileged path-caps (no flag bits);
            // they are layered on by `with_path_caps` from the install path.
            pcie_driver: false,
            platform: false,
            supervisor: false,
        }
    }

    /// Layer the path-triggered privileged authority onto a requested CapSet.
    /// These caps have no manifest flag bit (v1 manifest is full), so the install
    /// path is the request signal — but the resulting request is still run through
    /// the same `∩ ceiling` intersection as every other cap. This is the P-TRUST
    /// fix: the loader used to mint these by raw `path ==` AFTER the intersection,
    /// so `sys_spawn_from_elf(bytes, "/bin/nvme")` handed any SpawnCap holder
    /// `PcieDriverCap` regardless of its ceiling → DMA-anywhere.
    ///
    /// The cell-store block region for `/bin/vfs` is intentionally NOT folded here:
    /// the `/bin/vfs` operator-policy entry grants `block_regions = 0b111`, so
    /// folding `0b1000` into the request would be zeroed by the policy `∩` and
    /// break VFS. It stays a post-policy raw grant in the loader until a POLICY.BIN
    /// re-bake lets it be folded (documented follow-up).
    pub fn with_path_caps(mut self, path: &str) -> CapSet {
        if matches!(
            path,
            "/bin/nvme"
                | "/bin/e1000"
                | "/bin/virtio-net"
                | "/bin/block"
                | "/bin/input"
                | "/bin/virtio-gpu"
        ) {
            self.pcie_driver = true;
        }
        if path == "/bin/platform" {
            self.platform = true;
        }
        if path == "/bin/supervisor" {
            self.supervisor = true;
        }
        self
    }

    /// Field-wise minimum (bool AND, bitmask AND). The monotonic-downgrade core.
    pub fn intersect(self, o: CapSet) -> CapSet {
        CapSet {
            block_io: self.block_io && o.block_io,
            network: self.network && o.network,
            spawn: self.spawn && o.spawn,
            hypervisor: self.hypervisor && o.hypervisor,
            mmio_devices: self.mmio_devices & o.mmio_devices,
            block_regions: self.block_regions & o.block_regions,
            pcie_driver: self.pcie_driver && o.pcie_driver,
            platform: self.platform && o.platform,
            supervisor: self.supervisor && o.supervisor,
        }
    }

    /// Write the granted caps into a child Task's TCB fields. Pure data — block-IO
    /// VFS-handler registration and any other side effects stay in the loader,
    /// keyed off the *granted* (not requested) bits.
    pub fn apply_to(self, t: &mut super::tcb::Task) {
        t.block_io_cap = self.block_io.then(BlockIoCap::new);
        t.network_cap = self.network.then(NetworkCap::new);
        t.spawn_cap = self.spawn.then(SpawnCap::new);
        t.hypervisor_cap = self.hypervisor.then(HypervisorCap::new);
        t.mmio_devices = self.mmio_devices;
        t.block_regions = self.block_regions;
        t.pcie_driver_cap = self.pcie_driver.then(PcieDriverCap::new);
        t.supervisor_cap = self.supervisor.then(SupervisorCap::new);
        // NOTE: `platform_cap` is deliberately NOT written here. It is a singleton
        // (`try_grant_platform` enforces one-holder-ever); the loader consults the
        // latch when `granted.platform` is set. Writing it from a plain bool would
        // bypass the latch and allow two holders.
    }
}

/// Compute the granted x86 PKU isolation tier from the granted caps and the
/// manifest's requested tier (Manifest v2). Pure logic, host/target-agnostic —
/// extracted so it is unit-testable independent of the loader's spawn plumbing.
///
/// `tier` is a FLOOR, not a ceiling — the inverse of a capability. A higher tier
/// number means MORE isolation / LESS authority. A cell may always RAISE its own
/// tier (self-restrict further); it may NEVER lower it below the floor derived
/// from its granted caps (that would be a privilege escalation). Hence
/// `max(requested_tier, floor)`, not a plain assignment.
///
/// `floor`: cells holding real system authority (block_io/network/spawn/
/// hypervisor — the pre-v2 "is_trusted" set) may run at `TIER_TRUSTED_CORE` (0,
/// unfenced). Everything else has a floor of `TIER_STANDARD` (1) — it can never
/// claim tier 0 no matter what it asks for.
///
/// `TIER_LEGACY` (manifest absent, or a tier-less manifest) means "no explicit
/// request" — the granted tier is exactly the floor, reproducing the pre-v2
/// behaviour byte-for-byte (NOT `max(0xFF, floor)`, which would wrongly force
/// every such cell to the most-isolated tier).
pub fn granted_tier(granted: &CapSet, requested_tier: u8) -> u8 {
    use api::manifest::{TIER_LEGACY, TIER_STANDARD, TIER_TRUSTED_CORE};
    let is_trusted = granted.block_io || granted.network || granted.spawn || granted.hypervisor;
    let floor: u8 = if is_trusted {
        TIER_TRUSTED_CORE
    } else {
        TIER_STANDARD
    };
    if requested_tier == TIER_LEGACY {
        floor
    } else {
        core::cmp::max(requested_tier, floor)
    }
}

/// Who initiated a spawn — determines the capability ceiling for the new cell.
#[derive(Copy, Clone, Debug)]
pub enum Spawner {
    /// Kernel/boot-initiated (only `init`). No intersection — grant full manifest.
    Root,
    /// User-cell-initiated via syscall. Child caps = `manifest ∩ caps_of(tid)`.
    User(usize),
    /// Kernel-internal re-spawn (HotSwap) bounded by an explicit ceiling
    /// (the replaced cell's caps) — NOT the `Root` exemption.
    Ceiling(CapSet),
}

#[cfg(test)]
mod tests {
    use super::CapSet;

    #[test]
    fn intersect_is_monotonic_downgrade() {
        let spawner = CapSet {
            block_io: false,
            network: true,
            spawn: true,
            hypervisor: false,
            mmio_devices: 0b01,
            block_regions: 0b010,
            ..CapSet::EMPTY
        };
        let child = CapSet {
            block_io: true,
            network: true,
            spawn: false,
            hypervisor: true,
            mmio_devices: 0b11,
            block_regions: 0b111,
            ..CapSet::EMPTY
        };
        let g = child.intersect(spawner);
        assert!(!g.block_io, "child cannot gain block_io its spawner lacks");
        assert!(g.network);
        assert!(!g.spawn);
        assert!(!g.hypervisor);
        assert_eq!(g.mmio_devices, 0b01);
        assert_eq!(g.block_regions, 0b010);
    }

    #[test]
    fn all_intersect_child_is_child() {
        let child = CapSet {
            block_io: true,
            network: false,
            spawn: true,
            hypervisor: false,
            mmio_devices: 0b10,
            block_regions: 0b101,
            ..CapSet::EMPTY
        };
        // init (ALL) spawning a child must leave the child's requested caps intact.
        assert_eq!(child.intersect(CapSet::ALL), child);
    }

    #[test]
    fn privileged_path_cap_bounded_by_ceiling() {
        // P-TRUST: a /bin/nvme request carries pcie_driver, but a spawner whose
        // ceiling lacks it must NOT be able to hand it to the child (the closed
        // DMA-anywhere hole). EMPTY.with_path_caps sets the request bits.
        let requested = CapSet::EMPTY.with_path_caps("/bin/nvme");
        assert!(requested.pcie_driver, "path request sets pcie_driver");
        // Non-privileged spawner (no pcie_driver in its ceiling).
        let ceiling = CapSet {
            spawn: true,
            ..CapSet::EMPTY
        };
        assert!(
            !requested.intersect(ceiling).pcie_driver,
            "child cannot gain pcie_driver its spawner lacks"
        );
        // init (ALL) as ceiling → the legitimate driver spawn keeps it.
        assert!(
            requested.intersect(CapSet::ALL).pcie_driver,
            "init's Root ceiling permits the real driver cell"
        );
    }
}
