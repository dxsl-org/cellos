## Slice: KernelSyscall — files read: 19 — unsafe blocks reviewed: 31

Handlers covered (all arms of `handle_syscall_inner` + `map_syscall` + `check_allowlist`): Send/TrySend/Recv/TryRecv/RecvTimeout/SendGather/RecvScatter/Reply/Lend/BorrowRead/BorrowWrite/Grant/Map (IPC core bodies in `ipc*`/`async_ipc` are KernelIpcSched's), IpcSubmit/Take/Wait/Cancel/Current/Reply, Spawn, SetTlsBase(9), FutexWait/Wake(17/18), PipeCreate/Read/Write/Close/Share(19/22–25), Wait, ShmAlloc/ShmMap(20/21), Log, Exit, ForceExit(61), CapRevoke(219), NotifyOnExit(204), QueryRetirement(431), LookupBlockRoute(432), QueryMachineFacts(433), RegisterService/LookupService/LookupServiceBound(205/206/429), Heartbeat(207), SetTimer(3), GetProcs/GetProcs2/MemInfo/TelemetrySnapshot(30/239/243/430), Open/Read/Close/ReadDir/Fstat/Rename/ChDir/GetCwd/Write/MkDir/Cr…

### Findings

#### [S-1] SpawnFromElf verifies the signature over caller-mutable, unpinned grant memory (verify-then-load TOCTOU) — Severity: High — STRIDE: T/E — OWASP: A08
- Where: kernel/src/task/syscall.rs:7503-7595 (esp. 7513-7523, 7545, 7566); kernel/src/task/syscall.rs:2182-2205 (`session_elf_bytes`, same pattern for CellSessionV1 route 2); kernel/src/loader/aligned_elf.rs:23-26 (page-aligned input → `Borrowed`, no copy); kernel/src/loader/governed_spawn.rs:42-60
- What: The grant base is read under the table lock, the lock is dropped, then `from_raw_parts(base, len)` is handed to `spawn_gated`. Because grant bases are page-aligned, `aligned_elf::bytes` borrows instead of copying, so SHA-256, Ed25519 `verify_cell`, manifest classification and segment loading all read the live USER-RW grant pages. Nothing pins the grant (`refuse_if_pinned` sees nothing), so a concurrent `GrantFree` by another thread of the caller even returns the frames to the allocator mid-load (free_grant_pages restores a kernel RWX identity mapping, so the read does not fault).
- Exploit: A caller authorized for the Elf launch edge (shell/desktop/supervisor rows in launch_profile) allocates a grant, lets a second thread (or a `GrantShare` grantee with ReadWrite) rewrite the ELF bytes after `verify_cell` returns `Ok(true)` but before `load_segments` copies them; the modified (unsafe/unsigned) image is admitted with the signed cell's tier and capability ceiling. On SMP the window is the whole hash+verify+parse duration.
- Fix: Copy the ELF into a kernel-owned buffer (`try_reserve_exact` + `read_into` via the copy view, as `SpawnFromMem` already does at 8047-8061) before hashing, or pin the grant for the duration and verify/load from a snapshot. Same for `session_elf_bytes`.

#### [S-2] SAS user-copy output paths race GrantFree/GrantUnregister/Stack::drop: kernel writes into freed frames — Severity: High — STRIDE: T/E — OWASP: A04
- Where: kernel/src/task/user_copy/copy.rs:86-104, 164-183 (probe then `commit_sas` with no lease); kernel/src/task/user_copy/sv39_probe.rs:231-248; kernel/src/task/syscall.rs:3072-3085 (`write_user_slice`), 9515-9565 (`GrantFree`), 996-1041 (`free_grant_pages_inner`: unmap → remap kernel RWX → `deallocate_frame`); kernel/src/task/stack.rs:67-79; contrast kernel/src/task/syscall.rs:2992-3038 (`write_getrandom_output` holds PAGE/REG grant tables + SCHEDULER across the write — the only output path that does)
- What: `probe_sas` checks the destination pages are VALID|USER|WRITE, then `commit_sas` byte-copies through the VA. If another hart frees the backing (GrantFree of a grant the caller owns or that is shared to it, or exit of a sibling thread dropping its user stack) between probe and commit, the frame is remapped kernel-RWX (not unmapped) and returned to the allocator. The guarded copy does not fault (S-mode write to a kernel page) and continues writing user/sender-controlled bytes into a frame that may already be reallocated as a page table, another cell's stack/segment, or grant backing. The comment at sv39_probe.rs:244-246 claims the race becomes a recoverable error; it does not because the mapping is restored rather than removed.
- Exploit: Thread A: `Recv`/`ReadCap`/`Read`/`NetRx`/`PipeRead`/`GetProcs` with `buf_ptr` in a 16 MiB grant (copy takes ms, byte loop). Thread B (same cell, or the grant owner in another cell for a shared grant): `GrantFree`. Thread C: `GrantAlloc`/`Spawn` to get the freed frames reallocated as page tables/stacks; A's copy lands in them. Payload is attacker-chosen (a colluding IPC sender).
- Fix: Generalize the GetRandom lease: every SAS output copy must hold the grant-table/SCHEDULER lease (`sas_caller_owned_span` + write under lock) or pin the span (`SessionOutputPin`) across commit; alternatively, `free_grant_pages`/`release_frames` should leave the frame unmapped until a cross-hart shootdown completes so the guard actually faults.

#### [S-3] PlatformCap `RequestMmio` bypass user-maps arbitrary physical ranges, including kernel RAM — Severity: High — STRIDE: E — OWASP: A01
- Where: kernel/src/task/syscall.rs:9889-9940 (9930-9938); kernel/src/resource_registry.rs:380-412 (`request_mmio_unchecked`: only overlap check); kernel/src/memory/paging.rs:706-708, 815-836 (`map_mmio_user_x86` / `grant_mmio_user`: no RAM/kernel-range refusal)
- What: With `platform_cap`, any `(base,len)` passes `request_mmio_unchecked` (no allowlist, no RAM-range check) and is then mapped USER (x86: new identity mapping with PTE_USER; riscv/aarch64: `protect_page` sets USER on whatever is already mapped, i.e. the identity-mapped RAM incl. kernel text/data/page tables).
- Exploit: `RequestMmio(kernel_image_phys, len)` from the Platform Cell → kernel text/data become user-RW → full kernel compromise from a userspace driver cell. The cap is meant only to claim the ECAM window.
- Fix: Restrict the bypass to ranges that are not managed RAM (reuse the `FRAME_ALLOCATOR.managed_ranges()` overlap test from `RegisterDisplayFramebuffer`, 9676-9686) and not the kernel image; ideally an explicit ECAM allowlist from the board profile.

#### [S-4] Hypervisor syscalls dereference raw user pointers (no copy boundary): HypervisorCap cell gets arbitrary kernel read/write — Severity: High — STRIDE: T/I/E — OWASP: A01
- Where: kernel/src/task/syscall.rs:10116-10138 (`RunVcpu`), 10140-10163 (`VcpuRegs`), 10180-10212 (`Write/ReadGuestMemory`); kernel/src/hypervisor/registry.rs:521-524, 558-561 (`ptr::write(exit_out)`), 591 (`from_raw_parts_mut(buf_ptr, 32)`), 671-672, 729-730 (`copy_nonoverlapping` to/from user ptr)
- What: Only `validate_user_buf` (null/len/overflow) runs; no U-bit probe, no `TaskCopyView`. SUM is set for the whole syscall (11090-11093), so a kernel address passed as `out_ptr`/`buf_ptr`/`dst_ptr` is written directly; `src_ptr` lets kernel memory be copied into guest RAM and read back via `ReadGuestMemory`.
- Exploit: Hypervisor cell calls `ReadGuestMemory(vm, gpa, dst_ptr=<kernel addr>, len)` with guest-controlled bytes → arbitrary kernel write; `WriteGuestMemory(src_ptr=<kernel addr>)` → kernel memory disclosure.
- Fix: Route these through `caller_copy_view` (`read_into`/`write_bytes`) with kernel-side staging buffers, like every other handler.

#### [S-5] Domain grant `publish` races `revoke`: receiver root keeps a PTE to freed frames — Severity: High — STRIDE: E — OWASP: A04
- Where: kernel/src/task/domain_grant.rs:148-212 (`publish`: state checked at 154, receiver mapped at 177-200, no lock held across), 277-302 (`revoke`: CAS Live→Revoking, drain, store Revoked); kernel/src/task/syscall.rs:3736-3793 and 3796-3838 (publish called under the grant table lock), 9515-9565 and 1472-1491 (revoke deliberately run *after* the table lock is released, then `free_grant_pages`)
- What: `publish` has no synchronization with `revoke` beyond a non-atomic state read. Interleaving: receiver's `GrantSlice` reads `state()==Live`; owner's `GrantFree` (outside the table lock) CASes Revoking, drains the (old/absent) receiver, drains owner, stores Revoked, returns `true`, row removed, frames freed; the receiver's `publish` then maps the now-free frames into its private root. [INFERENCE] practical window is the drain + shootdown duration vs. the receiver's check→map gap; both run on different harts.
- Exploit: Two Tier-2 (domain) cells or one domain cell with two threads race `GrantSlice` against `GrantFree`; the receiver keeps a RW mapping to memory the allocator reuses (page tables, other cells).
- Fix: Hold a per-record mutex (or the receiver `Spinlock`) across the state check + map loop in `publish` and across the Live→Revoking transition in `revoke`; or move the CAS into the table-locked section so `publish` under the lock always observes Revoking.

#### [S-6] ShmAlloc hands out unzeroed frames and ShmMap maps them USER; no quota — Severity: Medium — STRIDE: I/D — OWASP: A02
- Where: kernel/src/task/syscall.rs:6442-6460, 6461-6497; kernel/src/memory/frame.rs:156-175 (`allocate_frame` does not zero); kernel/src/task/stack.rs:67-79 (freed stacks not zeroed)
- What: `ShmAlloc` takes a raw frame from the global allocator without zeroing or quota charge; `ShmMap` identity-maps it RW+USER. The frame may previously have been another cell's user stack, ELF segment, or kernel structure. `size` is ignored and nothing bounds the number of calls.
- Exploit: Loop `ShmAlloc`+`ShmMap` and scan for secrets (dead cell stacks, previous grant data, KMS keys if they were on a stack); or loop `ShmAlloc` to exhaust physical memory (frames are not charged to the cell's quota, contrary to docs/security-model.md "Cell allocates unbounded memory — mitigated").
- Fix: Zero the frame before `shm_register` (as `alloc_grant_pages` does at 955-959), charge `cell_quota`, and cap outstanding handles per incarnation.

#### [S-7] GrantAlloc/GrantRegister are not charged to the cell quota — Severity: Medium — STRIDE: D — OWASP: A04
- Where: kernel/src/task/syscall.rs:9283-9378, 9759-9887, 900-962 (`alloc_grant_pages`: only `FRAME_ALLOCATOR`, no `cell_quota`)
- What: Each call takes up to `MAX_GRANT_PAGES` (16 MiB) of contiguous frames; the per-call cap is the only limit. Heap is quota-charged (heap.rs:171-172) but grant frames are not.
- Exploit: Any cell with GrantAlloc/GrantRegister bits loops until `allocate_contiguous` fails, starving stacks/segments for every other spawn (OOM returned as `Ok(0)`, caller is not killed).
- Fix: `cell_quota::charge` the page bytes on alloc and refund on free/reap (the DMA path already does this via `try_reserve_dma`).

#### [S-8] x86_64 `SetTlsBase` writes a user value to IA32_FS_BASE via WRMSR: non-canonical base → ring-0 #GP — Severity: Medium — STRIDE: D — OWASP: A04
- Where: kernel/src/task/tls.rs:32-37, 99-116; kernel/src/task/syscall.rs:6143-6164 (no validation; always-permitted syscall)
- What: WRMSR to IA32_FS_BASE with a non-canonical value raises #GP in the kernel; the same value is re-installed on every resume (`install_published_base`). No canonical check anywhere.
- Exploit: Any cell: `SetTlsBase(0x8000_0000_0000_0000)` → kernel fault on x86_64 (fatal, panic=abort).
- Fix: On x86_64 reject non-canonical (bits 63:47 not sign-extended) bases before storing/writing.

#### [S-9] BlkReadAsync DMAs into a grant it does not pin — Severity: Medium — STRIDE: T — OWASP: A04
- Where: kernel/src/task/syscall.rs:10052-10080 (base read under lock at 10060-10066, lock dropped, `from_raw_parts_mut` + `read_sector` at 10075-10076)
- What: Unlike `GrantDma` (pins via `reserve_caller_owned_dma_range`) and VFS `GrantSlice` leases, this path holds no pin, so `GrantFree` of the same grant by another VFS thread succeeds mid-read and the device/bounce write lands in a freed frame.
- Exploit: Requires BlockIoCap (the VFS); a compromised/buggy VFS corrupts arbitrary reallocated frames.
- Fix: `pin` the 512-byte span before dropping the table lock and release after `read_sector` returns (or refuse if pinned-free raced).

#### [S-10] Pipes: `close_end` never wakes parked peers; park-after-check lost wakeup — Severity: Medium — STRIDE: D(-) — OWASP: A04
- Where: kernel/src/task/pipe.rs:248-275 (`close_end` adjusts counts, removes own waiter, wakes nobody; only `on_task_leaves` 376-425 wakes); kernel/src/task/syscall.rs:6238-6262 (PipeRead: `try_read` → Empty, PIPES released, then park under SCHEDULER), 6318-6341 (PipeWrite same); kernel/src/task/scheduler.rs:2260-2273 (only deadline wakes)
- What: (a) A reader blocked with `timeout_ticks=0` never observes EOF when the writer closes via `PipeClose` or a zero-length `PipeWrite` (the documented EOF signal) — it stays `PipeRead` forever; symmetric for writers when the last reader closes. (b) Between `try_read` returning `Empty` and `park_reader`, a writer can `try_write` and drain an empty `read_waiters`; the reader then parks with data in the ring and no wake pending (the comment at 6243-6246 assumes a re-check that does not exist).
- Exploit: Any pipe peer can hang the other side indefinitely (e.g. a child cell closes its stdout pipe; the shell's blocking read never returns).
- Fix: In `close_end`, drain and wake `read_waiters` when `writers==0` and `write_waiters` when `readers==0` (as `on_task_leaves`/`session_close` do); re-check the ring under `PIPES` after enqueuing before setting the parked state.

#### [S-11] WaitIrq on riscv64/aarch64 has no IRQ-ownership check — any driver cell can claim or drain any IRQ line — Severity: Medium — STRIDE: D — OWASP: A01
- Where: kernel/src/task/syscall.rs:9040-9094 (ownership check only under `#[cfg(target_arch = "x86_64")]` at 9070-9073; `take_pending`/`register_waiter` at 9075-9081)
- What: Holding PcieDriverCap/PlatformCap/UsbDriverCap is enough to `take_pending(irq)` (clearing another driver's pending flag) or `register_waiter(irq, …)` first (single-waiter policy → the real driver gets `TryAgain`) for any `irq < 256`.
- Exploit: A USB driver cell waits on the NIC/NVMe IRQ line and starves the storage/network driver.
- Fix: Bind IRQ lines to the MMIO/BDF the caller owns on all architectures (as x86 does via `pcie_msi::irq_owned_by`).

#### [S-12] Unchecked `system_ticks() + user_value` deadline arithmetic — Severity: Low — STRIDE: - — OWASP: -
- Where: kernel/src/task/futex.rs:158-162; kernel/src/task/syscall.rs:6224-6228 (PipeRead), 6301-6305 (PipeWrite), 7061-7065 (Heartbeat), 8104-8105 (SetTimer); contrast `wrapping_add` at 10281, 10684, 10700
- What: User-controlled u64/usize added without overflow handling. Release profile wraps (deadline in the past → immediate timeout / instant HUNG kill of self); any overflow-checks build (dev profile, `panic=abort`) aborts the kernel from userspace.
- Fix: `saturating_add` everywhere (a saturated deadline is "block indefinitely").

#### [S-13] Minor correctness/hygiene in pipe, serial, wait, PipeShare — Severity: Low — STRIDE: D — OWASP: -
- Where: kernel/src/task/syscall.rs:6233-6237 (PipeRead consumes ring bytes before `write_user_slice`; a bad `buf_ptr` loses data), 8994-9008 (SerialRead drains the UART before validating `ptr`), 6354-6398 (PipeShare installs handles into any target task's table with no consent/liveness filter; target cannot refuse, table grows), 6399-6441 (`Wait{pid}` is unprivileged and lets any cell block on and read any task's exit code; `pid == caller_id` self-waits forever)
- Fix: Validate/probe the output span (`preflight_user_output`) before consuming; reject `pid == caller_id`; consider a SpawnCap gate on `Wait` of foreign tasks and a per-task cap on inherited pipe handles.

#### [S-14] Guarded user copy runs byte-wise with interrupts masked for up to 64 MiB — Severity: Low — STRIDE: D — OWASP: -
- Where: kernel/src/task/user_copy/guard.rs:15-43, 51-68 (IRQs disabled for the whole `guarded_byte_copy`), 86-118 (1-byte loop); kernel/src/task/syscall.rs:1818 (`MAX_USER_BUF = 64 MiB`)
- What: A single `Read`/`Recv`/`NetRx` with a multi-MiB buffer keeps one hart's interrupts off for the full copy (tens of ms), defeating RT preemption and the watchdog on that hart.
- Fix: Chunk the guarded window (e.g. ≤ 64 KiB per arm/disarm) or copy word-wise.

#### [S-15] Build hygiene: without `native-domains` the copy boundary is a raw memcpy — Severity: Info — STRIDE: - — OWASP: -
- Where: kernel/src/task/copy_glue/mod.rs:226-236, 273-282, 288-293; kernel/Cargo.toml:71 (feature is in `default`)
- What: `--no-default-features` builds perform no pointer validation at all. Also docs vs code drift: sv39_probe.rs:222-230 says the U bit is "deliberately NOT required" for Sas but 237-241 requires it; syscall.rs:2614-2622 says validation does "NOT walk the page table" (it does, via the view).

Cross-slice (reported by DriversInputGpu-2, confirmed here): GpuFlush software fallback kernel/src/task/syscall.rs:8403-8426 reads `data_ptr` raw into the framebuffer (kernel memory → screen).

### Verified-safe areas
- Range preflight: null/overflow/`USER_LIMIT` in `validate_range` (user_copy/range.rs:84-98); `copy_from_user`/`copy_to_user` refuse length mismatch (copy.rs:19-50); Sas probe requires VALID|USER|R/W per page (sv39_probe.rs:231-248); Domain path pins reader before validating and re-walks PTEs under pin (sv39_probe.rs:154-204, copy.rs:117-162); scatter stages every destination before committing (user_copy/scatter.rs:15-74).
- `validate_user_buf` + `read_user_*`/`write_user_slice` used on all non-hypervisor handlers (syscall.rs:2624-2636, 3040-3085); SendGather/RecvScatter iovec count ≤ 8, per-entry and total ≤ MAX_USER_BUF with checked adds (5631-5660, 5685-5713); Recv death/message two-phase copy-then-commit (5420-5436, 5595-5604).
- Allowlist: non-live caller denied, unknown opcodes denied unless legacy-known, raw block ops gated on bit 36 + ZST cap, observe rights (4827-4838, 10870-10953, 5164-5181).
- ShmMap requires caller-incarnation-owned handle (6467-6476, 125-127); GrantDma range must lie in a caller-owned, generation-bound grant and BDF (747-868, 7846-7861); GrantFree/Unregister refuse pinned regions inside the table lock (1049-1070, 1075-1122, 9527-9545); GetRandom final write holds the SAS lease (2992-3038); GrantSlice `size_out` restricted to the caller's own stack (user_out.rs:11-47); VFS lease installed under grant-table→SCHEDULER (3506-3545).
- Capability gates before effect: ForceExit/NotifyOnExit/RegisterService SpawnCap, MACHINE_PIN only trusted init (6603-6640, 6788-6815, 6901-6911); CapRevoke double gate + fail-closed teardown (5007-5051, 6678-6845); supervisor-only 413/414/415/421/436; serial/PCIe/platform cap checks; raw 502 name-keyed (2078-2088); `pcie_usable_by` + claim-before-write (2456-2458, 2588-2609).
- SpawnFromMem copies ELF and name into kernel memory before verification (8047-8061); CellSessionV1 request/result overlap check + output pin (5196-5215); KillCell refuses tid 0/self/critical (8703-8725).
- CapSet intersection is field-wise AND; `platform_cap` never applied from a snapshot (cap.rs:644-667, 737-760); Platform singleton via CAS reservation (155-200).
- Futex deciding read + enqueue under SCHEDULER; wake only moves tasks still `FutexWait` on the same key; stale entries removed on timeout/exit (futex.rs:150-186, 192-219, 294-317); `read_word` alignment + copy view, never raw (119-131).
- Pipe handles: ownership enforced for Read/Write/Close/Share, end-kind checked, `from_raw` rejects 0 (6213-6222, 6272-6281, 6333-6352, pipe.rs:140-155); ring capacity clamped and quota-charged (pipe.rs:150-173).
- Stack guard pages verified by translation; partial failures release frames (stack.rs:189-253); IRQ index bounded (irq_wait.rs:80-84, 100-104); no struct-padding leaks in Fstat/GetProcs/MemInfo (ViFstatV1 repr(C,align 8) 32 bytes).

### Not covered
- kernel/src/task/syscall.rs:11143-13201 (`#[cfg(test)] mod tests`) and the `getrandom-sas-test`/`cellos_p01_trace`/`bench-counters` fixture code (test/feature-gated only, skimmed).
- Bodies of `ipc_send/recv/try_send/try_recv/ipc_grant/ipc_map/ipc_lend/ipc_borrow_*`, `async_ipc`, `completion_wait`, `cell_session`, `telemetry`, `state_stash`, `hotswap`, `loader::spawn_from_path`, `resource_registry` beyond `request_mmio_unchecked`, `hypervisor::registry` internals, `memory::pin` (other slices).
- kernel/src/task/user_copy_tests.rs beyond the header (test-hooks only); cap.rs policy tables 520-600 and 834-1509 (tests).

[Some lines truncated to 768 chars. Use agent://KernelSyscall-2:raw:3-3 to read line 3 whole]