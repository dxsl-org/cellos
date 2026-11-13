// SPDX-License-Identifier: MPL-2.0
//! Syscall helpers: decode (moved verbatim from `syscall.rs`, architecture
//! remediation Phase 08; the dispatcher stays in `mod.rs`).

use super::*;
#[allow(unused_imports)]
use super::{authority::*, dev::*, fs::*, grant::*, ipc::*, misc::*, spawn::*, vm::*};

/// The Fundamental Verbs of ViCell IPC (Hubris ABI + Lease System)
#[derive(Debug, Copy, Clone)]
pub enum Syscall {
    /// 0: Send (Blocking Message Send)
    Send {
        target: usize,
        msg_ptr: usize,
        msg_len: usize,
    },
    IpcSubmit {
        target: usize,
        ptr: usize,
        len: usize,
    },
    IpcTake {
        token: usize,
        ptr: usize,
        len: usize,
        status_ptr: usize,
    },
    IpcWait {
        ticks: u64,
    },
    IpcCancel {
        token: usize,
    },
    IpcCurrent,
    IpcReply {
        token: usize,
        ptr: usize,
        len: usize,
    },
    /// 4: TrySend (Non-blocking send — drops if target not in Recv)
    TrySend {
        target: usize,
        msg_ptr: usize,
        msg_len: usize,
    },
    /// 1: Recv (Blocking Message Receive)
    Recv {
        mask: usize,
        buf_ptr: usize,
        buf_len: usize,
        /// Receiver asked for a caller-identity trailer (a3 =
        /// `RECV_ATTEST_CALLER`). False for every pre-existing caller, which is
        /// why opting in cannot change any other receiver's buffer.
        attest_caller: bool,
    },
    /// 202: SendGather — send one IPC message from multiple non-contiguous buffers.
    SendGather {
        target: usize,
        iovec_ptr: usize,
        iovec_count: usize,
    },
    /// 203: RecvScatter — receive one IPC message into multiple non-contiguous buffers.
    RecvScatter {
        mask: usize,
        iovec_ptr: usize,
        iovec_count: usize,
    },
    /// 201: RecvTimeout — Recv with a monotonic-tick deadline (Phase 20).
    RecvTimeout {
        mask: usize,
        buf_ptr: usize,
        buf_len: usize,
        /// Deadline in kernel monotonic ticks from boot.  0 = non-blocking.
        deadline: u64,
        /// Write the caller-identity trailer (a timed `Recv` with
        /// `RECV_ATTEST_CALLER`); false for `RecvTimeout` itself.
        attest_caller: bool,
        /// A timed `Recv` reports expiry as an error; `RecvTimeout` keeps its
        /// historical `Ok(0)`.
        timeout_is_error: bool,
    },
    /// 3: SetTimer (Wake up after ticks)
    SetTimer {
        deadline: usize,
    },
    /// 7: TryRecv (Non-blocking Receive)
    TryRecv {
        mask: usize,
        buf_ptr: usize,
        buf_len: usize,
        attest_caller: bool,
    },
    /// 8: Spawn (Create new Task/Thread) - Returns Task ID
    Spawn {
        entry: usize,
        arg: usize,
    },
    /// ViSyscall 17: FutexWait — park until woken, the word changes, or the
    /// deadline elapses. `timeout_ticks == 0` blocks indefinitely.
    FutexWait {
        addr: usize,
        expected: u32,
        timeout_ticks: u64,
    },
    /// ViSyscall 18: FutexWake — wake up to `count` waiters on the caller's key.
    FutexWake {
        addr: usize,
        count: usize,
    },
    /// 11: Log (Debug Print)
    Log {
        msg_ptr: usize,
        msg_len: usize,
    },
    /// 14: Exit (Terminate Process)
    Exit {
        code: usize,
    },
    /// 61: ForceExit — terminate another task by TID; non-blocking return to caller.
    ForceExit {
        tid: usize,
    },
    /// 204: NotifyOnExit — register the caller to be notified when `watched` dies.
    NotifyOnExit {
        watched: usize,
    },
    /// 205: RegisterService — register `tid` as the current provider of `service_id`.
    /// SpawnCap owns the namespace; fixed-ID self-registration uses narrower caps.
    /// `flags` are `abi::syscall::service_flags`, namespace owner only.
    RegisterService {
        service_id: u16,
        tid: usize,
        flags: usize,
    },
    /// 429: LookupServiceBound — resolve `service_id` to its live provider binding
    /// `{ tid, cell_id, generation }` (open; 0 = none). The record is written whole
    /// or refused, never partially.
    LookupServiceBound {
        service_id: u16,
        out_ptr: usize,
        out_len: usize,
    },
    /// 207: Heartbeat — caller asserts liveness; (re)arm the hung-detection deadline
    /// `interval` ticks ahead (0 = disable).
    Heartbeat {
        interval: usize,
    },
    /// 10: SpawnFromMem (Spawn from Memory buffer via Struct)
    SpawnFromMem {
        args_ptr: usize,
    },
    /// 12: SpawnFromPath (Spawn cell by filesystem path)
    /// ABI: path_ptr in a0, path_len in a1.
    SpawnFromPath {
        path_ptr: usize,
        path_len: usize,
    },
    /// 238: SpawnFromElf (Spawn cell from ELF bytes in a caller-owned Grant).
    /// ABI: a0=grant_id, a1=len, a2=path_hint_ptr, a3=path_hint_len.
    SpawnFromElf {
        grant_id: usize,
        len: usize,
        path_ptr: usize,
        path_len: usize,
    },
    /// 16: SpawnPinned — spawn cell pinned to a core (single-core: core_id must be 0).
    /// ABI: a0=path_ptr, a1=path_len, a2=priority: u8, a3=core_id: usize.
    SpawnPinned {
        path_ptr: usize,
        path_len: usize,
        priority: u8,
        core_id: usize,
    },
    /// 240: SpawnSetDirs — name the directory handles the caller's next spawn
    /// passes to its child. ABI: a0 = ptr to `ViSpawnDirHandles` (0 clears).
    SpawnSetDirs {
        carrier_ptr: usize,
    },
    /// 241: QueryDirHandles — read the kernel's record of a cell's inherited
    /// directory handles. ABI: a0 = cell_id, a1 = buf_ptr, a2 = buf_len.
    QueryDirHandles {
        cell_id: u64,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 244: attest the root task owning the current VFS receive principal.
    ResolveCellOwner {
        cell_id: u64,
        generation: u64,
        out_ptr: usize,
        out_len: usize,
    },
    /// 245: atomically attest and subscribe to the principal's root death.
    WatchCellOwner {
        cell_id: u64,
        generation: u64,
        out_ptr: usize,
        out_len: usize,
    },
    /// 246: idempotently cancel one VFS root-death subscription.
    CancelCellOwnerWatch {
        token: u64,
    },
    /// 247: RV32-safe owner attestation through a fixed request record.
    ResolveCellOwnerRecord {
        request_ptr: usize,
        request_len: usize,
        out_ptr: usize,
        out_len: usize,
    },
    /// 248: RV32-safe atomic owner watch through a fixed request record.
    WatchCellOwnerRecord {
        request_ptr: usize,
        request_len: usize,
        out_ptr: usize,
        out_len: usize,
    },
    /// 441: measured image identity of one live Cell generation (VFS only).
    ResolveCellImage {
        request_ptr: usize,
        request_len: usize,
        out_ptr: usize,
        out_len: usize,
    },
    /// 13: OpenCap — open a file and return a CapId.
    OpenCap {
        path_ptr: usize,
        path_len: usize,
    },
    /// 14: ReadCap — read bytes from a cap-backed file.
    ReadCap {
        cap_id: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 15: CloseCap — revoke a capability.
    CloseCap {
        cap_id: usize,
    },
    /// 228: SeekCap — seek a cap-backed file cursor.
    /// offset is transmitted as a raw i64 bit-pattern via usize (reinterpret_cast).
    SeekCap {
        cap_id: usize,
        offset: usize,
        whence: usize,
    },
    /// 229: WriteCap — write bytes into a cap-backed file at the current cursor.
    WriteCap {
        cap_id: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 230: StatCap — query file size via cap (cursor unchanged).
    StatCap {
        cap_id: usize,
    },
    /// 231: TruncateCap — truncate file to len bytes via cap.
    TruncateCap {
        cap_id: usize,
        len: usize,
    },
    /// 232: SyncCap — flush dirty pages to block device via cap.
    SyncCap {
        cap_id: usize,
    },
    /// 233: GrantDma — map PCIe DMA range into IOMMU for calling Cell.
    /// a0 = bdf: u32, a1 = phys: u64, a2 = size: usize.
    GrantDma {
        bdf: u32,
        phys: u64,
        size: usize,
    },
    /// 8: Wait (Wait for task)
    Wait {
        pid: usize,
    },
    /// 20: ShmAlloc
    ShmAlloc {
        size: usize,
    },
    /// 21: ShmMap
    ShmMap {
        handle: usize,
        target_pid: usize,
    },
    /// 30: GetProcs
    GetProcs {
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 239: GetProcs2
    GetProcs2 {
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 243: MemInfo
    MemInfo {
        out_ptr: usize,
        out_len: usize,
    },
    /// 430: TelemetrySnapshot (Phase 06 S6-1)
    TelemetrySnapshot {
        out_ptr: usize,
        out_len: usize,
        flags: usize,
        reserved: usize,
    },
    /// 431: QueryRetirement (Phase 09B K1)
    QueryRetirement {
        root_tid: usize,
        out_ptr: usize,
        out_len: usize,
    },
    /// 432: LookupBlockRoute — copy route slot `index` as a `ViBlockRouteV1`.
    LookupBlockRoute {
        index: usize,
        out_ptr: usize,
        out_len: usize,
        reserved: usize,
    },
    /// 433: QueryMachineFacts — copy the current `ViMachineFactsV1`.
    QueryMachineFacts {
        out_ptr: usize,
        out_len: usize,
        a2: usize,
        a3: usize,
    },
    /// 434: QueryPcieFunction — copy the owned function's `ViPcieFunctionV1`.
    QueryPcieFunction {
        bdf: usize,
        out_ptr: usize,
        out_len: usize,
        reserved: usize,
    },
    /// 435: AllocPcieIrq — MSI-X/MSI vectors for the owned function.
    AllocPcieIrq {
        bdf: usize,
        want: usize,
        mode: usize,
        reserved: usize,
    },
    /// 436: SystemPower — reboot (a0 = 1) or poweroff (a0 = 2); SupervisorCap.
    SystemPower {
        action: usize,
        a1: usize,
        a2: usize,
        a3: usize,
    },
    /// 437: ExportDmaRange — lock caller-owned memory for a DMA export lease.
    ExportDmaRange {
        va: usize,
        len: usize,
        holder_service: usize,
        flags: usize,
    },
    /// 438: MapDmaExport — map a held export into the caller's VT-d domain.
    MapDmaExport {
        export_id: usize,
        bdf: usize,
        out_ptr: usize,
        out_len: usize,
    },
    /// 439: RevokeDmaExport — owner or holder ends an export lease.
    RevokeDmaExport {
        export_id: usize,
        a1: usize,
        a2: usize,
        a3: usize,
    },
    CellSessionV1 {
        request_ptr: usize,
        request_len: usize,
        result_ptr: usize,
        result_capacity: usize,
    },
    /// 263: GetCpuCount — query number of online logical CPUs/harts.
    GetCpuCount,

    // --- Legacy / Compatibility Layer ---
    /// 100: Service Lookup (Find driver ID by name)
    ServiceLookup {
        name_ptr: usize,
        name_len: usize,
    },
    /// 101: Open (Path -> FD)
    Open {
        path_ptr: usize,
        path_len: usize,
    },
    /// 102: Read (FD, Buffer -> Bytes Read)
    Read {
        fd: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 103: Close (FD)
    Close {
        fd: usize,
    },
    /// 105: ReadDir (Read Directory Entries)
    ReadDir {
        fd: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 254: write one fixed-width caller-scoped descriptor metadata record.
    Fstat {
        fd: usize,
        out_ptr: usize,
        out_len: usize,
    },
    /// 255: Rename (Old Path, New Path -> 0 on success)
    Rename {
        old_ptr: usize,
        old_len: usize,
        new_ptr: usize,
        new_len: usize,
    },
    /// 107: ChDir (Change Directory)
    ChDir {
        path_ptr: usize,
        path_len: usize,
    },
    /// 108: GetCwd (Get Current Directory)
    GetCwd {
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 109: Write (FD, Buffer -> Bytes Written)
    Write {
        fd: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 110: MkDir (Path)
    MkDir {
        path_ptr: usize,
        path_len: usize,
    },
    /// 111: Create (Path -> FD)
    /// 111: Create (Path -> FD)
    Create {
        path_ptr: usize,
        path_len: usize,
    },
    /// 104: Yield (Legacy)
    Yield,
    /// 106: Seek (FD, Offset, Whence)
    Seek {
        fd: usize,
        offset: isize,
        whence: usize,
    },
    /// 107: FileOp (Op, Arg1, Arg2)
    FileOp {
        op: usize,
        arg1: usize,
        arg2: usize,
    },
    /// 120: GetTime (Op)
    GetTime {
        op: usize,
    },
    /// 300: GpuFlush — copy cell pixel buffer to VirtIO GPU framebuffer.
    GpuFlush {
        data_ptr: usize,
        data_len: usize,
        xy: usize,
        wh: usize,
    },
    /// 219: CapRevoke — strip capabilities from a live cell at runtime.
    CapRevoke {
        target_tid: usize,
        cap_mask: u32,
    },
    /// 301: GpuCursor — set sprite (op=0) or move (op=1) the VirtIO GPU hardware cursor.
    GpuCursor {
        op: usize,
        data_ptr: usize,
        xy: usize,
        hot: usize,
    },
    /// 302: GpuGetResolution — return current GPU scanout (width, height) packed as (w<<32)|h.
    GpuGetResolution,
    /// 410: StateStash — save serialized cell state under `key` for hot-swap.
    StateStash {
        key: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 411: StateRestore — recover stashed state for `key` into the buffer.
    StateRestore {
        key: usize,
        buf_ptr: usize,
        buf_len: usize,
    },
    /// 412: StateStashClear — delete the stash entry for `key`, freeing its slot.
    StateStashClear {
        key: usize,
    },
    /// 413: FreezeCell — freeze a running Cell. Requires SupervisorCap.
    FreezeCell {
        target_tid: usize,
    },
    /// 414: Resume a frozen Cell or atomically commit a hot-swap cutover.
    /// Plain resume: a0=target_tid, a1..a3=0. Cutover: a0=new_tid,
    /// a1=old_tid, a2=service_id, a3=0. Requires SupervisorCap.
    ResumeCell {
        target_tid: usize,
        source_tid: usize,
        service_id: usize,
        reserved: usize,
    },
    /// 415: KillCell — forcibly terminate a Cell. Requires SupervisorCap.
    KillCell {
        target_tid: usize,
        exit_code: u32,
    },
    /// 416: RegisterBlockDriver — bind the route of the block device the
    /// caller owns (`kind` 1 = PCI BDF, 2 = MMIO base; `reserved` must be 0).
    RegisterBlockDriver {
        kind: usize,
        id: usize,
        reserved: (usize, usize),
    },
    /// 417: RegisterNicDriver — announce caller as the active NIC driver.
    RegisterNicDriver,
    /// 423: RegisterUsbHidProducer — announce caller as the USB HID event producer.
    RegisterUsbHidProducer,
    /// 418: FindPcieDevice — locate a PCIe device by class/subclass/prog_if.
    FindPcieDevice {
        class: u8,
        subclass: u8,
        prog_if: u8,
        out_ptr: usize,
    },
    /// 424: FindPcieDeviceByVendor — locate a PCIe device by exact
    /// (vendor_id, device_id). The second half of the device-selection fix for
    /// the two Ethernet Driver Cells that share class triple `02:00:00`.
    FindPcieDeviceByVendor {
        vendor_id: u16,
        device_id: u16,
        out_ptr: usize,
    },
    /// 425: SerialPortInfo — report one declared serial port's identity and
    /// whether the kernel probed a 16550 register set there.
    SerialPortInfo {
        index: u32,
        out_ptr: usize,
    },
    /// 426: SerialWrite — write bytes to a usable serial port.
    SerialWrite {
        index: u32,
        ptr: usize,
        len: usize,
    },
    /// 427: SerialRead — poll bytes from a usable serial port.
    SerialRead {
        index: u32,
        ptr: usize,
        len: usize,
    },
    /// 428: SerialConfigure — set a usable serial port's baud rate (8N1 kept).
    SerialConfigure {
        index: u32,
        baud: u32,
    },
    /// 419: QueryHotswapReady — check whether `target_tid` has called sys_hotswap_ready().
    /// Returns 1 if ready, 0 if not yet, usize::MAX if tid is unknown.
    QueryHotswapReady {
        target_tid: usize,
    },
    /// 421: SpawnReplacement — supervisor-only spawn using the frozen task's recorded cap ceiling.
    SpawnReplacement {
        old_tid: usize,
        path_ptr: usize,
        path_len: usize,
    },
    /// 422: PauseService — hide the expected provider from service lookups while
    /// it remains runnable for snapshot IPC. Requires SupervisorCap.
    PauseService {
        service_id: u16,
        expected_tid: usize,
    },
    /// 401: HotSwapReady — signal that the new cell has finished deserializing
    /// state and is ready to receive IPC.  No arguments.
    HotSwapReady,
    /// 420: Snapshot — serialize all allocated physical frames to disk for warm boot.
    Snapshot,
    /// 500: BlkRead — read one 512-byte sector from the VirtIO block device.
    /// Not in `ViSyscall` enum to preserve `libs/abi` stability (raw dispatch).
    BlkRead {
        sector: u64,
        buf_ptr: usize,
    },
    /// 501: BlkWrite — write one 512-byte sector to the VirtIO block device.
    BlkWrite {
        sector: u64,
        buf_ptr: usize,
    },
    /// 502: Shutdown — trigger SBI SRST system shutdown (S-mode → OpenSBI). No return.
    Shutdown,
    /// 503: BlkFlush — flush the VirtIO block device write cache to the backing image.
    BlkFlush,
    /// 504: BenchCounters — copy the `bench-counters` record to the caller.
    /// Raw opcode, compiled only with that feature (measurement builds).
    #[cfg(feature = "bench-counters")]
    BenchCounters {
        out_ptr: usize,
        out_len: usize,
    },
    /// 208: GrantAlloc — allocate n pages as a zero-copy Grant region.
    GrantAlloc {
        size: usize,
    },
    /// 209: GrantShare — share a Grant region with `target_cell` under `perm`.
    GrantShare {
        grant_id: usize,
        target_cell: usize,
        perm: usize,
    },
    /// 210: GrantSlice — return the user-space pointer for a Grant the caller owns/holds.
    /// A nonzero `expected_owner_tid` (ABI a2) must equal the Grant's owner;
    /// `flags` (ABI a3) bit 0 `LEASE` also publishes a holder lease on the Grant.
    GrantSlice {
        grant_id: usize,
        size_out_ptr: usize,
        expected_owner_tid: usize,
        flags: usize,
    },
    /// 211: GrantFree — unmap + deallocate a Grant region.
    GrantFree {
        grant_id: usize,
    },
    /// 212: BlkReadAsync — synchronous-but-zero-copy sector read into a Grant buffer.
    BlkReadAsync {
        sector: u64,
        grant_id: usize,
    },
    /// 213: RequestMmio — claim exclusive MMIO range for a peripheral Driver Cell.
    RequestMmio {
        base: usize,
        len: usize,
    },
    /// 442: ProbeVirtioMmio — read one VirtIO MMIO slot's identity unclaimed.
    ProbeVirtioMmio {
        base: usize,
        reserved: (usize, usize, usize),
    },
    /// 214: GetRandom — fill a caller buffer with VirtIO-RNG entropy bytes.
    GetRandom {
        buf_ptr: usize,
        len: usize,
    },
    /// 215: GrantRegister — allocate a persistent pre-pinned Grant buffer (lifetime = cell exit).
    GrantRegister {
        size: usize,
    },
    /// 216: GrantUnregister — explicitly release a registered buffer.
    GrantUnregister {
        reg_id: usize,
    },
    /// 217: WaitForEvent — block until `mask` bits fire or `deadline` ticks pass.
    /// `deadline = None` means block indefinitely.
    WaitForEvent {
        mask: u32,
        deadline: Option<u64>,
    },
    /// 249: synchronize an owned Grant subrange before device submission.
    GrantCacheSyncBegin {
        grant_id: usize,
        offset: usize,
        len: usize,
    },
    /// 250: synchronize device output and release one exact Grant pin.
    GrantCacheSyncComplete {
        token: usize,
    },
    /// 251: validate and expose a firmware-owned display framebuffer.
    RegisterDisplayFramebuffer {
        base: usize,
        size: usize,
        packed_dimensions: usize,
        pitch: usize,
    },
    /// 242: WaitCompletion — reserve a slot on the caller's completion queue for
    /// the source named by `mask`, wait for it to be filled, and write the
    /// result to `out_ptr`. `deadline = None` waits indefinitely.
    ///
    /// Carries a deadline for the same reason `WaitForEvent` does: its caller
    /// has maintenance work that must run on a timer even when no frame arrives.
    WaitCompletion {
        mask: u32,
        deadline: Option<u64>,
        out_ptr: usize,
    },

    // === Hypervisor (220-225) — HypervisorCap ZST-gated ===
    /// 220: CreateVm — allocate guest RAM + Stage-2 table → vm_id.
    CreateVm {
        guest_pages: usize,
    },
    /// 221: CreateVcpu — create a vCPU with `entry_pc` in `vm_id` → vcpu_id.
    CreateVcpu {
        vm_id: usize,
        entry_pc: u64,
    },
    /// 222: MapGuestMemory — map guest IPA range in `vm_id` Stage-2 table.
    MapGuestMemory {
        vm_id: usize,
        ipa: u64,
        size: usize,
        writable: bool,
    },
    /// 223: RunVcpu — world-switch into vCPU; write `ViVmExit` to `out_ptr`.
    RunVcpu {
        vm_id: usize,
        vcpu_id: usize,
        budget_ns: u64,
        out_ptr: usize,
    },
    /// 224: VcpuRegs — mode 0 reads and 1 writes 32 GP words; Pi mode 2
    /// reads CNTV_CTL_EL0/CNTV_CVAL_EL0 in words 0/1 and zeroes words 2..31.
    VcpuRegs {
        vm_id: usize,
        vcpu_id: usize,
        buf_ptr: usize,
        mode: usize,
    },
    /// 225: InjectIrq — inject GICv2 virtual interrupt (0 ≤ intid ≤ 1019).
    InjectIrq {
        vm_id: usize,
        vcpu_id: usize,
        intid: u32,
    },
    /// 226: WriteGuestMemory — copy `len` bytes from `src_ptr` to guest GPA.
    WriteGuestMemory {
        vm_id: usize,
        gpa: u64,
        src_ptr: usize,
        len: usize,
    },
    /// 227: ReadGuestMemory — copy `len` bytes from guest GPA into `dst_ptr`.
    ReadGuestMemory {
        vm_id: usize,
        gpa: u64,
        dst_ptr: usize,
        len: usize,
    },
    /// 234: WaitIrq — block until hardware IRQ `irq` fires (Driver Cell).
    /// ISR sets IRQ_PENDING[irq] (atomic, no lock); scheduler sweep wakes the task.
    /// `mmio_base`: VirtIO MMIO slot base for InterruptACK write, or 0 for non-VirtIO.
    WaitIrq {
        irq: u8,
        mmio_base: usize,
    },
    /// 235: RegisterPcieBar — Platform Cell announces a discovered PCIe BAR.
    /// Populates the kernel BAR allowlist used by `sys_request_mmio`.
    /// Requires singleton PlatformCap.
    RegisterPcieBar {
        bdf: u32,
        base: usize,
        len: usize,
    },
    /// 236: RegisterPciDevice — Platform Cell announces a PCI device with class/BAR info.
    /// Populates kernel PCI_DEVICES so find_class() works without a kernel ECAM scan.
    /// Requires singleton PlatformCap (allowlist bit 53).
    RegisterPciDevice {
        bdf: u32,
        cls: u32,
        bar0_base: usize,
        bar0_size: usize,
    },
    /// 237: ReadLog — drain up to `max` bytes from the kernel user-log ring.
    /// ABI: a0 = buf_ptr, a1 = max → bytes_copied.
    /// Gated by allowlist bit 54 (ReadLog).
    ReadLog {
        buf_ptr: usize,
        max: usize,
        source: usize,
    },
    /// ViSyscall 9: SetTlsBase — set the caller's own user thread pointer and
    /// return the previous value. Self-only (no target parameter) and always
    /// permitted: it carries no authority.
    SetTlsBase {
        base: usize,
    },
    /// ViSyscall 19: PipeCreate — create a pipe and write both endpoint handles
    /// into the caller's 2-word buffer.
    PipeCreate {
        capacity: usize,
        out_ptr: usize,
    },
    /// ViSyscall 22: PipeRead — read up to `len` bytes, blocking (with an optional
    /// deadline) while the ring is empty and a writer end is still open.
    PipeRead {
        handle: usize,
        buf_ptr: usize,
        len: usize,
        timeout_ticks: u64,
    },
    /// ViSyscall 23: PipeWrite — write up to `len` bytes, blocking while the ring
    /// is full. A zero-length write closes the writer end.
    PipeWrite {
        handle: usize,
        buf_ptr: usize,
        len: usize,
        timeout_ticks: u64,
    },
    /// ViSyscall 24: PipeClose — close one endpoint this task owns.
    PipeClose {
        handle: usize,
    },
    /// ViSyscall 25: PipeShare — duplicate an endpoint into another task's table.
    /// Owner-only: a task cannot share an endpoint it does not hold.
    PipeShare {
        handle: usize,
        target_tid: usize,
    },
    /// ViSyscall 440: AnonMemory — manage Tier-2 large anonymous memory.
    AnonMemory {
        op: usize,
        addr: usize,
        len: usize,
        flags: usize,
    },
}

/// Map a syscall ID + promoted register args to the internal [`Syscall`] enum.
///
/// All register values must already be promoted to `usize` by the caller.
/// Returns `None` for unknown/unhandled opcodes; the caller writes the
/// arch-appropriate sentinel (usize::MAX or u32::MAX) to the return register.
pub(super) fn map_syscall(
    syscall_id: usize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
) -> Option<Syscall> {
    let sc = match ViSyscall::from(syscall_id) {
        ViSyscall::IpcSubmit => Syscall::IpcSubmit {
            target: a0,
            ptr: a1,
            len: a2,
        },
        ViSyscall::IpcTake => Syscall::IpcTake {
            token: a0,
            ptr: a1,
            len: a2,
            status_ptr: a3,
        },
        ViSyscall::IpcWait => Syscall::IpcWait {
            ticks: (a0 as u64) | ((a1 as u64) << 32),
        },
        ViSyscall::IpcCancel => Syscall::IpcCancel { token: a0 },
        ViSyscall::IpcCurrent => Syscall::IpcCurrent,
        ViSyscall::IpcReply => Syscall::IpcReply {
            token: a0,
            ptr: a1,
            len: a2,
        },
        ViSyscall::Send => Syscall::Send {
            target: a0,
            msg_ptr: a1,
            msg_len: a2,
        },
        ViSyscall::TrySend => Syscall::TrySend {
            target: a0,
            msg_ptr: a1,
            msg_len: a2,
        },
        ViSyscall::Recv => match abi::caller_identity::recv_timeout_ticks(a3) {
            0 => Syscall::Recv {
                mask: a0,
                buf_ptr: a1,
                buf_len: a2,
                attest_caller: a3 & abi::caller_identity::RECV_ATTEST_CALLER != 0,
            },
            ticks => Syscall::RecvTimeout {
                mask: a0,
                buf_ptr: a1,
                buf_len: a2,
                deadline: (crate::task::system_ticks() as u64).wrapping_add(ticks),
                attest_caller: a3 & abi::caller_identity::RECV_ATTEST_CALLER != 0,
                timeout_is_error: true,
            },
        },
        ViSyscall::TryRecv => Syscall::TryRecv {
            mask: a0,
            buf_ptr: a1,
            buf_len: a2,
            attest_caller: a3 & abi::caller_identity::RECV_ATTEST_CALLER != 0,
        },
        ViSyscall::SendGather => Syscall::SendGather {
            target: a0,
            iovec_ptr: a1,
            iovec_count: a2,
        },
        ViSyscall::RecvScatter => Syscall::RecvScatter {
            mask: a0,
            iovec_ptr: a1,
            iovec_count: a2,
        },
        ViSyscall::RecvTimeout => Syscall::RecvTimeout {
            mask: a0,
            buf_ptr: a1,
            buf_len: a2,
            deadline: (crate::task::system_ticks() as u64).wrapping_add(a3 as u64),
            attest_caller: false,
            timeout_is_error: false,
        },
        ViSyscall::CellSessionV1 => Syscall::CellSessionV1 {
            request_ptr: a0,
            request_len: a1,
            result_ptr: a2,
            result_capacity: a3,
        },
        ViSyscall::GetCpuCount => Syscall::GetCpuCount,
        ViSyscall::Spawn => Syscall::Spawn { entry: a0, arg: a1 },
        ViSyscall::SpawnFromMem => Syscall::SpawnFromMem { args_ptr: a0 },
        ViSyscall::SpawnFromPath => Syscall::SpawnFromPath {
            path_ptr: a0,
            path_len: a1,
        },
        ViSyscall::SpawnFromElf => Syscall::SpawnFromElf {
            grant_id: a0,
            len: a1,
            path_ptr: a2,
            path_len: a3,
        },
        ViSyscall::SpawnPinned => Syscall::SpawnPinned {
            path_ptr: a0,
            path_len: a1,
            priority: a2 as u8,
            core_id: a3,
        },
        ViSyscall::ResolveCellOwnerRecord => Syscall::ResolveCellOwnerRecord {
            request_ptr: a0,
            request_len: a1,
            out_ptr: a2,
            out_len: a3,
        },
        ViSyscall::WatchCellOwnerRecord => Syscall::WatchCellOwnerRecord {
            request_ptr: a0,
            request_len: a1,
            out_ptr: a2,
            out_len: a3,
        },
        ViSyscall::ResolveCellImage => Syscall::ResolveCellImage {
            request_ptr: a0,
            request_len: a1,
            out_ptr: a2,
            out_len: a3,
        },
        ViSyscall::SpawnSetDirs => Syscall::SpawnSetDirs { carrier_ptr: a0 },
        ViSyscall::QueryDirHandles => Syscall::QueryDirHandles {
            cell_id: a0 as u64,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::ResolveCellOwner => Syscall::ResolveCellOwner {
            cell_id: a0 as u64,
            generation: a1 as u64,
            out_ptr: a2,
            out_len: a3,
        },
        ViSyscall::WatchCellOwner => Syscall::WatchCellOwner {
            cell_id: a0 as u64,
            generation: a1 as u64,
            out_ptr: a2,
            out_len: a3,
        },
        ViSyscall::CancelCellOwnerWatch => Syscall::CancelCellOwnerWatch { token: a0 as u64 },
        ViSyscall::OpenCap => Syscall::OpenCap {
            path_ptr: a0,
            path_len: a1,
        },
        ViSyscall::ReadCap => Syscall::ReadCap {
            cap_id: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::CloseCap => Syscall::CloseCap { cap_id: a0 },
        ViSyscall::SeekCap => Syscall::SeekCap {
            cap_id: a0,
            offset: a1,
            whence: a2,
        },
        ViSyscall::WriteCap => Syscall::WriteCap {
            cap_id: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::StatCap => Syscall::StatCap { cap_id: a0 },
        ViSyscall::TruncateCap => Syscall::TruncateCap {
            cap_id: a0,
            len: a1,
        },
        ViSyscall::SyncCap => Syscall::SyncCap { cap_id: a0 },
        ViSyscall::GrantDma => Syscall::GrantDma {
            bdf: a0 as u32,
            phys: a1 as u64,
            size: a2,
        },
        ViSyscall::Wait => Syscall::Wait { pid: a0 },
        ViSyscall::SetTlsBase => Syscall::SetTlsBase { base: a0 },
        ViSyscall::FutexWait => Syscall::FutexWait {
            addr: a0,
            expected: a1 as u32,
            timeout_ticks: a2 as u64,
        },
        ViSyscall::FutexWake => Syscall::FutexWake {
            addr: a0,
            count: a1,
        },
        ViSyscall::PipeCreate => Syscall::PipeCreate {
            capacity: a0,
            out_ptr: a1,
        },
        ViSyscall::PipeRead => Syscall::PipeRead {
            handle: a0,
            buf_ptr: a1,
            len: a2,
            timeout_ticks: a3 as u64,
        },
        ViSyscall::PipeWrite => Syscall::PipeWrite {
            handle: a0,
            buf_ptr: a1,
            len: a2,
            timeout_ticks: a3 as u64,
        },
        ViSyscall::PipeClose => Syscall::PipeClose { handle: a0 },
        ViSyscall::PipeShare => Syscall::PipeShare {
            handle: a0,
            target_tid: a1,
        },
        ViSyscall::ShmAlloc => Syscall::ShmAlloc { size: a0 },
        ViSyscall::ShmMap => Syscall::ShmMap {
            handle: a0,
            target_pid: a1,
        },
        ViSyscall::Exit => Syscall::Exit { code: a0 },
        ViSyscall::ForceExit => Syscall::ForceExit { tid: a0 },
        ViSyscall::NotifyOnExit => Syscall::NotifyOnExit { watched: a0 },
        ViSyscall::RegisterService => Syscall::RegisterService {
            service_id: a0 as u16,
            tid: a1,
            flags: a2,
        },
        ViSyscall::LookupServiceBound => Syscall::LookupServiceBound {
            service_id: a0 as u16,
            out_ptr: a1,
            out_len: a2,
        },
        ViSyscall::Heartbeat => Syscall::Heartbeat { interval: a0 },
        ViSyscall::Yield => Syscall::Yield,
        ViSyscall::SetTimer => Syscall::SetTimer { deadline: a0 },
        ViSyscall::Log => Syscall::Log {
            msg_ptr: a0,
            msg_len: a1,
        },
        ViSyscall::GetProcs => Syscall::GetProcs {
            buf_ptr: a0,
            buf_len: a1,
        },
        ViSyscall::GetProcs2 => Syscall::GetProcs2 {
            buf_ptr: a0,
            buf_len: a1,
        },
        ViSyscall::MemInfo => Syscall::MemInfo {
            out_ptr: a0,
            out_len: a1,
        },
        ViSyscall::TelemetrySnapshot => Syscall::TelemetrySnapshot {
            out_ptr: a0,
            out_len: a1,
            flags: a2,
            reserved: a3,
        },
        ViSyscall::QueryRetirement => Syscall::QueryRetirement {
            root_tid: a0,
            out_ptr: a1,
            out_len: a2,
        },
        ViSyscall::LookupBlockRoute => Syscall::LookupBlockRoute {
            index: a0,
            out_ptr: a1,
            out_len: a2,
            reserved: a3,
        },
        ViSyscall::QueryMachineFacts => Syscall::QueryMachineFacts {
            out_ptr: a0,
            out_len: a1,
            a2,
            a3,
        },
        ViSyscall::QueryPcieFunction => Syscall::QueryPcieFunction {
            bdf: a0,
            out_ptr: a1,
            out_len: a2,
            reserved: a3,
        },
        ViSyscall::AllocPcieIrq => Syscall::AllocPcieIrq {
            bdf: a0,
            want: a1,
            mode: a2,
            reserved: a3,
        },
        ViSyscall::SystemPower => Syscall::SystemPower {
            action: a0,
            a1,
            a2,
            a3,
        },
        ViSyscall::ExportDmaRange => Syscall::ExportDmaRange {
            va: a0,
            len: a1,
            holder_service: a2,
            flags: a3,
        },
        ViSyscall::MapDmaExport => Syscall::MapDmaExport {
            export_id: a0,
            bdf: a1,
            out_ptr: a2,
            out_len: a3,
        },
        ViSyscall::RevokeDmaExport => Syscall::RevokeDmaExport {
            export_id: a0,
            a1,
            a2,
            a3,
        },
        ViSyscall::Open => Syscall::Open {
            path_ptr: a0,
            path_len: a1,
        },
        ViSyscall::Read => Syscall::Read {
            fd: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::Close => Syscall::Close { fd: a0 },
        ViSyscall::ReadDir => Syscall::ReadDir {
            fd: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::Write => Syscall::Write {
            fd: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::Seek => Syscall::Seek {
            fd: a0,
            offset: a1 as isize,
            whence: a2,
        },
        ViSyscall::Fstat => Syscall::Fstat {
            fd: a0,
            out_ptr: a1,
            out_len: a2,
        },
        ViSyscall::Rename => Syscall::Rename {
            old_ptr: a0,
            old_len: a1,
            new_ptr: a2,
            new_len: a3,
        },
        ViSyscall::FileOp => Syscall::FileOp {
            op: a0,
            arg1: a1,
            arg2: a2,
        },
        ViSyscall::GetTime => Syscall::GetTime { op: a0 },
        ViSyscall::GpuFlush => Syscall::GpuFlush {
            data_ptr: a0,
            data_len: a1,
            xy: a2,
            wh: a3,
        },
        ViSyscall::CapRevoke => Syscall::CapRevoke {
            target_tid: a0,
            cap_mask: a1 as u32,
        },
        ViSyscall::GpuCursor => Syscall::GpuCursor {
            op: a0,
            data_ptr: a1,
            xy: a2,
            hot: a3,
        },
        ViSyscall::GpuGetResolution => Syscall::GpuGetResolution,
        ViSyscall::StateStash => Syscall::StateStash {
            key: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::StateRestore => Syscall::StateRestore {
            key: a0,
            buf_ptr: a1,
            buf_len: a2,
        },
        ViSyscall::StateStashClear => Syscall::StateStashClear { key: a0 },
        ViSyscall::FreezeCell => Syscall::FreezeCell { target_tid: a0 },
        ViSyscall::ResumeCell => Syscall::ResumeCell {
            target_tid: a0,
            source_tid: a1,
            service_id: a2,
            reserved: a3,
        },
        ViSyscall::KillCell => Syscall::KillCell {
            target_tid: a0,
            exit_code: a1 as u32,
        },
        ViSyscall::RegisterBlockDriver => Syscall::RegisterBlockDriver {
            kind: a0,
            id: a1,
            reserved: (a2, a3),
        },
        ViSyscall::RegisterNicDriver => Syscall::RegisterNicDriver,
        ViSyscall::RegisterUsbHidProducer => Syscall::RegisterUsbHidProducer,
        ViSyscall::FindPcieDevice => Syscall::FindPcieDevice {
            class: a0 as u8,
            subclass: a1 as u8,
            prog_if: a2 as u8,
            out_ptr: a3,
        },
        ViSyscall::FindPcieDeviceByVendor => Syscall::FindPcieDeviceByVendor {
            vendor_id: a0 as u16,
            device_id: a1 as u16,
            out_ptr: a2,
        },
        ViSyscall::SerialPortInfo => Syscall::SerialPortInfo {
            index: a0 as u32,
            out_ptr: a1,
        },
        ViSyscall::SerialWrite => Syscall::SerialWrite {
            index: a0 as u32,
            ptr: a1,
            len: a2,
        },
        ViSyscall::SerialRead => Syscall::SerialRead {
            index: a0 as u32,
            ptr: a1,
            len: a2,
        },
        ViSyscall::SerialConfigure => Syscall::SerialConfigure {
            index: a0 as u32,
            baud: a1 as u32,
        },
        ViSyscall::QueryHotswapReady => Syscall::QueryHotswapReady { target_tid: a0 },
        ViSyscall::SpawnReplacement => Syscall::SpawnReplacement {
            old_tid: a0,
            path_ptr: a1,
            path_len: a2,
        },
        ViSyscall::PauseService => Syscall::PauseService {
            service_id: a0 as u16,
            expected_tid: a1,
        },
        ViSyscall::HotSwapReady => Syscall::HotSwapReady,
        ViSyscall::Snapshot => Syscall::Snapshot,
        ViSyscall::GrantAlloc => Syscall::GrantAlloc { size: a0 },
        ViSyscall::GrantShare => Syscall::GrantShare {
            grant_id: a0,
            target_cell: a1,
            perm: a2,
        },
        ViSyscall::GrantSlice => Syscall::GrantSlice {
            grant_id: a0,
            size_out_ptr: a1,
            expected_owner_tid: a2,
            flags: a3,
        },
        ViSyscall::GrantFree => Syscall::GrantFree { grant_id: a0 },
        ViSyscall::GrantCacheSyncBegin => Syscall::GrantCacheSyncBegin {
            grant_id: a0,
            offset: a1,
            len: a2,
        },
        ViSyscall::GrantCacheSyncComplete => Syscall::GrantCacheSyncComplete { token: a0 },
        ViSyscall::RegisterDisplayFramebuffer => Syscall::RegisterDisplayFramebuffer {
            base: a0,
            size: a1,
            packed_dimensions: a2,
            pitch: a3,
        },
        ViSyscall::BlkReadAsync => Syscall::BlkReadAsync {
            sector: a0 as u64,
            grant_id: a1,
        },
        ViSyscall::RequestMmio => Syscall::RequestMmio { base: a0, len: a1 },
        ViSyscall::ProbeVirtioMmio => Syscall::ProbeVirtioMmio {
            base: a0,
            reserved: (a1, a2, a3),
        },
        ViSyscall::GetRandom => Syscall::GetRandom {
            buf_ptr: a0,
            len: a1,
        },
        ViSyscall::GrantRegister => Syscall::GrantRegister { size: a0 },
        ViSyscall::GrantUnregister => Syscall::GrantUnregister { reg_id: a0 },
        ViSyscall::WaitForEvent => {
            // ABI: a0 = mask (u32), a1 = timeout_ticks_lo, a2 = timeout_ticks_hi.
            let mask = a0 as u32;
            let timeout = (a1 as u64) | ((a2 as u64) << 32);
            let deadline = if timeout == 0 {
                None
            } else {
                Some((crate::task::system_ticks() as u64).wrapping_add(timeout))
            };
            Syscall::WaitForEvent { mask, deadline }
        }
        ViSyscall::WaitCompletion => {
            // ABI: a0 = source mask (u32), a1 = timeout_ticks_lo,
            // a2 = timeout_ticks_hi, a3 = pointer to the result record.
            // Bits 32..63 of a0 are reserved and must be zero: a mask that
            // does not fit becomes `u32::MAX`, which the source rule refuses
            // with InvalidInput instead of silently dropping the high bits.
            let mask = u32::try_from(a0).unwrap_or(u32::MAX);
            let timeout = (a1 as u64) | ((a2 as u64) << 32);
            let deadline = if timeout == 0 {
                None
            } else {
                Some((crate::task::system_ticks() as u64).wrapping_add(timeout))
            };
            Syscall::WaitCompletion {
                mask,
                deadline,
                out_ptr: a3,
            }
        }
        // Hypervisor syscalls 220-225.
        ViSyscall::CreateVm => Syscall::CreateVm { guest_pages: a0 },
        ViSyscall::CreateVcpu => Syscall::CreateVcpu {
            vm_id: a0,
            entry_pc: a1 as u64,
        },
        ViSyscall::MapGuestMemory => Syscall::MapGuestMemory {
            vm_id: a0,
            ipa: a1 as u64,
            size: a2,
            writable: a3 != 0,
        },
        ViSyscall::RunVcpu => Syscall::RunVcpu {
            vm_id: a0,
            vcpu_id: a1,
            budget_ns: a2 as u64,
            out_ptr: a3,
        },
        ViSyscall::VcpuRegs => Syscall::VcpuRegs {
            vm_id: a0,
            vcpu_id: a1,
            buf_ptr: a2,
            mode: a3,
        },
        ViSyscall::InjectIrq => Syscall::InjectIrq {
            vm_id: a0,
            vcpu_id: a1,
            intid: a2 as u32,
        },
        ViSyscall::WriteGuestMemory => Syscall::WriteGuestMemory {
            vm_id: a0,
            gpa: a1 as u64,
            src_ptr: a2,
            len: a3,
        },
        ViSyscall::ReadGuestMemory => Syscall::ReadGuestMemory {
            vm_id: a0,
            gpa: a1 as u64,
            dst_ptr: a2,
            len: a3,
        },
        ViSyscall::WaitIrq => Syscall::WaitIrq {
            irq: a0 as u8,
            mmio_base: a1,
        },
        ViSyscall::RegisterPcieBar => Syscall::RegisterPcieBar {
            bdf: a0 as u32,
            base: a1,
            len: a2,
        },
        ViSyscall::RegisterPciDevice => Syscall::RegisterPciDevice {
            bdf: a0 as u32,
            cls: a1 as u32,
            bar0_base: a2,
            bar0_size: a3,
        },
        ViSyscall::ReadLog => Syscall::ReadLog {
            buf_ptr: a0,
            max: a1,
            source: a2,
        },
        ViSyscall::Chdir => Syscall::ChDir {
            path_ptr: a0,
            path_len: a1,
        },
        ViSyscall::Getcwd => Syscall::GetCwd {
            buf_ptr: a0,
            buf_len: a1,
        },
        ViSyscall::AnonMemory => Syscall::AnonMemory {
            op: a0,
            addr: a1,
            len: a2,
            flags: a3,
        },
        _ => match syscall_id {
            100 => Syscall::ServiceLookup {
                name_ptr: a0,
                name_len: a1,
            },
            110 => Syscall::MkDir {
                path_ptr: a0,
                path_len: a1,
            },
            111 => Syscall::Create {
                path_ptr: a0,
                path_len: a1,
            },
            // Block I/O — intentionally absent from ViSyscall/libs/abi (avoids Law 1).
            255 => Syscall::Rename {
                old_ptr: a0,
                old_len: a1,
                new_ptr: a2,
                new_len: a3,
            },
            500 => Syscall::BlkRead {
                sector: a0 as u64,
                buf_ptr: a1,
            },
            501 => Syscall::BlkWrite {
                sector: a0 as u64,
                buf_ptr: a1,
            },
            502 => Syscall::Shutdown,
            503 => Syscall::BlkFlush,
            #[cfg(feature = "bench-counters")]
            crate::bench_counters::OPCODE => Syscall::BenchCounters {
                out_ptr: a0,
                out_len: a1,
            },
            _ => return None,
        },
    };
    Some(sc)
}
