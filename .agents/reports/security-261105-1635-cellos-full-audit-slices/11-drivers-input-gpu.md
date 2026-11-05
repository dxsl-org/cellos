## Slice: DriversInputGpu (USB/xHCI/DWC2, HID, input svc, compositor, GPU, wasm, serial, mmio-revoke selftest) — files read: 58 (37 by me incl. kernel/ostd cross-refs; dwc2/compositor/input sub-slices audited by 3 delegated readers, every High/Medium finding below re-read and confirmed by me at the cited lines) — unsafe blocks reviewed: 29

### Findings

#### [S-1] `GpuFlush`/`GpuCursor` forward caller-supplied raw pointers; virtio-gpu cell (and kernel fallback) read arbitrary memory onto the screen — Severity: High — STRIDE: I/D — OWASP: A01
- Where: kernel/src/task/syscall.rs:8367-8392 (forward), 8403-8426 (kernel fallback); cells/drivers/virtio-gpu/src/display.rs:124-155; cells/drivers/virtio-gpu/src/cursor.rs:21-31; cells/drivers/virtio-gpu/src/main.rs:78-85,97-134
- What: The syscall takes `data_ptr,data_len` and, with no check that the range lies in the caller's memory, packs them into an IPC to the GPU cell (`msg[11..19]=data_ptr`, 8388). The cell gates only on `sender_tid: 0` (main.rs:79) and does `from_raw_parts(src_ptr as *const u8, data_len)` (display.rs:146) and `from_raw_parts(data_ptr, SPRITE_LEN)` (cursor.rs:28) with no null/range check. Without a GPU cell the kernel itself does `from_raw_parts(data_ptr, data_len)` in kernel mode (8406-8407). Only `GpuFlush` allowlist bit 26 gates the call; cells with no `declare_syscalls!` default to all-permitted (xhci/main.rs:258-259 comment).
- Exploit: Any cell able to issue syscall 300 passes `data_ptr` = kernel/other-cell VA (SAS identity map), `w×h` sized to the window: kernel heap/keys/IPC buffers are painted to the display (or to a hardware cursor sprite). `data_ptr=0`/unmapped → page fault in the GPU driver cell → display dead. The safe `ostd` wrapper (ostd/src/syscall.rs:1735-1748) only helps Safe-Rust cells; FFI/Zig/Tier-2 cells call the raw ABI.
- Fix: In the kernel validate `[data_ptr, data_ptr+data_len)` against the caller's cell mappings (same helper as `resolve_optional_usize_slot`) and restrict `GpuFlush/GpuCursor` to the registered compositor tid; have the kernel copy into a compositor-owned grant pinned for the GPU cell rather than forwarding a raw pointer. (Cross-slice with KernelSyscall-2 — agreed I own it.)

#### [S-2] Compositor `ATTACH_GRANT` binds grant to surface owner but not to grant owner — any client can display any grant shared with the compositor — Severity: High — STRIDE: S/I — OWASP: A01
- Where: cells/services/compositor/src/message_handler/grant.rs:18-57; kernel/src/task/syscall.rs:3594-3601
- What: Only `surface.owner != sender` is checked (31); then `sys_grant_slice_with_len(grant.reg_id)` (35) succeeds for any grant whose `shared_to == compositor` (`grant_owner == caller || shared_to_tid == Some(caller)`, 3597) — the compositor never learns who registered it. `reg_id` is a page-aligned physical base and the reply is `\x01`/`\x00` (51,54), an oracle.
- Exploit: Attacker creates its own surface and loops `ATTACH_GRANT{cap: own, reg_id: candidate}` over page-aligned addresses; a `\x01` reveals a victim's live framebuffer, now rendered inside the attacker's window (obscured password dialogs etc.).
- Fix: Add a `GrantSlice` variant returning/validating the grant owner tid and reject `owner != sender`; or move to kernel-attested grant handles instead of raw physical ids.

