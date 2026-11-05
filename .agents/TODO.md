# TODO

Việc **chưa xong** và con trỏ tới bằng chứng. Trạng thái thật do source/test quyết định,
không phải bởi dòng chữ ở đây.

Mục nào đã đóng thì **xoá khỏi file này** — không để lại dòng "đã đóng": lịch sử ở
`CHANGELOG.md` / `docs/project-changelog.md`, quyết định ở `docs/decisions/`, kế hoạch +
báo cáo ở `.agents/<plan>/`, cách làm ở `docs/guides/`. Chuỗi tiền lệ: các mục đã đóng
2026-09-19 → 2026-09-27 và đợt dọn 2026-11-02 đã được gỡ, nội dung của chúng nằm ở bốn chỗ trên.

## Current queue — Intel x86-64 C2C Anywhere only (cập nhật 2026-11-02)

[ADR-0022](../docs/decisions/0022-intel-x86-64-c2c-only-direction.md) supersedes
earlier queue ordering. Use the [portfolio](plan-portfolio.md) and
[current focus](../docs/roadmap/current-focus.md) before selecting any task.
Every task must name a direct dependency or measured defect of this direction,
its acceptance scenario and evidence ceiling. Other work is **parked**.

Vừa đóng (không mở lại; chi tiết ở portfolio/CHANGELOG): Ginux lessons đóng 2026-11-01
([plan](261030-1945-ginux-lessons-hardening/plan.md)); x86 runtime board selection và aarch64
core/backend split xong 2026-11-02 ([x86](261031-1241-x86-runtime-board-selection/plan.md),
[aarch64](261031-1241-aarch64-core-backend-split/plan.md)); hv vGIC fix `287377e5d` (guest
AArch64 QEMU virt nhận interrupt, lane hypervisor-machinery tới guest shell); **C2C Phase 03 done
2026-11-02** ở trần x86 QEMU/TCG (slice e CP2,
[record](260927-1100-c2c-anywhere-tier-aware/phase-03e-closure-cp2-approval.md)).

**Ứng viên kế tiếp** (chưa admit; chọn trong portfolio theo promotion rule):
- [ ] **C2C slice tiếp theo** (Phase 03 đã done) — Phase 01 đã sign-off 2026-10-09, Phase 02 slice A+B xong. Toàn
  bộ execution còn lại bị khoá bởi `CELL-CONTRACT-0024`: cần owner handoff theo slice
  ([plan](260927-1100-c2c-anywhere-tier-aware/plan.md), [locks](cell-contract-0024-locks.md)).
- [ ] **VFS/storage/search research** — parked; chỉ tiếp tục khi owner yêu cầu
  ([plan](261019-1534-vfs-storage-search-research/plan.md)).
- [ ] **Cellbox physical/production gates** — blocked (Intel vật lý, production). Deferred trong
  [plan](261020-1750-cellbox-cli/plan.md): F-GP-1 (`Bad packet length` 512 KiB), OSD boot proof,
  Tier-3 lane re-pin, D60-3 clock-unit; CBX-24 partial (KSSH).
- [ ] **Ginux Phase 02 debt #23 (owner admission)** — blocked bởi
  [tier1-baseline-admission](260821-1700-tier1-baseline-admission/plan.md) Phase 03
  ([phase-02](261030-1945-ginux-lessons-hardening/phase-02-mutable-global-inventory.md)).
