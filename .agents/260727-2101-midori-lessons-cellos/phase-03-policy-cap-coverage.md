# Phase 03 — POLICY.BIN v2 + enumeration + bít escape hatch

## Context Links

- Plan: [plan.md](plan.md) · Kế tiếp: [phase-04](phase-04-deprivilege-init-shell.md)
- Spec: `docs/specs/15-kernel-boundary.md` (security root-of-trust thuộc kernel)
- Bối cảnh: `project-loader-trust-model-repair`, `project-cap-ceiling-spawnreplacement-audit`
- Midori nguồn: không leo thang quyền — thẩm quyền chỉ đi xuống, và không có cửa hậu

## Overview

- **Ưu tiên**: P1 — latent break, sẽ nổ đúng lúc chuyển sang fleet posture thật
- **Trạng thái**: Planned (post-red-team revision)
- **Mô tả**: Ba cap mạnh nhất (`pcie_driver`, `platform`, `supervisor`) không diễn đạt được trong
  POLICY.BIN; và POLICY.BIN **chưa từng được bake** vào image nào nên toàn bộ tầng policy đang là
  no-op. Phase này làm layout v2, enumerate đủ boot set, và bít `maintenance-mode`.

> **Red-team correction (2 điểm)**:
> 1. Req 5 cũ (fold `/bin/vfs` block region `0b1000` + xoá raw grant ở loader) **đã chuyển sang
>    phase 04** — nó bất khả thi trong phase này, xem C1 trong `plan.md`.
> 2. Framing "3 cell đặc quyền bị fail-closed" **sai**. Phạm vi thật ~14 cell, và `/bin/platform`
>    thì đã miễn policy hoàn toàn.

## Key Insights

