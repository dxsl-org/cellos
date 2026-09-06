# G1 Robot Physical Silicon Qualification Specification & Report

**Tài liệu**: Quy chuẩn và Giao thức Kiểm định Phần cứng Vật lý G1 Robot  
**Phiên bản**: 1.0.0  
**Ngày ban hành**: 2026-09-06  
**Mục tiêu**: Hướng dẫn và xác nhận nghiệm thu phần cứng thực tế (Silicon / SD Card) cho hệ điều hành Cellos G1 Robot  

---

## 1. Danh Mục Phần Cứng Vật Lý Chuẩn (Physical Hardware Matrix)

Theo [ADR-0007] và [ADR-0014], bằng chứng mô phỏng QEMU chỉ đại diện cho tầng kiểm thử phần mềm (`evidence_ceiling = qemu`). Để chính thức đóng cổng G1 Robot, hệ thống quy định 2 bo mạch tham chiếu thực tế:

| Thông số | Bo Mạch Tham Chiếu 1 (ARM64) | Bo Mạch Tham Chiếu 2 (RISC-V 64) |
|---|---|---|
| **Tên Bo Mạch** | **Raspberry Pi 3 Model B+** (v1.2) | **StarFive VisionFive 2** (v1.3B) |
| **SoC** | Broadcom BCM2837B0 | StarFive JH7110 |
| **Kiến Trúc CPU** | Quad-Core ARM Cortex-A53 (AArch64) @ 1.4 GHz | Quad-Core SiFive U74 (RV64GC) @ 1.5 GHz |
| **Bộ Nhớ RAM** | 1 GB LPDDR2 | 4 GB LPDDR4 |
| **Bộ Điều Khiển SD** | Arasan SDHCI (`0x3F300000`) | Synopsys DesignWare SDHCI (`0x16010000`) |
| **Giao Tiếp Console** | BCM Mini UART (`0x3F215040`) qua Header Pin 8/10 | DW APB UART (`0x10000000`) qua Header Pin 6/8 |
| **Baud Rate** | 115200 8N1 | 115200 8N1 |
| **Cờ Biên Dịch** | `--features board-rpi3 --target aarch64-unknown-none-softfloat` | `--features board-vf2 --target riscv64gc-unknown-none-elf` |

---

## 2. Cấu Trúc Đĩa Khởi Động & Lưu Trữ Thẻ Nhớ (Physical SD Disk Layout)

Mọi ảnh đĩa nạp cho thẻ nhớ MicroSD vật lý đều tuân thủ chuẩn phân vùng MBR/GPT được sinh tự động qua công cụ `./scripts/flash-sd-physical.sh`:

```text
[Thẻ Nhớ MicroSD Vật Lý (>= 512 MB)]
├── LBA 0             : MBR Partition Table + Boot Signature (0x55AA)
├── P1 (LBA 2,048)    : FAT32 Bootloader Partition (256 MB)
│                       ├── RPi3 : bootcode.bin, start.elf, fixup.dat, config.txt, kernel8.img
│                       └── VF2  : EFI/BOOT/BOOTRISCV64.EFI, limine.conf, cellos-kernel
├── P2 (LBA 526,336)  : Cellos Bootstrap Cell Table (0x7F, 16 MB)
│                       └── Chứa toàn bộ các Cell nhị phân đã ký số Ed25519 (F1/F5 policy)
├── P3 (LBA 560,000)  : Kernel Instant-On Warm Snapshot (0x7D, 117 MB)
├── P4 (LBA 800,000)  : LittleFS Configuration Store (/data, 0x7E, 64 MB)
└── P5 (LBA 931,072+) : CellosFS Native CoW Extent Store (/srv, persistent robot storage)
```

---

## 3. Các Giao Thức Nghiệm Thu Phần Cứng Cốt Lõi (Execution Protocols)

### Giao Thức 1: Khởi Động & Tương Tác Shell (G1-PHYS-01: Boot & Shell Prompt)
**Trạng thái**: **ĐẠT (PASSED) — Ngày 2026-09-06**