- [ ] **KVM `-cpu host`: test-hooks x86 kernels triple-fault in the VMX smoke** (found 2026-11-05).
  - `hal/arch/x86/src/x86_64/vmx_vcpu.rs`: `HOST_GS_BASE` written from the low 32 bits only; `HOST_TR_BASE` never written; `EXIT_ACKNOWLEDGE_INTERRUPT` set but the external-interrupt exit is not handled (IPI 0xf0 stuck in ISR). Since `15830f4fd`.
  - Fix proven in a throwaway worktree (mlibc witness PASS on KVM `-cpu host`): [report](debug/debug-261105-1036-kvm-host-vmx-boot-exit.md), [patch](debug/debug-261105-1036-kvm-host-vmx-boot-exit.patch). Touches `hal/arch/x86/**` and `kernel/src/hypervisor/svm_registry.rs`, so it needs the Intel lane and a handoff. Until then KVM runs use `X86_CPU_MODEL=qemu64,+pdpe1gb`.
  - The patch exposes an existing SVM-guest triple fault on TCG (`[hv-x86] guest triple-fault: rip=0x6663`); not investigated.
- [ ] **aarch64 secondary-core wait counts timer ticks with interrupts masked** (`kernel/src/task/smp.rs`, found by Apple A1): a core that never comes online hangs boot instead of falling back to one core.
- [ ] **`scripts/check-posix-ledger.py` fails on 20 Tier-A rows** (`docs/posix-ledger.md:223-259`), present before the 2026-11-05 runtime merges.

**C2C program (mở, có gate):**
- [ ] Reconcile one exact Intel machine against the HCL (máy x86 Dell mini PC là ứng viên); physical
  bring-up và acquisition gated riêng. Bắt buộc kiểm thử đạt trên cả 3 môi trường (QEMU TCG, QEMU/KVM,
  VMware) trước khi nạp lên board thật. Kèm: Ginux Phase 03 KVM baseline cần máy vật lý có
  governor/timer metadata. No AMD/new ARM/RISC-V program.
- [ ] **Intel VMX/EPT:** VMX vCPU backend đã landed (`15830f4fd`: VMCS accessors, `VmxVcpu`,
  VMLAUNCH/VMRESUME, CR0/CR4 fixed MSRs) nhưng **chưa chạy thật** (TCG không có VMX; VMX AP path chưa
  exercise). Còn: qualify guest lifecycle/storage/network trên Intel
  ([tier3b-x86-vtx P09](260711-1917-tier3b-x86-vtx/plan.md)); C2C Phase 09 guest bridge phụ thuộc mục này.
- [ ] Qualify two independently verified Intel nodes for LAN C2C after first-node bring-up,
  including restart, authority denial, disconnect/reconnect and uncertain outcomes.
- [ ] Close explicit all-tier adapters and relay identity/time/persistence gates;
  measure a bounded workload against Linux on identical hardware/semantics.

## Desktop Ecosystem & GUI Roadmap

[Unified desktop](261019-1310-unified-desktop/plan.md) ghi 11/11 pha completed (2026-10-19); theme,
session tabs, taskbar/Spotlight, snap/F11, Files, Quick Look, GUI Terminal và đóng gói desktop x86
(`scripts/assemble-x86-image.py --profile desktop`, promote S10-5 `8d9c8165c`) đã có trong code.
Còn mở:
- [ ] **Tier 3 app thật trong VM:** pha 10 ghi completed nhưng `cells/apps/chrome/src/main.rs` chỉ vẽ
  UI kiosk trên host (không tạo VM; chuỗi `Intel VT-x (VMX root)` là literal). Cần guest app-only
  fullscreen, interactive host tab, resize/input/dialog/teardown; không dùng log launcher thay bằng
  chứng. Phụ thuộc mục Intel VMX/EPT ở trên.
- [ ] **File Association & MIME Router:** chưa có registry/Open With/launch intent (không có
  `mime`/`open_with` trong `cells/apps` hay `libs/api/src/services`; Quick Look chỉ hiện gợi ý
  "Open With"). Theo [hợp đồng đề xuất](261019-1310-unified-desktop/contracts.md): tách Open/Preview
  khỏi Run script/ELF, quyền tối thiểu, gate ABI nếu cần; nối vào Files + Desktop launcher.
- Deferred by approved scope (plan): Files split-pane/cây thư mục đầy đủ, Trash/Undo, drag-to-group/detach.

## Khắc phục bảo mật & hiệu năng (audit 2026-10-21)