- `CAP_BYTES = 6` ([policy.rs:30](../../kernel/src/policy.rs#L30)) chỉ phủ block_io, network, spawn,
  hypervisor, mmio_devices, block_regions.
- Parser fill `..CapSet::EMPTY` ([policy.rs:185-189](../../kernel/src/policy.rs#L185-L189)) → ba cap
  P-TRUST **luôn false** trong mọi `Permit`. Vì `Permit(p) → caps.intersect(p)`
  ([policy.rs:306](../../kernel/src/policy.rs#L306)), hệ quả là **luôn bị tước**, không phải
  "không được cấp" như comment ở đó ngụ ý.
- **POLICY.BIN hiện KHÔNG có trong bất kỳ embedded VIFS1 nào** (xác minh 2026-07-27: hit "POLICY"
  duy nhất trong `kernel/src/embedded/kernel_fs.img` là log string `" policy says no restart"`, không
  phải FAT directory entry; `embedded-aarch64` và `embedded-x86_64` zero hit). Nên `policy.rs:91-103`
  đang đi nhánh `Absent` → dev-permissive → **toàn bộ tầng policy là no-op hôm nay**. Bake của phase
  này là **lần bake đầu tiên** trên đường boot thật.
- **Phạm vi `policy-required` là ~14 cell, không phải 3**: `scripts/sign-policy.py:37-41` có đúng 4
  entry (`/bin/vfs`, `/bin/net`, `/bin/shell`, `/bin/init`). Init spawn ~14-20 path:
  `/bin/config`, `/bin/input`, `/bin/compositor`, `/bin/silo`, `/bin/net-broker`, `/bin/supervisor`
  ([init/src/main.rs:88-98](../../cells/tools/init/src/main.rs#L88-L98)), `/bin/block` +
  `/bin/nvme` ([:142-143](../../cells/tools/init/src/main.rs#L142-L143)), `/bin/virtio-net` +
  `/bin/e1000` ([:164-165](../../cells/tools/init/src/main.rs#L164-L165)), `/bin/virtio-gpu`
  ([:183](../../cells/tools/init/src/main.rs#L183)), `/bin/fb-console` ([:232](../../cells/tools/init/src/main.rs#L232)),
  `/bin/hypervisor` ([:238](../../cells/tools/init/src/main.rs#L238)). Tất cả rơi vào `NoEntry` →
  `CapSet::EMPTY` dưới `policy-required` ([policy.rs:314-322](../../kernel/src/policy.rs#L314-L322)).
- **`/bin/block` là ca chết người nhất**: nó là Block Driver Cell sở hữu thiết bị và phục vụ FAT
  cell-store tại `/bin` qua VFS (CLAUDE.md → Kernel Boundary Law). Mất `block_io` là **mọi**
  `sys_spawn_from_path` non-ramdisk fail.
- **`/bin/platform` KHÔNG bị fail-closed**: nó spawn `Spawner::Root` từ
  [main.rs:682](../../kernel/src/main.rs#L682) → miễn policy hoàn toàn
  ([loader.rs:288](../../kernel/src/loader.rs#L288)). Khoảng trống thật của nó là `try_grant_platform`
  singleton latch nằm ngoài policy by construction, không phải fail-closed. **Nuance arch** (validation
  V3): nó nằm sau `#[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]` → **ARM64 không spawn
  platform cell**, và failure là non-fatal by design.
- **Init spawn qua `sys_spawn_from_path`** ([init/src/main.rs:142-165](../../cells/tools/init/src/main.rs#L142-L165))
  → `Spawner::User(init_tid)` → ceiling = cap của init (validation V4). Đây là lý do fold `/bin/vfs`
  không thể ở phase này: ceiling của init zero bit 3 **trước** khi `policy::apply` chạy.
- **Boot set phân kì theo arch**: `/bin/e1000` (x86 PCIe NIC) vs `/bin/virtio-net` (RV/ARM MMIO NIC);
  `/bin/platform` chỉ x86+RV. Quyết định D4: **một blob union cho cả 3 arch** — entry của path không
  tồn tại trên một arch là vô hại (không ai spawn nó). Một blob, một script sign, một thứ để review.
  Đánh đổi đã nhận: blob mang entry không dùng, và layout v2 **không** biểu diễn được "cell này chỉ
  được tồn tại trên x86" (phương án per-entry arch mask đã bị loại — thêm field vào đúng chỗ parser
  phải panic-free).
- `maintenance-mode` bypass toàn bộ policy narrowing ([policy.rs:337-341](../../kernel/src/policy.rs#L337-L341))
  chỉ bằng một build feature. Một image build sai flag = mọi cap được cấp.
- `dropped` bitmask audit chỉ phủ 4 cap đầu ([policy.rs:346-349](../../kernel/src/policy.rs#L346-L349))
  — việc tước cap mạnh nhất lại là việc không được log.

## Enumeration (deliverable bước 1 — hoàn thành 2026-07-28)

Nguồn: `cells/tools/init/src/main.rs` (mọi nhánh), `kernel/src/main.rs:682`,
`CapSet::with_path_caps` ([cap.rs:259-278](../../kernel/src/task/cap.rs#L259-L278)),
`CapSet::from_manifest` ([cap.rs:211-244](../../kernel/src/task/cap.rs#L211-L244)) và
`declare_manifest!` của từng cell. `mmio`: UART=1, GPIO=2, PCIE=4.
`block_regions`: `data | (lfs<<1) | (lfs<<2)`.

**Chỉ những path cần cap khác rỗng mới phải có entry.** Cell không cap nào rơi vào `NoEntry` →
`CapSet::EMPTY` dưới `policy-required`, mà EMPTY đúng là thứ nó cần (Send/Recv/Log không do CapSet gác).

| Path | Cap cần | Nguồn cap | Spawner | Arch |
|------|---------|-----------|---------|------|
| `/bin/init` | spawn | manifest | **kernel, Root → miễn policy** | mọi |
| `/bin/platform` | platform | `with_path_caps` | **kernel, Root → miễn policy** | x86+RV (ARM64 không spawn) |
| `/bin/vfs` | block_io, regions `0b111` | manifest (`part_data`+`part_lfs`) | init | mọi |
| `/bin/block` | pcie_driver | `with_path_caps` | init (trước VFS) | mọi |
| `/bin/nvme` | pcie_driver | `with_path_caps` | init (trước VFS + retry) | mọi (device x86) |
| `/bin/input` | pcie_driver | `with_path_caps` | init | mọi |
| `/bin/net` | network | manifest | init | mọi |
| `/bin/virtio-net` | pcie_driver | `with_path_caps` | init | mọi (device RV/ARM) |
| `/bin/e1000` | pcie_driver | `with_path_caps` | init | mọi (device x86) |
| `/bin/virtio-gpu` | pcie_driver | `with_path_caps` | init | mọi |
| `/bin/net-broker` | network | manifest | init | mọi |
| `/bin/supervisor` | spawn, supervisor | manifest + `with_path_caps` | init | mọi |
| `/bin/shell` | spawn, mmio `3` (gpio\|uart) | manifest | init | mọi |
| `/bin/hypervisor` | hypervisor | manifest (gate H-ext/EL2/x86-virt) | init | ARM64 EL2, x86 SVM |
| `/bin/bench`, `/bin/bench-probe` | spawn | manifest | shell | image bench |
| `/bin/periph-demo` | mmio `3` | manifest | shell | mọi |
| `/bin/periph-test` | mmio `3` | manifest | shell | mọi |
| `/bin/robot-demo` | mmio `2`, network | manifest | shell | mọi |
| `/bin/sensor-demo`, `/bin/spi-demo`, `/bin/pwm-demo` | mmio `2` | manifest | shell | mọi |
| `/bin/gpio-test-rv` | mmio `2` | manifest | shell | RV |

Không cần entry (cap rỗng): `config`, `compositor`, `silo`, `fb-console`, `adc-demo`, `can-demo`,
`audio-demo`, `doom`, `tetris-c`, `lua`, `wasm`, `input-test`, `vfs-test`, `srv-test`, `silo-test`,
`hypervisor-test`.

**Ba phát hiện chỉnh lại giả định của plan:**

1. **Demo do *shell* spawn cũng thuộc phạm vi, không chỉ path init spawn.** Req 3 chỉ nói "path init
   spawn + path trong `with_path_caps`", nhưng dưới `policy-required` mọi path không có entry đều
   `EMPTY` — nên 6 demo/test cần MMIO sẽ chết. Success Criteria đã yêu cầu 3 peripheral demo chạy
   được, nên chúng **phải** có entry. Bảng trên đã gộp.
2. **`/bin/shell` trong `sign-policy.py:39` đang là `mmio = 0`.** Bake nguyên trạng sẽ zero gpio|uart
   của shell, và vì demo do shell spawn nên ceiling zero luôn → mất cả 6 demo MMIO. Đây chính là
   rủi ro "không behaviour-neutral" ở Risk Assessment, và nó là **lỗi có sẵn trong script**, không
   phải thứ phát sinh khi thêm entry.
3. **Embedded VIFS1 chỉ có 6 cell** (`init`, `shell`, `vfs`, `config`, `platform`, `block` — verified
   bằng `tools/inspect_fat.py` trên `kernel/src/embedded/kernel_fs.img`), phần còn lại nằm ở
   FAT32 cell-store trên disk image. Nhưng POLICY.BIN đọc từ **VIFS1** (`policy.rs:32`) nên chỉ cần
   bake vào embedded image; entry cho cell nằm trên disk vẫn có hiệu lực vì lookup theo path.
   Có **8** thư mục `kernel/src/embedded*/`, không phải 1 — xem bước 2.

## Requirements

**Functional**

1. POLICY.BIN v2: `CAP_BYTES` phủ cả `pcie_driver`, `platform`, `supervisor`. Bump `VERSION`
   (`VERSION_V1 = 1`, `VERSION_V2 = 2`), parser chọn `cap_bytes_for(version)`, v1 vẫn parse được.
2. Domain-validate ba byte mới: chỉ nhận 0/1, khác → `Invalid` (fail-closed, đúng bất biến hiện có).
3. **Enumeration deliverable** (thay framing "3 cell"): liệt kê **mọi path init spawn trên MỌI arch**
   (union — D4) + **mọi path match trong `with_path_caps`**
   ([cap.rs:259-276](../../kernel/src/task/cap.rs#L259-L276)), cùng cap mà mỗi path cần. Bảng này là
   output của bước 1, không phải giả định. Ghi rõ arch nào spawn path nào (thông tin cho người đọc;
   blob không mã hoá arch).
4. `sign-policy.py` emit entry cho toàn bộ boot set từ bảng ở req 3.
5. **Blob phải behaviour-neutral**: vì đây là lần bake đầu tiên, không entry nào được siết chặt hơn
   hành vi dev-permissive hôm nay. Cụ thể `/bin/shell` giữ `mmio = 3` (gpio|uart) — việc hạ nó
   xuống 0 thuộc phase 04, cùng lần re-bake với việc bỏ manifest bit của shell.
6. `maintenance-mode` không còn là bypass thuần build-time: cần thêm một flag bit đã ký trong
   POLICY.BIN (`MAINTENANCE_PERMITTED`) mới thực sự bypass. Escape hatch cần **hai** yếu tố.
7. Audit `dropped` bitmask phủ 7 cap; **và audit cả lúc ba cap P-TRUST được CẤP**, không chỉ lúc bị
   tước — xem Risk Notes → deferred finding.

**Non-functional**

8. Verify-then-parse giữ nguyên: signature phủ `blob[..len-64]`, verify TRƯỚC khi parse field nào.
   `version` chỉ được đọc sau khi signature pass (nó nằm trong vùng được ký).
9. Parser panic-free (bounds-check mọi field).
10. Không đụng libs/api → không kích hoạt Law 1.

## Architecture

```
POLICY.BIN v2 (thay đổi duy nhất: CAP_BYTES 6 → 9)
  magic "VPOL" (4) | version=2 (1) | flags (1) | entry_count (2)
  entry[]: path_len (1) | path (N) | caps (9)   ◄── 6 cũ + pcie_driver, platform, supervisor
  sig (64) — Ed25519 phủ blob[..len-64]

flags bit mới: MAINTENANCE_PERMITTED  ◄── req 6
```

**Không** đụng `REGION_MASK` / `MMIO_MASK` trong phase này — cả hai cần widen, nhưng việc đó gắn
với fold `/bin/vfs` và init ceiling nên thuộc phase 04. Phase 03 chỉ ràng buộc: mọi giá trị nó bake
phải nằm trong mask hiện tại (`REGION_MASK = 0b111`, `MMIO_MASK = GPIO|UART|PCIE`). Vi phạm ràng
buộc này là brick toàn fleet — xem Risk Assessment.

## Related Code Files

| File | Hành động |
|------|-----------|
| `kernel/src/policy.rs` | Modify — CAP_BYTES theo version, VERSION v1/v2, maintenance flag, audit 7-bit + audit-on-grant |
| `scripts/sign-policy.py` | Modify — v2 layout, 9 cap byte, toàn bộ boot set |
| `cells/tools/init/src/main.rs` | Đọc-để-enumerate (không sửa ở phase này) |
| `kernel/src/task/cap.rs` | Đọc-để-enumerate `with_path_caps` (sửa ở phase 04) |
| Nơi bake POLICY.BIN vào 4 disk image + embedded VIFS1 | Modify — bake lần đầu |
| `kernel/src/policy.rs` self-test (quanh dòng 241-256) | Modify — case cho cap mới + case v1 |

## Implementation Steps

1. **Enumeration (deliverable của phase, làm trước tiên)**: quét `init/src/main.rs` (mọi nhánh `#[cfg]`,
   cả 3 arch) + `main.rs:682` + `with_path_caps`, lập bảng `path → cap cần → arch nào spawn`. Union
   theo D4. Ghi bảng vào phase file này. Không có bảng này thì mọi bước sau là đoán.
2. Xác định chính xác nơi POLICY.BIN được bake — cả 4 disk image **và** embedded VIFS1 (hiện chưa có
   ở đâu cả, nên đây là thêm mới, không phải thay thế).
3. Thêm `VERSION_V2`, tách `cap_bytes_for(version)`; parser đọc theo version, v1 vẫn parse.
4. Domain-validate ba byte mới. Mở `dropped` bitmask lên 7 bit; thêm audit-on-grant cho 3 cap P-TRUST.
5. Gate `maintenance-mode` bằng flag bit đã ký.
6. Sửa `sign-policy.py`: v2 + toàn bộ boot set từ bảng bước 1, `/bin/shell` giữ `mmio = 3`.
7. **Host-side parse self-test TRƯỚC khi bake image nào**: chạy parser trên blob mới ở host (hoặc
   qua unit test của `policy.rs`), khẳng định `parse` trả `Some` và mọi entry đúng như mong đợi. Đây
   là gate duy nhất chặn được lỗi brick-toàn-fleet — chạy suite sau khi bake là quá muộn.
8. Bake vào **một** image trước, boot, chạy 3 peripheral demo (`periph-demo`, `robot-demo`,
   `sensor-demo`) để chứng minh blob behaviour-neutral. Chỉ sau đó bake các image còn lại.
9. Boot cả build thường và build `--features policy-required`. Build sau là bài kiểm tra thật.
10. clippy + full suite 3 arch.

## Todo List

- [x] **Enumeration**: bảng `path → cap cần → arch spawn` — xem `## Enumeration` ở trên (23 entry)
- [x] Xác định nơi bake POLICY.BIN — VIFS1 (`kernel/src/embedded*/kernel_fs.img`), 8 thư mục; disk image không cần (policy đọc từ VIFS1)
- [x] `VERSION_V2` + `cap_bytes_for(version)`, v1 vẫn parse
- [x] Domain validation 3 byte cap mới (strict 0/1) + flag bit lạ + version lạ
- [x] Audit `dropped` lên 7 bit (+2 bit mask) + **audit-on-grant** `PrivilegedCapGranted = 23`
- [x] Gate `maintenance-mode` bằng flag bit đã ký (`PolicyMaintenanceBypass = 24`)
- [x] `sign-policy.py` v2 + toàn bộ boot set, `/bin/shell` `mmio = 3`
- [x] **Host-side parse self-test trước khi bake** — `assert_round_trip`, chạy vô điều kiện
- [x] Bake image rv64 boot-ramdisk → boot → **diff A/B chứng minh behaviour-neutral**
- [x] **`gen_disk.ps1` đã bake** (2026-07-28) — image dev chính + image CI job integration. Kèm
      assertion `inspect_fat` sau build và `pip install cryptography` ở job `boot-suite`
- [x] **Bake test-hooks + shell-test + srv-test + aarch64** (`a8516c767`) — mỗi lane verify riêng:
      suite xanh **và** boot trực tiếp thấy `[policy] loaded + verified (23 entries)`
- [x] **Bake 3 lane cuối** (`edbb20ba5`) — `embedded-hv` + `embedded-x86_64` verify bằng boot thật
      (`[policy] loaded + verified (23 entries)`); `embedded-hv-x86` bake nhưng **chưa verify**
      (cần Alpine x86 artifact `.alpine-cache/vmlinux`, không có sẵn). **8/8 lane đã bake.**
      Lưu ý: `kernel/src/embedded-x86_64/kernel_fs.img` **được track** và CI **không** chạy
      `build-x86_64-cells.ps1` (4 lần xuất hiện trong ci.yml đều là comment) → phải commit image,
      không thì bake chỉ nằm trong script mà vắng ở mọi lần CI chạy.
- [~] Peripheral demo với policy đã load — **2/3 chạy, 1 không tồn tại trong image nào**:
  - `periph-demo` (aarch64): **PASS đúng nghĩa** — test launch `periph-demo &` **từ shell prompt như
    người dùng** rồi chờ `[periph-demo] GPIO PL061 opened`, tức MMIO thật qua ceiling shell→demo, trên
    image có blob.
  - `robot-demo` (rv64): chạy từ shell, `done (5 cycles)`, policy đã load. **Nhưng đi nhánh sim** —
    `GPIO not available`, vì rv64 `virt` không có GPIO device (allowlist chỉ có SiFive GPIO của máy
    `sifive_u`). Log **không** có dòng `denied`/`DENY` nào → thiếu thiết bị, không phải bị tước cap.
    Chứng minh spawn + policy OK; **không** chứng minh MMIO.
  - `sensor-demo`: `gen_disk.ps1` không đưa nó vào disk nào, `build-aarch64-cells.ps1` cũng không →
    chưa image nào dựng ở đây có nó. Không chạy được mà không sửa nội dung image.
- [x] Boot với `policy-required` — PASS, và **negative control chứng minh có hiệu lực**
- [x] clippy `-D warnings` 3 arch + 5 tổ hợp feature

## Success Criteria

**Done khi**

- Build với `--features policy-required` boot tới shell, và **toàn bộ boot set** hoạt động (block,
  nvme, input, config, compositor, net, gpu…) — không phải chỉ 3 cell.
- 3 peripheral demo chạy từ shell prompt **sau** khi bake, giống trước khi bake (behaviour-neutral).
- Host-side parse self-test pass trên blob v2, và trên một blob v1 (backward compat).
- Audit log ghi cả lúc 3 cap P-TRUST được cấp.

**Validation**

- Policy self-test: v1 blob parse; v2 blob với byte cap ngoài domain → `Invalid`; blob v2 hợp lệ →
  `Permit` đúng entry.
- Suite 3 arch pass. x86 NVMe-under-VT-d scenario pass (caller thật của `PcieDriverCap`).

## Risk Assessment

| Rủi ro | Xác suất | Ảnh hưởng | Giảm thiểu |
|--------|----------|-----------|------------|
| **Bake một giá trị ngoài mask → blob `Invalid` → `DenyAll` toàn bộ** (`policy.rs:288`) → mọi cell ngoài vfs/shell/net nhận `CapSet::EMPTY` | Trung bình | **Brick toàn fleet, cần re-flash** | Ràng buộc cứng: mọi giá trị bake phải trong `REGION_MASK = 0b111` / `MMIO_MASK` hiện tại. Host-side parse self-test (bước 7) là gate. Bake 1 image trước (bước 8). Lưu ý: CI "boot to prompt" vẫn PASS trong ca này vì shell lên từ ramdisk — không dùng nó làm tiêu chí |
| Enumeration bỏ sót một path → cell đó fail-closed dưới `policy-required` | Cao | Cell chết | Bước 1 là deliverable có review, không phải giả định; boot `policy-required` (bước 9) là bài kiểm tra |
| Blob không behaviour-neutral → phase 03 làm vỡ peripheral demo trước khi phase 04 có broker | Cao | Baseline của phase 04 bị nhiễm | Req 5 + bước 8 chạy demo ngay sau bake |
| Parser v2 accept blob v1 sai độ dài | Thấp | Cap sai | Length check theo version trước khi đọc field |
| `dev-policy-key` vs khoá production lệch khi bake | Trung bình | Verify fail → `Invalid` → DenyAll | Xác nhận feature nào đang bật trong CI image trước khi bake |

**Risk Notes — deferred finding (red-team, không fix trong phase này)**

> Mở `CAP_BYTES` cho 3 cap P-TRUST là **net-negative khi `NoEntry` còn dev-permissive**. Sau phase
> này `with_path_caps` vẫn mint 3 cap đó theo path ([cap.rs:259-266](../../kernel/src/task/cap.rs#L259-L266)),
> nên một POLICY.BIN thiếu entry (typo path, bake sót — đúng rủi ro ở bảng trên) rơi vào `NoEntry`
> → dev-permissive ([policy.rs:323-326](../../kernel/src/policy.rs#L323-L326)) → cell **giữ** cap
> DMA-anywhere. Trước phase này, cùng tình huống đó ít nhất bị `Permit ∩` zero về false.
>
> Fix đúng: `NoEntry` fail-closed cho mọi path có `with_path_caps` khác rỗng, bất kể feature. Đó là
> đổi fleet posture → ngoài scope phase 03.
>
> **Ràng buộc**: phase 03 **KHÔNG được claim "bít escape hatch"** trong changelog/roadmap chừng nào
> chưa land điều này. Nó chỉ bít được `maintenance-mode`. Audit-on-grant (req 7) là biện pháp bù tối
> thiểu — ít nhất việc cấp cap mạnh sẽ để lại vết.

## Security Considerations

- **Verify-then-parse là bất biến tuyệt đối.** Mở rộng layout không được làm parser chạy trên byte
  chưa verify.
- Fail-closed cho giá trị ngoài domain: một policy *đã ký nhưng malformed* vẫn phải bị từ chối — đây
  là chống lại chính operator ký sai, không chỉ chống kẻ tấn công. Nhưng lưu ý mặt tối: fail-closed
  ở tầng blob là **DenyAll toàn cục**, nên "an toàn" ở đây đồng nghĩa "brick". Gate ở host, không ở boot.
- Escape hatch phải cần **hai** yếu tố (image + policy đã ký).
- Sau phase này, ngoại lệ cấp cap còn lại là `Spawner::Root` (phase 04) và
  `try_grant_platform` singleton latch (đúng chỗ — bool không enforce được singleton).

## Evidence (2026-07-28)

Branch `feat/policy-bin-v2`. Mọi số liệu dưới đây tôi tự chạy và tự capture.

**Đã đạt**

| Tiêu chí | Bằng chứng |
|----------|-----------|
| POLICY.BIN load lần đầu trên thiết bị | `[policy] loaded + verified (23 entries, flags 0x00)` — trước đó mọi image đều `absent` |
| Self-test v1 + v2 | `policy verify+parse self-test PASS`; `v2_parse_cases` phủ stride 9-byte, priv byte ngoài {0,1}, mmio ngoài mask, flag lạ, version lạ, entry cụt |
| Host gate chặn blob xấu | 5/5 case malformed bị `decode_body` từ chối (mmio 0xF0, pcie=2, flag 0x02, version 3, truncated) — chạy trực tiếp, không phải suy luận |
| **Behaviour-neutral** | Diff A/B log boot có-policy vs không-policy: **khác đúng 1 dòng** (chính dòng log policy). Mọi dòng khác giống hệt |
| `policy-required` boot | PASS cả 2 posture; diff REQUIRED vs DEFAULT chỉ khác entry address do feature flag |
| **Negative control** | Cùng kernel + cùng image + cùng disk, chỉ khác blob: **có** entry `/bin/block` → `[virtio-blk] ready` + `block driver registered: tid=3`; **bỏ** entry → `[virtio-blk] no free device (kernel-owned) — exiting`. Policy thật sự có răng |
| clippy `-D warnings` | riscv64 / aarch64 / x86_64 exit 0; 5 tổ hợp feature (`maintenance-mode`, `policy-required`, `dev-policy-key`, 2 tổ hợp) exit 0 |

**Bổ sung 2026-07-28 — `gen_disk.ps1` đã bake (`f7e4bb4e7`)**

| Tiêu chí | Bằng chứng |
|----------|-----------|
| Image dev/CI-integration có policy | `SFN POLICY.BIN … sz=596` ở root; magic `VPOL`, version **2**, **23 entry** |
| Policy active trong đúng kernel suite boot | `[policy] loaded + verified (23 entries, flags 0x00)` |
| `pcie_driver` sống qua policy narrowing | `block driver registered: tid=3` + `[virtio-blk] ready` (disk_v3 gắn kèm) |
| Không hồi quy | `cargo test --test boot` **54 pass / 0 fail** (baseline 53/1, cái fail duy nhất là flake input-burst) |
| Gate mới | assertion `inspect_fat` sau build (kiểm cả `/POLICY.BIN` và `/bin/vfs`) + `pip install cryptography` ở job `boot-suite` |

Trước bổ sung này **không test nào chạm tầng policy**: image integration đi nhánh `Absent` →
dev-permissive, nên 54 test chạy trên một tầng policy không làm gì.

**Chưa đạt (còn lại sau 2026-07-28)**

- **`embedded-hv-x86` bake nhưng chưa verify** — cần Alpine x86 artifact (`.alpine-cache-x86/vmlinux`),
  không có sẵn. Đáng lưu ý khi fetch: `scripts/fetch-alpine-x86.sh` tải
  `extract-vmlinux` từ `torvalds/linux/**master**` **không pin checksum**, trong khi header của chính
  script tuyên bố *"never download without checksum verification"* — mâu thuẫn có sẵn, nên tôi không
  chạy fetch. 7/8 lane còn lại đã verify bằng boot.
- **`sensor-demo` không có trong image nào** (xem Todo). Chạy được nó cần đổi nội dung image, không
  phải đổi policy.
- **`NoEntry` vẫn dev-permissive** → ràng buộc "KHÔNG claim bít escape hatch" ở Risk Notes vẫn hiệu lực.

**Phát hiện ngoài kế hoạch**

1. **`self_test` hardcode kỳ vọng dev-permissive** → mọi build `policy-required` **luôn** fail
   self-test, đúng cái posture cần nó nhất, và fail đó chỉ advisory nên không ai dừng. Lỗi có sẵn,
   không do phase này. Đã sửa: kỳ vọng theo posture, + pin nhánh trusted-core cho cả hai posture.
2. **Lane CI ramdisk không quan sát được việc enforce policy.** Diskless nên `/bin/block` exit ngay
   dù có cap hay không; `/bin/vfs` và `/bin/shell` là trusted-core nên miễn nhiễm `NoEntry`. Negative
   control chỉ chạy được sau khi gắn `disk.img`. Cùng họ với lỗi `qemu-boot-test.sh` assert
   "FAT16 mounted" — oracle không chạm tới thứ cần kiểm.
3. **Bake blob dev-signed = landmine cho build production.** Image mang blob này chỉ verify khi
   kernel có feature `dev-policy-key` (đang nằm trong `default`). Build không có nó → `Invalid` →
   `DenyAll`. Trước phase này không tồn tại rủi ro đó vì không image nào có blob. Đã ghi cảnh báo
   ngay tại chỗ bake trong `build-boot-ramdisk-ci.sh`.
4. `scripts/build-boot-ramdisk-ci.sh` vẫn default `CC_riscv64gc_unknown_none_elf=riscv64-unknown-elf-gcc`
   (không tồn tại trên máy này) — `b26a896bb` sửa python/mktemp/MSYS nhưng không sửa CC.
5. **Bake list rộng hơn "8 thư mục embedded" — phải gồm `gen_disk.ps1`.** Verified 2026-07-28:
   `gen_disk.ps1` dựng lại `kernel/src/embedded/kernel_fs.img` (FAT32, 40 MB, 48 cell) và **không**
   bake POLICY.BIN — `grep POLICY` trên image nó tạo ra = 0 hit. Đây là image dev chính **và** là
   image CI dùng cho job integration (CI chạy `pwsh ./gen_disk.ps1` trước bộ `--test boot`). Nên
   chừng nào chưa wire vào đó thì mọi lần chạy suite local/CI đều ở posture `absent` = dev-permissive,
   tức **bộ integration không hề exercise tầng policy**. Cùng lý do, `xorriso` chưa có nên lane
   x86_64 (boot bằng ISO Limine, không phải `-kernel`) chưa verify được ở đây.
6. **Ba loại image, cùng một đường dẫn kernel — nguồn báo cáo sai.** `build-test-hooks-ci.sh`,
   `build-boot-ramdisk-ci.sh` và `gen_disk.ps1` đều ghi kernel vào
   `target/<triple>/release/vicell-kernel`, mà mỗi suite lại cần một loại khác nhau. Chạy suite ngay
   sau script sai cho ra số liệu đỏ trông y như regression: đã gặp **27 pass/27 FAIL** (kernel là bản
   test-hooks) rồi **44/10** (ramdisk 6 cell thiếu `bench`), cả hai đều 0 lỗi code. Xem
   [[feedback-build-skew-contaminates-test-suites]].

## Next Steps

- Phase 04 xử lý `Spawner::Root` exemption, hạ `mmio` của shell, và hấp thụ req 5 cũ (fold
  `/bin/vfs` region + widen `REGION_MASK`/`MMIO_MASK` + widen init ceiling).