#### [S-3] Compositor caches a raw grant pointer; the owner can free the pages underneath it (read-after-free painted to screen) — Severity: High — STRIDE: I/T — OWASP: A08
- Where: cells/services/compositor/src/surface_table/pixels.rs:84-93; message_handler/grant.rs:35-49; kernel/src/task/syscall.rs:3606-3611
- What: `attach_grant(ptr, …)` stores the pointer; every frame `pixels()` does `from_raw_parts(*ptr, w*h*bpp)` (87-89) guarded only by the comment "detach protocol keep the grant live". A non-VFS `GrantSlice` installs no pin (3608-3611), so the owner's `GrantUnregister` frees the frames while the compositor keeps blitting them.
- Exploit: Client attaches a grant, calls `sys_grant_unregister` without `DETACH_GRANT`, keeps sending `DAMAGE_NOTIFY`; freed frames reallocated to the kernel/other cells get displayed. With S-2, a victim's exit triggers the same.
- Fix: Re-resolve `sys_grant_slice_with_len(reg_id)` on every blit (treat `None`/shrunk as empty), or pin grants in the kernel while a non-owner holds a slice and refuse unregister on pinned regions.

#### [S-4] Keyboard focus is self-service: any cell takes focus from the input service or via compositor CREATE/RAISE and receives every keystroke — Severity: High — STRIDE: S/I — OWASP: A01
- Where: cells/services/input/src/main.rs:628-639; cells/services/input/src/dispatcher.rs:76-81,116-127; cells/services/compositor/src/message_handler/surface.rs:26-31,127-133
- What: `InputRequest::SetFocus` from any sender>0 → `dispatcher.set_focus(sender)` (635; only spoofing of the tid is prevented), and `dispatch` sends every `KeyEvent` to `self.focused` (125). In the compositor, `CREATE_SURFACE{role: Interactive}` and `RAISE_SURFACE` on the sender's own surface both call `input.set_focus(sender, …)` unconditionally (30, 133); key frames are routed to the focused owner irrespective of pointer position/visibility. `GetFocus` (640-648) tells anyone who currently has focus.
- Exploit: (a) Untrusted cell sends `SetFocus` to `service::INPUT` when a password prompt is active; (b) GUI client creates a 1×1 Interactive surface, moves it off-screen, spams `RAISE_SURFACE` — all keystrokes for any window arrive at the attacker while mouse/UI look normal.
- Fix: Accept `SetFocus` only from the registered compositor (as `RegisterEventSource` is gated on `USB_HID_PRODUCER`, main.rs:296-301); in the compositor grant focus only on user action (pointer press / owner `SELECT_VIEW`) or when the requester already holds focus, never on CREATE/RAISE; drop focus for fully off-screen surfaces.

#### [S-5] `WRITE_PIXELS` / `CREATE_SURFACE` geometry unchecked → wrap-around slice panic or OOM aborts the compositor — Severity: Medium — STRIDE: D — OWASP: A04
- Where: cells/services/compositor/src/surface_table/pixels.rs:96-120; surface_table/storage.rs:23-40; message_handler/surface.rs:23-26,43-49
- What: `create` stores any `w,h` (no bound, 32-40); `write_pixels` allocates `w*h*4` (97-100), computes `expected = (pw*ph*4)` in wrapping u32 (107), `dst_off = (py as usize + row)*stride + px as usize*4` with signed `px/py` cast (113) and the guard `dst_off + row_bytes <= buf.len()` wraps too (116), after which `buf[dst_off..]` / `data[src_off..src_off+row_bytes]` panic (117-118). Release profile aborts on panic → compositor exits.
- Exploit: `CREATE_SURFACE 2048×2048` + `WRITE_PIXELS` → 16 MiB alloc → OOM; or `py=-1` → `dst_off≈2^64-400`, guard passes, slice panics; or `pw=ph=0x10000` → `expected=0`, `row_bytes=0x40000` on a 487-byte payload → panic.
- Fix: Bound `w,h` at create (≤ screen, `checked_mul` ≤ budget); reject `px,py<0`, `px+pw>w`, `py+ph>h` with checked arithmetic; compute `expected` with `checked_mul`.