Gate cũ "chờ cellbox-cli" đã mở: Cellbox đóng 2026-10-31
([ADR-0028](../docs/decisions/0028-cellbox-cli-first-structured-pipelines-and-tui.md),
[plan](261020-1750-cellbox-cli/plan.md)). Mỗi mục vẫn phải được chọn làm slice (security repair) trong
portfolio; nhiều mục chạm `kernel/src/task/syscall.rs` nên làm tuần tự. Nguồn: audit đọc-only 4 agent;
cite dòng theo tree 2026-10-21 — đọc lại theo tên symbol trước khi sửa.
Không trùng với `docs/roadmap/open-risk-register.md` (signing/dev-key/weak-rng/socket CapId/Spectre/VMX đã ghi ở đó).

**Đợt 1 — Critical/High, sửa trước:**
- [ ] **sshd xác thực mọi credential:** `cells/services/sshd/src/session.rs` nhánh `SSH_MSG_USERAUTH_REQUEST`
  gửi `USERAUTH_SUCCESS` vô điều kiện. Cần public-key auth thật với authorized keys được provision;
  chỉ vào `ActiveSession` sau khi verify.
- [ ] **sshd private key hard-code:** `cells/services/sshd/src/kex.rs` (seed host key Ed25519 literal,
  `StaticSecret::from([...])` X25519 tĩnh cho mọi handshake). Host key qua KMS/sealed storage;
  X25519 ephemeral mới mỗi handshake từ `GetRandom`.
- [ ] **`ShmAlloc`/`ShmMap` còn thiếu quota và record đầy đủ:** owner-bind (cell, generation) + thu hồi
  khi owner retire đã có (`990c04d08`); còn: `ShmAlloc { size: _ }` bỏ `size` (luôn 1 frame), handle =
  địa chỉ vật lý, không tính vào cell quota, `ShmMap { target_pid: _ }`, chưa có recipient/rights/revoke.
- [ ] **`GpuFlush` deref con trỏ user chưa validate:** `syscall.rs` nhánh `Syscall::GpuFlush`
  `from_raw_parts(data_ptr, data_len)` sau check số học; nhánh driver forward raw address. Đọc qua
  `TaskCopyView` hoặc grant pinned có owner-check, gửi grant id thay địa chỉ.
- [ ] **PTE cài trước khi reserve ledger → mapping mồ côi khi lỗi:** `kernel/src/memory/address_space.rs`.
  Reserve ledger/rollback trước khi đổi page table, hoặc unmap + TLB invalidate trên mọi nhánh lỗi.
- [ ] **httpd `GET /files/...` không auth + VFS root rule `allow_read_all: true`:**
  `cells/services/httpd/src/router.rs` (`Route::File`), `cells/services/vfs/src/access/rules.rs` (rule `/`).
  Giới hạn document-root; thay global read rule bằng capability theo caller.
- [ ] **`L2Send/L2Recv` và `MulticastJoin/Leave` không kiểm tra caller/cap:**
  `cells/services/net/src/handlers.rs` (ai cũng inject/drain frame thô; `cap_id: _`).
  Yêu cầu binding hypervisor sống hoặc capability raw-L2; multicast refcount theo socket owner.
- [ ] **Supply chain:** `cells/services/vfs/Cargo.toml` `fatfs` git không `rev`; `deny.toml` thiếu `[sources]`;
  `.github/workflows/release.yml` `dtolnay/rust-toolchain@master` với `contents: write`;
  `Dockerfile` `ubuntu:latest` + `curl | sh`. Pin SHA/digest, commit lockfile, deny unknown git sources.
- [ ] **Non-x86: MMIO VirtIO/ECAM/peripheral map `USER` lúc boot** (`kernel/src/memory/paging.rs`) và
  **stack RWX** trên RISC-V/AArch64 (`remap_range_user` có `EXECUTE`). Ưu tiên thấp hơn vì ADR-0022 chỉ x86,
  nhưng phải sửa trước khi mở lại bất kỳ lane ARM/RISC-V nào.

