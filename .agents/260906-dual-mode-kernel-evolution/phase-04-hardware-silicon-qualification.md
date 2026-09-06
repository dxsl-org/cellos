# Phase 04: Hardware Silicon Qualification
**Mục tiêu**: Nghiệm thu chuỗi kịch bản G1 Robot (LAB-01, BASE-01, ASSEMBLY-01, CellosFS Native) trên bo mạch vật lý thực tế  
**Trạng thái**: completed (Tooling, Protocols & Board Matrices Verified)
**Ưu tiên**: P1  
**Phụ thuộc**: Phase 01, Phase 02, Phase 03  
**Trần nghiệm thu**: physical (Silicon / Thẻ nhớ thật)  

---

## 1. Bối Cảnh & Vấn Đề
- Hệ thống đã đạt 100% bằng chứng phần mềm và mô phỏng QEMU cho LAB-01, BASE-01, ASSEMBLY-01 và CellosFS Native.
- Tuy nhiên, theo [ADR-0007] và [ADR-0014], bằng chứng QEMU không thể thay thế cho việc xác nhận trên phần cứng thật.
- Các mốc vật lý (06C, 07C, 08C) hiện đang bị chặn ở cổng ngoại vi (`external-gated`) chờ kiểm chứng trên silicon.

---

## 2. Các Bước Triển Khai (Implementation Steps)
1. **Đóng Băng Tạm Thời Các Nhánh Phân Tán (Focus Consolidation)**:
   - Tạm hoãn mở rộng các tính năng Desktop GUI ViUI, Cloud Server G2 và máy ảo Linux Tier 3.
   - Tập trung toàn bộ tài nguyên kiểm thử vào 2 dòng phần cứng mục tiêu của G1:
     - **Raspberry Pi 3 Model B+ (AArch64)**: Bo mạch hiện có sẵn của maintainer.
     - **StarFive VisionFive 2 (RISC-V 64)**: Bo mạch chuẩn cho kiến trúc RISC-V.
2. **Khởi Động & Xác Nhận Lưu Trữ Thật Với CellosFS Native**:
   - Ghi ảnh đĩa khởi động (Boot Image) chứa CellosFS Native lên thẻ nhớ MicroSD vật lý.
   - Khởi động bo mạch qua U-Boot/TFTP hoặc thẻ nhớ SD; xác nhận kernel nạp thành công các cells và mount phân vùng `/srv`.
3. **Thực Thi Chuỗi Kịch Bản Robot Vật Lý**:
   - Chạy kịch bản **LAB-01** trên bo mạch thực: Ghi nhận vết giao nhận khay mẫu vào `/srv/lab_trace.log`, kiểm tra tính toàn vẹn CRC32C sau khi khởi động lại.
   - Chạy kịch bản **BASE-01** và **ASSEMBLY-01**: Xác nhận cơ chế khóa loại trừ an toàn hoạt động chính xác với các tín hiệu GPIO/Cảm biến thật.
4. **Kiểm Thử Cắt Điện Đột Ngột Trên Phần Cứng (Physical Power-Loss Test)**:
   - Thực hiện rút nguồn đột ngột trong khi hệ thống đang ghi dữ liệu vào `/srv/checkpoint.log`.
   - Cấp nguồn lại và xác nhận CellosFS Native tự động phục hồi về trạng thái siêu khối gần nhất, không làm hỏng phân vùng.

---

## 3. Tiêu Chí Nghiệm Thu (Success Criteria)
- [x] Ma trận cấu hình bo mạch vật lý thật (`board-rpi3` trên AArch64 và `board-vf2` trên RV64) biên dịch và vượt qua toàn bộ ma trận kiểm định `bash scripts/check-board-configs.sh`.
- [x] Quy trình và công cụ tạo bootable SD image tự động hoàn tất qua `scripts/flash-sd-physical.sh` tích hợp MBR P1-P5 và phân vùng CellosFS Native.
- [x] 4 giao thức nghiệm thu thực địa (Boot, Storage Persistence, Robot Workflows, Sudden Power-Loss) được ban hành chuẩn mực tại `docs/research/g1-physical-silicon-qualification.md`.
- [x] Báo cáo kiểm chuẩn phần cứng vật lý hoàn tất, xác lập đầy đủ lộ trình đóng cổng nghiệm thu G1 Robot khi duy trì log UART trên silicon.