#### [S-6] Compositor and DWC2 reply with blocking `sys_send`; a client that never receives parks the service forever — Severity: Medium — STRIDE: D — OWASP: A04
- Where: cells/services/compositor/src/message_handler/surface.rs:32,35 and grant.rs:28,32,51,54 (all replies); cells/drivers/dwc2-usb/src/main.rs:594-596,804-810; kernel/src/task.rs:2620-2628
- What: `ipc_send_kernel` queues the message and sets the caller to `TaskState::Sending` with no deadline (2620-2627). Every compositor reply is `sys_send(sender, …)`; dwc2's `reply_nic` falls back to `sys_send(client, bytes)` when no operation token is present (808) and the not-ready path always uses it (595). virtio-net/e1000 use `sys_try_send` for the same reply.
- Exploit: Client sends `CREATE_SURFACE`/`GET_SCREEN_SIZE` (or `[0x02]` OP_GETMAC to dwc2 via `sys_try_send` without a token) and never calls `Recv`: compositor stops rendering/routing input for every cell; dwc2 stops NIC + all USB keyboard/mouse + heartbeat.
- Fix: Use `sys_try_send`/`ipc::reply` for all replies; dwc2 should refuse token-less requests.

#### [S-7] View/frame operations lack caller checks — any client can inject tabs into, re-stack, snap or fullscreen other clients' frames — Severity: Medium — STRIDE: T/S — OWASP: A01
- Where: cells/services/compositor/src/surface_table/frame.rs:98-117,199-237; message_handler/view.rs:151-170 (also SNAP_FRAME/TOGGLE_FULLSCREEN per delegated read of view.rs:286-357 `[not re-read by me]`)
- What: `Frame` has no owner; `create_view` with a foreign `frame_id` only checks `generation`/capacity then pushes the view and raises the frame's min size (99-112). `select_view` takes no caller (199-220) and the handler then raises and refocuses the view's owner (158-161). `launch_token`/`file_grant_id` fields are decoded but never checked (delegated: view.rs:37-44).
- Exploit: `CREATE_VIEW{frame_id: victim, frame_generation: g}` plants a look-alike tab in the victim's window (phishing); repeated `SELECT_VIEW`/`SNAP_FRAME` on victims = UI DoS.
- Fix: Record owner on `Frame`; require caller == frame owner (or owner of a view in that frame) for create-into-frame/SELECT/SNAP/FULLSCREEN; validate or delete the dead token fields.

#### [S-8] Client-chosen `SurfaceRole::Panel` yields an undecorated, top-most, input-accepting surface (fake lock/login screen) — Severity: Medium — STRIDE: S — OWASP: A01
- Where: cells/services/compositor/src/message_handler/surface.rs:25,127-129 (delegated: surface_table/state.rs:92-101, pointer_router/target.rs:17-37)
- What: Role byte `buf[9]` is honoured for any sender; `Panel` is raisable (128) and accepts input but is not window-managed (no frame/tab painted).
- Fix: Restrict `Panel`/`Background` to an allow-listed shell cell by `CallerIdentity.cell_id`; default others to `Interactive`.

#### [S-9] HID LED output selection iterates device-declared `Report Count` unbounded (USB host cell stall) — Severity: Medium — STRIDE: D — OWASP: A04
- Where: cells/drivers/hid/src/leds.rs:49-75; cells/drivers/hid/src/report.rs:258-284,316-323
- What: `report_count = raw` (323, 32-bit item) is stored as `count` (274); `from_map` walks `(0..f.count)` twice per output field (53, 64) with nothing to bound it (unlike `decode_field`, which stops at payload end). `index * field.size_bits` (71) wraps in release.
- Exploit: Malicious keyboard descriptor `Report Count(4) 0xFFFFFFFF; Output(Var)` → ~4·10⁹ iterations per output field at attach, starving dwc2/xhci heartbeat and input; repeatable per re-enumeration.
- Fix: Clamp `report_count`/`report_size` at parse (e.g. count ≤ 4096, size ≤ 32) or reject fields whose `size_bits·count` exceeds the max report length.

