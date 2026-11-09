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
- [ ] **C2C slice tiếp theo** (Phases 03 & 08 done) — Phase 01 đã sign-off 2026-10-09, Phase 02 slice A+B xong, Phase 03e closure xong, Phase 08 fastpath xong (`885525b20`). Toàn bộ execution còn lại bị khoá bởi `CELL-CONTRACT-0024`: cần owner handoff theo slice ([plan](260927-1100-c2c-anywhere-tier-aware/plan.md), [locks](cell-contract-0024-locks.md)).
- [ ] **VFS/storage/search research** — parked; chỉ tiếp tục khi owner yêu cầu
  ([plan](261019-1534-vfs-storage-search-research/plan.md)).
- [ ] **Cellbox physical/production gates** — blocked (Intel vật lý, production). Deferred trong
  [plan](261020-1750-cellbox-cli/plan.md): F-GP-1 (`Bad packet length` 512 KiB), OSD boot proof,
  Tier-3 lane re-pin, D60-3 clock-unit; CBX-24 partial (KSSH).
- [ ] **Ginux Phase 02 debt #23 (owner admission)** — blocked bởi
  [tier1-baseline-admission](260821-1700-tier1-baseline-admission/plan.md) Phase 03
  ([phase-02](261030-1945-ginux-lessons-hardening/phase-02-mutable-global-inventory.md)).

**C2C program (mở, có gate):**
- [ ] Reconcile one exact Intel machine against the HCL: khảo sát và chọn máy Dell OptiPlex 7080 Micro đã hoàn tất (`docs/roadmap/intel-x86-hcl-procurement-survey.md`); physical bring-up và acquisition gated riêng (chờ máy thật). Kiểm thử đã đạt trên cả 3 môi trường ảo hoá (QEMU TCG, QEMU/KVM, VMware). Kèm: Ginux Phase 03 KVM baseline cần máy vật lý có governor/timer metadata. No AMD/new ARM/RISC-V program.
- [ ] **Intel VMX/EPT:** VMX vCPU backend đã verified chạy thật dưới KVM hardware acceleration (`VMXON` active, `X86-VMM-SMOKE: PASS (api-path PortOut+Hlt)` qua VMLAUNCH/VMRESUME và VMCS decode). Còn: qualify full guest lifecycle/storage/network trên Intel ([tier3b-x86-vtx P09](260711-1917-tier3b-x86-vtx/plan.md)); C2C Phase 09 guest bridge phụ thuộc mục này.
- [ ] Qualify two independently verified Intel nodes for LAN C2C: kịch bản ảo hoá 2 node kết nối qua bus multicast và kiểm tra khả năng phục hồi khi nút rời mạng / khởi động lại kết nối lại (peer departure / restart reconnection) đã hoàn tất và PASS trên KVM (`scripts/qemu-c2c-node-restart-test.sh`, `72a0aa4a3`). Còn: đo kiểm trên 2 node vật lý sau khi mua máy.
- [ ] Close explicit all-tier adapters and relay identity/time/persistence gates;
  measure a bounded workload against Linux on identical hardware/semantics.

## Desktop Ecosystem & GUI Roadmap

[Unified desktop](261019-1310-unified-desktop/plan.md) ghi 11/11 pha completed (2026-10-19); theme,
session tabs, taskbar/Spotlight, snap/F11, Files, Quick Look, GUI Terminal và đóng gói desktop x86
(`scripts/assemble-x86-image.py --profile desktop`, promote S10-5 `8d9c8165c`) đã có trong code.
Còn mở:
- [ ] **Tier 3 app thật trong VM:** IPC contract `vmm_session` (`VmmLaunchRequest`, `VmmLaunchReply`, `VmmInputForwardRequest`), `ScanoutBridge` `SurfaceRole::Interactive`, client wiring trong `chrome` và `vm-runner` (`c47ccafae`), và server-side dispatcher trong `hypervisor` (`run_loop_x86.rs:service_app_session_ipc`, `b31d55dba`) đã hoàn tất; smoke test x86 VMM PVH guest boot đạt PASS trên QEMU 10.2.0 (`scripts/qemu-hypervisor-smoke-x86.sh`). Còn: đo kiểm GUI tương tác trực tiếp qua desktop compositor khi chạy full image.

## Kiến trúc & nợ kỹ thuật tồn đọng
**Kiến trúc (không có fix đơn lẻ, ghi để không quên):** `syscall.rs` 11 863 dòng / `snapshot.rs` 234 KB (x86
`kernel_image()` trả `None`) / `cell_session.rs` 132 KB; CI đã bổ sung bước build-check kernel với
`--no-default-features --features production-relay-image` trong job `kms-ssh-dev-posture`;
provenance `kernel/src/embedded-*/kernel_fs.img` đã làm rõ: canonical `embedded-x86_64` được commit có `manifest.json` bảo vệ, các lane khác tạo động qua `EMBEDDED_OVERRIDE` (build script), `build.rs` đã cập nhật mô tả.

## Parked / deferred backlog — subject to the current queue

Chỉ còn việc chưa xong. Old `[in-progress]`/`next` wording is not permission to resume; preserve
source/tests/evidence and do not rerun recorded failures just to reconfirm them.

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
- allocation infallible còn lại trên đường spawn/exit: `BTreeMap::insert`, `queue.clone()` trong `scheduler::exit_task` đã loại bỏ hoàn toàn bằng `task.completion.take()`, `cell_owners` đã chuyển thành sparse `BTreeMap` (commit `3d6a31617`), `Box::new` trong `spawn_with_stacks_configured`, `to_vec`/`clone` nhỏ trong test-hooks;
  - cell không load khi khách có ≥4 GiB RAM: đã nâng `CELL_VA_START` lên 64 GiB (`0x10_0000_0000`), loại bỏ triệt để va chạm identity-map với RAM vật lý cao.
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