**Đợt 2 — Medium bảo mật:**
- [ ] CSpace `expiry` lưu và attenuate khi derive nhưng không enforce theo thời gian ở `verify_ticket`/`park_file`
  (`kernel/src/cell/cspace.rs`).
- [ ] `RunVcpu`/`VcpuRegs` forward raw pointer sau `validate_user_buf` → dùng struct kernel-local + `TaskCopyView`.
- [ ] Audit ring: ring đầy nay evict record cũ nhất và đếm (`990c04d08`), nhưng `drain()`/`dropped_count()`
  vẫn không có caller (`kernel/src/audit.rs`). Thêm drain interface có quyền + event synthetic ghi số record đã mất.
- [ ] Beacon replay: `boot_epoch = sys_get_time_ms()`, state không persist
  (`cells/services/net-broker/src/local_runtime.rs`, `beacon.rs`).
- [ ] sshd một client pre-auth chiếm slot duy nhất + `rx_buf` không giới hạn (`sshd/src/main.rs`, `session.rs`).
- [ ] Allowlist unsafe hết hạn chỉ in `NOTE`, không fail (`scripts/cellos_sign/policy.py` `stale_entries`,
  `cli.py`; `scripts/unsafe-allowlist.toml` `max_age_days = 90`). Làm stale = violation trong `--strict`.
- [ ] Docs lệch code: `docs/security-model.md:45` nói default G1 để `signing-required` off, `kernel/Cargo.toml`
  default **có**; `plan-portfolio.md` mâu thuẫn trạng thái 10 pha ADR-0024 ("COMPLETED MILESTONE" vs
  "ten-phase DAG remains pending").

**Đợt 3 — Hiệu năng (làm benchmark harness trước; `evidence/benchmark-std-parity-*.json` là `fixture_only`, chỉ đo syscall-yield):**
- [ ] x86 TLB: `kernel/src/memory/tlb_shootdown.rs` vẫn gọi `tlb_flush_all()` cho mọi thay đổi và probe ack
  2×10⁹ TSC (completion/ack đã sửa ở `1917c7448`, `81ceceec4`). → `invlpg`/`INVPCID` theo địa chỉ; defer frame
  reuse qua `deferred_release`.
- [ ] Scheduler tick hart 0 giữ `SCHEDULER` quét toàn bộ `tasks` nhiều lần + `collect()` allocate
  (`kernel/src/task/scheduler.rs`). → index deadline/heartbeat/poll; làm việc ngoài lock.
- [ ] Frame allocator: `find_free` quét từng bit, `find_free_run` O(F×N), dưới `FRAME_ALLOCATOR` lock
  (`kernel/src/memory/frame.rs`). → `trailing_zeros` + bitmap tầng 2 hoặc buddy.
- [ ] Recv copy payload vào `Vec` tạm dưới `SCHEDULER` rồi copy lần 2 ra user (`syscall.rs`).
- [ ] `kernel/src/task/async_ipc.rs`: `Vec::remove`/`position` dưới `SCHEDULER` → slab theo token.
- [ ] User-copy probe `page_proof_for`: `ledger.lock().iter().find_map` mỗi page → O(N·L) (`address_space.rs`).
- [ ] VFS LRU `touch` `retain()` O(n) mỗi hit (`cells/services/vfs/src/page_cache.rs`).
- [ ] Net: maintenance wake cố định 10 tick ≈ 100 ms (`cells/services/net/src/service-runtime.rs`) → deadline từ
  smoltcp `poll_at` + NIC IRQ; `Box<[u8]>` mới mỗi frame RX/TX (`interface.rs`) → frame pool.