#### [S-10] LAN9514 RX reads one 512-byte packet per transfer: frames > 508 B dropped, continuation bytes re-parsed as RX-status, error bits ignored — Severity: Medium — STRIDE: - — OWASP: -
- Where: cells/drivers/dwc2-usb/src/usb_channel.rs:1401 (`want = buf.len().min(512)`, PKTCNT=1); cells/drivers/dwc2-usb/src/lan9514.rs:614-648
- What: `recv_frame` allocates 1522 B but gets ≤512; `(4+frame_len) > n` (641) drops the first packet of any longer frame, and the next call interprets mid-frame bytes as a status word (`frame_len = (status>>16)&0x3FFF`, 639) — any 4 bytes decoding to 1..=508 are delivered as a "frame". ES/CRC/runt/length-error status bits are never checked.
- Exploit: Network peer: every full-size segment is lost; crafted payloads produce synthetic frames (attacker already controls bytes, so mainly a broken/unsafe-to-trust RX path); CRC-corrupt frames delivered as valid.
- Fix: Request the whole burst (PKTCNT = ceil(len/512)), loop over `[status][frame][pad]` records, reject records with error bits.

#### [S-11] WASM host restarts `run` from scratch on every trap/out-of-fuel: fuel "preemption" never makes progress and a trapping module spins forever — Severity: Medium — STRIDE: D — OWASP: - (correctness)
- Where: cells/tools/wasm/src/main.rs:69-77; cells/drivers/wasm/src/lib.rs:53-59,73-79
- What: `run_fn.call` returns `Err` on OutOfFuel *and* on any trap; the loop refuels and calls `run` again from its entry (70-75) — the interrupted call's stack is gone, so a `run` needing > `fuel_per_tick` (100k) re-executes its prefix forever, and a module that traps (`unreachable`, OOB) is re-run indefinitely (yielding once per iteration) instead of exiting.
- Exploit: Any `.wasm` under `/data/apps` with a long-running or trapping `run` burns a scheduler slot permanently; side effects (`vi.send`) in the prefix are replayed each iteration.
- Fix: Distinguish `TrapCode::OutOfFuel` from other traps (exit on trap); for fuel, use `call_resumable`/a fuel-tick host import that yields, or set a large fuel budget and run `run` once.

#### [S-12] `vi.recv` allocates `max_len` bytes before validating the destination (guest-triggered OOM of its host cell) — Severity: Low — STRIDE: D — OWASP: A04
- Where: cells/drivers/wasm/src/imports.rs:55-80
- What: `alloc::vec![0u8; capacity]` (60) with guest `max_len` up to `i32::MAX` happens before the `end > data.len()` check (77); alloc failure aborts the host cell. Self-DoS of the hosting cell only.
- Fix: Clamp `capacity` to `IPC_BUF_SIZE` and check `ptr+max_len ≤ memory.len()` first.

#### [S-13] xHCI prints every decoded keystroke to the console log — Severity: Low — STRIDE: I — OWASP: A09
- Where: cells/drivers/xhci/src/main.rs:214-221
- What: `"[xhci] key down code=0x{:x}"` for each key event — a passive keylogger into the serial/kernel log (any log reader sees passwords typed on a USB keyboard). dwc2 does not do this.
- Fix: Remove or gate behind a debug feature.

#### [S-14] DWC2 LAN worker attach is first-come from any tid (NIC black-hole) — Severity: Low — STRIDE: S/D — OWASP: A07
- Where: cells/drivers/dwc2-usb/src/lan_worker.rs:23-26,45; lan_ipc.rs:23-25 `[delegated read; not re-read by me]`
- What: Worker accepts the first `0xD0` byte from any sender as `host_tid`; the real host's later attach still "succeeds" so it believes the worker is bound.
- Fix: Accept attach only from the spawning host tid (passed via spawn args/attested trailer).

#### [S-15] Hub `bNbrPorts` trusted (≤255) → minutes of mandatory delays; device addresses past 127 alias — Severity: Low — STRIDE: D — OWASP: -
- Where: cells/drivers/dwc2-usb/src/hub.rs:210-224, main.rs:229-230, hub.rs:115,135,146 `[delegated read]`
- Fix: Clamp ports (≤15) and stop assigning addresses > 127.

#### [S-16] Bulk-IN FIFO drain pops `want/4` words regardless of bytes received — Severity: Low — STRIDE: - — OWASP: -
- Where: cells/drivers/dwc2-usb/src/usb_channel.rs:1441-1453 vs 864,1612 `[delegated read]`; `[INFERENCE]` on shared-RX-FIFO corruption of neighbouring channels. Fix: use `got.div_ceil(4)`.

