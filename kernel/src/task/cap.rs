//! Kernel-internal capability state.
//!
//! A Cell's rights are one [`CapSet`] record per incarnation, held on the Cell's
//! root task (`Task::rights`) and read through `Scheduler::cell_rights`
//! (architecture review H-1). The names `SpawnCap`, `SupervisorCap`,
//! `PcieDriverCap`, … in comments and logs denote the matching `CapSet` field.
//! Only kernel code can construct or change a record.

/// Test-hooks-only authority to publish the caller as `service::SILO`.
///
/// Minted only by the exact governed `/bin/silo` launch route. It is neither
/// manifest-requestable nor included in `CapSet`, so it cannot be delegated.
#[cfg(feature = "test-hooks")]
#[derive(Copy, Clone, Debug)]
pub struct DevelopmentSiloRegistrationCap(());

#[cfg(feature = "test-hooks")]
impl DevelopmentSiloRegistrationCap {
    pub(crate) fn new() -> Self {
        Self(())
    }
}

use core::sync::atomic::{AtomicBool, Ordering};

/// The Platform right (`CapSet::platform`) is a singleton: at most one Cell
/// ever holds it, so no second `/bin/platform` can declare fake BARs in the
/// allowlist. Granted only through [`reserve_platform`] and
/// [`PlatformCapReservation::commit_into`].
///
/// Singleton latch: held by either the one live reservation or the permanently
/// committed Platform task. A reservation owns the only transition back to
/// `false`; once committed, the latch intentionally remains set forever.
static PLATFORM_CAP_GRANTED: AtomicBool = AtomicBool::new(false);

/// Rollback owner for the Platform singleton while a task is unpublished.
pub(crate) struct PlatformCapReservation {
    committed: bool,
}

impl PlatformCapReservation {
    /// Permanently transfer the singleton grant into a complete unpublished TCB.
    pub(crate) fn commit_into(mut self, task: &mut super::tcb::Task) {
        task.rights.platform = true;
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
    pub usb_driver: bool,
    /// Port-I/O serial authority: lets `/bin/serial` drive the 16550 ports the
    /// kernel itself probed. No DMA and no MMIO — it reaches only the declared
    /// ports, which is why it is its own cap rather than a reuse of `pcie_driver`.
    pub serial_port: bool,
    // Coarse per-Cell network rights (Phase 04 S2-N). Enforced by service-net,
    // not by any kernel syscall: the kernel only carries them from the bounded
    // spawn grant into the attested caller trailer (`CALLER_FLAG_NET_*`). Like
    // the P-TRUST fields they have no manifest bit — the install path is the
    // request (`with_path_caps`) and the reviewed ceiling bounds it — but they
    // are NOT P-TRUST: `without_ptrust` keeps them.
    /// `NetRequest::Resolve`.
    pub net_resolve: bool,
    /// Outbound sockets: `TcpConnect`, `UdpCreate`, `UdpSend`.
    pub net_connect: bool,
    /// Inbound sockets: `TcpListen`, `UdpBind`, `MulticastJoin`.
    pub net_listen: bool,
    /// Raw Ethernet frames: `L2Send`, `L2Recv`.
    pub net_l2: bool,
    // Edge-bounded telemetry and VFS-mutation rights (Phase 04 S2-T/S2-W annex).
    // Kernel-only: no manifest bit; the install path is the request
    // (`with_path_caps`: every path except the `/apps/`, `/data/bin/`, `/home/`
    // sandbox) and the reviewed ceiling and operator policy bound it. Not P-TRUST.
    /// X: `GetProcs` / `GetProcs2` are refused at dispatch without it.
    pub observe_tasks: bool,
    /// M: `MemInfo` is refused at dispatch without it.
    pub observe_memory: bool,
    /// W: without it the kernel never attests `CALLER_FLAG_VFS_MUTATE`, even when
    /// allowlist bit 63 is declared, so the VFS refuses every classified mutation.
    pub vfs_mutate: bool,
    /// Q (Phase 01 CP1, 2026-10-25): write the shell key namespace of
    /// service-config (`ConfigRequest::Set` / `Delete`). Enforced by
    /// service-config, not by any kernel syscall: the kernel only carries it from
    /// the bounded spawn grant into the attested caller trailer
    /// (`CALLER_FLAG_CONFIG_WRITE`). No manifest bit; `/bin/cellbox` is the only
    /// install path that requests it (`crate::catalog`). Not P-TRUST.
    pub config_write: bool,
    /// KSSH K3 (2026-10-30): ask service-kms to sign an SSH userauth challenge.
    /// Enforced by service-kms, not by any kernel syscall: the kernel only carries
    /// it from the bounded spawn grant into the attested caller trailer
    /// (`CALLER_FLAG_KMS_SSH_SIGN`). No manifest bit; `/bin/cellbox-ssh` is the
    /// only install path that requests it (`crate::catalog`) and the
    /// reviewed `SSH_CLIENT_EDGE` the only launch ceiling that carries it. Not
    /// P-TRUST. Never held together with `kms_ssh_admin`.
    pub kms_ssh_sign: bool,
    /// KSSH K3 (2026-10-30): the service-kms SSH key administration class,
    /// carried as `CALLER_FLAG_KMS_SSH_ADMIN`. No manifest bit; `/bin/kms-ssh-admin`
    /// is the only install path that requests it (`crate::catalog`), and
    /// no launch edge or boot row carries it yet, so nothing can grant it until a
    /// later reviewed slice adds one. Not P-TRUST. Never held together with
    /// `kms_ssh_sign`.
    pub kms_ssh_admin: bool,
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
        usb_driver: false,
        serial_port: false,
        net_resolve: false,
        net_connect: false,
        net_listen: false,
        net_l2: false,
        observe_tasks: false,
        observe_memory: false,
        vfs_mutate: false,
        config_write: false,
        kms_ssh_sign: false,
        kms_ssh_admin: false,
    };

