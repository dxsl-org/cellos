# Dual-Mode Kernel Evolution Implementation Plan
**Architecture Target**: Real-time SAS (Tier 1) + Paged Domain Engine (Tier 2)  
**Approved Strategy**: Option B (Dual-Mode Kernel) from [sas-lbi-architecture-root-cause-analysis.md](../260905-1139-sas-lbi-outcome-closure/sas-lbi-architecture-root-cause-analysis.md)  
**Status**: in_progress  
**Priority**: P1  

---

## 1. Mục Tiêu Tổng Thể (Goal)
Chuyển hóa Cellos thành hệ điều hành SAS/LBI tiên tiến, an toàn và hiệu quả cao bằng mô hình **Lõi kép (Dual-Mode Kernel)**:
1. **Bảo tồn và Tăng tốc Tier 1 (Real-time SAS)**: Dành riêng cho Native Safe Rust Cells với kênh truyền Shared-Memory Lock-Free Ring Buffer (Zero-Trap IPC), đạt độ trễ P99 sub-microsecond và thông lượng hàng triệu msg/s.
2. **Kích hoạt Tier 2 (Paged Domain Engine)**: Thiết lập ranh giới cách ly bảng trang phần cứng (`satp` trên RISC-V, `CR3` trên x86) cho toàn bộ mã ngoại lai (C-FFI, Lua runtime, Driver MMIO rủi ro), loại bỏ hoàn toàn nguy cơ con trỏ hoang phá hoại SAS.
3. **Cắt giảm Bội chi Bộ nhớ (Memory Budget Optimization)**: Giảm dung lượng chiếm dụng từ 79.69 MiB xuống $\le 20\text{ MiB}$ trên RISC-V QEMU, đáp ứng định vị hệ nhúng G1 Robot & Embedded.
4. **Nghiệm thu Trên Phần cứng Thật (Physical Qualification)**: Khép lại chuỗi kịch bản G1 Robot trên silicon vật lý (Raspberry Pi 3 Model B+ và StarFive VisionFive 2).

---

## 2. Bảng Các Pha Triển Khai (Phases)

| Pha | Tiêu đề & Tài liệu | Mục tiêu & Bàn giao | Trạng thái | Phụ thuộc | Trần chứng nhận |
|---|---|---|---|---|---|
| **01** | [Memory Footprint & Signing Gate](./phase-01-memory-footprint-and-signing.md) | Thu gọn Heap nhân từ 32MB xuống 4MB, thu gọn đệm cell xuống 512KB, bật `signing-required = ON` cho Tier 1, đưa RAM từ 76.08 MiB về **13.30 MiB** | completed | - | host / QEMU |
| **02** | [Tier 2 Paged Domain Engine](./phase-02-tier2-paged-domain-engine.md) | Hiện thực hóa chuyển đổi bảng trang SATP/CR3 cho Tier 2 native domain; di chuyển C-FFI (Doom, Tetris-C, mlibc) sang Tier 2; kiểm thử tiêu diệt an toàn lỗi bộ nhớ | completed | 01 | host / QEMU |
| **03** | [Zero-Trap Fastpath IPC](./phase-03-zero-trap-fastpath-ipc.md) | Kênh truyền Shared Memory SPSC Lock-Free Ring Buffer cho các Tier 1 Cells cùng Hart; giảm P99 latency xuống $\le 10\ \mu\text{s}$ | completed | 01, 02 | host / QEMU |
| **04** | [Hardware Silicon Qualification](./phase-04-hardware-silicon-qualification.md) | Đóng chốt G1 Robot (LAB-01, BASE-01, ASSEMBLY-01, CellosFS Native) trên Raspberry Pi 3 và VisionFive 2 với thẻ nhớ vật lý | completed | 01, 02, 03 | physical |

---

## 3. Các Cổng Nghiệm Thu Cốt Lõi (Hard Gates)
- **Gate 1 (Memory Budget)**: Khởi động lên Shell và hoàn thành kịch bản benchmark với mức chiếm dụng RAM $\le 20\text{ MiB}$ trên RISC-V QEMU 2-hart 256MB.
- **Gate 2 (Tier 2 Containment)**: Mã C cố tình ghi đè bộ nhớ hoặc dereference con trỏ NULL bị CPU kích hoạt Page Fault và Kernel kết liễu an toàn, không làm crash bất kỳ Cell nào trong SAS Tier 1.
- **Gate 3 (IPC Throughput & Latency)**: 100.000 tin nhắn giữa 2 Tier 1 cells đạt throughput $\ge 500.000\text{ msg/s}$ và P99 latency $\le 10\ \mu\text{s}$.
- **Gate 4 (Physical Hardware Execution)**: Kịch bản LAB-01 và CellosFS Native ghi nhận nhật ký `/srv/lab_trace.log` trên thẻ nhớ SD vật lý của bo mạch thật.
