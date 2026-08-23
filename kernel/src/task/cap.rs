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

/// Permits lifecycle operations such as restart supervision, hot-swap, and
/// unrestricted spawn-family syscalls.
/// Granted to `/bin/init` and other reviewed supervisors, never to `/bin/shell`.
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

/// Singleton latch: held by either the one live reservation or the permanently
/// committed Platform task. A reservation owns the only transition back to
/// `false`; once committed, the latch intentionally remains set forever.
static PLATFORM_CAP_GRANTED: AtomicBool = AtomicBool::new(false);

impl PlatformCap {
    /// Create a `PlatformCap` token. Only callable within the kernel crate.
    pub(crate) fn new() -> Self {
        Self(())
    }
}

/// Rollback owner for the Platform singleton while a task is unpublished.
pub(crate) struct PlatformCapReservation {
    committed: bool,
}

impl PlatformCapReservation {
    /// Permanently transfer the singleton grant into a complete unpublished TCB.
    pub(crate) fn commit_into(mut self, task: &mut super::tcb::Task) {
        task.platform_cap = Some(PlatformCap::new());
        self.committed = true;
    }
}

impl Drop for PlatformCapReservation {
    fn drop(&mut self) {
        if !self.committed {
            let reset = PLATFORM_CAP_GRANTED.compare_exchange(
                true,
                false,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
            debug_assert!(
                reset.is_ok(),
                "Platform reservation lost singleton ownership"
            );
        }
    }
}

/// Reserve the singleton grant until atomic task publication commits.
pub(crate) fn reserve_platform() -> Result<PlatformCapReservation, types::ViError> {
    PLATFORM_CAP_GRANTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map(|_| PlatformCapReservation { committed: false })
        .map_err(|_| types::ViError::PermissionDenied)
}

#[cfg(feature = "test-hooks")]
pub(crate) fn platform_reserved_or_committed() -> bool {
    PLATFORM_CAP_GRANTED.load(Ordering::SeqCst)
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

    /// Every cap this kernel can express — a **reference upper bound for
    /// ceiling self-tests only**. It is NOT granted to any task.
    ///
    /// Boot authority comes from the per-path table in
    /// `crate::loader::boot_ceiling`, which states what ONE path may hold. This
    /// constant is the union of everything and therefore never constrains
    /// anything; using it as a ceiling would admit every request while reading
    /// like a restriction. The self-tests use it in the opposite direction — as
    /// the widest possible ceiling, to prove a request is not *over*-tightened.
    pub const ALL: CapSet = CapSet {
        block_io: true,
        network: true,
        spawn: true,
        hypervisor: true,
        mmio_devices: crate::resource_registry::DEV_GPIO
            | crate::resource_registry::DEV_UART
            | crate::resource_registry::DEV_CAN
            | crate::resource_registry::DEV_ADC
            | crate::resource_registry::DEV_I2C
            | crate::resource_registry::DEV_SPI,
        block_regions: 0b1111,
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
            && (crate::cpu_features::has_h_ext()
                || crate::cpu_features::has_el2()
                || crate::cpu_features::has_x86_virt());
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
        if m.has_i2c() {
            mmio |= crate::resource_registry::DEV_I2C;
        }
        if m.has_spi() {
            mmio |= crate::resource_registry::DEV_SPI;
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
    /// `/bin/vfs` requests the cell-store block region here so the same ceiling and
    /// operator-policy intersection that bounds every other authority also bounds bit 3.
    pub fn with_path_caps(mut self, path: &str) -> CapSet {
        if path == "/bin/vfs" {
            self.block_regions |= 0b1000;
        }
        if matches!(
            path,
            "/bin/nvme"
                | "/bin/e1000"
                | "/bin/virtio-net"
                | "/bin/block"
                | "/bin/input"
                | "/bin/virtio-gpu"
                | "/bin/bcm-display"
        ) {
            self.pcie_driver = true;
            self.mmio_devices |= crate::resource_registry::DEV_DISPLAY;
        }
        // NOTE: /bin/dwc2-usb and /bin/lan9514 intentionally receive NO path-triggered
        // caps here. USB host controller authority (DWC2 MMIO + IRQ) requires policy v3
        // with a signed USB byte before it can be expressed through the capability system.
        // Gate with a test matrix in policy::self_test first. See resource_registry.rs.
        if path == "/bin/platform" {
            self.platform = true;
        }
        if path == "/bin/supervisor" {
            self.supervisor = true;
        }
        self
    }

    /// Whether `with_path_caps` layers any privileged (P-TRUST) authority onto a
    /// request for `path` — i.e. whether the install path alone is enough to ask
    /// for `pcie_driver` / `platform` / `supervisor`.
    ///
    /// Derived from `with_path_caps` itself rather than a second path list, so the
    /// match arms above stay the single source of truth. Callers use it to decide
    /// how dangerous a *missing* authorisation for `path` is: for these paths a
    /// permissive default hands out DMA-anywhere or cell-orchestration authority,
    /// so they must fail closed where an ordinary path may not.
    pub fn path_mints_ptrust(path: &str) -> bool {
        let requested = CapSet::EMPTY.with_path_caps(path);
        requested.pcie_driver || requested.platform || requested.supervisor
    }

    /// Drop every privileged (P-TRUST) cap, keeping the ordinary ones.
    ///
    /// The privileged three are the caps whose holder can DMA anywhere or drive
    /// other cells; removing only those keeps a cell runnable (and its failure
    /// diagnosable) where zeroing the whole set would look like a crash.
    pub fn without_ptrust(self) -> CapSet {
        CapSet {
            pcie_driver: false,
            platform: false,
            supervisor: false,
            ..self
        }
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
        // `platform_cap` is transferred only from `PlatformCapReservation`.
        // Applying a plain capability snapshot must never bypass the singleton.
    }
}

/// Compute the granted x86 PKU protection class from the granted caps and the
/// manifest's requested byte (Manifest v2). Pure logic, host/target-agnostic —
/// extracted so it is unit-testable independent of the loader's spawn plumbing.
///
/// The requested byte is a FLOOR, not a ceiling — the inverse of a capability. A
/// higher value means MORE isolation / LESS authority. A cell may always RAISE
/// its own protection class (self-restrict further); it may NEVER lower it below
/// the floor derived from its granted caps (that would be a privilege
/// escalation). Hence `max(requested_protection_class, floor)`, not a plain
/// assignment.
///
/// `floor`: cells holding real system authority (block_io/network/spawn/
/// hypervisor — the pre-v2 "is_trusted" set) may run at
/// `PROTECTION_CLASS_TRUSTED_CORE` (0, unfenced). Everything else has a floor of
/// `PROTECTION_CLASS_STANDARD` (1) — it can never claim class 0 no matter what
/// it asks for.
///
/// `PROTECTION_CLASS_LEGACY` (manifest absent, or a tier-less manifest) means
/// "no explicit request" — the granted class is exactly the floor,
/// reproducing the pre-v2 behaviour byte-for-byte (NOT `max(0xFF, floor)`,
/// which would wrongly force every such cell to the most-isolated class).
pub fn granted_protection_class(granted: &CapSet, requested_protection_class: u8) -> u8 {
    use api::manifest::{
        PROTECTION_CLASS_LEGACY, PROTECTION_CLASS_STANDARD, PROTECTION_CLASS_TRUSTED_CORE,
    };
    let is_trusted = granted.block_io || granted.network || granted.spawn || granted.hypervisor;
    let floor: u8 = if is_trusted {
        PROTECTION_CLASS_TRUSTED_CORE
    } else {
        PROTECTION_CLASS_STANDARD
    };
    if requested_protection_class == PROTECTION_CLASS_LEGACY {
        floor
    } else {
        core::cmp::max(requested_protection_class, floor)
    }
}

/// Compatibility wrapper for callers still using the legacy "tier" terminology.
#[deprecated(
    note = "use granted_protection_class(); the manifest byte is now named protection class"
)]
pub fn granted_tier(granted: &CapSet, requested_tier: u8) -> u8 {
    granted_protection_class(granted, requested_tier)
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
    use super::{granted_protection_class, CapSet};

    #[test]
    fn manifest_maps_i2c_and_spi_to_distinct_mmio_classes() {
        let i2c = api::manifest::CellManifest::with_all(
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            true,
            false,
            api::manifest::TIER_LEGACY,
        );
        let spi = api::manifest::CellManifest::with_all(
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            true,
            api::manifest::TIER_LEGACY,
        );
        assert_eq!(
            CapSet::from_manifest(&i2c).mmio_devices,
            crate::resource_registry::DEV_I2C
        );
        assert_eq!(
            CapSet::from_manifest(&spi).mmio_devices,
            crate::resource_registry::DEV_SPI
        );
    }

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
    fn vfs_path_request_adds_cell_store_region() {
        let requested = CapSet {
            block_io: true,
            block_regions: 0b111,
            ..CapSet::EMPTY
        }
        .with_path_caps("/bin/vfs");
        assert_eq!(requested.block_regions, 0b1111);
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

    #[test]
    fn granted_protection_class_matches_legacy_tier_semantics() {
        let untrusted_caps = CapSet::EMPTY;
        let trusted_caps = CapSet {
            block_io: true,
            ..CapSet::EMPTY
        };
        assert_eq!(
            granted_protection_class(
                &untrusted_caps,
                api::manifest::PROTECTION_CLASS_TRUSTED_CORE
            ),
            api::manifest::PROTECTION_CLASS_STANDARD
        );
        assert_eq!(
            granted_protection_class(&trusted_caps, api::manifest::PROTECTION_CLASS_TRUSTED_CORE),
            api::manifest::PROTECTION_CLASS_TRUSTED_CORE
        );
        assert_eq!(
            granted_protection_class(&trusted_caps, api::manifest::PROTECTION_CLASS_UNTRUSTED),
            api::manifest::PROTECTION_CLASS_UNTRUSTED
        );
        assert_eq!(
            granted_protection_class(&trusted_caps, api::manifest::PROTECTION_CLASS_LEGACY),
            api::manifest::PROTECTION_CLASS_TRUSTED_CORE
        );
    }
}