**Kiến trúc (không có fix đơn lẻ, ghi để không quên):** `syscall.rs` 11 863 dòng / `snapshot.rs` 234 KB (x86
`kernel_image()` trả `None`) / `cell_session.rs` 132 KB; CI chưa build kernel với
`--no-default-features --features production-relay-image` (chỉ chạy test của checker script); provenance
`kernel/src/embedded-*/kernel_fs.img` mâu thuẫn giữa `kernel/build.rs` (gitignored) và
`scripts/build-x86_64-c2c-lifecycle-ci.sh` (committed) **(verify)**.

## Rust `std::thread`/`std::sync` trên Cellos — thread = kernel task cùng Cell (nghiên cứu 2026-10-21)

Gate "chờ cellbox-cli" đã mở (Cellbox đóng 2026-10-31); chưa admit trong portfolio.
Báo cáo: [research-261021-1540-std-threads-cell-vs-os-thread.md](reports/research-261021-1540-std-threads-cell-vs-os-thread.md).
Quyết định: **Cell không thay OS thread** (Cell = authority/recovery/CSpace/quota; `std::sync` cần heap + atomic chung;
`FutexWait` key có generation nên by design không wake xuyên Cell Tier‑2, `kernel/src/task/futex.rs:1-12`). Triển khai
theo mẫu Hermit (`sys/thread/hermit.rs` + `sys/sync/futex/hermit.rs`) / Motor OS. Actor/C2C (ADR‑0021) là tầng trên `std`.
Substrate kernel đã có: `Spawn=5` (task cùng Cell), `FutexWait=17`/`FutexWake=18`, `Wait=8`,
`SetTlsBase=9`; `libs/port-platform/cellos_pthread.c` đã dùng đúng bộ này. Thiếu: PAL, `PT_TLS`, số hart.

- [ ] **PAL `library/std/src/sys/thread/cellos.rs`** (sửa `patches/rust-std-cellos.patch`, hiện `Thread = unsupported`):
  `Thread::new` = `Box<ThreadInit>` → `sys_spawn(thread_start, ptr)`; `thread_start` chạy `init.init()()`,
  `thread_local::destructors::run()`, `rt::thread_cleanup()`, `sys_exit(0)`; `join` = `Wait=8`; `sleep` = TIMER completion
  (không busy-yield); `available_parallelism` = số hart — **cần syscall/`MemInfo` trả số hart** (hiện hard-code 1 trong patch).
- [ ] **`sys/sync/futex/cellos.rs`**: `futex_wait(&AtomicU32, expected, timeout)` → `FutexWait=17` (Duration→tick, 0 = vô hạn;
  `ValueMismatch`→`true`, `TimedOut`→`false`); `futex_wake` → `FutexWake=18(count=1)`; `futex_wake_all` → `count=0`.
  Thêm `target_os = "cellos"` vào nhánh futex của `sys/sync/{mutex,condvar,rwlock,once}/mod.rs` và `thread_parking`.
  **Không viết Mutex/Condvar riêng.**
- [ ] **TLS**: loader expose `PT_TLS` (size/align/init image/tbss) qua launch profile (`docs/guides/tier1b-c-zig.md`);
  runtime user cấp block per-thread (+ TCB self-pointer cho x86_64 variant II) và gọi `SetTlsBase` đầu `thread_start`;
  `target_thread_local = true` trong `targets/*.json` → std dùng `thread_local/native` (bỏ `no_threads` trong patch).
  Không đi đường key-based `thread_local/os`. Cũng gỡ chặn C `__thread`/`thread_local` (residual của cell-native portability).
- [ ] **Kernel**: xem lại `MAX_THREADS_PER_CELL = 32` (`scheduler.rs`) và ràng buộc stack contiguous `STACK_PAGES+1`
  (đã ghi ở risk register D5); xác nhận Tier‑2 Cell spawn thread (map stack vào domain, clone PKU) và `Wait=8` trả về khi
  worker bị kernel kill (cần cho `thread::scope`).
- [ ] **Witness `tests/rust-std-promotion`**: `thread::scope` + `Arc<Mutex<Vec>>` + `Condvar` + `thread_local!` destructor
  + 32 create/join vòng; đối chiếu `cells/tests/c-pthread/src/witness.c`.
