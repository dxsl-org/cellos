# Phase 11 — `cellos-sign`: chữ ký chỉ cấp sau khi kiểm F1

## Context Links

- Plan: [plan.md](plan.md) · Độc lập với mọi phase khác (toàn tooling/CI, không đụng kernel)
- Spec: `docs/specs/16-rustc-tcb.md` (chính sách F1/F5), `docs/specs/18-cell-trust-tiers.md`
  §2.1 (ADR đã accept — phase này hiện thực Tier-1 admission)
- Nguồn: gap-analysis 2026-07-30 (gap 2) — audit cho thấy **25/71** crate cell có
  `#![forbid(unsafe_code)]`; `sign-cell.py` ký mà không kiểm gì

## Overview

- **Ưu tiên**: P1 (đây là tường cell↔cell duy nhất cho data trên phần cứng thật — Spec 19 §1)
- **Trạng thái**: Complete + runtime verified (2026-07-31) — xem § Deviation Log · **ABI gate**: Không (biến
  thể manifest-bit là stretch, Law 1, tách PR riêng nếu làm — chưa làm)
- **Mô tả**: Nâng pipeline ký từ "ký mọi ELF được đưa vào" thành "**build + kiểm F1 + ký
  trong một bước**". Chữ ký đổi nghĩa: từ chứng thực nguồn gốc thành chứng thực *được build
  bởi pipeline đã enforce F1*. Kiểm và ký không được tách rời — tool không bao giờ ký một
  ELF ngoại lai.

## Key Insights

- Thực trạng F1 (đo 2026-07-30): 25/71 crate có forbid; cell chứa `unsafe` gồm cả
  **non-driver**: `shell` (4 file), `vfs` (2 file + 2 khối "caller blocks" mà phase 07 phải
  audit), `silo`. Driver (nvme, virtio-*, e1000) có lý do MMIO chính đáng → allowlist.
- Cơ chế kiểm phải chống được bypass vô ý, không cần chống dev nội bộ ác ý (threat model
  Tier 1 = honest-but-fallible; dev ngoài ác ý thuộc Tier 2, không đi qua đường này —
  Spec 18). Hai lớp kiểm bổ trợ:
  (a) attribute check: crate root của cell + mọi dep ngoài allowlist phải có
  `#![forbid(unsafe_code)]` (parse source, không tin Cargo.toml);
  (b) token check: quét `unsafe` trong source tree của các crate đó (bắt trường hợp file
  bị loại khỏi module graph… và làm nhiễu ngược — chấp nhận false positive, allowlist xử).
  Cân nhắc `cargo geiger` nếu chạy được trên workspace no_std; không bắt buộc.
- Khoá: dev key (seed cố định, `dev-signing-key` feature) giữ nguyên cho dev loop;
  **prod key chỉ tồn tại trong CI/KMS** — `cellos-sign` từ chối chạy chế độ prod ngoài CI
  (kiểm env). Chính sách khoá là guarantee, tool chỉ là cái cổng (Spec 18 §2.1).
- Việc đưa 71 crate về tuân thủ là phần việc lớn nhất và có giá trị độc lập: mỗi crate
  hoặc thêm forbid, hoặc vào allowlist kèm lý do + tham chiếu `// SAFETY:` audit. Shell
  có `unsafe` là **mùi thiết kế** — mục tiêu là xoá, không phải allowlist.
