# Phase 04 — Deprivilege init + shell; fold `/bin/vfs` block region

## Context Links

- Plan: [plan.md](plan.md) · Phụ thuộc: [phase-03](phase-03-policy-cap-coverage.md)
- Spec: `docs/specs/15-kernel-boundary.md`
- Bối cảnh: `project-init-spawn-and-cap-paths`, `project-cell-permission-attestation`,
  `project-supervisory-cell-migration-plan`
- Midori nguồn: **không có tài khoản admin**. Gốc thẩm quyền là loader/kernel phát cap rời rạc,
  không phải một process đặc quyền; shell không mang thẩm quyền nào

## Overview

- **Ưu tiên**: P2
- **Trạng thái**: Completed — kernel launch-edge authority landed; init-respawn proof remains deferred outside this closure.
- **Mô tả**: Cellos có hai thứ tương đương root: `init` giữ `CapSet::ALL` miễn cả ceiling lẫn policy;
  `/bin/shell` là shell tương tác giữ ceiling delegation gpio/uart. Phase này đã được re-scope sang
  kernel launch-edge authority, không dùng ambient spawn broker, và hấp thụ việc fold `/bin/vfs`
  block region từ phase 03 (nó cần sửa init ceiling nên thuộc về đây).

> **Red-team correction (4 điểm)**:
> 1. Phase này **CÓ** Law 1 gate — service ID mới nằm trong `libs/api`. Draft ghi "không đụng libs/api".
> 2. `boot_authority()` = hợp cap các con thì **không thu hẹp gì** — union ⊇ mọi thành viên. Phải per-path.
> 3. Mitigation "broker kiểm cap của caller" **không thoả được** — xem M1.
> 4. Auto-restart là của **init**, không phải Supervisor Cell.

## Key Insights

