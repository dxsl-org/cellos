// SPDX-License-Identifier: MPL-2.0

// `ViSyscall`, its `allowlist_bit` and the raw-id `From<usize>` decode are
// generated from `libs/api/abi/syscalls.toml` by `libs/api/build.rs`.
include!(concat!(env!("OUT_DIR"), "/syscall_table.rs"));

/// Error sentinels of the generic syscall return encoding, as offsets below the
/// all-ones "error" register value (`usize::MAX` on 64-bit, `u32::MAX` on RV32).
/// One definition for the kernel encoder and the `ostd` decoders (architecture
/// remediation Phase 08, Law-1 CP1); the values are frozen.
pub mod result_sentinel {
    /// Every refusal of a syscall without a typed encoding.
    pub const GENERIC: usize = 0;
    /// Out of memory, for the syscalls whose row says `result = "typed_oom"`.
    pub const OUT_OF_MEMORY: usize = 1;
    /// A stream write with no reader end left.
    pub const BROKEN_PIPE: usize = 2;
}

/// Fixed-width little-endian status written only by a successful `IpcTake`.
/// Words: version=1, kind, reply byte length, reserved=0.
pub const IPC_STATUS_LEN: usize = 16;
pub const IPC_STATUS_VERSION: u32 = 1;
pub mod ipc_status {
    pub const REPLY: u32 = 0;
    pub const PEER_GONE: u32 = 1;
    pub const PRE_DISPATCH_TIMEOUT: u32 = 2;
    pub const INDETERMINATE: u32 = 3;
    pub const CANCELLED: u32 = 4;
}

/// Byte length of the frozen [`ViFstatV1`] wire record.
pub const VI_FSTAT_V1_LEN: usize = 32;

/// [`ViFstatV1::kind`] value for character devices.
pub const VI_FSTAT_KIND_CHARACTER: u32 = 1;
/// [`ViFstatV1::kind`] value for regular files.
pub const VI_FSTAT_KIND_REGULAR: u32 = 2;
/// [`ViFstatV1::kind`] value for directories.
pub const VI_FSTAT_KIND_DIRECTORY: u32 = 3;

/// [`ViFstatV1::access`] bit indicating readable access.
pub const VI_FSTAT_ACCESS_READ: u32 = 1;
/// [`ViFstatV1::access`] bit indicating writable access.
pub const VI_FSTAT_ACCESS_WRITE: u32 = 2;

/// Version 1 caller-scoped descriptor metadata.
///
/// This fixed-width record is the kernel wire format. It deliberately exposes
/// only facts the current backends can report truthfully: descriptor kind,
/// access direction, and byte size. `reserved` is always zero on output.
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViFstatV1 {
    /// One of `VI_FSTAT_KIND_*`.
    pub kind: u32,
    /// A non-empty combination of `VI_FSTAT_ACCESS_*`.
    pub access: u32,
    /// File size in bytes; zero for character devices and directories.
    pub size: u64,
    /// Reserved for future versions; always zero in version 1.
    pub reserved: [u64; 2],
}

const _: () = {
    assert!(core::mem::size_of::<ViFstatV1>() == VI_FSTAT_V1_LEN);
    assert!(core::mem::align_of::<ViFstatV1>() == 8);
    assert!(core::mem::offset_of!(ViFstatV1, kind) == 0);
    assert!(core::mem::offset_of!(ViFstatV1, access) == 4);
    assert!(core::mem::offset_of!(ViFstatV1, size) == 8);
    assert!(core::mem::offset_of!(ViFstatV1, reserved) == 16);
    assert!(VI_FSTAT_KIND_CHARACTER == 1);
    assert!(VI_FSTAT_KIND_REGULAR == 2);
    assert!(VI_FSTAT_KIND_DIRECTORY == 3);
    assert!(VI_FSTAT_ACCESS_READ == 1);
    assert!(VI_FSTAT_ACCESS_WRITE == 2);
    assert!(ViSyscall::Fstat as usize == 254);
    assert!(matches!(ViSyscall::Fstat.allowlist_bit(), Some(61)));
    assert!(ViSyscall::Rename as usize == 255);
    assert!(matches!(ViSyscall::Rename.allowlist_bit(), Some(62)));
};