- [ ] **Docs**: ADR mới "std threads = intra‑Cell kernel tasks; Cells are not threads"; bỏ invariant
  `available_parallelism = 1` ở `docs/roadmap/runtime-and-platform-tracks.md`; mở class B (OS threads) của
  ADR‑0018 §2.2 sang L1 khi PAL + TLS xong.
- [ ] **POSIX theo sau (ADR‑0018, không mở rộng phạm vi)**: khi TLS + futex PAL xong → mở rộng `cellos_pthread.c`
  (rwlock, barrier, `pthread_key`, detach, `__thread` C) và bật pthread trong mlibc `vicell` (thay futex yield-spin
  `third_party/mlibc/sysdeps/vicell/generic/generic.cpp`); `poll`/`select` → `WaitCompletion` khi có IPC source;
  `fork`/`dlopen`/`mprotect`/signals/`mmap(MAP_SHARED)` giữ `ENOSYS` → Tier 3.

## Parked / deferred backlog — subject to the current queue

Chỉ còn việc chưa xong. Old `[in-progress]`/`next` wording is not permission to resume; preserve
source/tests/evidence and do not rerun recorded failures just to reconfirm them.

- [ ] **Tier 3 x86 persistent backend — A/C/D cần trace khi tái hiện** (lỗi `seg_max` (B) đã sửa, xem
  `CHANGELOG.md`). Lane: `scripts/qemu-x86-virtio-e2e.sh`. Không quy chung nguyên nhân khi chưa có trace.
  (A) **VFS short read**: `[hv-blk] VFS read response: GrantDone { bytes: 15360 }` cho request 64 KiB; fatfs thấy
  `guest_disk.img` 850944 B trong khi file host 16 777 216 B. `[vfs] short read ... fatfs_size=...` đã có; bước kế:
  đối chiếu kích thước ở mount và sau `write_at` với directory entry trên host cùng boot. Cùng lớp: FAT nhúng
  (VIFS1) đọc `/bin/bench-probe` 78 760 B so với entry 78 824 B — `FatFile` read path đã viết lại ở `81ceceec4`
  **(verify)** ([report](reports/d5-cell-scale-remeasure-261003.md)).
  (C) **flush ngắt quãng ở tầng raw**: `[hv-blk] VFS flush failed: Ok(Err(1))` + `[hv-blk-host] request failed
  type=4 … status=1`; batch gần nhất 5/5 xanh. Thu log cả hai tầng khi tái hiện; không thêm retry khi chưa phân loại.
  (D) **flaky boot Cellos**: 1/5 run `FAIL: evidence rdinit was not selected in run 1`. Chưa điều tra.
- [ ] **Tier 3 application evidence:** Python gate (`scripts/qemu-x86-python-gate.sh`, Alpine 256 MiB) PASS trên
  QEMU-TCG; job CI `qemu-x86-tier3-python` chưa có kết quả hosted quan sát được **(verify)**; lần lỗi apk kế tiếp cần
  `QEMU_NET_CAPTURE` để tách CDN/SLIRP/guest. Cổng **browser** còn mở: RAM, virtio-gpu scanout, virtio-input và tương
  tác thật qua compositor. Boot-to-shell ARM64 nghiêm ngặt cần KVM/phần cứng thật (QEMU-TCG chỉ là machinery evidence).