- Kernel-side đã có sẵn: verify Ed25519 + `signing_required` flag (tắt ở dev build,
  [elf_tests.rs:41-46](../../kernel/src/loader/elf_tests.rs#L41-L46)). Phase này không đổi
  kernel; bật `signing_required` cho posture production là việc của release checklist.

## Requirements

**Functional**

1. Tool `cellos-sign` (nâng cấp từ `scripts/sign-cell.py` + `lib-sign-cells.sh`; giữ Python
   hoặc chuyển crate `tools/cellos-sign` — chọn lúc implement, ưu tiên đường ít phá CI):
   một lệnh = build cell (toolchain theo pin) → kiểm F1 → ký artifact vừa build. Không có
   mode ký-file-rời cho prod key.
2. Kiểm F1: attribute check + token check (Key Insights); dep tree lấy từ `cargo metadata`
   của chính lần build; allowlist tại `scripts/unsafe-allowlist.toml` — mỗi entry:
   crate, lý do, người duyệt, ngày.
3. Kiểm F5: toolchain đang chạy khớp `rust-toolchain.toml`; lệch ⇒ từ chối ký.
4. CI: job kiểm mọi cell trong image qua `cellos-sign --check` (không ký); cell fail ⇒ CI đỏ.
   `cargo-deny` (deny.toml sẵn có) chạy cùng job.
5. Migration 71 crate: thêm `#![forbid(unsafe_code)]` cho mọi crate sạch; allowlist driver
   + ostd + (tạm thời) vfs/silo với lý do; **shell phải về sạch** — 4 file `unsafe` của
   shell được sửa trong phase này (dự kiến là buffer/FFI vụn, không phải MMIO).

**Non-functional**

6. Không đổi format chữ ký/section `__ViCell_sig` — kernel verify giữ nguyên.
7. Dev loop không chậm đi quá 10% thời gian build image (check là parse source, rẻ).

**Stretch (Law 1 — 2× confirmation, PR riêng, không chặn phase)**

8. Bit "F1-attested" trong `CellManifest.reserved` để kernel/policy phân biệt tier-1
   attested với chữ ký kiểu cũ trong giai đoạn chuyển tiếp.

## Related Code Files

| File | Hành động |
|------|-----------|
| `scripts/sign-cell.py` → `cellos-sign` | Modify/Create — gộp build+check+sign |
| `scripts/lib-sign-cells.sh` | Modify — gọi cellos-sign, bỏ đường ký rời |
| `scripts/unsafe-allowlist.toml` | Create — allowlist có hồ sơ |
| `.github/workflows/ci.yml`, `security.yml` | Modify — job `cellos-sign --check` + cargo-deny |
| `cells/**/src/main.rs` (~46 crate) | Modify — thêm forbid |
| `cells/tools/shell/src/*` (4 file) | Modify — xoá `unsafe` |
| `docs/specs/18-cell-trust-tiers.md` | Read — hợp đồng phase này hiện thực |

## Implementation Steps

1. Inventory chính xác: script liệt kê (crate, có forbid?, số khối `unsafe`, file) — commit
   báo cáo vào `.agents/reports/`.
2. `cellos-sign --check` (chưa ký): attribute + token + toolchain check, allowlist rỗng →
   chạy trên toàn workspace để có baseline fail list.
3. Migration đợt 1: thêm forbid cho mọi crate sạch (cơ học, một PR).
4. Migration đợt 2: xoá `unsafe` khỏi shell; allowlist driver/ostd/vfs/silo kèm lý do
   (vfs ghi chú: 2 khối "caller blocks" chờ phase 07 audit — không hợp thức hoá vĩnh viễn).
5. Gộp check vào đường ký (`lib-sign-cells.sh`), chặn prod-mode ngoài CI.
6. CI job + cargo-deny; xác nhận image lanes xanh.
7. Cập nhật `docs/security-model.md` mô tả nghĩa mới của chữ ký.

## Todo List

- [x] Inventory script + báo cáo baseline (`.agents/reports/phase-11-f1-baseline-inventory-260730.md`)
- [x] `cellos-sign --check` (attribute/token/toolchain)
- [x] PR forbid hàng loạt cho crate sạch (16 → 51 crate)
- [x] Shell về sạch `unsafe` (36 → 2, cả 2 nằm ở `cmd_fs.rs`, xem Deviation Log);
      allowlist có hồ sơ cho phần còn lại
- [x] Check gộp vào đường ký; prod key chỉ trong CI
- [x] CI job (cargo-deny đã chạy sẵn ở job Security Scan — không nhân bản)
- [x] Image lane signs through F1/F5 and boots; real RV64 sign/verify/tamper rejection passes
      (`ALL PASS`), and the signed image passed W^X 2/2 plus the recorded RV64 boot 54/54 gate.
- [x] Cập nhật security-model.md
- [ ] (Stretch, Law 1) manifest bit F1-attested — hỏi user trước

## Deviation Log

| # | Loại | Nội dung |
|---|------|----------|
| 1 | Surprise | Số liệu trong phase file sai: 76 crate (không phải 71), 16 crate có forbid (không phải 25); 25 là số crate *chứa* unsafe. Đo lại 2026-07-30, ghi vào báo cáo baseline. |
| 2 | Decision | `check-cells-unsafe-ratchet.py` bị **thay thế** (xoá), không bọc lại: luật token + 49 entry của nó chuyển nguyên vào `scripts/unsafe-allowlist.toml`; giữ hai script là giữ hai bản luật sẽ trôi khỏi nhau. Hai workflow CI nay gọi `cellos-sign --check`. |
| 3 | Deviation | Sửa `libs/ostd` (ngoài File Ownership): thêm `entry.rs` với macro `cell_main!`. Lý do: `#![forbid(unsafe_code)]` biến `#[no_mangle]` thủ công thành hard error, nên 46 cell sạch không thể mang attribute. `app_entry!` của ostd đã dựa vào đúng tính chất "lint không bắn trong external macro"; thay đổi thuần additive, không crate nào đang có bị đổi hành vi. |
| 4 | Deviation | `ConfigClient::get()` dùng `Box::leak` để thoả chữ ký `-> ViResult<&str>` của trait mà không launder lifetime. Bản cũ *unsound* (trả `&str` trỏ vào buffer sẽ bị `get()` sau ghi đè). Fix đúng là đổi trait sang `String` — nằm ở `libs/api`, ngoài phạm vi. Shell hiện không gọi `get()` nên chưa leak byte nào. Ghi contract ngay trên hàm. |
| 5 | Decision | F5 so **commit-hash của rustc** (`rustc -Vv` vs `rustc +<pin> -Vv`), không so tên toolchain: `rustup show active-toolchain` đọc chính `rust-toolchain.toml` đang cần kiểm nên gần như tautology. Bản hash bắt được `RUSTUP_TOOLCHAIN` / `rustup override` — đã test. |
| 6 | Decision | Cả hai lớp kiểm quét đúng tập file **git-tracked**. CI chỉ checkout file tracked, nên quét filesystem sẽ nghiêm hơn CI ở máy dev và làm đỏ build vì một crate đang viết dở (đã gặp thật: `cells/tests/wx-test/` untracked của phase song song). |
| 7 | Decision | Không thêm `cargo-deny` vào job mới: `ci.yml` job Security Scan đã chạy nó trên cùng workspace. |
| 8 | Deviation | 2 khối unsafe còn lại của shell nằm ở `cmd_fs.rs`: `ostd::fast_ipc::call_vfs` là `unsafe fn` trong ostd, và `VfsResponse::DataPtr` deref con trỏ vào bộ nhớ của VFS. Cả hai chỉ biến mất khi `DataPtr` biến mất (phase 06) — allowlist tạm kèm `review_by` + `tracking`, không giả code an toàn. |
| 9 | Deviation | Sửa `gen_disk.ps1` (ngoài File Ownership) khi vá lỗ ký không kiểm: lane Windows gọi thẳng `sign-cell.py --in/--out`, tức là một đường ký dev-key không qua F1/F5. Đổi sang gom danh sách rồi gọi `scripts/cellos-sign --sign` một lần (scan F1 là per-tree, không per-binary). Không đụng file nào của phase song song. |
| 10 | Decision | `sign-cell.py` từ chối ký trừ khi `cellos_sign.signing` đã đặt sentinel `_CHECKED`, hoặc caller truyền `--unchecked-dev-signature` (chỉ dev key, dùng cho `test-cell-signing.sh`). Không hạ docstring cho khớp lỗ: dev key cũng nặng như prod vì mọi image local/QEMU đều là dev-key build. |
| 11 | Decision | `--strict` do `run_sign` tự bật, không để call site truyền. Chữ ký chứng thực cả F1 lẫn F5, nên host không xác minh được toolchain phải **từ chối ký** thay vì in `SKIP` rồi ký; một flag caller có thể quên thì không phải là bảo đảm. |
| 12 | Deviation | Tách `lexer.py` khỏi `scan.py`: bộ rút gọn source giờ lex cả string/raw-string/char literal (không chỉ comment), vì `"/*"` trong literal làm scanner mù với `unsafe` phía sau và `"#![forbid(unsafe_code)]"` trong literal giả được attribute — kết hợp hai cái là bypass cả hai lớp. `FORBID_RE` neo đầu dòng. Quét vi sai 834 file: chỉ đổi 2 token ở `kernel/src/main.rs` (chữ "unsafe" trong chuỗi log), 0 thay đổi về forbid. |
| 13 | Decision | Xoá `Crate.path_deps` + `_dep_dirs` thay vì dùng: `forbid` là per-crate nên attribute layer không với tới dep, và biên thật của F1 là "cells sạch forbid, `libs/*` là TCB được review". State thu thập rồi không dùng mà trông như một biện pháp an ninh thì tệ hơn là không có. |
| 14 | Verification | Rerun 2026-07-31: F1/F5 check and 35 signer unit tests pass; a real RV64 ELF signs, verifies, and rejects PT_LOAD tampering (`ALL PASS`). The signed image lane boots and has the recorded W^X 2/2 and RV64 boot 54/54 evidence. See `.agents/reports/a4-runtime-gates-260731.md`. |

## Success Criteria

- `cellos-sign --check` pass toàn workspace với allowlist đã duyệt; CI đỏ nếu một crate
  mới thêm `unsafe` ngoài allowlist.
- 76/76 crate: hoặc forbid, hoặc trong allowlist kèm lý do — không còn trạng thái thứ ba.
- Shell: 2 khối `unsafe` còn lại đều nằm trong `cmd_fs.rs`, có allowlist + tracking tới lúc
  `DataPtr`/`fast_ipc` được thay thế; không có khối shell nào ngoài hồ sơ.
- Thử nghiệm phá: thêm `unsafe {}` vào một cell sạch → build image fail tại bước ký, thông
  báo nêu crate + file.
- Format chữ ký/kernel verify không đổi; real RV64 sign/verify pass và PT_LOAD tamper bị từ chối.

## Risk Assessment

| Rủi ro | Xác suất | Ảnh hưởng | Giảm thiểu |
|--------|----------|-----------|------------|
| Token check false-positive (chuỗi "unsafe" trong comment/string) | Cao | CI đỏ oan | Parse bằng syn/tree-sitter thay vì grep nếu ồn; allowlist per-file là lối thoát cuối |
| vfs/silo nằm allowlist "tạm" rồi thành vĩnh viễn | Trung bình | F1 mất nghĩa dần | Entry allowlist có ngày + con trỏ phase 07; CI cảnh báo entry quá 90 ngày |
| Dev quen đường ký cũ, bypass bằng sign-cell.py trực tiếp | Trung bình | Check bị né | `sign-cell.py` từ chối ký nếu thiếu sentinel `_CHECKED` (chỉ `cellos_sign.signing` đặt, sau khi check pass); opt-in `--unchecked-dev-signature` chỉ cho dev key và có cảnh báo; mode prod vẫn đòi env CI |
| Ferrocene sau này đổi toolchain pin | Thấp | Check F5 fail hàng loạt | Check đọc pin từ `rust-toolchain.toml`, không hardcode |

## Security Considerations

- Threat model phải ghi thẳng trong tool README: chống **lỗi vô ý** của dev tin được;
  KHÔNG chống dev ác ý giữ khoá — đó là việc của chính sách khoá (CI/KMS) và của Tier 2
  (Spec 18). Đừng bán quá.
- Allowlist là bề mặt tấn công xã hội của scheme: mọi entry phải qua review như code, vì
  nó *là* một lỗ được phê duyệt trên tường LBI.

## Next Steps

- Spec 18 §2.3: confidential build (G2+) tái dùng nguyên `cellos-sign` bên trong CVM image.
- Release checklist: bật `signing_required` cho production posture sau khi phase này + 10 ổn định.