    /// The three S2-T/S2-W rights (task observation, system memory, VFS
    /// mutation) and nothing else. They were ambient before the annex, so every
    /// pre-existing non-sandbox reviewed row ORs this in to keep image behavior;
    /// the sandbox default arm and future restricted rows leave it out.
    pub const LEGACY_OBSERVE_MUTATE: CapSet = CapSet {
        observe_tasks: true,
        observe_memory: true,
        vfs_mutate: true,
        ..CapSet::EMPTY
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
            | crate::resource_registry::DEV_SPI
            | crate::resource_registry::DEV_DISPLAY,
        block_regions: 0b1111,
        pcie_driver: true,
        platform: true,
        supervisor: true,
        usb_driver: true,
        serial_port: true,
        net_resolve: true,
        net_connect: true,
        net_listen: true,
        net_l2: true,
        observe_tasks: true,
        observe_memory: true,
        vfs_mutate: true,
        config_write: true,
        // `ALL` is the test/self-test ceiling "every field", never a Cell's own
        // CapSet, so it spans both KMS SSH classes that no Cell may hold together.
        kms_ssh_sign: true,
        kms_ssh_admin: true,
    };

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
            usb_driver: false,
            serial_port: false,
            // Network rights have no manifest bit either; `with_path_caps`
            // requests them from the install path.
            net_resolve: false,
            net_connect: false,
            net_listen: false,
            net_l2: false,
            // Nor do the S2-T/S2-W rights; `with_path_caps` requests them.
            observe_tasks: false,
            observe_memory: false,
            vfs_mutate: false,
            config_write: false,
            kms_ssh_sign: false,
            kms_ssh_admin: false,
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
    /// The per-path request lives in `crate::catalog` (one row per install path);
    /// `/bin/vfs`, for example, requests the cell-store block region there so the
    /// same ceiling and operator-policy intersection also bounds bit 3.
    pub fn with_path_caps(self, path: &str) -> CapSet {
        crate::catalog::union(self, crate::catalog::request(path))
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
        requested.pcie_driver
            || requested.platform
            || requested.supervisor
            || requested.usb_driver
            || requested.serial_port
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
            usb_driver: false,
            serial_port: false,
            ..self
        }
    }

    /// Drop the three S2-T/S2-W rights, keeping everything else.
    ///
    /// The caller-owned-ELF launch gate uses it: those rights were ambient
    /// before the annex, so a row that carries only them still counts as
    /// carrying no authority for that route (D-TW1, behavior-preserving).
    pub fn without_observe_mutate(self) -> CapSet {
        CapSet {
            observe_tasks: false,
            observe_memory: false,
            vfs_mutate: false,
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
            usb_driver: self.usb_driver && o.usb_driver,
            serial_port: self.serial_port && o.serial_port,
            net_resolve: self.net_resolve && o.net_resolve,
            net_connect: self.net_connect && o.net_connect,
            net_listen: self.net_listen && o.net_listen,
            net_l2: self.net_l2 && o.net_l2,
            observe_tasks: self.observe_tasks && o.observe_tasks,
            observe_memory: self.observe_memory && o.observe_memory,
            vfs_mutate: self.vfs_mutate && o.vfs_mutate,
            config_write: self.config_write && o.config_write,
            kms_ssh_sign: self.kms_ssh_sign && o.kms_ssh_sign,
            kms_ssh_admin: self.kms_ssh_admin && o.kms_ssh_admin,
        }
    }

    /// The attested caller-trailer flags for this Cell's network rights
    /// (`api::caller_identity::CALLER_FLAG_NET_*`). Only these four bits; the
    /// caller ORs in any other flag it owns.
    pub fn net_caller_flags(&self) -> u32 {
        use api::caller_identity::{
            CALLER_FLAG_NET_CONNECT, CALLER_FLAG_NET_L2, CALLER_FLAG_NET_LISTEN,
            CALLER_FLAG_NET_RESOLVE,
        };
        [
            (self.net_resolve, CALLER_FLAG_NET_RESOLVE),
            (self.net_connect, CALLER_FLAG_NET_CONNECT),
            (self.net_listen, CALLER_FLAG_NET_LISTEN),
            (self.net_l2, CALLER_FLAG_NET_L2),
        ]
        .iter()
        .filter(|(held, _)| *held)
        .fold(0, |flags, (_, flag)| flags | flag)
    }

    /// The attested caller-trailer flag for this Cell's Q right
    /// (`api::caller_identity::CALLER_FLAG_CONFIG_WRITE`), or 0.
    pub fn config_caller_flags(&self) -> u32 {
        if self.config_write {
            api::caller_identity::CALLER_FLAG_CONFIG_WRITE
        } else {
            0
        }
    }

    /// The attested caller-trailer flags for this Cell's KMS SSH rights
    /// (`api::caller_identity::CALLER_FLAG_KMS_SSH_SIGN` /
    /// `CALLER_FLAG_KMS_SSH_ADMIN`), or 0. Only these two bits; the caller ORs in
    /// any other flag it owns.
    pub fn kms_caller_flags(&self) -> u32 {
        use api::caller_identity::{CALLER_FLAG_KMS_SSH_ADMIN, CALLER_FLAG_KMS_SSH_SIGN};
        let mut flags = 0;
        if self.kms_ssh_sign {
            flags |= CALLER_FLAG_KMS_SSH_SIGN;
        }
        if self.kms_ssh_admin {
            flags |= CALLER_FLAG_KMS_SSH_ADMIN;
        }
        flags
    }

    /// This set minus the rights named by a `CapRevoke` mask
    /// (`api::syscall::cap_mask`): the boolean caps it names, and the MMIO-class
    /// and block-region bits in its parameterised fields.
    pub fn without_revoke_mask(self, cap_mask: u32) -> CapSet {
        use api::syscall::cap_mask as CM;
        let keep = |bit: u32| cap_mask & bit == 0;
        CapSet {
            block_io: self.block_io && keep(CM::BLOCK_IO),
            network: self.network && keep(CM::NETWORK),
            spawn: self.spawn && keep(CM::SPAWN),
            hypervisor: self.hypervisor && keep(CM::HYPERVISOR),
            pcie_driver: self.pcie_driver && keep(CM::PCIE_DRIVER),
            platform: self.platform && keep(CM::PLATFORM),
            supervisor: self.supervisor && keep(CM::SUPERVISOR),
            mmio_devices: self.mmio_devices & !((cap_mask >> CM::MMIO_SHIFT) as u8),
            block_regions: self.block_regions & !((cap_mask >> CM::BLKREGION_SHIFT) as u8),
            ..self
        }
    }

    /// Install the granted caps as `t`'s incarnation record (`Task::rights`).
    /// Pure data — block-IO VFS-handler registration and any other side
    /// effects stay in the loader, keyed off the *granted* (not requested) bits.
    /// The Platform singleton is never set here: only
    /// `PlatformCapReservation::commit_into` grants it.
    pub fn apply_to(self, t: &mut super::tcb::Task) {
        t.rights = CapSet {
            platform: t.rights.platform,
            ..self
        };
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

    const NET_ALL: CapSet = CapSet {
        net_resolve: true,
        net_connect: true,
        net_listen: true,
        net_l2: true,
        ..CapSet::EMPTY
    };

    fn net_rights(c: CapSet) -> (bool, bool, bool, bool) {
        (c.net_resolve, c.net_connect, c.net_listen, c.net_l2)
    }

    #[test]
    fn manifest_never_requests_network_rights() {
        // Every manifest flag set: still no net right, because the install path —
        // not the manifest — is the request signal.
        let all = api::manifest::CellManifest::with_all(
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            api::manifest::TIER_LEGACY,
        );
        let caps = CapSet::from_manifest(&all);
        assert!(caps.network, "fixture must be a fully-flagged manifest");
        assert_eq!(net_rights(caps), (false, false, false, false));
    }

    #[test]
    fn intersect_ands_each_network_right() {
        let request = CapSet {
            net_resolve: true,
            net_connect: true,
            net_listen: false,
            net_l2: true,
            ..CapSet::EMPTY
        };
        let ceiling = CapSet {
            net_resolve: true,
            net_connect: false,
            net_listen: true,
            net_l2: true,
            ..CapSet::EMPTY
        };
        assert_eq!(
            net_rights(request.intersect(ceiling)),
            (true, false, false, true)
        );
        assert_eq!(
            net_rights(NET_ALL.intersect(CapSet::EMPTY)),
            (false, false, false, false)
        );
        assert_eq!(NET_ALL.intersect(CapSet::ALL), NET_ALL);
    }

    #[test]
    fn path_requests_follow_the_reviewed_grant_table() {
        let rc = (true, true, false, false);
        for path in [
            "/bin/curl",
            "/bin/wget",
            "/bin/mqtt",
            "/bin/ocel",
            "/bin/llm-gateway",
            "/bin/https-demo",
            "/bin/robot-demo",
            "/bin/lua-net",
        ] {
            assert_eq!(net_rights(CapSet::EMPTY.with_path_caps(path)), rc, "{path}");
        }
        for path in ["/bin/nc", "/bin/net-broker"] {
            assert_eq!(
                net_rights(CapSet::EMPTY.with_path_caps(path)),
                (true, true, true, false),
                "{path}"
            );
        }
        for path in ["/bin/httpd", "/bin/sshd"] {
            assert_eq!(
                net_rights(CapSet::EMPTY.with_path_caps(path)),
                (false, false, true, false),
                "{path}"
            );
        }
        assert_eq!(
            net_rights(CapSet::EMPTY.with_path_caps("/bin/hypervisor")),
            (false, false, false, true)
        );
        // D-N2/D-N3 and the census: everything else requests nothing.
        for path in [
            "/bin/cellbox",
            "/bin/lua",
            "/bin/python",
            // `/bin/lua-net` is matched exactly: no prefix, suffix or sandbox copy.
            "/bin/lua-net2",
            "/bin/lua-net/",
            "/apps/lua-net",
            "/data/bin/lua-net",
            "/home/user/lua-net",
            "/bin/net",
            "/bin/e1000",
            "/bin/e1000e",
            "/bin/vmxnet3",
            "/bin/ocel-js",
            "/bin/ocel-quickjs",
            "/bin/async-echo",
            "/bin/tokio-smoke",
            "/bin/p03-filter",
            "/apps/demo",
            "/data/bin/tool",
            "/home/user/app",
        ] {
            assert_eq!(
                net_rights(CapSet::EMPTY.with_path_caps(path)),
                (false, false, false, false),
                "{path}"
            );
        }
    }

    #[test]
    fn network_rights_are_not_ptrust() {
        assert!(!CapSet::path_mints_ptrust("/bin/curl"));
        assert!(!CapSet::path_mints_ptrust("/bin/sshd"));
        assert_eq!(NET_ALL.without_ptrust(), NET_ALL);
        // The hypervisor's own request keeps both its L2 right and its manifest cap.
        let hv = CapSet {
            hypervisor: true,
            ..CapSet::EMPTY
        }
        .with_path_caps("/bin/hypervisor");
        assert!(hv.hypervisor && hv.net_l2);
    }

    #[test]
    fn network_rights_map_to_exact_trailer_flags() {
        use api::caller_identity::{
            CALLER_FLAG_NET_CONNECT, CALLER_FLAG_NET_L2, CALLER_FLAG_NET_LISTEN,
            CALLER_FLAG_NET_RESOLVE, CALLER_FLAG_NET_RIGHTS_MASK,
        };
        assert_eq!(CapSet::EMPTY.net_caller_flags(), 0);
        assert_eq!(NET_ALL.net_caller_flags(), CALLER_FLAG_NET_RIGHTS_MASK);
        // Non-network caps never leak into the trailer.
        let everything_but_net = CapSet {
            net_resolve: false,
            net_connect: false,
            net_listen: false,
            net_l2: false,
            ..CapSet::ALL
        };
        assert_eq!(everything_but_net.net_caller_flags(), 0);
        for (caps, flag) in [
            (
                CapSet {
                    net_resolve: true,
                    ..CapSet::EMPTY
                },
                CALLER_FLAG_NET_RESOLVE,
            ),
            (
                CapSet {
                    net_connect: true,
                    ..CapSet::EMPTY
                },
                CALLER_FLAG_NET_CONNECT,
            ),
            (
                CapSet {
                    net_listen: true,
                    ..CapSet::EMPTY
                },
                CALLER_FLAG_NET_LISTEN,
            ),
            (
                CapSet {
                    net_l2: true,
                    ..CapSet::EMPTY
                },
                CALLER_FLAG_NET_L2,
            ),
        ] {
            assert_eq!(caps.net_caller_flags(), flag);
        }
    }

    fn observe_mutate(c: CapSet) -> (bool, bool, bool) {
        (c.observe_tasks, c.observe_memory, c.vfs_mutate)
    }

    #[test]
    fn manifest_never_requests_observe_mutate_rights() {
        let all = api::manifest::CellManifest::with_all(
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            api::manifest::TIER_LEGACY,
        );
        assert_eq!(
            observe_mutate(CapSet::from_manifest(&all)),
            (false, false, false)
        );
    }

    #[test]
    fn intersect_ands_each_observe_mutate_right() {
        let request = CapSet {
            observe_tasks: true,
            observe_memory: false,
            vfs_mutate: true,
            ..CapSet::EMPTY
        };
        let ceiling = CapSet {
            observe_tasks: true,
            observe_memory: true,
            vfs_mutate: false,
            ..CapSet::EMPTY
        };
        assert_eq!(
            observe_mutate(request.intersect(ceiling)),
            (true, false, false)
        );
        let legacy = CapSet::LEGACY_OBSERVE_MUTATE;
        assert_eq!(observe_mutate(legacy), (true, true, true));
        assert_eq!(legacy.intersect(CapSet::EMPTY), CapSet::EMPTY);
        assert_eq!(legacy.intersect(CapSet::ALL), legacy);
        // The constant carries the three rights and nothing else.
        assert_eq!(legacy.without_observe_mutate(), CapSet::EMPTY);
    }

    /// Annex S2-T/S2-W §3 (D-TW1): every path requests X/M/W except the
    /// user-app sandbox prefixes, which request none.
    #[test]
    fn path_requests_observe_mutate_except_sandbox() {
        for path in [
            "/bin/x",
            "/bin/cellbox",
            "/bin/vfs",
            "/bin/curl",
            "/bin/nvme",
            "/bin/s2tw-probe",
        ] {
            assert_eq!(
                observe_mutate(CapSet::EMPTY.with_path_caps(path)),
                (true, true, true),
                "{path}"
            );
        }
        for path in [
            "/apps/demo",
            "/data/bin/tool",
            "/data/bin/s2tw-probe",
            "/home/user/app",
        ] {
            assert_eq!(
                observe_mutate(CapSet::EMPTY.with_path_caps(path)),
                (false, false, false),
                "{path}"
            );
        }
        // The request is still bounded: an EMPTY ceiling (the sandbox arm) or a
        // ceiling without them drops all three.
        assert_eq!(
            observe_mutate(
                CapSet::EMPTY
                    .with_path_caps("/bin/x")
                    .intersect(CapSet::EMPTY)
            ),
            (false, false, false)
        );
    }

    #[test]
    fn observe_mutate_rights_are_not_ptrust() {
        assert!(!CapSet::path_mints_ptrust("/bin/x"));
        assert_eq!(
            CapSet::LEGACY_OBSERVE_MUTATE.without_ptrust(),
            CapSet::LEGACY_OBSERVE_MUTATE
        );
        assert_eq!(CapSet::LEGACY_OBSERVE_MUTATE.net_caller_flags(), 0);
        // Dropping them leaves every other field untouched.
        let all = CapSet::ALL.without_observe_mutate();
        assert_eq!(observe_mutate(all), (false, false, false));
        assert_eq!(
            CapSet {
                observe_tasks: true,
                observe_memory: true,
                vfs_mutate: true,
                ..all
            },
            CapSet::ALL
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

    /// Phase 01 CP1 (Q, D-Q5): only `/bin/cellbox` requests `config_write`; the
    /// manifest never does; the ceiling bounds it; it is not P-TRUST and maps to
    /// exactly `CALLER_FLAG_CONFIG_WRITE`.
    #[test]
    fn config_write_is_requested_by_the_shell_path_only() {
        use api::caller_identity::CALLER_FLAG_CONFIG_WRITE;
        let all = api::manifest::CellManifest::with_all(
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            api::manifest::TIER_LEGACY,
        );
        assert!(!CapSet::from_manifest(&all).config_write);
        assert!(CapSet::EMPTY.with_path_caps("/bin/cellbox").config_write);
        for path in [
            "/bin/cellbox-ssh",
            "/bin/config",
            "/bin/init",
            "/bin/lua",
            "/bin/lua-net",
            "/bin/python",
            "/bin/terminal",
            "/bin/curl",
            "/bin/x",
            "/apps/cellbox",
            "/data/bin/cellbox",
            "/home/user/cellbox",
        ] {
            assert!(!CapSet::EMPTY.with_path_caps(path).config_write, "{path}");
        }
        let request = CapSet::EMPTY.with_path_caps("/bin/cellbox");
        assert!(
            !request
                .intersect(CapSet::LEGACY_OBSERVE_MUTATE)
                .config_write
        );
        assert!(request.intersect(CapSet::ALL).config_write);
        assert!(!CapSet::path_mints_ptrust("/bin/cellbox"));
        assert!(request.without_ptrust().config_write);
        assert!(request.without_observe_mutate().config_write);
        assert_eq!(request.net_caller_flags(), 0);
        assert_eq!(request.config_caller_flags(), CALLER_FLAG_CONFIG_WRITE);
        assert_eq!(CapSet::EMPTY.config_caller_flags(), 0);
        assert_eq!(
            CapSet {
                config_write: false,
                ..CapSet::ALL
            }
            .config_caller_flags(),
            0
        );
    }

    /// KSSH K3: only the exact path `/bin/cellbox-ssh` requests `kms_ssh_sign`,
    /// only `/bin/kms-ssh-admin` requests `kms_ssh_admin`, the manifest never
    /// requests either, the ceiling bounds both, neither is P-TRUST, no request
    /// carries both, and each maps to exactly its own trailer flag.
    #[test]
    fn kms_ssh_sign_is_requested_by_the_ssh_path_only() {
        use api::caller_identity::{
            CALLER_FLAG_CONFIG_WRITE, CALLER_FLAG_KMS_SSH_ADMIN, CALLER_FLAG_KMS_SSH_SIGN,
            CALLER_FLAG_NET_CONNECT, CALLER_FLAG_NET_RESOLVE,
        };
        let all = api::manifest::CellManifest::with_all(
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            api::manifest::TIER_LEGACY,
        );
        let from_manifest = CapSet::from_manifest(&all);
        assert!(!from_manifest.kms_ssh_sign && !from_manifest.kms_ssh_admin);

        let ssh = CapSet::EMPTY.with_path_caps("/bin/cellbox-ssh");
        assert!(ssh.kms_ssh_sign && !ssh.kms_ssh_admin);
        let admin = CapSet::EMPTY.with_path_caps("/bin/kms-ssh-admin");
        assert!(admin.kms_ssh_admin && !admin.kms_ssh_sign);
        for path in [
            "/bin/cellbox",
            "/bin/curl",
            "/bin/kms",
            "/bin/net",
            "/bin/config",
            "/bin/init",
            "/bin/supervisor",
            "/bin/cellbox-ssh2",
            "/bin/cellbox-ssh/",
            "/bin/cellbox-ssh ",
            "/bin/CELLBOX-SSH",
            "bin/cellbox-ssh",
            "/mem/cellbox-ssh",
            "/apps/cellbox-ssh",
            "/apps/bin/cellbox-ssh",
            "/data/bin/cellbox-ssh",
            "/home/user/cellbox-ssh",
            "/home/user/kms-ssh-admin",
            "/bin/kms-ssh-admin2",
            "",
        ] {
            let r = CapSet::EMPTY.with_path_caps(path);
            assert!(!r.kms_ssh_sign && !r.kms_ssh_admin, "{path:?}");
        }
        // No request names both classes.
        for path in ["/bin/cellbox-ssh", "/bin/kms-ssh-admin", "/bin/cellbox"] {
            let r = CapSet::EMPTY.with_path_caps(path);
            assert!(!(r.kms_ssh_sign && r.kms_ssh_admin), "{path}");
        }

        // The ceiling bounds both rights; `ALL` is the only ceiling that keeps them.
        let net_client = CapSet {
            net_resolve: true,
            net_connect: true,
            ..CapSet::LEGACY_OBSERVE_MUTATE
        };
        assert!(!ssh.intersect(net_client).kms_ssh_sign);
        assert!(!ssh.intersect(CapSet::LEGACY_OBSERVE_MUTATE).kms_ssh_sign);
        assert!(!ssh.intersect(CapSet::EMPTY).kms_ssh_sign);
        assert!(ssh.intersect(CapSet::ALL).kms_ssh_sign);
        assert!(
            ssh.intersect(CapSet {
                kms_ssh_sign: true,
                ..CapSet::EMPTY
            })
            .kms_ssh_sign
        );
        assert!(!admin.intersect(net_client).kms_ssh_admin);
        assert!(!admin.intersect(CapSet::LEGACY_OBSERVE_MUTATE).kms_ssh_admin);
        assert!(admin.intersect(CapSet::ALL).kms_ssh_admin);
        // One right never stands in for the other under a ceiling.
        let sign_only_ceiling = CapSet {
            kms_ssh_admin: false,
            ..CapSet::ALL
        };
        assert!(!admin.intersect(sign_only_ceiling).kms_ssh_admin);
        assert!(!admin.intersect(sign_only_ceiling).kms_ssh_sign);

        // Neither is P-TRUST; both survive the two other `without_*` filters.
        for path in ["/bin/cellbox-ssh", "/bin/kms-ssh-admin"] {
            assert!(!CapSet::path_mints_ptrust(path), "{path}");
        }
        assert!(ssh.without_ptrust().kms_ssh_sign);
        assert!(ssh.without_observe_mutate().kms_ssh_sign);
        assert!(admin.without_ptrust().kms_ssh_admin);
        assert!(admin.without_observe_mutate().kms_ssh_admin);

        // Flag mapping: exactly its own bit, no leak into the NET or CONFIG flags.
        assert_eq!(ssh.kms_caller_flags(), CALLER_FLAG_KMS_SSH_SIGN);
        assert_eq!(admin.kms_caller_flags(), CALLER_FLAG_KMS_SSH_ADMIN);
        assert_eq!(CapSet::EMPTY.kms_caller_flags(), 0);
        assert_eq!(
            CapSet::ALL.kms_caller_flags(),
            CALLER_FLAG_KMS_SSH_SIGN | CALLER_FLAG_KMS_SSH_ADMIN
        );
        assert_eq!(
            CapSet {
                kms_ssh_sign: false,
                kms_ssh_admin: false,
                ..CapSet::ALL
            }
            .kms_caller_flags(),
            0
        );
        assert_eq!(
            ssh.net_caller_flags(),
            CALLER_FLAG_NET_RESOLVE | CALLER_FLAG_NET_CONNECT
        );
        assert_eq!(ssh.net_caller_flags() & ssh.kms_caller_flags(), 0);
        assert_eq!(ssh.config_caller_flags(), 0);
        // The Q and KMS flags come from different fields and never overlap.
        let shell = CapSet::EMPTY.with_path_caps("/bin/cellbox");
        assert_eq!(shell.kms_caller_flags(), 0);
        assert_eq!(shell.config_caller_flags(), CALLER_FLAG_CONFIG_WRITE);
    }
}