- [ ] **D5 scale profiles (parked, portfolio "Parked candidates"):** đo 2026-10-03 được 295 cell với boot heap 4 MiB;
  heap 64-bit nay 32 MiB (`1d791550d`, `kernel/src/main.rs` `HEAP_FRAMES`) ⇒ baseline N=64/128/256/512 với M heavy
  phải đo lại (`scripts/qemu-cell-scale.sh --heavy M`,
  [report](reports/d5-cell-scale-remeasure-261003.md), `docs/roadmap/beam-parity-backend-roadmap.md` §2.2–2.3). Đòn bẩy/nợ liên quan:
  - chi phí heap ≈9,7 KB/cell, phần lớn là `CellSegments.pages` ⇒ chia sẻ trang immutable của ELF hoặc nén danh sách segment;
  - heap phía cell là arena tĩnh (`ostd::heap`, `declare_custom_heap!`), chưa có syscall grow (`brk`/`mmap`-style);
  - `Scheduler::cell_owners` vẫn là `Vec` đặc `MAX_CELLS` slot → `BTreeMap<u32, CellOwnerSlot>`;
  - allocation infallible còn lại trên đường spawn/exit: `BTreeMap::insert`, `queue.clone()` trong `scheduler::exit_task`,
    `Box::new` trong `spawn_with_stacks_configured`, `to_vec`/`clone` nhỏ trong test-hooks;
  - cell không load khi khách có ≥4 GiB RAM: `ELF: load VA 0x100000000 already mapped` cho `/bin/platform` và init
    (VA base cell đụng mapping kernel) — chặn mọi phép đo ở 4 GiB.
- [ ] **Beam-parity B1/B2** (B0 done): B1 concurrency trong cell + cancellation (cần ADR cancellation); B2 cost/scale
  per-request (WIP-limited cùng D5).
- [ ] **Cell-native portability residual:** port bên thứ ba cần `fork`/`exec` (process tree) vẫn class D; C TLS xem mục std thread.
- [ ] **mlibc** chưa hoàn tất: `third_party/mlibc` chỉ có `sysdeps`, không có `build*/libc.a`; thiếu
  `malloc`, `printf`, `free`, `clock_gettime`.
- [ ] **Manifest & tooling (item 18):** Manifest v2 + tooling done (`85df7fc0f`); đổi field vật lý blocked chờ Manifest v3
  + phê duyệt ABI riêng (tách `execution_tier`, `runtime_profile`, `protection_class`, `capabilities`, `admission evidence`).
- [ ] **Cổng hoàn tất App tiers (item 20):** bỏ Tier 1b/3b/SDK L1/L2 ngoài compatibility; Tier 1 production admission;
  Tier 3 ít nhất một lane hardware-qualified; Tier 2 chỉ công bố khi private-domain containment có negative evidence;
  SDK module/profile matrix + examples khớp code. Ledger: `docs/app-tier-acceptance-ledger.json`.
- [ ] **Floor backend + production admission (items 1, 2, 7, 8, 9):** chọn floor backend & hardware; security review
  floor/A-B; production-admission authenticated runner (Phase 07 mới software-only); hostile + physical power-loss matrix;
  evidence bundle bất biến.
- [ ] **RPi3 (parked 2026-10-08, ADR-0022 "Explicitly deferred"):** SD, HDMI, I2C/SPI, EL2 monitor, USB DWC2, bàn phím
  và guest Alpine tới `~ #` đã chạy trên board; IPC/routing/MAC/data-toggle/Tx-FIFO của đường LAN9514 đã sửa. Blocker
  còn lại: **thiết bị LAN9514 tự rời bus** (`GINTSTS.DISCONNINT`, `HPRT0=0x400`, `HFNUM` đóng băng, `GINTMSK` còn đúng)
  trong cửa sổ không có transfer USB. Ảnh cuối `cellos.uimg` `7389e532…` (lượt 56). Ba luồng mở:
  (1) recovery re-enumeration khi mất port (`reset_port` → re-enumerate hub/bàn phím/LAN9514 → `init()` → reset
  toggle/FIFO → đăng ký lại front-end); (2) thử nguồn 5 V ≥2,5 A và một boot không cắm bàn phím (brownout);
  (3) ảnh chẩn đoán `no-hid-poll`. Còn: MAC placeholder `02:00:00:00:00:01` phải duy nhất theo board; `--app`
  (stage app + `rdinit=/bin/<app>`, init guest `scripts/pi-guest-init`, initramfs qua `tools/prepare-guest-initramfs.py`)
  và lớp base 512 MiB cho browser; surface virtio-gpu của guest vào compositor; trim driver cho board khác. RPi3 không là
  đích Tier 3 (vào EL2 rồi `eret` xuống EL1). Plan: [phase-05-lan9514](260823-rpi3-hardware-completion/phase-05-lan9514-ethernet.md).
