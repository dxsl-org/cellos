## Slice: ServicesSupervisorApi — files read: 61 (self) + 59 (three read-only sub-audits, key findings re-verified by me) — unsafe blocks reviewed: ~15 (self) + 336 (sub-audits)

### Findings

#### [S-1] Supervisor parses stale bytes from an uncleared `AppContext` recv buffer → power-button / `shutdown` / snapshot requests silently rejected — Severity: Medium — STRIDE: D — OWASP: A04
- Where: libs/ostd/src/app.rs:153-165,190-193,216-218,345-399 (`recv_buf` allocated once, never cleared, full 4096 B passed to `parse_event_owned`, `data: buf[2..].to_vec()`); kernel/src/task/syscall.rs:5541-5546,5595-5603 (Recv copies only `copy_len`, rest of user buffer untouched, returns sender tid not length); cells/services/supervisor/src/protocol.rs:52-82,100-113 (`parse_power_request`/`SnapshotRequest::parse` reject any non-zero byte after the fixed fields; `HotswapRequest` doc at 55-58 and module doc 9-12 wrongly assume a "zero-padded receive buffer"); cells/services/supervisor/src/main.rs:138-164; kernel/src/acpi/sci.rs:38-43,209-215 (relay is a 4-byte message).
- What: After the supervisor receives any message longer than 4 bytes (a 193-byte `hotswap` request, init's 9-byte `0xE1` relay, or arbitrary junk), the tail of `recv_buf` holds non-zero residue. Every subsequent 2-byte `PowerRequest` (`/bin/shutdown`) gets `STATUS_INVALID_REQUEST`, and the kernel's 4-byte ACPI power-button relay is dropped with no reply. Reproducible without an attacker: run `hotswap …` then `shutdown`.
- Exploit: any Cell (Send is in the base allowlist) `sys_send`s ≤4096 bytes of 0xFF to the supervisor tid; it is delivered as `RawMessage` and ignored, but from then on the hardware power button and operator shutdown never act until the supervisor restarts.
- Fix: `AppContext` must deliver only the received length (kernel Recv doesn't return it; either add a length-returning recv variant or `recv_buf.fill(0)` before each `sys_recv`). Supervisor parsers should not depend on padding being zero.

#### [S-2] Supervisor and Config reply to untrusted senders with blocking `sys_send` → one hostile client parks the Tier-1 service forever — Severity: Medium — STRIDE: D — OWASP: A04
- Where: cells/services/supervisor/src/main.rs:65-67,145-149,166-168 (replies, including `STATUS_REJECTED_CALLER` to *any* sender); cells/services/config/src/main.rs:124-127; kernel/src/task.rs:2603-2628 (`ipc_send_kernel` queues then puts caller in `TaskState::Sending` until the target consumes that delivery, lines 2620-2628); kernel/src/task.rs:2922-2938 (`wake_sender_token` only on matching consume). Contrast silo: cells/services/silo/src/ipc.rs:33-38 uses `reply`/`sys_try_send`.
- What: A sender that never calls `Recv` leaves the service in `Sending` indefinitely; the supervisor stops servicing hotswap, power button and shutdown; config stops serving every `Get`/`Set` (shell `PATH` lookups). Neither cell arms a heartbeat, so the kernel does not reap/restart them.
- Exploit: hostile Cell sends `[0xAC,0x00,0x04,0x00]` (or any `ConfigRequest`) and busy-loops without receiving.
- Fix: reply with `sys_try_send` (or `ostd::ipc::reply` on the async op) and arm the App SDK heartbeat so a stuck service is restarted.

#### [S-3] `ChannelClient::connect` / `endpoint` turn an attacker-supplied u64 into `&BiRingChannel` from safe code — Severity: High — STRIDE: T/E — OWASP: A08
- Where: libs/ostd/src/ring_channel.rs:167-170,184-208 (handle is a raw `Box` address; `connect` checks only non-null + alignment; `endpoint` does `unsafe { &*self.ptr }`); ring ops in libs/api/src/services/ring_channel.rs:131-208 then perform atomic stores at that address.
- What: A `#![forbid(unsafe_code)]` client that trusts a host-provided handle performs 64-bit writes (`slot.state/len/seq/data`, `consumer_idx`) at an arbitrary address — an LBI escape primitive through a safe API. The borrow is also unbounded by the host's `Box` lifetime (dangling after host drop). No in-tree caller of `connect` today (latent).
- Exploit: hostile host cell sends `handle = <victim/kernel-shared address>` over IPC.
- Fix: make `connect`/`endpoint` `unsafe fn` with a documented contract, or derive the pointer from a kernel-validated grant (`sys_grant_slice_with_len` ≥ `size_of::<BiRingChannel>()`).

#### [S-4] `ostd` heap allocator is unsynchronised `static mut` while safe `task::spawn` creates preemptible threads — Severity: High — STRIDE: - — OWASP: A04
- Where: libs/ostd/src/heap.rs:36-39,77-99 (SAFETY: "single-task cells never enter the allocator concurrently"); libs/ostd/src/task.rs:21-48 (safe `spawn`; `thread_entry` deallocates on the new thread); live multithread users: cells/tests/futex-test, cells/tests/top-load, cells/apps/c2c-render `[INFERENCE from sub-audit grep]`.
- What: Two threads of one Cell can be preempted inside `allocate_first_fit`/`deallocate`, corrupting the free list — memory-unsafety in safe Rust. Cell-local blast radius (SAS caveat: corrupted pointers can hit anything).
- Exploit: any multithreaded Cell under timer preemption; input timing steers it.
- Fix: lock the heap (spin mutex living in `.bss`) or make `task::spawn` `unsafe`/feature-gated with a locked allocator.

#### [S-5] POSIX `net.rs` uses `Recv`'s return (sender tid) as a byte count and accepts replies from any task — Severity: High — STRIDE: S — OWASP: A01
- Where: libs/api/src/services/posix/net.rs:150-168 (`connect`), 326-345 (`recv`); same pattern at 194-205, 254-264, 476-487 `[sub-audit]`; kernel/src/task/syscall.rs:5622-5623 (`Ok(sender_tid)`); contrast sysio.rs:343-356 which treats it as tid and checks `sender == vfs` `[sub-audit]`.
- What: `decode(&resp_buf[..n])` with `n` = net-service tid: tid ≥ 4097 → slice panic (cell abort); tid shorter than the encoded reply → decode error (`recv` returns −1). Mask `0` also means any co-resident Cell that can `Send` to the victim can inject a forged `NetResponse::Data`/`CapId`/`Addr`.
- Exploit: hostile Cell sends a postcard `NetResponse::Data` to a POSIX socket cell between its Send and Recv → injected network payload; or forged `CapId` redirects the socket.
- Fix: `Recv` with `mask = net`, verify `n as usize == net`, decode from the whole buffer.

#### [S-6] `vsnprintf_core` reads past the format string on a trailing `%`; `fmt_float` overflows its 64-byte buffer for `prec ≥ 43` — Severity: Medium — STRIDE: I/D — OWASP: A04
- Where: libs/api/src/services/posix/stdio_fmt.rs:192-196 (NUL check only at loop head), 205-259 (flag/width/precision/length scanners and `spec = *fmt.add(fi); fi += 1` never stop at 0); 51-91 + 380-384 `[sub-audit]` (`tmp: [u8;64]`, unbounded `prec`).
- What: `"…%"` consumes the terminator and keeps copying adjacent bytes into the output until the next zero (info leak / fault at page edge). `printf("%.50f", x)` panics → cell abort.
- Exploit: C code or a format built from untrusted input; file/network-derived precision.
- Fix: `if spec == 0 { break }` before `fi += 1` (and in each scanner); clamp `prec` and size `tmp` accordingly.

#### [S-7] RISC-V `setjmp`/`longjmp` omit callee-saved `fs2`–`fs11` — Severity: Medium — STRIDE: - — OWASP: -
- Where: libs/api/src/services/posix/setjmp.rs:13,46-66,78-105 `[sub-audit; not re-read by me]`.
- What: LP64D makes f18–f27 callee-saved; C code (Lua pcall, PDF error recovery) holding doubles across `setjmp` gets silently corrupted values after `longjmp`.
- Fix: save/restore f18–f27, `JMP_BUF_WORDS = 26`.

#### [S-8] `SpscRing::try_pop` trusts peer-written `slot.len` beyond `RING_SLOT_BYTES` → index panic — Severity: Medium — STRIDE: D — OWASP: A04
- Where: libs/api/src/services/ring_channel.rs:189-208 `[sub-audit]`.
- What: `len` is checked only against `out.len()`; `len > 64` with a large `out` indexes `words[8]` out of bounds (panic, `forbid(unsafe)` file).
- Exploit: hostile producer sets `len = 1000`, `state = READY`.
- Fix: `if len > RING_SLOT_BYTES { return Err(MessageTooLarge) }`.

#### [S-9] Device/Platform-reported PCIe BAR windows registered without RAM/aperture overlap check; `PcieDriverCap` cells can then user-map them — Severity: Medium — STRIDE: E — OWASP: A04
- Where: kernel/src/resource_registry.rs:336-367 (`valid_pcie_bar_window`: size/alignment only); kernel/src/task/syscall.rs:9941-9959 (`RequestMmio` PCIe path → `user_map_pcie_bar`); cells/services/platform/src/scan.rs:237-274 (forwards device-reported base/size via `sys_register_pcie_bar`/`sys_register_pci_device`); kernel/src/task/syscall.rs:9930-9940 (PlatformCap path is `request_mmio_unchecked` on any range); cells/services/platform/src/main.rs:117-164 (ECAM base taken from argv).
- What: A hostile PCIe function (or a compromised Platform Cell) can advertise a BAR aliasing RAM/kernel/IOMMU tables; the window is accepted and later mapped user-RW uncached for a driver Cell. In Tier-1 SAS this is not a new read primitive, but it is for Tier-2 page-isolated driver cells and it corrupts caching attributes. `[INFERENCE]` real firmware-assigned BARs never overlap RAM.
- Fix: reject windows outside the firmware MMIO aperture (MCFG/_CRS/DT `ranges`) or intersecting the frame allocator/kernel image/ECAM/IOMMU pages.

#### [S-10] IOMMU page tables never freed on Cell death → kernel heap leak → `alloc_page` assert panic — Severity: Medium — STRIDE: D — OWASP: A04
- Where: kernel/src/task/drivers/iommu_pt.rs:11-26,71-74,189-192; iommu_x86.rs:871-875; iommu_riscv.rs:286-295,454-457 `[sub-audit]`.
- What: Domains drop without walking/freeing L1/L0 tables; sparse `GrantDma` + repeated driver restarts grow the heap monotonically until the non-null assert aborts the kernel.
- Fix: `Drop` for `VtdSlpt`/`Sv39IommuPt` after acknowledged teardown; charge table pages to DMA quota.

#### [S-11] `RegisterService` lets any SpawnCap holder rebind any well-known service id (incl. SUPERVISOR, CONFIG, KMS) to any live tid — Severity: Medium — STRIDE: S — OWASP: A01
- Where: kernel/src/task/syscall.rs:6999-7013 (only `caller_has_spawn`); kernel/src/cell/service_registry.rs:88-131 ("replacing any prior entry"); SpawnCap edges include a driver (`dwc2-usb`) and several app roles: kernel/src/loader/launch_profile/mod.rs:85-97; supervisor relies on lookups: cells/services/supervisor/src/main.rs:98-99, hotswap.rs:97, kernel/src/acpi/sci.rs:209. (Kernel-side; cross-cuts KernelSyscall slice.)
- What: A compromised SpawnCap cell that declares `RegisterService` can impersonate the supervisor (power relay, `shutdown`/`hotswap` tools talk to it) or config (shell `PATH`/`OS` answers). Not reachable from the implicit `app_syscall_set(spawn=true)` (libs/ostd/src/runtime.rs:82-97) — needs the bit declared.
- Fix: refuse replacing a *live* provider unless caller holds SupervisorCap (or is init); record and check ownership per service id.

#### [S-12] Config store is unbounded; `List` reply fails once keys exceed one IPC buffer — Severity: Low — STRIDE: D — OWASP: A04
- Where: cells/services/config/src/main.rs:69-72,76-86,88-96 (no key count/size cap; `Keys(list)` → `ERR_MALFORMED` when encode fails); rights at rights.rs:26-56 (only cellbox-class `config_write` holders).
- Exploit: shell-class cell writes thousands of distinct 64-byte keys → config quota kill (restart loses state) or permanent `List` failure.
- Fix: cap entries/bytes; paginate `List`.

#### [S-13] `ostd::hotswap::restore` caps at 64 KiB while `stash` allows 1 MiB → silent state truncation on hot-swap — Severity: Low — STRIDE: - — OWASP: -
- Where: libs/ostd/src/hotswap.rs:98-119 (`MAX = 64*1024`, `truncate(n2)`), 57-77; kernel cap `MAX_STASH_LEN` at kernel/src/task/syscall.rs:8571.
- What: A cell with >64 KiB state (e.g. a large config map) restores a truncated prefix; `deserialize_state` then fails or loads partial state.
- Fix: length-probe with an empty buffer (supported, syscall.rs:8606-8612) and allocate exactly.

#### [S-14] Supervisor ships with a permit-all syscall allowlist (`run_app!` emits no `__ViCell_syscalls`) — Severity: Low — STRIDE: E — OWASP: A05
- Where: libs/ostd/src/lib.rs:176-190 (`run_app!` has no allowlist static); cells/services/supervisor/src/main.rs:205-218 (only `declare_manifest!`); kernel/src/loader/governed_spawn.rs:14-22 (`unwrap_or(u64::MAX)`); contrast platform: cells/services/platform/src/main.rs:41-50.
- What: The most privileged Cell (SupervisorCap + SpawnCap) has no in-image syscall minimisation; a bug in its parser becomes full syscall surface. Same for every other `run_app!` cell lacking `declare_syscalls!`.
- Fix: `declare_syscalls!` in the supervisor; make `run_app!` emit a default minimal set.

#### [S-15] Authorization by task *name* (supervisor, raw-shutdown gate, argv stash gate) — sound today only because every launch edge is exact-match and the Mem route is denied — Severity: Low — STRIDE: S — OWASP: A01
- Where: cells/services/supervisor/src/main.rs:63-66,141-150,182-203 (`sender_has_exact_name` over `GetProcs`, 64 rows); hostile_backend_recovery.rs:16-22 (feature-gated); kernel/src/task/syscall.rs:2068-2086 (`RAW_SHUTDOWN_CALLERS`), 8547-8554 (`is_shell` by name); name = basename of path: kernel/src/loader/governed_spawn.rs:197,266-273 (Elf route uses caller's advisory path); kernel/src/loader/mem_spawn_gate.rs:75-90 (`/mem/<name>` keeps the basename → task name `shutdown` is producible if any profile ever admits `LaunchRoute::Mem`); profiles currently deny Mem: kernel/src/loader/launch_profile/profiles.rs:21-23,63-87,88-94,116-120,348-369; only cellbox may start `/bin/hotswap` (Path|Elf) and `/bin/shutdown` (Path): profiles.rs:65-69, targets.rs (no generi…
- What: Any future edge admitting a caller-named Mem/Elf spawn silently grants `OP_POWER`, `OP_HOTSWAP`, raw 502 and argv-stash authority to arbitrary bytes. Also `GetProcs` is capped at 64 rows: a legit sender beyond row 64 is refused (fail-closed), and `power::shut_down` (power.rs:49-66) freezes only the first 64 tasks.
- Fix: gate on kernel-attested `CallerIdentity` + a dedicated right (e.g. `CALLER_FLAG_POWER`) rather than names; use `GetProcs2`/paging for the freeze sweep.

#### [S-16] `MmioRegion::read/write<T: Copy>` admit types with validity invariants and unaligned offsets — Severity: Medium — STRIDE: E — OWASP: A08
- Where: libs/ostd/src/mmio.rs:62-74,79-89.
- What: `read::<fn()>`, `read::<bool>`, `read::<&'static T>` of device bytes is UB; no `offset % align_of::<T>()` check (misaligned `read_volatile` is UB / traps on RISC-V device memory). Only reachable by a Cell that chooses such a `T`, i.e. it defeats the "signed `forbid(unsafe)` ⇒ safe" admission argument rather than attacking an honest cell.
- Fix: sealed trait for `u8/u16/u32/u64`; alignment check.

#### [S-17] Grant `with_bytes*`/`with_shared_bytes*` hand out `&[u8]`/`&mut [u8]` over memory a peer may write concurrently — Severity: Low — STRIDE: T — OWASP: A04
- Where: libs/ostd/src/grant.rs:155-172,189-207 `[sub-audit]`.
- What: Data race (UB) and TOCTOU on lengths read from the slice; the protocol precondition is doc-only.
- Fix: copy-out API (`sys_grant_copy_to_slice` exists, libs/ostd/src/syscall.rs:2112-2153) or `unsafe` marking.

#### [S-18] Kernel driver minor issues (sub-audit, not re-read by me) — Severity: Low — STRIDE: D/E/T
- RISC-V PSCID exhaustion `expect` panics instead of refusing: iommu_riscv.rs:422-428 (x86 refuses: iommu_x86.rs:593-600).
- RISC-V `map_range_for_cell` leaves stale BDF in a prior domain; its teardown later zeroes the live owner's DC: iommu_riscv.rs:434-440,447-472 `[INFERENCE: needs failed teardown + re-claim]`.
- `AllocPcieIrq` (435) sets Bus Master unconditionally, bypassing the `PublishedUnconfirmed` posture of `GrantDma`: pcie_msi/program.rs:33-39,57,103 vs syscall.rs:7996-8006.
- Unbounded UART drain loops in ISR/timer context: uart.rs:385-452, console_drv.rs:95-130.
- `memmove` byte loop may be rewritten by LLVM loop-idiom into a self-call (no `#![no_builtins]`): libs/api/src/services/posix/strings.rs:29-43 `[INFERENCE, build-dependent]`.
- `ReadDir::next` decodes kernel bytes straight into `FileType` enum: libs/ostd/src/fs.rs:26-34 `[INFERENCE on VFS canonicalisation]`.
- `register_driver_service` accepts `UsbDriverCap` for the singleton NIC role, contradicting its own comment: kernel/src/task/syscall.rs:2290-2296,2331-2337; driver_cell.rs:251-261 overwrites the previous owner.

#### [S-19] Skeleton services carry unused `network = true` and stubbed security checks — Severity: Info — STRIDE: - — OWASP: A05
- Where: cells/services/workload-router/src/main.rs:10,22-28 (manifest `network=true`, loop only heartbeats); admission.rs:18-22 (`OriginProof::verifies` compares `deadline_ms` to itself → always true, `ForgedOriginProof` unreachable); cells/services/placement-controller/src/main.rs:10; budget_client.rs:28-35 (`request_reservation` always `Ok`). Not wired to IPC today, so no live impact; Spec 26 "controllers cannot multiply budget" is unenforced in this code.
- Also: cells/services/silo/src/artifact.rs:19-35 + build.rs:79-84 (digest is generated from the same build output it checks — self-consistency, not provenance); cells/services/silo/silo-guest.bin tracked as 0 bytes (stale artifact); `sys_set_tls_base` is a safe fn accepting any address (libs/ostd/src/syscall.rs:489-492).

### Verified-safe areas
- Supervisor request parsing bounds: protocol.rs:58-71 (len check before field slices), 127-134 (`/bin/<name>` only, no `/`, `..`, `//`); power action filter so the kernel relay can only poweroff: main.rs:151-156; `settle()` accepts only sender 0 + exact bytes: power.rs:130-144.
- Kernel `SpawnReplacement` requires SupervisorCap, reads path via `read_user_string`, authorises the supervisor launch edge, and seals the stash to the exact replacement: kernel/src/task/syscall.rs:8776-8834; supervisor edge limited to `/bin/` with boot/reviewed ceilings: profiles.rs:336-342; Elf-route ceilings stripped of authority: launch_profile/mod.rs:105-116.
- `SpawnFromMem` names are reduced to `/mem/<label>` so no path-keyed privilege is selectable: mem_spawn_gate.rs:49-96 (and no profile admits the Mem route today).
- Config writes: kernel-attested trailer written after payload on every delivery path (syscall.rs:5409,5496,5583,5587,5622; zeros when identity unknown 3851-3853); `CallerIdentity::from_recv_buf` rejects cell_id 0 (caller_identity.rs:140-143); key namespace `[A-Za-z_][A-Za-z0-9_]{0,63}` minus `PATH`/`OS` (rights.rs:26-35); config `deserialize_state` bounds every read (main.rs:181-214).
- Silo: fresh buffer per receive, KMS-only binding by live service tid + attested identity + generation, monotonic request seq: silo/src/ipc.rs:15-32, protocol.rs:84-124; mailbox response validation: mailbox.rs:8-30; HVC register redaction: vm_exit.rs:26-36.
- boot-config: size cap 16 KiB, ≤32 cells, name/path charset, `/bin/<basename>` only, reserved identities, argv ≤512 B with `checked_add`, `deny_unknown_fields`: libs/boot-config/src/lib.rs:203-341. config/services.toml and autoload.toml carry no capability grants (grants come from manifests + kernel edges); `hotswap-demo`/`recovery-authority` are packaging-feature-only.
- cell-initial-stack: `phnum ≤ 16`, `top` alignment/underflow checked, all offsets computed from bounded constants: libs/cell-initial-stack/src/lib.rs:92-148.
- ostd syscall wrappers: kernel-returned lengths clamped — `telemetry_bytes` (syscall.rs:1672-1677), `sys_telemetry_snapshot` (1661-1666), `sys_query_machine_facts` (2665-2672), grant copies bounded by registered len (2112-2153); argv restore clamps `n ≤ buf_len` kernel-side (kernel syscall.rs:8585-8603) and `set_spawn_argv` bounds 512 B with `checked_add` (args.rs:25-46).
- Platform ECAM arithmetic: `ecam_window_size`/`bus_offset`/`BusConfig` use checked ops; MMIO access goes through bounds-checked `MmioRegion`: platform/src/main.rs:166-171, scan.rs:66-94.
- Driver-cell role publication is transactional under `ROLE_PUBLICATION` with exact-TID teardown; block routes refuse a live foreign holder: driver_cell.rs:120-134,148-213; `register_driver_service` requires a live cap holder and records live identity: kernel syscall.rs:2286-2318.
- Sub-audit verified-safe (not re-read by me): GrantDma validation/quota/pin ordering (syscall.rs:7937-7980), VT-d DID allocator and acknowledged invalidation (iommu_x86.rs:104-152,265-340,857-903), MSI vector pool and kernel-chosen MSI address/data (vectors.rs:35-48, program.rs:60-88), ECAM offset/BDF range checks (pcie_ecam.rs:271-291,1251-1261), PS/2 table indexing (ps2_keyboard.rs:293-311,362-369), POSIX `malloc/calloc/realloc` overflow checks and magic (alloc.rs:64-135), `emit_byte` bounds (stdio_fmt.rs:160-165), `getentropy` fail-closed (entropy.rs:19-33).

### Not covered
- libs/types/src/kms/** and libs/ostd/src/clients/kms/** (AuthorityKms slice); libs/api/src/abi/cell_session*.rs, telemetry.rs, dir_handles.rs; libs/api/src/services/{display,session,input,ssh_completion,machine_pin}.rs; libs/ostd/src/{session,child_context,display/*,font*,repl,http,cluster_endpoint,actor/*}.rs beyond the unsafe sites the sub-audit sampled.
- kernel/src/task/drivers/{irq_wait,gpio_irq,input_irq_ack,registry,block,nic,ramdisk,virtio_hal}.rs; `virtio_drivers` crate internals; cfg(test)/`test-hooks` selftests in drivers.
- Build-dependent claims (LLVM loop-idiom on `memmove`, aarch64 softfloat setjmp) and hardware behaviour are unverified (read-only slice, no builds run).
- Kernel IPC death-notification delivery through `AppContext` (Recv returning a dead tid with an 8-byte reason in the buffer, kernel syscall.rs:5390-5411) was noticed but its interaction with supervisor name checks was not traced.

[Some lines truncated to 768 chars. Use agent://ServicesSupervisorApi-2:raw:83-83 to read line 83 whole]