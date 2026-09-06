# Phase 02: Tier 2 Paged Domain Engine
**Mục tiêu**: Hiện thực hóa cơ chế chuyển đổi bảng trang phần cứng (SATP / CR3) cho Tier 2 Native Domain  
**Trạng thái**: completed (100% verified on QEMU)
**Ưu tiên**: P1  
**Phụ thuộc**: Phase 01  
**Trần nghiệm thu**: host / QEMU  

---

## 1. Bối Cảnh & Vấn Đề
- Tài liệu kiến trúc quy định Tier 2 dành cho các Cell không tin cậy hoặc chứa mã C-FFI, được bảo vệ bằng bảng trang phần cứng thay vì niềm tin vào trình biên dịch.
- Tuy nhiên, hiện tại `docs/system-architecture.md:138` xác nhận chưa có cơ chế runtime cho Tier 2; mọi cell C-FFI (Doom, Lua, mlibc) đều đang chạy trần trong SAS.
- Hơn 550 dòng miễn trừ trong `scripts/unsafe-allowlist.toml` là nguy cơ đe dọa trực tiếp sự toàn vẹn của nhân.

---

## 2. Các Bước Triển Khai (Implementation Steps)
1. **Quản lý Bảng Trang Riêng (Per-Domain Page Table)**:
   - Triển khai cấu trúc `DomainPageTable` trong `kernel/src/memory/paging.rs`.
   - Ánh xạ vùng nhớ Nhân (Kernel Space) ở chế độ Read-Only hoặc Không thể thực thi (NX) đối với Domain Cell.
   - Cấp phát không gian ảo riêng biệt cho không gian người dùng của Domain Cell (User Space).
2. **Cơ Chế Chuyển Đổi Ngữ Cảnh Bảng Trang (World Switch)**:
   - Khi chuyển đổi task (`kernel/src/task/scheduler.rs`):
     - Nếu chuyển giữa hai Tier 1 Cells: Giữ nguyên `satp` (không flush TLB, tận dụng SAS zero-cost switch).
     - Nếu chuyển sang Tier 2 Cell: Nạp địa chỉ vật lý của `DomainPageTable` vào thanh ghi `satp` (RISC-V) hoặc `CR3` (x86), kích hoạt ASID / PCID để giảm thiểu chi phí TLB flush.
3. **Sao Chép Dữ Liệu An Toàn Qua Biên Giới (Safe User Pointer Copy)**:
   - Hiện thực hóa cơ chế copy an toàn có thể phục hồi (Recoverable Page Fault / `copy_from_user` / `copy_to_user`) trong `kernel/src/task/syscall.rs`.
   - Mọi con trỏ từ Tier 2 Cell truyền vào syscall đều phải được thẩm định phạm vi và xử lý ngoại lệ nếu con trỏ không hợp lệ.
4. **Di Chuyển Mã C-FFI Sang Tier 2**:
   - Chuyển `doom`, `tetris-c`, `posix-shim-test`, và `mlibc-smoke` sang khai báo manifest `execution_tier = 2`.
   - Loại bỏ các mục miễn trừ C-FFI tương ứng trong `scripts/unsafe-allowlist.toml`.
5. **Bộ Kiểm Thử Xâm Phạm Bộ Nhớ Tiêu Cực (Negative Exploit Suite)**:
   - Xây dựng bài kiểm thử `tier2-memory-fault-test`: một C-cell cố tình ghi đè bộ nhớ nhân hoặc dereference `0x0`.
   - Xác nhận CPU kích hoạt Instruction/Load/Store Page Fault; Kernel bắt bẫy và kết liễu cell vi phạm mà không gây Kernel Panic.

---

## 3. Tiêu Chí Nghiệm Thu (Success Criteria)
- [x] Tier 2 Domain Cell được cấp bảng trang riêng với ánh xạ bộ nhớ cô lập (AddressSpace Sv39).
- [x] Chuyển đổi task giữa các Tier 1 Cell không thay đổi `satp`; chuyển sang Tier 2 Cell kích hoạt đúng `satp` của domain đó (World Switch).
- [x] Cell vi phạm bộ nhớ bị tiêu diệt an toàn (`scause=0xf`, `addr=0x0`), nhân ghi log cảnh báo và hệ thống tiếp tục chạy bình thường (được chứng minh qua `tests/integration/tests/tier2_fault_isolation.rs`).
- [x] Toàn bộ mã C-FFI / unsigned được cách ly triệt để khỏi SAS Tier 1 thông qua cổng nạp `governed_spawn.rs`.