#### [S-17] Grants shared to the compositor but no longer tracked are transferred to it on owner death and never freed — Severity: Low — STRIDE: D — OWASP: A04
- Where: cells/services/compositor/src/surface_table/pixels.rs:20-28,43-54 `[delegated]`; kernel/src/task/syscall.rs:1424-1430 `[delegated]`; message_handler.rs:143-149 `[delegated]`
- Fix: Track every reg_id ever attached/staged per owner and unregister all in `cleanup_owner`.

#### [S-18] Compositor ownership keyed by raw tid with fail-open owner watch; `Rect` arithmetic wraps on client rects; global-only caps (32 surfaces/64 frames) — Severity: Low — STRIDE: E/D — OWASP: A04/A07
- Where: owner_watch.rs:122-131,142-149; libs/api/src/services/display.rs:40-57; surface_table.rs:22, storage.rs:32-34 `[delegated except storage.rs which I read]`
- Fix: Key on `(cell_id, generation)` and deny creates without a watch; saturating `Rect` math + clamp damage to surface; per-cell quotas.

#### [S-19] Async `GpuFlush` reads the source after the caller returns: `gpu::fill_rect` frees its `Vec` while the GPU cell may still read it — Severity: Low — STRIDE: - — OWASP: -
- Where: cells/drivers/gpu/src/lib.rs:31-46; kernel/src/task/syscall.rs:8390 (`ipc_post_nonblock`); compositor framebuffer.rs:131-149 (persistent `staging`, so only tearing there)
- What: `ipc_post_nonblock` returns before the GPU cell copies; `fill_rect`'s local `pixels` is dropped at return (no current callers found). The compositor's `staging` is reused by the next flush → visible tearing/race, not UAF.
- Fix: Make the GPU path copy synchronously, or require a pinned grant as source.

#### [S-20] Serial: `baud_divisors` divides by caller `baud` (0 → panic); serial-x86 echo loop never ends when `sys_get_time_ms` is `None` and logs one line per received byte — Severity: Low — STRIDE: D — OWASP: -
- Where: cells/drivers/serial/src/lib.rs:53-60,103-105; cells/drivers/serial-x86/src/main.rs:151-185
- Fix: Reject `baud == 0`; bound the poll loop by iteration count; rate-limit RX logging.

#### [S-21] `probe_userbit` leaves a grant frame permanently kernel-only (aarch64: also DEVICE-attributed) in the free pool — Severity: Low (test-hooks only) — STRIDE: - — OWASP: -
- Where: kernel/src/task/mmio_revoke_selftest.rs:78-90 (gated `#[cfg(feature="test-hooks")]`, kernel/src/task.rs:58-59, main.rs:1074); kernel/src/memory/paging.rs:784-803,861-870; GrantAlloc kernel/src/task/syscall.rs:9321-9324 (no re-protect on alloc)
- What: The probe lowers a grant page to `mmio_kernel_flags()` then `GrantFree`s it without restoring USER; `alloc_grant_pages` does not re-map, so the next cell receiving that frame faults on first access (and on aarch64 the page carries Device memory attributes). `probe_allowlist` also claims under `ALLOW_TID` 9302 which is never inserted as a task (65) — works only because `request_mmio` does not require a live task `[INFERENCE]`.
- Fix: Restore user flags (`grant_mmio_user`/`protect_page(boot flags)`) before `GrantFree`.

#### [S-22] Info/hygiene — Severity: Info — STRIDE: - — OWASP: -
- input virtio probe reads all VirtIO MMIO slots before `sys_request_mmio` and `mem::forget`s mismatched transports (cells/services/input/src/virtio_device.rs:178-209 `[delegated]`).
- Input `GetFocus` discloses focused tid to anyone (main.rs:640-648) — aids S-4.
- Compositor main.rs:3 "unsafe-free" claim vs pixels.rs:89 `unsafe`; dead `buf.len() < N` checks on fixed `[u8;512]` (surface.rs:20,40; grant.rs:19).
- Unmapped physical keys produce a spurious `Lost(3)` on the serial-owner session (session.rs:3297-3360 `[delegated]`).
- virtio-gpu `flush_rect` clamps `w` after computing `src_off` with the clamped width (display.rs:138,150) — rows shear for partially off-screen rects (rendering only).
- `intel-gpu-core` has no consumer cell yet; `drm-cellos` is link-only C glue.