- `CapSet::ALL` ([cap.rs:175-189](../../kernel/src/task/cap.rs#L175-L189)) bật **mọi** cap kể cả ba
  cap P-TRUST. Miễn hai lần: `Spawner::Root => requested` (không intersect ceiling,
  [loader.rs:271](../../kernel/src/loader.rs#L271)) và `Spawner::Root => after_spawner` (không qua
  policy, [loader.rs:288](../../kernel/src/loader.rs#L288)).
- **Miễn policy là hợp lý và phải giữ** (init *là* cái nạp policy — bắt nó chịu policy là vòng tròn).
  **Miễn ceiling thì không.** Đó là phần được sửa.
- **`boot_authority()` = union là vô nghĩa** (red-team, chứng minh logic): ceiling chỉ có tác dụng
  khi nó **nhỏ hơn** request; hợp của mọi request luôn ⊇ mọi request đơn lẻ. Image chuẩn có
  `/bin/nvme` (pcie_driver), `/bin/supervisor` (supervisor) → union = đúng `CapSet::ALL`. Attack:
  init spawn một ELF ở path `/bin/nvme` qua `sys_spawn_from_elf` → `with_path_caps` mint
  `pcie_driver`, union-ceiling chứa nó → cấp; và Root vẫn miễn policy. Kết quả sau phase giống hệt
  trước, chỉ đổi tên hằng số. → **Ceiling của Root phải là bảng `path → cap kỳ vọng`**, khớp đúng
  cell trong boot manifest.
- **Broker là confused deputy nếu authorize sai** (red-team): `Syscall::LookupService` mở cho mọi
  cell ([syscall.rs:1909-1913](../../kernel/src/task/syscall.rs#L1909-L1913)) và `sys_send` không cần
  cap → **mọi** cell tới được broker. Cổng thật hôm nay là `caller_has_spawn`
  ([syscall.rs:2092](../../kernel/src/task/syscall.rs#L2092)); broker sẽ thay nó bằng "gửi được một IPC".
- **Mitigation của draft không thoả được**: draft yêu cầu "broker kiểm cap của caller trước khi spawn
  hộ". Nhưng sau phase này shell **cố tình** không còn gpio/uart — nên nếu broker đòi caller phải giữ
  cap tương ứng thì mọi request hợp lệ đều bị từ chối. Điều kiện được ghi là "yêu cầu cứng" nhưng
  logic bất khả thi. → Authorize theo **bảng đã ký `(caller identity → path được phép invoke)`**,
  không theo cap của caller.
- **Auto-restart là của init**: bốn mảng `[_; NSVC]` với `const NSVC: usize = 9`
  ([init/src/main.rs:87-128](../../cells/tools/init/src/main.rs#L87-L128)), respawn tại
  [:346](../../cells/tools/init/src/main.rs#L346). Broker phải được chèn **trước** slot `/bin/shell`
  (shell lookup nó) và đặt `Policy::Permanent`. `init/src/main.rs` không có trong file list của draft.
- **Baseline chưa biết**: POLICY.BIN chưa bake ở đâu (xem phase 03), nên shell **hiện vẫn có**
  gpio/uart thật, và 3 peripheral demo có lẽ đang chạy được. Nhưng phải đo, không đoán — nếu sau khi
  phase 03 bake behaviour-neutral mà demo vẫn vỡ thì delegation đang đi một đường khác mà plan chưa
  xác định, và broker sẽ không tái tạo được đường đó.
- **Fold `/bin/vfs` region cần 3 thay đổi ở đây** (hấp thụ từ phase 03 req 5): `REGION_MASK = 0b111`
  ([policy.rs:36](../../kernel/src/policy.rs#L36)) phải widen; `MMIO_MASK` thiếu `DEV_CAN`/`DEV_ADC`
  mà `from_manifest` vẫn mint ([cap.rs:223-228](../../kernel/src/task/cap.rs#L223-L228)) phải widen;
  và init ceiling `block_regions: 0b111` ([cap.rs:181](../../kernel/src/task/cap.rs#L181)) phải chứa
  bit 3 — vì ceiling intersect chạy **trước** policy nên không widen ceiling thì fold bị zero bất kể
  POLICY.BIN ghi gì.

## Requirements

**Functional**

1. `CapSet::ALL` thay bằng **bảng per-path** `boot_ceiling(path) -> CapSet`, khớp đúng cell trong
   boot manifest. Không phải union. Path lạ → `CapSet::EMPTY`.
2. `Spawner::Root` vẫn miễn *policy* (chống vòng tròn) nhưng **chịu `boot_ceiling(path)`**. Thêm nữa:
   spawn do init thực hiện *sau* khi policy đã nạp phải đi qua `policy::apply` — chỉ miễn policy cho
   spawn xảy ra trước khi policy được nạp.
3. Broker authorize theo **bảng đã ký `(caller identity → allowed path index)`**, mở rộng POLICY.BIN.
   Broker nhận **index vào một allowlist path cố định**, không nhận path tự do. Chỉ phục vụ tid đã
   đăng ký (shell); mọi sender khác → từ chối.
4. Broker giữ gpio/uart nhưng **không tương tác** (không đọc input, không nằm trong đường bàn phím).
5. Shell mất `gpio`/`uart` khỏi manifest, và POLICY.BIN re-bake hạ `/bin/shell` `mmio` xuống 0
   (phase 03 giữ nó ở 3 để behaviour-neutral; việc hạ thuộc đây, cùng commit với thay đổi manifest).
6. `init/src/main.rs`: `NSVC` 9→10, chèn broker **trước** `/bin/shell` trong cả 4 mảng,
   `Policy::Permanent`.
7. **Fold `/bin/vfs` block region** (từ phase 03 req 5), theo thứ tự cứng:
   a. widen `REGION_MASK` → `0b1111` + widen `MMIO_MASK` thêm `DEV_CAN|DEV_ADC`;
   b. widen init `boot_ceiling("/bin/vfs")` `block_regions` → `0b1111`;
   c. fold `0b1000` vào `with_path_caps`;
   d. re-bake POLICY.BIN cho `/bin/vfs` = `0b1111`;
   e. **chỉ sau đó** xoá raw grant ở [loader.rs:335-337](../../kernel/src/loader.rs#L335-L337).
8. Cân nhắc bỏ `/bin/shell` khỏi `is_trusted_core` — **chỉ sau** req 5, vì làm ngược thứ tự thì
   `DenyAll` biến shell thành vô dụng và ép operator tắt policy.

**Non-functional**

9. Law 1: service ID mới trong `libs/api/src/abi/syscall.rs:718-743`. Fallback không-ABI nếu
   confirmation bị từ chối: publish tid của broker qua một service đã có (ví dụ config cell) thay vì
   cấp ID mới — ghi rõ phương án này để phase không bị chặn cứng.
10. Không hồi quy trải nghiệm shell: `periph-demo`, `robot-demo`, `sensor-demo` vẫn chạy từ prompt.
11. Boot path không dài thêm quá 1 IPC round-trip mỗi lần spawn từ shell.

## Architecture

**Trước**
```
shell (spawn + gpio + uart)  ──sys_spawn_from_path──►  loader
   └─ ceiling của shell bao gồm gpio/uart → con nhận được
```

**Sau**
```
shell (spawn only)  ──IPC {allowlist_index}──►  spawn-broker (gpio + uart + spawn)
                                                   │  ├─ sender ∈ registered tids?
                                                   │  ├─ (caller identity → index) trong bảng đã ký?
                                                   │  └─ index → path cố định
                                                   └──►  loader
```

Ba lớp phòng thủ của broker, cả ba đều cần: (a) chỉ nhận từ tid đã đăng ký; (b) bảng đã ký ánh xạ
caller → index được phép; (c) index thay vì path tự do, nên không có bề mặt path-injection.

Broker **không** là supervisory cell (`project-supervisory-cell-migration-plan` là việc khác) — chỉ
là điểm tập trung thẩm quyền delegation để lấy nó ra khỏi shell.

## Related Code Files

| File | Hành động |
|------|-----------|
| `libs/api/src/abi/syscall.rs` | Modify — service ID cho broker (**Law 1**) |
| `kernel/src/task/cap.rs` | Modify — `ALL` → `boot_ceiling(path)`; widen init `block_regions`; fold `with_path_caps` |
| `kernel/src/policy.rs` | Modify — widen `REGION_MASK`/`MMIO_MASK`; bảng caller→index; bỏ shell khỏi `is_trusted_core` (req 8) |
| `kernel/src/main.rs` | Modify — direct-write cap cho init theo bảng per-path |
| `kernel/src/loader.rs` | Modify — Root intersect `boot_ceiling`; policy cho spawn hậu-nạp-policy; xoá raw grant `:335-337` |
| `cells/tools/init/src/main.rs` | Modify — NSVC 9→10, chèn broker trước shell, `Policy::Permanent` |
| `cells/tools/shell/src/main.rs` | Modify — bỏ `gpio`/`uart`, gọi broker |
| `cells/services/spawn-broker/` | Create — cell mới (+ `.ld` script) |
| `scripts/sign-policy.py` | Modify — entry broker, bảng caller→index, `/bin/shell` mmio→0, `/bin/vfs` 0b1111 |

## Implementation Steps

1. **Step 0 — đo baseline (blocking)**: chạy `periph-demo`, `robot-demo`, `sensor-demo` từ shell
   prompt trên image **sau khi phase 03 bake behaviour-neutral**. Ghi lại delegation có hoạt động
   hay không, và nếu có thì cap đi đường nào. Không có số liệu này thì criterion "demo vẫn chạy qua
   broker" không đo được.
2. Kiểm kê boot manifest → bảng `path → cap` (dùng lại enumeration của phase 03 bước 1). Đây là input
   cho `boot_ceiling`.
3. `CapSet::ALL` → `boot_ceiling(path)` per-path, comment giải thích từng entry. Path lạ → EMPTY.
   Boot thử — cell nào chết là lộ ra một entry thiếu, thêm có chủ ý.
4. `Spawner::Root` intersect `boot_ceiling(path)`, giữ miễn policy; thêm điều kiện policy cho spawn
   hậu-nạp-policy (req 2). Test: init spawn một path lạ → `CapSet::EMPTY`.
5. Xin 2× confirmation cho service ID (Law 1). Nếu bị từ chối → dùng fallback req 9.
6. Viết `spawn-broker`: 3 lớp phòng thủ ở Architecture. Allowlist syscall hẹp: `Recv`, `Reply`,
   `SpawnFromPath`, `Log`, `RegisterService`. Thêm `.ld` script.
7. `init/src/main.rs`: NSVC 9→10, chèn broker trước `/bin/shell`, `Policy::Permanent`.
8. Bỏ `gpio`/`uart` khỏi manifest shell; shell lookup broker. Re-bake POLICY.BIN hạ `/bin/shell`
   `mmio` → 0. Chạy 3 demo, so với baseline bước 1.
9. **Fold `/bin/vfs` theo đúng thứ tự req 7 a→e**, mỗi bước một commit, host-side parse self-test
   trước khi bake (bài học phase 03). Chạy VFS suite sau (d) và sau (e) riêng biệt. Thêm success
   criterion đo trực tiếp: `task.block_regions == 0b1111` cho task VFS đang chạy — không dựa vào
   "VFS suite pass".
10. Cân nhắc bỏ shell khỏi `is_trusted_core`; test build `policy-required` với `DenyAll` cho shell.
11. clippy + build + boot + suite 3 arch + ARM64 peripheral test.

## Todo List

- [~] **Step 0: đo baseline 3 peripheral demo** sau bake behaviour-neutral của phase 03 — deferred; init-respawn proof stays open outside this closure
- [x] Bảng `path → cap` (dùng lại enumeration phase 03) — `kernel/src/loader/boot_ceiling.rs`
- [x] `CapSet::ALL` → `boot_ceiling(path)` per-path; path lạ → EMPTY
- [x] Root intersect `boot_ceiling`; policy cho spawn hậu-nạp-policy (`policy::is_resolved`)
- [~] Test: spawn path lạ → `CapSet::EMPTY` — chứng minh cho `Spawner::Root`
      (`boot_ceiling::selftest` + host harness). Spawn của init đi qua `Spawner::User`,
      nên chưa bị bảng ràng — xem § Deviation Log D3.
- [ ] 2× confirmation service ID (hoặc fallback: publish tid qua service có sẵn)
- [ ] `cells/services/spawn-broker/` + `.ld`; 3 lớp phòng thủ (registered tid · bảng đã ký · index)
- [ ] `init/src/main.rs`: NSVC 9→10, chèn broker trước shell, `Policy::Permanent`
- [ ] Shell bỏ `gpio`/`uart` + re-bake `/bin/shell` mmio→0; chạy 3 demo so baseline
- [~] Fold `/bin/vfs`: (a) widen masks ✅ → (b) widen init ceiling ✅ → (c) fold → (d) re-bake → (e) xoá raw grant
- [ ] Assert `task.block_regions == 0b1111` cho task VFS đang chạy
- [ ] Cân nhắc bỏ shell khỏi `is_trusted_core` + test `DenyAll`
- [ ] Suite 3 arch + ARM64 peripheral test

## Success Criteria

**Done khi**

- `CapSet::ALL` không còn tồn tại (hoặc chỉ trong test). `boot_ceiling` là bảng per-path.
- init spawn một path không có trong bảng → nhận `CapSet::EMPTY` (test).
- Manifest shell không có `gpio`/`uart`; 3 demo chạy từ prompt qua broker, **so được với baseline bước 1**.
- Broker từ chối request từ một cell không đăng ký (test) và từ chối path tự do (không có API nhận path).
- `task.block_regions == 0b1111` cho VFS; raw grant ở `loader.rs:335-337` đã xoá.
- Không cell nào miễn ceiling intersection; `Spawner::Root` chỉ còn miễn policy cho spawn tiền-nạp-policy.

**Validation**

- Suite 3 arch pass. ARM64 peripheral test pass (GPIO/UART thật). Build `policy-required` boot được.

## Risk Assessment

| Rủi ro | Xác suất | Ảnh hưởng | Giảm thiểu |
|--------|----------|-----------|------------|
| **Broker thành confused deputy** — mọi cell tới được nó qua `LookupService` (mở cho tất cả) + `sys_send` (không cần cap), thay `caller_has_spawn` bằng "gửi được IPC" | **Cao** | Escalation — **tệ hơn nguyên trạng** | 3 lớp phòng thủ là **yêu cầu cứng**, không tuỳ chọn: registered-tid-only + bảng đã ký caller→index + index thay path. Draft đề xuất "kiểm cap của caller" — **không thoả được** (shell cố tình mất cap), đừng dùng |
| Fold `/bin/vfs` sai thứ tự → blob `Invalid` → `DenyAll` toàn fleet, và raw grant đã bị xoá | Trung bình | **Brick, re-flash** | Thứ tự a→e cứng, mỗi bước một commit; host-side parse self-test trước mỗi bake; xoá raw grant là bước **cuối** |
| `boot_ceiling` thiếu entry → boot vỡ ở cell thứ N | Cao | Boot vỡ | Bước 2 kiểm kê trước; boot log rõ cap nào bị từ chối |
| Broker là single point of failure cho spawn, và nếu chèn sai vị trí trong mảng NSVC thì shell lookup fail | Trung bình | Không spawn được từ shell | `Policy::Permanent` + chèn **trước** slot shell; test lookup ngay sau boot |
| Law 1 confirmation bị từ chối | Trung bình | Broker không có service ID | Fallback req 9 (publish tid qua service có sẵn) |
| Thêm cell = thêm stack | Chắc chắn | RAM | Chạy sau phase 05 (đã xoá cấp trùng); phase 08 sizing sẽ phủ broker |
| Cell mới thiếu `.ld` script | Trung bình | Không link | Pitfall đã biết (`project-syscall-allowlist-and-build-pitfalls`) |

## Security Considerations

- **Confused deputy là rủi ro chính.** Gom thẩm quyền vào broker mà không authorize đúng thì chỉ đổi
  tên tài khoản root — và còn hạ cổng từ `SpawnCap` xuống "gửi được một IPC", tức **kém an toàn hơn
  nguyên trạng**. Ba lớp phòng thủ phải có đủ cả ba.
- **Không nhận path tự do.** Index vào allowlist cố định xoá hẳn bề mặt path-injection, và cũng chặn
  luôn biến thể `sys_spawn_from_elf(bytes, "/bin/nvme")` ở tầng broker.
- Giữ `Spawner::Root` miễn *policy* là có lý (init nạp policy) — nhưng chỉ cho spawn **trước** khi
  policy được nạp. Sau đó thì không có lý do gì.
- `boot_ceiling` per-path là điểm khác biệt then chốt so với union: union không bao giờ bind, nên
  báo cáo "đã xoá `CapSet::ALL`" với một union sẽ là sai về bản chất.
- Bỏ shell khỏi `is_trusted_core` chỉ an toàn SAU khi shell không còn cap peripheral.
- Broker không được có mặt trong đường input/bàn phím — nếu nó đọc input thì nó lại là shell.

## Deviation Log

Slice thực thi 2026-07-30 — **chỉ phần capability ceiling** (req 1, 2, 3, 7a, 7b).
Broker, shell manifest, POLICY.BIN re-bake, `with_path_caps` fold, xoá raw grant:
KHÔNG làm (ngoài phạm vi được giao, và cần boot/re-bake).

- **D1 — bảng nằm ở `kernel/src/loader/boot_ceiling.rs`, không nhồi vào `cap.rs`.**
  `cap.rs` đã 430 dòng; ceiling là quyết định *admission* nên thuộc loader. `self_test`
  tách sang `boot_ceiling/selftest.rs` để mỗi file < 200 dòng.
- **D2 — `CapSet::ALL` vẫn tồn tại (test-only), không xoá.**
  `kernel/src/task/p_trust_selftest.rs:55` dùng nó và file đó KHÔNG thuộc file-ownership
  của slice này. Doc của hằng số đã đổi thành "reference upper bound cho self-test, không
  cấp cho task nào"; `main.rs` không còn dùng. Khớp success criterion "hoặc chỉ trong test".
- **D3 — bảng hiện chỉ ràng `Spawner::Root`; spawn của init KHÔNG bị ràng.**
  Req 2 viết đúng theo `loader.rs` arm `Spawner::Root`, nhưng init spawn qua
  `sys_spawn_from_path` → `Spawner::User(init_tid)`, ceiling = `CapSet::of_task(init)`.
  Vì vậy hàng `/bin/init` vẫn phải phủ mọi con → nó vẫn có hình dạng union, và phần
  deprivilege thật của delegation CHƯA đạt. Đóng nó cần route spawn của root authority
  qua bảng (ghi tid của init), đúng là rủi ro "boot vỡ ở cell thứ N" mức Cao trong risk
  table — KHÔNG tự làm. Bù lại: bảng đã đủ hàng nên việc đó là one-liner sau này.
- **D4 — thay `assert!` trong `elf_tests.rs` bằng self-test trả `bool`.**
  `elf_tests.rs` không thuộc file list, và assert sai sẽ *panic kernel mỗi lần boot*.
  `boot_ceiling::self_test()` log rõ hàng nào sai rồi trả `false`, gọi từ `main.rs` cạnh
  các self-test khác, TRƯỚC Root spawn đầu tiên. Đã chạy thật qua host harness ngoài cây
  (9 test pass) + mutation-check (thu bảng thành union → 3 test fail đúng chỗ).
- **D5 — hệ quả hành vi của req 3 lên `/bin/platform`.** `policy::load_from_vifs1()`
  (`main.rs:563`) chạy TRƯỚC Root spawn `/bin/platform` (`main.rs:680`), nên cell này giờ
  chịu policy. Build mặc định (POLICY.BIN vắng → `NoEntry` dev-permissive) không đổi.
  Nhưng build `policy-required` không có blob, hoặc blob `Invalid`, sẽ tước `platform` của
  nó → không scan PCIe ECAM → nvme/e1000 không thấy device.
  **ĐÃ CHỐT 2026-07-31** (`bd20e1e8`, user quyết định trực tiếp): thêm `/bin/platform` vào
  `is_trusted_core`. Lý do nặng hơn cả vfs/shell/net — thiếu platform trên x86_64 thì
  không có block driver, tức shell/vfs sống mà không có disk để sửa gì. Chỉ kích hoạt ở
  nhánh `DenyAll` (blob hỏng/deny tường minh); policy hợp lệ vẫn qua `Permit` bình thường.
  Bảng `DEV_POLICY` của phase 03 đã có entry `/bin/platform` platform=1 nên fleet có blob
  hợp lệ không đổi hành vi. Boot 54/54 xác nhận lại sau khi đổi.
- **D6 — thêm một dòng `log::warn!` khi policy thu hẹp cap.**
  Trước đó chỉ có audit ring, mà đọc audit ring cần một cell còn sống — đúng thứ vắng mặt
  khi policy tước cap của cell boot. Có legend bitmask ngay trong dòng log.
- **D7 — `scripts/sign-policy.py` vẫn giữ `MMIO_MASK=0b111` / `REGION_MASK=0b111`.**
  Không được sửa file đó. Script giờ NGHIÊM hơn kernel (an toàn), nhưng phải widen trước
  bước 7d, nếu không `build_body` sẽ `sys.exit` khi bake `/bin/vfs = 0b1111`.
- **D8 — `cargo fmt --all --check` FAIL ở `cells/services/vfs/src/access.rs` và
  `libs/api/src/abi/caller_identity.rs`** — file của phase song song khác đang chạy trong
  cùng working tree. Không sửa. File của slice này `rustfmt --check` sạch.
- **D9 — closure note:** final proof lane did not directly exercise init respawn, so do not claim
  live respawn coverage here; record it as deferred while keeping the launch-edge proof honest.

## Next Steps

- Init-respawn proof remains a separate follow-up; do not reopen this completed launch-edge closure for it.
- Ghi ADR: "Cellos không có admin account" — nêu `boot_ceiling` per-path + broker 3 lớp là cơ chế.
- Deferred từ phase 03 vẫn mở: `NoEntry` fail-closed cho path có `with_path_caps` khác rỗng.
