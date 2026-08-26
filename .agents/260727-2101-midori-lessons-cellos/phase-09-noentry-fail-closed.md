# Phase 09 — `NoEntry` fail-closed cho path mang cap P-TRUST

## Context Links

- Plan: [plan.md](plan.md) · Phụ thuộc: [phase-03](phase-03-policy-cap-coverage.md) (enumeration
  + POLICY.BIN đã bake) → [phase-04](phase-04-deprivilege-init-shell.md) (ceiling per-path)
- Nguồn: mục **Deferred** của plan.md — finding "mở `CAP_BYTES` cho 3 cap P-TRUST là
  net-negative khi `NoEntry` còn dev-permissive". Phase này là điều kiện để plan được claim
  "bít escape hatch".
- Spec: `docs/specs/16-rustc-tcb.md` (mô hình trust), `docs/specs/18-cell-trust-tiers.md`

## Overview

- **Ưu tiên**: P2 · **Trạng thái**: Runtime-verified for the fail-closed branch and architecture smoke; demo packaging breadth remains incomplete (2026-07-31) · **ABI gate**: Không
- **Mô tả**: Sau phase 03, POLICY.BIN đã bake nhưng path **thiếu entry** (typo, bake sót,
  cell mới quên khai) rơi vào nhánh `NoEntry` → dev-permissive → giữ nguyên cap mà
  `with_path_caps` mint ([cap.rs:259](../../kernel/src/task/cap.rs#L259)) — kể cả cap
  P-TRUST (DMA-anywhere). Một lỗi bake trở thành một cell cầm quyền DMA không ai phê duyệt.
  Phase này đóng nhánh đó **chỉ cho path nguy hiểm**, giữ dev loop cho path thường.

## Key Insights

- `PolicyDecision::NoEntry` hiện fail-safe theo hướng dev: giữ spawner-intersected caps
  ([policy.rs:83-85](../../kernel/src/policy.rs#L83-L85)).
- Nguy hiểm chỉ nằm ở giao của hai tập: path có mặt trong bảng `with_path_caps` (được mint
  P-TRUST theo path) **và** không có entry trong POLICY.BIN. Path ngoài bảng `with_path_caps`
  không mint gì mạnh → dev-permissive cho chúng là vô hại và nên giữ.
- Phase 03 đã giao enumeration (mọi path init spawn + mọi path match `with_path_caps`) —
  chính là danh sách để test "không path nào rơi vào giao hai tập" trước khi bake.
- Phải làm **sau phase 04**: trước 04, ceiling per-path chưa có, việc siết NoEntry sẽ lặp
  lại đúng vòng C1 (fold bị zero trước khi policy chạy) mà red-team đã bắt.
- `Spawner::Root` miễn policy ([loader.rs:288](../../kernel/src/loader.rs#L288)) — giữ nguyên,
  phase 04 đã xử lý phần Root bằng bảng ceiling; phase này không đụng đường Root.

## Requirements

**Functional**

1. Khi decision là `NoEntry` **và** `CapSet::EMPTY.with_path_caps(path)` khác EMPTY: tước
   toàn bộ cap P-TRUST khỏi kết quả (giữ cap thường), phát `AuditEvent` mới
   (`PolicyNoEntryStripped { path }` hoặc tương đương trong khung audit hiện có).
2. Path không nằm trong bảng `with_path_caps`: hành vi giữ nguyên (dev-permissive).
3. `PolicyState::Absent` (không có POLICY.BIN — dev boot thường): hành vi giữ nguyên toàn
   bộ. Phase này chỉ đổi nghĩa của "có policy nhưng thiếu entry".
4. Host-side: script sign-policy thêm bước so enumeration (phase 03) với entry list; thiếu
   entry cho path P-TRUST ⇒ **fail lúc bake**, không đợi tới runtime.

**Non-functional**

5. Không ABI change; không đổi format POLICY.BIN.
6. Boot 3 arch + 3 peripheral demo + suite xanh với blob hiện tại (blob phase 03 đã đủ
   entry cho mọi path P-TRUST — xác nhận lại bằng req 4 trước khi merge).

## Related Code Files

| File | Hành động |
|------|-----------|
| `kernel/src/policy.rs` | Modify — nhánh `NoEntry` trong `decision_to_caps` nhận thêm thông tin path-has-ptrust |
| `kernel/src/task/cap.rs` | Read — nguồn sự thật bảng `with_path_caps`; thêm helper `path_mints_ptrust(path) -> bool` |
| `kernel/src/audit.rs` | Modify — event mới |
| `scripts/sign-policy.py` | Modify — bake-time check theo enumeration |
| `kernel/src/policy.rs` (self-test) | Modify — case mới trong self-test sẵn có (`:306-328`) |

## Implementation Steps

1. Helper `path_mints_ptrust` cạnh `with_path_caps` (một nguồn sự thật, không nhân đôi bảng).
2. Sửa nhánh `NoEntry`: strip P-TRUST khi helper true; audit event.
3. Self-test: `NoEntry` + `/bin/nvme` ⇒ mất `pcie_driver`, giữ cap thường; `NoEntry` +
   `/bin/app` ⇒ giữ nguyên.
4. Bake-time check trong `sign-policy.py` + chạy lại bake cho image test.
5. Boot 3 arch, 3 peripheral demo, suite. Xác nhận không audit event nào phát trong boot
   chuẩn (mọi path P-TRUST đều có entry).

## Todo List

- [x] `path_mints_ptrust` helper (`kernel/src/task/cap.rs`, cạnh `with_path_caps`)
- [x] `NoEntry` strip P-TRUST + audit event (`PolicyNoEntryStripped = 26`)
- [x] Self-test 2 case (thành 3: `/bin/nvme` loaded, `/bin/nvme` absent, `/bin/app` loaded)
- [x] `sign-policy.py` bake-time enumeration check (`assert_ptrust_covered`)
- [x] RV64, AArch64, x86_64 shell smoke; incomplete signed policy emits the strip event;
      complete policy emits zero false positives.
- [x] AArch64 `periph-demo` passes.
- [ ] `sensor-demo` and `robot-demo` breadth: current ARM image/disk does not package either
      binary. Fresh full RV64 serial suite also exceeded the 20-minute harness timeout.

## Success Criteria

- Cell tại path có `with_path_caps` P-TRUST, không có entry trong POLICY.BIN đã bake ⇒
  spawn được nhưng không giữ cap P-TRUST nào; audit log ghi nhận.
- Bake blob thiếu entry P-TRUST ⇒ `sign-policy.py` fail với thông báo nêu path.
- Dev boot không POLICY.BIN: không thay đổi hành vi nào (diff suite = 0).
- Sau merge, plan.md được phép cập nhật claim "bít escape hatch" (ràng buộc Deferred đã thoả).

## Risk Assessment

| Rủi ro | Xác suất | Ảnh hưởng | Giảm thiểu |
|--------|----------|-----------|------------|
| Blob production thiếu entry hợp lệ → cell driver mất cap giữa fleet | Thấp sau req 4 | Driver không hoạt động | Bake-time check chặn trước; audit event làm chẩn đoán 1 dòng |
| Bảng `with_path_caps` và enumeration lệch nhau | Trung bình | Check bake-time thủng | Helper dùng chính bảng runtime; enumeration chỉ là input đối chiếu |
| Lặp lại C1 (ceiling zero trước policy) | Thấp nếu đúng thứ tự | Kết quả sai như C1 | Phase blocked trên 04 — ghi trong Overview |

## Security Considerations

- Đây là chốt fail-closed đầu tiên của lớp policy: từ chỗ "quên = mở", thành "quên = mất
  quyền mạnh + có vết". Audit phải log cả lúc **strip** (phase này) lẫn lúc **cấp** P-TRUST
  (đã có từ phase 03) để đối chiếu được hai chiều.

## Deviation Log

| # | Loại | Nội dung |
|---|------|----------|
| 1 | Decision | `PolicyDecision::NoEntry` đổi thành `NoEntry { policy_loaded: bool }` (+ `#[derive(Copy, Clone)]`) thay vì thêm variant mới. Lý do: variant mới cho phép match cũ vẫn biên dịch mà bỏ sót nhánh mới; đổi shape buộc mọi call-site phải nói rõ nó đang ở tình huống nào. Không có consumer nào ngoài `policy.rs` (đã grep toàn repo). |
| 2 | Decision | Thêm `CapSet::without_ptrust()` cạnh `path_mints_ptrust` (phase file chỉ nêu helper). Lý do: giữ danh sách 3 cap P-TRUST ở một chỗ, `policy.rs` không phải liệt kê field. |
| 3 | Decision | Audit event dùng discriminant **26** (byte trống kế tiếp; 25 = `ThreadCapReached`). `audit.rs` đã có tiền lệ hai nhánh song song cùng giành 23 — nếu phase khác cũng thêm event trong session này thì phải kiểm tra lại byte trước khi merge. |
| 4 | Decision | `sign-policy.py` **parse** `with_path_caps` trong `kernel/src/task/cap.rs` để lấy danh sách path P-TRUST, không copy list sang Python. Guard: không tìm thấy file/hàm, hoặc parse ra 0 path ⇒ `sys.exit` (check rỗng còn tệ hơn không có check). |
| 5 | Deviation | Bỏ dòng trỏ tới `.agents/…/phase-03-policy-cap-coverage.md` trong comment của `DEV_POLICY`: rule của repo là không để plan reference trong source, và `.agents/` gitignored nên pointer đó không ship. Nội dung cần thiết được viết lại ngay tại chỗ. |
| 6 | Surprise | Giao "path mint P-TRUST" có **8** path, không phải 7: `/bin/e1000` cũng nằm trong `with_path_caps`. Cả 8 đều đã có entry trong `DEV_POLICY` ⇒ luật mới hôm nay không từ chối gì. |
| 7 | Deviation | Req 6 (boot 3 arch + 3 peripheral demo + suite) **không verify được**: máy build không có QEMU/cross-gcc. Thay bằng harness std ngoài cây (`#[path]` vào chính `policy.rs`/`cap.rs`) chạy `policy::self_test()` thật, cộng một case end-to-end nạp blob dev-signed cố tình thiếu `/bin/nvme`, cộng 2 mutation check. Kết quả runtime trên thiết bị vẫn là UNVERIFIED. |
| 8 | Verification | Rerun 2026-07-31 disproved the environment premise. A validly signed 22-entry policy omitting `/bin/nvme` loaded and emitted `privileged caps stripped 0b001`; the complete 23-entry policy reached shell with zero strip events. RV64/AArch64/x86_64 shell smoke and AArch64 `periph-demo` passed. See `.agents/reports/a4-runtime-gates-260731.md`. |
| 9 | Packaging gap | The current ARM embedded image and disk contain `periph-demo` but not `sensor-demo` or `robot-demo`. Their demo breadth remains open; no silent skip is claimed. |

## Next Steps

- Khi Tier 2 (Spec 18) land ở plan sau: cân nhắc mở rộng fail-closed cho toàn bộ path khi
  build mang posture `policy-required`.