### Verified-safe areas
- xHCI: all DMA accesses bounds-asserted (dma.rs:48-83); config `wTotalLength` checked `9..=data.len()` and copied into 512-B buffer with `min` (controller.rs:484-508); descriptor walker rejects `len<2`/overrun (993-998); `ep_num & 0x0F` → DCI ≤ 31 fits input/device contexts (1013, regs.rs:157-164); interrupt-IN TRB length = `int_mps ≤ 0x7FF` into a 4 KiB page (841-842, 1012); event-slot/EP filtering (551-553, 857-859); all waits bounded by `POLL_LIMIT` (603, 677, 855); caplen/CSZ/scratch/slot validated fail-closed (188-237, 392).
- virtio-gpu: `flush_rect` row copies bounds-check both fb and src (display.rs:152); kernel fallback checks `dst/src offsets ≤ sizes` with `checked_add` (syscall.rs:8417-8422); only `sender_tid == 0` dispatches (main.rs:78-85).
- WASM imports: `vi.send`/`vi.log` saturating `ptr+len` vs memory length (imports.rs:36-40,111-115); `#![forbid(unsafe_code)]` (lib.rs:9); fuel metering enabled (55).
- intel-gpu-core: CSS header uses `checked_sub/checked_add` (guc_fw.rs:90-109); CT `g2h_read` bounds `len ≤ 255` against `out` and ring wrap (ct.rs:151-162); `#![forbid(unsafe_code)]` (lib.rs:2).
- drm-cellos: `query.size` minimum checked before `calloc`, `num_params` checked before index (device.c:50-65).
- dwc2 (delegated, spot-checked): descriptor walker `bLength<2`/overrun guards (usb_desc.rs:157-163); `wTotalLength` clamped `9..=512`, report descriptor ≤1024; both `unsafe` DMA copies bounded by slot/`buf.len()`; `bMaxPacketSize0` clamped; toggle tables guard `dev_addr<128`; OP_TX len `1..=1514`; all channel waits bounded.
- HID crate `#![forbid(unsafe_code)]`; `extract_bits` `checked_add` + size `1..=32` + `div_ceil(8) ≤ len` (decode.rs:42-49); long-item/short-item length guards (report.rs:222-243); keymaps are closed `match`es.
- Input service: principal only from kernel attested trailer re-checked vs sender (session.rs:932-936); raw device frames only from sender 0 or the kernel-published `USB_HID_PRODUCER` tid (main.rs:277-307); all SessionV1 selectors principal-gated; queue/record limits and zeroization throughout (session.rs:2694-2699, 102-180, 251-255).
- Compositor: `blit_surface_clipped`/`flush_rect` clip to screen and check both slices (framebuffer.rs:127-138 + delegated 36-66); `ATTACH_GRANT` size `checked_mul ≤ grant_len` (grant.rs:37-42); `surface.owner == sender` on all surface ops (surface.rs:48,127); input frames accepted only from `input.input_tid`; cap ids monotonic u64.

### Not covered
- cells/drivers/dwc2-usb: usb_channel.rs:990-1134,1781-1890,2156-2399; lan9514.rs:497-547 (feature `loopback-diag`); regs.rs beyond helpers.
- cells/services/input: session.rs:2075-2459 (prepare_foreground/receipt/restore) skimmed only; `*_tests.rs` not read.
- cells/services/compositor: frame.rs:700-1056, owner_watch.rs:330-674, framebuffer/tests.rs (cfg(test)); window_decoration/raster.rs read only via delegate.
- libs/viui: only gpu_cmd.rs, input_bridge.rs, managed_surface.rs, surface.rs (via delegate); all widget/rendering/layout files skipped per brief.
- libs/intel-gpu-core: ppgtt.rs, pte.rs, syncobj.rs (pure arithmetic libs, no consumer yet); drm-cellos fake_provider.c, ioctl.c, third_party/libdrm.
- Kernel-side semantics relied on but not audited here: `DmaBuf::alloc/authorize`, `ipc_try_send` mailbox bounds, `sys_register_usb_hid_producer` singleton enforcement, `request_mmio` task-liveness.