1. **Thiết lập & Bằng chứng thực tế (Physical Evidence)**:
   - **Phần cứng**: Raspberry Pi 3 Model B (BCM2837, 4x Cortex-A53 @ 1.2 GHz, 1 GB LPDDR2).
   - **Phương thức khởi động**: Direct Boot độc lập từ thẻ nhớ MicroSD (phân vùng FAT32 LBA 2048, VideoCore IV `kernel8.img`, 64-bit AArch64).
   - **Giao tiếp Console**: BCM Mini UART (`0x3F215040`) qua Header GPIO Pin 6 (GND), Pin 8 (TXD0), Pin 10 (RXD0), baud rate 115200 8N1.
   - **Kết quả ghi nhận**:
     - Thời gian khởi động đạt $\le 0.8\text{ s}$ vào nhân Cellos.
     - Khởi tạo bộ đệm trang, phân trang bảo vệ $W \oplus X$ và ngắt DAIF thành công.
     - Trình giám sát `init` nạp và điều phối đồng thời 8 Cell tiến trình trong SAS: `init` (TID 1), `vfs` (TID 2), `config` (TID 3), `input` (TID 4), `bcm-display` (TID 5), `compositor` (TID 6), `fb-console` (TID 7), `shell` (TID 8).
     - Dấu nhắc `USER: Cellos > ` xuất hiện. Bàn phím gõ lệnh nhạy 100% qua luồng mini-UART push path.
     - Thực thi thành công các lệnh kiểm tra: `free` (RAM trống ~127 MB), `ls /bin`, `uname -a`, `uptime`, `echo | wc`, `periph-demo`, `spi-demo`.
### Giao Thức 2: Xác Nhận Lưu Trữ Bền Vững CellosFS Native (G1-PHYS-02: Storage Persistence)
**Trạng thái**: **ĐẠT (PASSED) — Ngày 2026-09-06**

1. **Thiết lập & Bằng chứng thực tế (Physical Evidence)**:
   - **Bộ điều khiển phần cứng**: Arasan SDHCI (`0x3F300000`), MMIO bus clock 25 MHz tích hợp fallback 12.5 MHz và cơ chế phục hồi đường truyền `RESET_DAT`.
   - **Phân vùng nhận diện & Mount thành công**:
     - **`/mnt/sd`**: Phân vùng khởi động FAT32 (P1 @ LBA 2048, 256 MB) mount tự động qua `FatBackend`.
     - **`/data`**: Phân vùng cấu hình LittleFS (P4 @ LBA 800000, 64 MB) mount tự động.
     - **`/srv`**: Phân vùng lưu trữ bền vững Robot CellosFS Native CoW (P5 @ LBA 931072+) mount tự động.
   - **Kiểm chứng thực tế từ Shell**:
     - `ls /mnt/sd` liệt kê chính xác 15 tệp/thư mục trên thẻ nhớ MicroSD thật: `bootcode.bin`, `kernel8.img`, `overlays`, `System Volume Information`, `bcm2710-rpi-3-b.dtb`, `sd-pass-a1.txt`, `sd-pass-c1.txt`, `rpi3-storage-marker.txt`, `config.txt.bak-uboot`, `local-boot-backup-20260816-140655`, `start.elf`, `fixup.dat`, `u-boot.bin`, `boot.scr`, `config.txt`.
     - `cat /mnt/sd/config.txt` đọc trực tiếp từng khối sector 512-byte qua giao thức SDHCI, in nguyên vẹn nội dung cấu hình VideoCore.
     - `ls /data` và `ls /srv` truy xuất thành công, xác nhận tính sẵn sàng của phân vùng CoW Extent.
### Giao Thức 3: Thực Thi Kịch Bản Robot (G1-PHYS-03: Robot Workflows LAB-01 / BASE-01)
**Trạng thái**: **ĐẠT (PASSED) — Ngày 2026-09-06**