- [ ] **RISC-V board bring-up** (StarFive VisionFive 2, Pioneer) — parked theo ADR-0022.

## Blocked (chờ phần cứng hoặc governance)
- [blocked] **`cohort.dirty_bundle` còn worktree-sensitive** (`scripts/app_tier_acceptance/cohort.py`): so bytes của
  `patch.path` với `git diff --binary <revision>` của *worktree hiện tại*, nên một claim `dirty: true` sẽ đỏ ngay khi
  có commit sau đó. Ledger thật hiện chỉ có 1 claim không có `dirty`/`dirty_bundle`; đổi ngữ nghĩa là quyết định
  thiết kế của owner.
- [blocked] **SDK relay client mutual TLS**: đường relay hai real-broker bị chặn bởi các entry gate
  protected-persistence, authenticated-time và reviewed pending-key-binding dưới KMS opcodes 9–14 đã freeze trong
  `.agents/260825-1726-kms-silo-production-root/phase-04-service-net-mutual-tls-integration.md`;
  chỉ mở lại khi DEV_REFERENCE Phase 8 phát ra đúng `GO: PHASE4_ENTRY_GATES_SATISFIED`. Không phải blocker toàn
  cục cho single-guest hay việc local khác.
- [blocked] **Phase 04 two-node/direct-LAN** theo authority mTLS; không chặn single-guest gate.
- [blocked] **Phase 06 trên ARM64**: synchronous TCG fault xảy ra trước guest probe; không hạ gate và không suy diễn
  PASS từ kết quả x86. `287377e5d` cho ARM64 hostile runner chạy mọi probe stimulus **(verify)**.
- [blocked] **App Tiers completion** (item 17): cần phần cứng (RPi4b + secure controller riêng, hoặc secure boot +
  remote CAS service). Tier 1 baseline và Tier 1 rust std done (ledger Phase 06 `IMPLEMENTED` 2026-09-29;
  promotion/approvals vẫn blocked); Tier 3 blocked.
- [blocked] **AI inference server demo = G2 Level A** (item 21): đường CPU done (`/bin/ai`, `service::AI = 15`);
  NPU RK3588 + P99 bound + front HTTP chờ board; **[owed]** Law 1 xác nhận 2 lần cho interface AI trước khi coi ABI frozen.
- [blocked] **Port Drivers phase 06**: cần board RK3588 (Radxa ROCK 5B) để boot và giữ UART log; chốt phiên bản/giấy
  phép/quyền phân phối RKNN SDK; chưa chạy inference thật (load → input → run → output → cleanup và các đường lỗi);
  chưa chứng minh buffer/DMA/cache lifecycle và quyền sở hữu IOMMU/SMMU của NPU; chưa có P50/P95/P99,
  memory-pressure, restart/fault-injection, stale DMA; chưa có X390 cho implementation thứ hai.
- [blocked] **Hardware + AWS cho KMS Silo**: StarFive VisionFive 2 v1.3B, STM32H573I-DK,
  Infineon OPTIGA TPM 2.0 SLB9672, và AWS DEV account/region.
- [blocked] **Production trust keys**: test/release key separation đã landed (`a88658288`, `release-posture`,
  `CELLOS_RELEASE_CELL_PUBLIC_KEY`); còn provisioning release key thật (CI secret) và đường public-key fleet
  (`docs/security-model.md` dòng 45) **(verify)**.
- [blocked] **Ginux physical:** Intel vật lý, KVM và durable (CellosFS-owner) recovery không được claim; xem
  [plan](261030-1945-ginux-lessons-hardening/plan.md).
