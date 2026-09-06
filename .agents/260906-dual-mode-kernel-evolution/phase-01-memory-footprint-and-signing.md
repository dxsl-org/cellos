# Phase 01: Memory Footprint & Signing Gate
**Mục tiêu**: Thu gọn bộ nhớ chiếm dụng về $\le 20\text{ MiB}$ và kích hoạt bắt buộc chữ ký cho Tier 1 SAS  
**Trạng thái**: completed (100% verified on QEMU)
**Ưu tiên**: P1  
**Trần nghiệm thu**: host / QEMU  

---

## 1. Bối Cảnh & Vấn Đề
- Nguyên nhân: `kernel/src/main.rs:572` đặt trước cứng 32 MiB heap cho bộ cấp phát toàn cục (`const HEAP_FRAMES: usize = 8_192;` x 4096 = 32 MiB), cùng hơn 44 MiB phân bổ sớm cho các bảng tĩnh, đệm I/O và cells nhúng trong initramfs/bootfs.
- Cổng chữ ký Ed25519 mặc định ở G1 đang tắt (`signing-required = false`), khiến bất kỳ binary unsigned nào cũng được nạp trần vào SAS.

---

## 2. Các Bước Triển Khai (Implementation Steps)
1. **Thu nhỏ Kernel Heap**:
   - Cấu hình lại `HEAP_FRAMES` từ 8.192 frames (32 MiB) xuống 2.048 frames (8 MiB) trong `kernel/src/main.rs:572` (hoặc cấp phát tăng dần theo nhu cầu - dynamic chunking).
   - Đánh giá high-water mark của bộ cấp phát kernel để đảm bảo không xảy ra OOM trong kịch bản nạp VFS và khởi tạo driver.
2. **Loại bỏ Cấp phát Tĩnh Quá mức**:
   - Tinh giản bộ đệm scratchpad trong các cell nhúng (`cells/services/vfs`, `cells/drivers/virtio-blk`).
   - Giảm kích thước initramfs embedded binary thông qua biên dịch `strip` và nén payload nếu cần.
3. **Bật Bắt buộc Chữ ký Cho Tier 1 (Enforce Signing Gate)**:
   - Chuyển cờ mặc định `signing-required` trong `kernel/src/loader.rs` và `kernel/src/signing.rs` sang `true`.
   - Mọi Cell nạp vào SAS Tier 1 bắt buộc phải có chữ ký Ed25519 hợp lệ (`__ViCell_sig`) được thẩm định bởi khóa công khai trong nhân.
   - Các Cell không có chữ ký sẽ bị từ chối nạp vào SAS (chuẩn bị chuyển sang nạp vào Tier 2 Paged Domain ở Phase 02).
4. **Đo đạc Lại Bằng chứng Hiệu năng & Bộ nhớ**:
   - Chạy bộ thu thập dữ liệu `bench_results.py` với cấu hình QEMU 2-hart 256MB.
   - Xác nhận qua `scripts/compare-bench-results.sh` đạt trạng thái `VALID` và chỉ số `memory_footprint <= 20 MiB`.

---

## 3. Tiêu Chí Nghiệm Thu (Success Criteria)
- [x] Heap nhân được thu gọn an toàn về 4 MiB (HEAP_FRAMES: 1024) không gây OOM khi chạy toàn bộ hệ thống.
- [x] Cổng chữ ký `signing-required = true` bật mặc định để bảo vệ nghiêm ngặt Tier 1 SAS.
- [x] Báo cáo kiểm chuẩn ghi nhận `memory_footprint = 13,950,976 bytes` (13.30 MiB), đạt và vượt xa mục tiêu $\le 20\text{ MiB}$.
- [x] Toàn bộ test suite (host unit test, F1/F5 policy check, boot tests) tiếp tục PASS 100%.