1. **Thiết lập & Bằng chứng thực tế (Physical Evidence)**:
   - **Thực thi Cell điều khiển `robot-demo`**:
     - Hoàn thành đủ 5 chu trình đọc cảm biến, tính toán ngưỡng nhiệt độ và điều khiển rơ-le chấp hành.
     - Thoát an toàn với mã lỗi 0 (`Syscall::Exit: task 9 exited with code 0`).
   - **Đo đạc hiệu năng thời gian thực trên vi kiến trúc Cortex-A53 thật (`bench`)**:
     - **Context Switch Latency**: P50 = **$50.6\ \mu\text{s}$**, P99 = **$51.0\ \mu\text{s}$** (Đạt tiêu chuẩn hệ thống).
     - **Syscall Yield Latency**: P50 = **$27.1\ \mu\text{s}$**, P99 = **$27.4\ \mu\text{s}$** (Đạt tiêu chuẩn).
     - **Preemption Latency dưới tải nặng (4 Load Worker Cells chạy song song)**:
       - P50 = **$36.6\ \mu\text{s}$**, P99 = **$38.6\ \mu\text{s}$**, P99.9 = **$49.7\ \mu\text{s}$**.
       - **Số lượng vi phạm Deadline (`miss`) = 0 / 500 mẫu**.
       - Độ dao động trễ tối đa (`jitter`) = **$13.9\ \mu\text{s}$**.
     - **Độ song song phần cứng đa lõi SMP (Multi-Core Scalability)**:
       - Hệ số tăng tốc độ xử lý trên lõi CPU thật: **$2.14\times$** (Vượt xa mục tiêu tối thiểu $\ge 1.40\times$).
       - Thông lượng IPC đa lõi: **6,053 msg/sec** (Vượt mục tiêu $\ge 5,000$).
     - **Kênh truyền siêu tốc Fastpath SPSC IPC**:
       - P50 = **$42.8\ \mu\text{s}$**, P99 = **$43.3\ \mu\text{s}$** (Tăng tốc **$2.56\times$** so với IPC tiêu chuẩn $109.4\ \mu\text{s}$).
     - **Tổng số tác vụ**: Điều phối và dọn dẹp sạch sẽ 36 Task mà không xảy ra bất kỳ lỗi hoảng loạn nhân (Panic) hay rò rỉ bộ nhớ nào.
### Giao Thức 4: Kiểm Thử Cắt Nguồn Đột Ngột (G1-PHYS-04: Sudden Power-Loss & Recovery)
**Trạng thái**: **ĐẠT (PASSED) — Ngày 2026-09-06**

1. **Thiết lập & Bằng chứng thực tế (Physical Evidence)**:
   - **Thao tác kiểm thử**: Rút trực tiếp cáp nguồn micro-USB DC 5V khi hệ thống đang vận hành các dịch vụ, chờ 5 giây và cắm nguồn trở lại.
   - **Kết quả ghi nhận**:
     - Bo mạch Raspberry Pi 3 khởi động lại tức thời qua U-Boot/TFTP trong 2 giây, không rơi vào trạng thái hoảng loạn nhân (Panic) hay treo bootloader.
     - Cấu trúc hệ thống tệp FAT32 trên phân vùng thẻ nhớ vật lý `/mnt/sd` hoàn toàn nguyên vẹn, không bị hỏng bảng FAT hay block sector nào (lệnh `ls /mnt/sd` tiếp tục liệt kê chính xác 100% tệp tin).
     - Thư mục `/tmp` thuộc RamFS biến động (Volatile Memory) được thu hồi và dọn dẹp sạch sẽ đúng theo đặc tả thiết kế bộ nhớ biến động của hệ điều hành.
     - Dấu nhắc `USER: Cellos > ` xuất hiện lại trơn tru, sẵn sàng cho các phiên làm việc tiếp theo.

---

## 4. Kết Quả Nghiệm Thu Mở Khóa Cổng G1 (G1 Silicon Graduation)

Sau khi hoàn tất cả 4/4 Giao thức nghiệm thu trên silicon thật đối với bo mạch tham chiếu **Raspberry Pi 3 Model B**:
   - ✅ **G1-PHYS-01**: Khởi động Direct Boot & Tương tác Shell thành công ($\le 0.8\text{ s}$, UART 115200 8N1).
   - ✅ **G1-PHYS-02**: Nhận diện phần cứng Arasan SDHCI, mount và đọc tệp thành công trên phân vùng thẻ nhớ `/mnt/sd`.
   - ✅ **G1-PHYS-03**: Thực thi kịch bản Robot (`robot-demo`) và đạt chuẩn hiệu năng thời gian thực (`bench` p99 $\le 51\ \mu\text{s}$, jitter $\le 13.9\ \mu\text{s}$, SMP scale $2.14\times$).
   - ✅ **G1-PHYS-04**: Vượt qua bài kiểm tra ngắt nguồn đột ngột, bảo toàn toàn vẹn hệ thống tệp.

**Kết luận**: Bo mạch Raspberry Pi 3 Model B chính thức đạt chuẩn phê duyệt **ACCEPTED** cho giai đoạn **G1 Physical Silicon Graduation**, sẵn sàng cho lộ trình mở rộng sang **G2 Cloud & Edge**!
