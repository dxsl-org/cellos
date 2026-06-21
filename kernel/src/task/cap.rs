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
    pub(crate) fn new() -> Self { Self(()) }
}

impl NetworkCap {
    /// Create a `NetworkCap` token.  Only callable within the kernel crate.
    pub(crate) fn new() -> Self { Self(()) }
}

impl SpawnCap {
    /// Create a `SpawnCap` token.  Only callable within the kernel crate.
    pub(crate) fn new() -> Self { Self(()) }
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
    pub(crate) fn new() -> Self { Self(()) }
}

// ─── Capability set + spawn-delegation (P2 — monotonic downgrade) ────────────

/// A plain-data snapshot of a Task's capabilities, used to enforce spawn-time
/// **intersection**: a child is granted `manifest ∩ spawner`, so no cell can
/// hand a child a capability it does not itself hold (Fuchsia/Genode monotonic
/// downgrade). Single source of truth for "what caps does X hold".
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CapSet {
    pub block_io:      bool,
    pub network:       bool,
    pub spawn:         bool,
    pub hypervisor:    bool,
    pub mmio_devices:  u8, // bitmask of resource_registry::DEV_*
    pub block_regions: u8, // P03 partition bitmask
}

impl CapSet {
    /// No capabilities (used for an unknown spawner — fail-safe).
    pub const EMPTY: CapSet = CapSet {
        block_io: false, network: false, spawn: false,
        hypervisor: false, mmio_devices: 0, block_regions: 0,
    };

    /// Full capability authority — granted ONLY to `init` (the root authority,
    /// like seL4's initial task holds the root CNode). Direct-write in `main.rs`;
    /// never reached via the manifest path. `hypervisor` is set unconditionally
    /// here (init never exercises H-ext CSRs; a child's H-ext gate lives in
    /// `from_manifest`, and intersection preserves it).
    pub const ALL: CapSet = CapSet {
        block_io: true, network: true, spawn: true, hypervisor: true,
        mmio_devices: crate::resource_registry::DEV_GPIO | crate::resource_registry::DEV_UART,
        block_regions: 0b111,
    };

    /// Snapshot a (running) Task's current capabilities.
    pub fn of_task(t: &super::tcb::Task) -> CapSet {
        CapSet {
            block_io:      t.block_io_cap.is_some(),
            network:       t.network_cap.is_some(),
            spawn:         t.spawn_cap.is_some(),
            hypervisor:    t.hypervisor_cap.is_some(),
            mmio_devices:  t.mmio_devices,
            block_regions: t.block_regions,
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
        if m.has_gpio() { mmio |= crate::resource_registry::DEV_GPIO; }
        if m.has_uart() { mmio |= crate::resource_registry::DEV_UART; }
        CapSet {
            block_io:   m.has_block_io(),
            network:    m.has_network(),
            spawn:      m.has_spawn(),
            hypervisor: hv,
            mmio_devices: mmio,
            block_regions: (m.has_part_data() as u8)
                         | ((m.has_part_lfs() as u8) << 1)
                         | ((m.has_part_lfs() as u8) << 2),
        }
    }

    /// Field-wise minimum (bool AND, bitmask AND). The monotonic-downgrade core.
    pub fn intersect(self, o: CapSet) -> CapSet {
        CapSet {
            block_io:      self.block_io      && o.block_io,
            network:       self.network       && o.network,
            spawn:         self.spawn         && o.spawn,
            hypervisor:    self.hypervisor    && o.hypervisor,
            mmio_devices:  self.mmio_devices  &  o.mmio_devices,
            block_regions: self.block_regions &  o.block_regions,
        }
    }

    /// Write the granted caps into a child Task's TCB fields. Pure data — block-IO
    /// VFS-handler registration and any other side effects stay in the loader,
    /// keyed off the *granted* (not requested) bits.
    pub fn apply_to(self, t: &mut super::tcb::Task) {
        t.block_io_cap   = self.block_io.then(BlockIoCap::new);
        t.network_cap    = self.network.then(NetworkCap::new);
        t.spawn_cap      = self.spawn.then(SpawnCap::new);
        t.hypervisor_cap = self.hypervisor.then(HypervisorCap::new);
        t.mmio_devices   = self.mmio_devices;
        t.block_regions  = self.block_regions;
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
        let spawner = CapSet { block_io: false, network: true, spawn: true,
            hypervisor: false, mmio_devices: 0b01, block_regions: 0b010 };
        let child = CapSet { block_io: true, network: true, spawn: false,
            hypervisor: true, mmio_devices: 0b11, block_regions: 0b111 };
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
        let child = CapSet { block_io: true, network: false, spawn: true,
            hypervisor: false, mmio_devices: 0b10, block_regions: 0b101 };
        // init (ALL) spawning a child must leave the child's requested caps intact.
        assert_eq!(child.intersect(CapSet::ALL), child);
    }
}
