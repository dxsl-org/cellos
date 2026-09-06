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
1. **Thiết lập**:
   - Nạp các Cell điều khiển: `robot-demo`, `robot-dashboard`, `service-input`.
   - Kích hoạt bài test chuyển giao khay mẫu `bench-probe ctl-loop`.
   - Các Cell điều khiển sử dụng kênh truyền `FastpathEndpoint` (SPSC Lock-Free Ring Buffer).
2. **Tiêu chí Đạt (Pass Criteria)**:
   - Các tín hiệu điều khiển khay mẫu hoàn thành đúng chuỗi trạng thái (`READY` $\to$ `LOCK` $\to$ `TRANSFER` $\to$ `RELEASE`).
   - Độ trễ truyền thông IPC giữa 2 Cell điều khiển trên Hart đạt $P99 \le 10\ \mu\text{s}$ (đo đạc trực tiếp trên chu kỳ CPU thật).
   - Cơ chế khóa an toàn loại trừ không để xảy ra tranh chấp dữ liệu giữa các thao tác cơ khí ảo/thật.

### Giao Thức 4: Kiểm Thử Cắt Nguồn Đột Ngột (G1-PHYS-04: Sudden Power-Loss & Recovery)
1. **Thiết lập**:
   - Chạy tiến trình ghi liên tục vào CellosFS Native: `bench-probe --scenario native_stateful --continuous`.
   - Trong lúc đèn LED hoạt động của thẻ nhớ SD đang nhấp nháy ghi dữ liệu, **rút trực tiếp cáp nguồn DC 5V** của bo mạch.
   - Chờ 5 giây, cắm nguồn trở lại.
2. **Tiêu chí Đạt (Pass Criteria)**:
   - Hệ thống khởi động lại bình thường, không rơi vào trạng thái Kernel Panic hoặc treo bootloader.
   - Trình điều khiển `CellosFsBackend` tự động kiểm tra tính nhất quán CoW Extent:
     - Khối dữ liệu chưa hoàn tất commit bị bỏ qua an toàn.
     - Superblock khôi phục về trạng thái hợp lệ gần nhất ($Gen - 1$).
   - Phân vùng `/srv` mount thành công ở chế độ ghi/đọc, không bị corrupt hệ thống tập tin.

---

## 4. Quy Trình Mở Khóa Cổng G1 (Graduation Gate Clearance)

Sau khi duy trì nhật ký kiểm định UART và checksum đầy đủ theo 4 giao thức trên:
1. Cập nhật `docs/app-tier-acceptance-ledger.json`:
   - Chuyển `subject: "physical-rpi3"` và `"physical-vf2"` từ `BLOCKED` sang `ACCEPTED`.
   - Đính kèm đường dẫn tệp log UART và mã băm SHA-256 của ảnh đĩa boot.
2. Cập nhật `docs/project-roadmap.md`:
   - Ghi nhận hoàn tất cột mốc **G1 Physical Silicon Qualification** (06C, 07C, 08C).
   - Chính thức đóng chốt phiên bản **Cellos v0.2.1-dev Mycelium** sẵn sàng cho giai đoạn phát triển G2 Cloud & Edge.