/// Bit constants for the `cap_mask` argument of `ViSyscall::CapRevoke`.
///
/// The 32-bit mask encodes which capability classes to strip from the target cell.
/// Boolean caps occupy the low 4 bits; parameterized caps use two 8-bit sub-fields.
pub mod cap_mask {
    /// Revoke raw block-device access (BlkRead/BlkWrite/BlkFlush).
    pub const BLOCK_IO: u32 = 1 << 0;
    /// Revoke network authority (`NetworkCap`).
    pub const NETWORK: u32 = 1 << 1;
    /// Revoke lifecycle spawn authority (`ForceExit`, `NotifyOnExit`,
    /// `RegisterService`, snapshot control, and any exact launch
    /// profile that explicitly requires lifecycle authority).
    pub const SPAWN: u32 = 1 << 2;
    /// Revoke RISC-V H-extension / ARM64 EL2 hypervisor access.
    pub const HYPERVISOR: u32 = 1 << 3;
    /// Revoke PCIe driver authority: the Cell's whole DMA domain is torn down
    /// (IOMMU domain + requester entries), its BDFs are released, and the BAR
    /// windows it claimed are unmapped from its page tables.
    ///
    /// A revoked Cell ends in the same hardware state as a dead one — that is
    /// what makes this a revocation rather than a label change.
    pub const PCIE_DRIVER: u32 = 1 << 4;
    /// Revoke Platform authority: the ECAM config-space window it claimed is
    /// released and unmapped, and the next `RegisterPciDevice`/`RegisterPcieBar`
    /// from the Cell fails closed.
    pub const PLATFORM: u32 = 1 << 5;
    /// Revoke Supervisor authority: lifecycle control (`ForceExit`,
    /// `NotifyOnExit`, `RegisterService`, snapshot control) stops being
    /// authorized. Kernel-minted and never propagated, so this is a field clear
    /// with no ambient resource behind it.
    pub const SUPERVISOR: u32 = 1 << 6;

    /// Bit offset of the `mmio_devices` sub-field within the mask.
    pub const MMIO_SHIFT: u32 = 8;
    /// Mask selecting the `mmio_devices` 8-bit sub-field (bits 8-15).
    /// Each bit within this byte corresponds to a `resource_registry::DEV_*` flag.
    pub const MMIO_MASK: u32 = 0xFF << MMIO_SHIFT;

    /// Bit offset of the `block_regions` sub-field within the mask.
    pub const BLKREGION_SHIFT: u32 = 16;
    /// Mask selecting the `block_regions` 8-bit sub-field (bits 16-23).
    pub const BLKREGION_MASK: u32 = 0xFF << BLKREGION_SHIFT;

    /// Revoke all capability classes at once.
    pub const ALL: u32 = BLOCK_IO
        | NETWORK
        | SPAWN
        | HYPERVISOR
        | PCIE_DRIVER
        | PLATFORM
        | SUPERVISOR
        | MMIO_MASK
        | BLKREGION_MASK;
}

/// Compact bitset of permitted syscalls, stored as a `u64`.
///
/// Used to build the allowlist embedded in the ELF `__ViCell_syscalls` section
/// via [`declare_syscalls!`].  All methods are `const` so the value can be
/// computed at compile time.
///
/// ```ignore
/// const MY_ALLOWED: SyscallSet = SyscallSet::EMPTY
///     .with(ViSyscall::Send)
///     .with(ViSyscall::Recv)
///     .with(ViSyscall::Log);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SyscallSet(pub u64);

impl SyscallSet {
    /// Empty set — all syscalls denied (not useful on its own; combine with `with`).
    pub const EMPTY: Self = Self(0);
    /// Permit-all sentinel — the default for cells without a `__ViCell_syscalls` section.
    pub const ALL: Self = Self(u64::MAX);

    /// Return a new `SyscallSet` with `syscall` added.
    ///
    /// If `syscall` has no allowlist bit (always-permitted syscalls like `Yield`
    /// or `Exit`) the set is returned unchanged.
    pub const fn with(self, syscall: ViSyscall) -> Self {
        match syscall.allowlist_bit() {
            Some(bit) => Self(self.0 | (1u64 << bit)),
            None => self,
        }
    }

    /// Raw bit-mask value to embed in `__ViCell_syscalls`.
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Returns `true` if `syscall` is permitted by this set.
    ///
    /// Always-permitted syscalls (`Yield`, `Exit`, …) return `true` regardless
    /// of the stored bits.
    pub const fn permits(self, syscall: ViSyscall) -> bool {
        match syscall.allowlist_bit() {
            Some(bit) => (self.0 >> bit) & 1 == 1,
            None => true,
        }
    }
}

/// Embed a syscall allowlist into the current Cell's ELF binary.
///
/// Places a `u64` bitset into the `__ViCell_syscalls` ELF section.  The kernel
/// reads it at spawn time and enforces it on every syscall — any call whose bit
/// is not set is rejected with `PermissionDenied` before the handler runs.
///
/// Cells that do **not** call this macro default to permit-all (backwards
/// compatible).  Cells that call it are restricted to exactly the listed
/// syscalls plus always-permitted ones (`Yield`, `Exit`, `ForceExit`, …).
///
/// # Usage
/// ```ignore
/// api::declare_syscalls![Send, Recv, TryRecv, Log, Heartbeat, LookupServiceBound];
/// ```
/// Allowlist bit index declaring authority to mutate VFS state (create/write/unlink/rename).
pub const VFS_MUTATE_DECLARATION_BIT: u8 = 63;

/// Allowlist bitmask declaring authority to mutate VFS state.
pub const VFS_MUTATE_DECLARATION_MASK: u64 = 1u64 << VFS_MUTATE_DECLARATION_BIT;

/// The static is also exported as `__vicell_syscall_allowlist`, so the SDK can
/// read the Cell's own allowlist (exactly the value the loader reads from the
/// section) and pick a syscall without probing one it may be denied.
#[macro_export]
macro_rules! declare_syscalls {
    ($($syscall:ident),* $(,)?) => {
        #[used]
        #[link_section = "__ViCell_syscalls"]
        #[export_name = "__vicell_syscall_allowlist"]
        pub static VICELL_SYSCALLS: u64 = const {
            let mut mask: u64 = 0u64;
            $(
                $crate::__declare_syscall_entry!(mask, $syscall);
            )*
            mask
        };
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! __declare_syscall_entry {
    ($mask:ident, VfsMutate) => {
        $mask |= $crate::syscall::VFS_MUTATE_DECLARATION_MASK;
    };
    ($mask:ident, $syscall:ident) => {
        if let Some(bit) = $crate::syscall::ViSyscall::$syscall.allowlist_bit() {
            $mask |= 1u64 << bit;
        }
    };
}

/// `GrantSlice` (210) `a3` flag bits (approved 2026-11-10).
pub mod grant_slice {
    /// Publish a holder lease keyed `(caller, a2 owner, grant)` with the
    /// lookup; requires a nonzero `expected_owner_tid`. See
    /// [`super::ViSyscall::GrantSlice`] for when it ends.
    pub const LEASE: usize = 1 << 0;
    /// Every defined bit; any other bit set answers `usize::MAX`.
    pub const KNOWN_FLAGS: usize = LEASE;
    /// Distinct `(owner, grant)` leases one holder keeps at once; a new key
    /// past it answers `usize::MAX`.
    pub const MAX_LEASES_PER_HOLDER: usize = 64;
}

/// `SystemPower` (436) actions and error sentinels.
pub mod system_power {
    /// a0: reset the machine.
    pub const ACTION_REBOOT: usize = 1;
    /// a0: enter ACPI S5 (soft off).
    pub const ACTION_POWEROFF: usize = 2;
    /// No `SupervisorCap`, or the allowlist omits bit 49.
    pub const ERR_PERMISSION_DENIED: usize = usize::MAX;
    /// Unknown action, or a non-zero a1..a3.
    pub const ERR_INVALID_INPUT: usize = usize::MAX - 3;
    /// Not x86_64; poweroff without a static `\_S5_` or a usable PM1 control
    /// block; or the machine still runs after the sleep write.
    pub const ERR_NOT_SUPPORTED: usize = usize::MAX - 5;
}

/// `ProbeVirtioMmio` (442) result encoding.
///
/// Success packs the slot's VirtIO `DeviceID` (bits 0..16) and the low byte
/// of its `Status` register (bits 16..24) into one word. Both fit: assigned
/// device IDs are far below 2^16 and device status is an 8-bit field. The
/// packed value stays below every error sentinel on RV32 as well.
pub mod virtio_probe {
    /// MagicValue is not "virt", or DeviceID does not fit 16 bits: no VirtIO
    /// transport a Driver Cell can name answers at this slot.
    pub const ERR_NOT_FOUND: usize = usize::MAX - 2;
    /// `base` is not one of the board's VirtIO MMIO slot bases, or a1..a3 are
    /// non-zero.
    pub const ERR_INVALID_INPUT: usize = usize::MAX - 3;

    /// VirtIO MMIO MagicValue ("virt", little-endian).
    pub const MAGIC: u32 = 0x7472_6976;
    /// Register offsets the kernel reads to answer a probe.
    pub const MAGIC_OFFSET: usize = 0x000;
    pub const DEVICE_ID_OFFSET: usize = 0x008;
    pub const STATUS_OFFSET: usize = 0x070;
    /// Status bit: the driver finished initialisation and the device is live.
    pub const STATUS_DRIVER_OK: u8 = 0x04;

    /// What a slot reports before any Driver Cell claims it.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SlotIdentity {
        /// VirtIO DeviceID; `0` is an empty transport (no device attached).
        pub device_id: u16,
        /// Low byte of the device Status register.
        pub status: u8,
    }

    /// Pack the identity read from a slot whose MagicValue matched; `None`
    /// when the DeviceID does not fit the encoding (reported as not found).
    pub const fn encode(device_id: u32, status: u32) -> Option<usize> {
        if device_id > u16::MAX as u32 {
            return None;
        }
        Some(device_id as usize | ((status & 0xFF) as usize) << 16)
    }

    /// Unpack a successful return; `None` for any value `encode` cannot
    /// produce, which includes every error sentinel.
    pub const fn decode(ret: usize) -> Option<SlotIdentity> {
        if ret > 0x00FF_FFFF {
            return None;
        }
        Some(SlotIdentity {
            device_id: ret as u16,
            status: (ret >> 16) as u8,
        })
    }
}

/// Kernel event bit masks for `WaitForEvent` / `signal_event`.
///
/// The same bits name sources for `WaitCompletion`, which also accepts
/// [`crate::completion::source::IPC`]; that source is not an event bit and
/// `WaitForEvent` never fires it. `WaitForEvent` accepts any combination;
/// `WaitCompletion` accepts any non-empty combination of its sources and
/// reports exactly one per return.
pub mod events {
    /// A NIC RX frame is available for the net cell to drain.
    pub const NET_RX: u32 = crate::completion::source::NET_RX;
    /// A finite completion wait deadline elapsed.
    ///
    /// This source belongs to `WaitCompletion`; `WaitForEvent` continues to use
    /// its existing timeout return value rather than firing this bit.
    pub const TIMER: u32 = crate::completion::source::TIMER;
}

/// Well-known service IDs for the kernel Service Registry (`RegisterService` /
/// `LookupServiceBound`). Stable ABI — values must not change once shipped. `0` is
/// reserved to mean "no provider".
///
/// A client resolves the live provider of a service by `LookupServiceBound(service::VFS)`
/// instead of hard-coding a tid, so it transparently reconnects when the supervisor
/// respawns that service under a new tid.
pub mod service {
    /// Virtual filesystem service (`/bin/vfs`).
    pub const VFS: u16 = 1;
    /// Network stack service (`/bin/net`).
    pub const NET: u16 = 2;
    /// Input/event service (`/bin/input`).
    pub const INPUT: u16 = 3;
    /// Configuration store service (`/bin/config`).
    pub const CONFIG: u16 = 4;
    /// Display compositor service (`/bin/compositor`).
    pub const COMPOSITOR: u16 = 5;
    /// Development-only AArch64 Silo provider; it accepts only the live KMS cell.
    pub const SILO: u16 = 6;
    /// Hot-swap demo cell — used by the hotswap integration demo only.
    /// Not a production service; registered at demo spawn time.
    pub const HOTSWAP_DEMO: u16 = 7;
    /// Cluster net-broker — the userspace trust anchor for all cross-machine
    /// communication (Noise KKpsk0 p2p, XChaCha20-Poly1305 gossip, RemoteServiceProxy).
    /// Supervised by init; respawning re-registers under a new tid, so clients
    /// must resolve via `provider_tid(service::NET_BROKER)`.
    pub const NET_BROKER: u16 = 8;
    /// Retired single block-driver id: block routes are per device (416/432), and
    /// the kernel refuses to register it.
    pub const BLOCK_DRIVER: u16 = 9;
    /// NIC driver cell. The net cell moves every frame through it; with none
    /// registered, no frame moves.
    pub const NIC_DRIVER: u16 = 10;
    /// Supervisor Cell — hotswap/snapshot orchestration. Trusted by init; holds SupervisorCap.
    /// Send `HotswapRequest` / `SnapshotRequest` IPC to trigger live cell replacement.
    pub const SUPERVISOR: u16 = 11;
    /// VirtIO GPU Driver Cell — owns the VirtIO GPU MMIO and framebuffer DMA.
    /// Kernel `GpuFlush`/`GpuCursor` syscalls forward to this Cell when registered.
    pub const GPU_DRIVER: u16 = 12;
    /// Key-management service for stable Cell-to-Cell node identity and opaque
    /// static DH operations.
    pub const KMS: u16 = 13;
    /// Unified AI inference service (`/bin/ai`) — Spec 24. Clients resolve the live
    /// provider tid, submit a prompt over typed IPC (`ai_proto::AiRequest`), and poll
    /// the session for generated tokens.
    pub const AI: u16 = 15;
    /// Ocel Tier 2 JavaScript engine service (`/bin/ocel-js`).
    /// Executes untrusted web scripts in a hardware MMU-isolated domain.
    pub const OCEL_JS: u16 = 16;
    /// USB HID event producer role — the TID that the kernel verified via
    /// `RegisterUsbHidProducer` (syscall 423) and that the input service accepts
    /// raw `USB_HID_HOST` frames from.
    ///
    /// Deliberately separate from [`NIC_DRIVER`]: on a PC with e1000/virtio-net,
    /// a USB host cell that claimed the NIC role would redirect and drop network
    /// IPC. `/bin/xhci` registers only this role; `/bin/dwc2-usb` registers both
    /// because on RPi3 the LAN9514 is also the NIC.
    ///
    /// Ids 1–13 are the services above, 14 is
    /// [`crate::hypervisor::HYPERVISOR_SERVICE_ID`], 15 `AI`, 16 `OCEL_JS`,
    /// 17 this producer role, 18 `OCEL_PDF`, 19 `OCEL_ACTIVATOR`, 20
    /// `MACHINE_PIN` and 21 `XE_GPU`; next free: 22.
    pub const USB_HID_PRODUCER: u16 = 17;
    /// Ocel's isolated native PDF rasterizer (`/bin/ocel-pdf`).
    pub const OCEL_PDF: u16 = 18;
    /// Init's allowlisted, demand-only Ocel engine activation endpoint.
    pub const OCEL_ACTIVATOR: u16 = 19;
    /// Init's machine pin endpoint ([`crate::services::machine_pin`]). The
    /// kernel accepts this registration only from trusted init naming itself;
    /// the VFS admits the live provider alone to `/srv/.cellos-machine/`.
    pub const MACHINE_PIN: u16 = 20;
    /// Intel GPU driver Cell serving the Linux `xe` uAPI over IPC (ADR-0033).
    pub const XE_GPU: u16 = 21;
}

/// `RegisterService` flags (`a2`): kernel rights a registry row grants the Cell
/// incarnation it names. Only the namespace owner (trusted init or a
/// `SupervisorCap` holder) may set them; a set bit outside [`ALL`](service_flags::ALL)
/// is refused. A hot-swap commit carries the row's flags to the replacement.
pub mod service_flags {
    /// The provider keeps its outer caller (grant leases, owner resolution,
    /// owner-death watches) across the nested IPC it makes while serving; only
    /// its next wildcard receive or its reply ends that context. Holds while the
    /// row is Active or Paused, so requests already in flight finish.
    pub const RETAINS_OUTER_CALLER: usize = 1 << 0;
    /// The provider may watch the root death of the principal it is serving
    /// (`WatchCellOwner`/`WatchCellOwnerRecord`). Only while the row is Active:
    /// a hot-swap quiesce grants no new watch.
    pub const OWNER_WATCH: usize = 1 << 1;
    /// Every defined flag.
    pub const ALL: usize = RETAINS_OUTER_CALLER | OWNER_WATCH;
}

/// Arguments for `SpawnFromMem`.
///
/// Using `repr(C)` for ABI stability. The caller supplies raw ELF bytes plus a
/// caller-chosen label; the kernel may use that label only for mem-route
/// bookkeeping and diagnostics, never as `/bin/*` path authority.
#[repr(C)]
pub struct ViSpawnArgs {
    /// Address of buffer containing ELF.
    pub buffer_addr: usize,
    /// Size of buffer.
    pub buffer_size: usize,
    /// Pointer to name string (utf8).
    pub name_ptr: usize,
    /// Length of name string.
    pub name_len: usize,
    /// Pointer to arguments string (utf8, space separated or null separated).
    pub args_ptr: usize,
    /// Length of arguments string.
    pub args_len: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessInfo {
    pub id: usize,
    pub state: usize, // 0=Ready, 1=Running, 2=Waiting, 3=Terminated
    pub name: [u8; 32],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessInfoV2 {
    pub id: u64,
    pub state: u32, // 0=Ready, 1=Running, 2=Waiting, 3=Terminated
    pub reserved0: u32,
    pub name: [u8; 32],
    pub sample_ticks: u64,
    pub cpu_run_ticks: u64,
    pub heap_bytes: u64,
    pub owned_bytes: u64,
}

/// Version 1 global physical-frame allocator snapshot.
///
/// All fields are fixed-width so the layout is identical on RV32, RV64,
/// AArch64, and x86_64. `used_frames` measures allocator-committed frames,
/// including frames reserved for the kernel heap.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViMemInfoV1 {
    pub total_frames: u64,
    pub used_frames: u64,
    pub free_frames: u64,
    pub page_size: u64,
}
