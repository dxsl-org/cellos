# Báo Cáo Phân Tích Kiến Trúc & Lộ Trình Cellos (SAS / LBI)
**Dưới chuẩn kỹ năng `hl-brainstorm` | Đa lăng kính: Architect · Scientist · Devil**
**Ngày lập:** 2026-09-06

---

## 1. Khung Phân Tích & Giới Hạn Phạm Vi (Recon & Framing)

### 1.1 Dữ liệu thực tế từ Codebase (Recon Bullets)
- **Mô hình SAS/LBI trên giấy vs thực tế**: Cellos định vị là hệ điều hành Single Address Space (SAS) dựa trên Language-Based Isolation (LBI - Rust type system) thay vì phần cứng MMU (`README.md:6-8`). Tuy nhiên, nhân vẫn duy trì một bảng trang gốc duy nhất (`KERNEL_ROOT` tại `kernel/src/memory/paging.rs`), không hề chuyển đổi bảng trang (`satp`/`CR3`/`TTBR0`) giữa các Native Cell.
- **Thực trạng các Tiers**:
  - **Tier 1 (Trusted SAS Cell)**: Hoạt động dựa trên `#![forbid(unsafe_code)]` và kiểm tra chữ ký Ed25519 tại loader (`kernel/src/signing.rs`). Nhưng mặc định `signing-required` đang **TẮT** ở G1; mọi cell unsigned đều được nạp thẳng vào SAS.
  - **Tier 2 (Domain Isolated Cell - Private Page Table)**: Hoàn toàn **CHƯA TỒN TẠI** trong mã nguồn thực thi (`docs/system-architecture.md:138` xác nhận: *"No Tier 2 runtime mechanism exists"*).
  - **Tier 3 (Virtual Machine - Stage-2 Hypervisor)**: Đã chạy guest Linux trên QEMU x86/ARM64, nhưng là giải pháp quá cồng kềnh cho các ứng dụng native vừa và nhỏ.
- **Lỗ hổng LBI qua mã ngoại lai (Unsafe / C FFI)**: Tập tin `scripts/unsafe-allowlist.toml` chứa hơn **550 dòng miễn trừ** cho C-FFI (mlibc, doom, lua, tetris), Driver MMIO (e1000, nvme, virtio), và Legacy raw pointers. Bất kỳ một lỗi tràn bộ đệm hay con trỏ hoang nào từ C/FFI đều có thể ghi đè bộ nhớ nhân hoặc các cell khác trong cùng SAS.
- **Bội chi bộ nhớ (Memory Footprint Bloat)**: Kết quả đo đạc chuẩn mới nhất (`perf-local-20260905T224048Z...json`) ghi nhận hệ thống chiếm **79.69 MiB**, vượt gần 8 lần so với mục tiêu trần của hệ nhúng (`< 10 MiB`). Trong đó, riêng `kernel-heap` đã đặt trước cố định **32 MiB**, cộng với hơn 44 MiB phân bổ sớm cho các bảng tĩnh và cell nhúng.
- **Chi phí IPC nghịch lý**: Dù chia sẻ cùng không gian địa chỉ, IPC giữa các cell hiện vẫn phải thực hiện lệnh bẫy nhân (`ecall`/`syscall`), đi qua lập lịch (`scheduler.rs`), kiểm tra `CapTable` và xử lý hàng đợi tin nhắn (`ipc_aware_wake`). Điều này khiến Cellos phải trả chi phí chuyển ngữ cảnh (trap latency) của Microkernel mà không nhận được sự bảo vệ vật lý của Microkernel.

### 1.2 Định hình 5 Yếu tố Cốt lõi (5 Capture Items)
1. **Output**: Bản phân tích nguyên nhân gốc rễ (Root Causes) về an toàn, hiệu năng, bộ nhớ và kiến trúc; kèm 3 phương án giải pháp chiến lược và lộ trình tái cấu trúc có cổng nghiệm thu rõ ràng.
2. **Acceptance Criteria**: Chỉ ra chính xác các mâu thuẫn kiến trúc có dẫn chứng mã nguồn; đề xuất phải giải quyết được "tam giác bất khả thi" của Cellos: **An toàn LBI** — **Hiệu năng Zero-Copy** — **Độ nhỏ gọn cho Embedded**.
3. **Scope Boundary**: Phạm vi thiết kế và chiến lược kiến trúc hệ thống (Kernel, Memory, IPC, Tiers, Drivers, Workflow). Không sửa code trực tiếp trong phiên thảo luận này.
4. **Constraints**: Tuân thủ nguyên tắc độc lập phần cứng [ADR-0007], solo maintainer [ADR-0013], và luồng sản phẩm Robot LAB-01 [ADR-0014].
5. **Touchpoints**: `kernel/src/memory/`, `kernel/src/task/syscall.rs`, `kernel/src/ipc/`, `libs/ostd/`, `cells/services/vfs/`, `docs/project-roadmap.md`.

---

## 2. Phân Tích Đa Lăng Kính (Persona Analysis)

### 2.1 Góc nhìn Kiến trúc sư Hệ thống (The Architect)
- **Nghịch lý "Microkernel trá hình trong SAS"**: Lợi thế lớn nhất của SAS (như trong nghiên cứu của *Midori* hay *Singularity*) là việc giao tiếp giữa các thành phần phần mềm an toàn có thể đạt chi phí gần bằng **gọi hàm con trỏ** (Function Call / Local Ring-Buffer) vì không cần đổi bảng trang và không cần trap phần cứng. Thế nhưng Cellos lại thiết kế IPC theo kiểu Microkernel truyền thống: mỗi message đều trap vào S-Mode/Ring-0, tra cứu bảng `CapTable`, bốc dỡ vào hàng đợi task, rồi đánh thức scheduler. Chúng ta đang gánh trọn **độ trễ trap của Microkernel** nhưng lại từ bỏ **sự cách ly phần cứng của Microkernel**.
- **Khoảng trống chết chóc mang tên Tier 2**: Tài liệu thiết kế chia 3 tầng: Tier 1 (Safe SAS), Tier 2 (Domain Paged Cell), Tier 3 (VM Guest). Nhưng thực tế trên nhánh phát triển, Tier 2 chưa từng được hiện thực hóa. Kết quả là mọi thành phần không an toàn (như C runtime, mã thử nghiệm FFI, thư viện bên thứ ba) hoặc phải bị ép chạy trong Tier 1 (đe dọa làm sập toàn bộ SAS), hoặc phải nâng lên Tier 3 (kéo theo cả một bộ máy ảo Linux cồng kềnh với hàng chục megabyte RAM).
- **Driver Cell nằm ngoài Nhân nhưng nằm trong SAS**: [Kernel Boundary Law] đã đúng đắn khi đưa driver ra khỏi kernel binary. Tuy nhiên, khi chuyển Driver thành các Cell chạy ở Ring-3/EL0, vì không có IOMMU vật lý trên các board nhúng giá rẻ (như RPi3) và không có per-cell MMU, các driver này thao tác thanh ghi MMIO và DMA descriptors thông qua con trỏ thô trong cùng không gian địa chỉ. Một lỗi logic nhỏ của driver NVMe hay VirtIO sẽ làm hỏng trực tiếp bộ nhớ của Kernel.

### 2.2 Góc nhìn Nhà khoa học Thực nghiệm (The Scientist)
- **Bộ số liệu thực nghiệm vạch trần mục tiêu**:
  - *Mục tiêu định vị*: G1 Robot & Embedded $\to$ Thiết bị nhúng tài nguyên hạn chế.
  - *Chỉ số thực tế*: Footprint **79.69 MiB** (vượt trần `< 10 MiB` đến 697%).
  - *Cấu trúc lạm phát bộ nhớ*: 32 MiB dành cứng cho heap nhân, 44 MiB cho boot image, buffers tĩnh và driver scratchpads. Nếu đem Cellos nạp lên một vi điều khiển hay vi xử lý công nghiệp có 64 MiB hoặc 128 MiB RAM, hệ thống sẽ cạn kiệt bộ nhớ ngay từ giai đoạn khởi động mà chưa kịp chạy bất kỳ thuật toán điều khiển robot nào.
- **Tính xác thực của kiểm chuẩn**: Phần lớn các chỉ số benchmark hiệu năng hiện tại được đo trên **QEMU TCG ảo hóa phần mềm** trên máy chủ x86/Linux. QEMU TCG che giấu hoàn toàn các hiện tượng vật lý: chi phí thực sự của TLB shootdown giữa các CPU core, độ trễ phân tán của bộ nhớ cache L1/L2/L3 khi nhiều cell cùng truy cập bus, và tác động của ngắt ngoại vi thời gian thực. Bằng chứng phần mềm trên QEMU là cần thiết nhưng không thể phản ánh chính xác hiệu năng SAS/LBI trên silicon thật.

### 2.3 Góc nhìn Phản biện Nghịch lý (The Devil's Advocate)
- **LBI chỉ là ảo tưởng nếu còn Unsafe Allowlist**: LBI dựa trên một tiền đề toán học tuyệt đối: *"Toàn bộ mã nguồn chạy trong không gian địa chỉ chung đều phải tuân thủ nghiêm ngặt hệ thống kiểu của Rust"*. Khi hệ điều hành chấp nhận hơn 550 dòng file allowlist cho C-FFI (Doom, Lua, mlibc), mã C này được biên dịch trực tiếp thành mã máy nhị phân chạy trần. Trình biên dịch Rust **hoàn toàn mù** trước các hoạt động của mã C. Chỉ cần một con trỏ NULL dereference, một lỗi off-by-one trong `doom` hay `lua`, toàn bộ Kernel và hệ thống an toàn SAS đều bốc hơi.
- **Cái bẫy Spectre v1/v2**: Trong hệ điều hành truyền thống, các cuộc tấn công rò rỉ kênh bên (Side-channel attacks) bị chặn lại bởi biên giới bảng trang giữa các tiến trình. Trong SAS của Cellos, mọi địa chỉ RAM (kể cả khóa bí mật Ed25519 của kernel, bảng phân quyền capabilities, dữ liệu nhạy cảm của các cell khác) đều nằm trong tầm với suy đoán (speculative execution) của CPU. Nếu Cellos muốn trở thành hệ điều hành cho đám mây hoặc đa người dùng (G2, G4), SAS thuần túy mà không có phần cứng bảo vệ là một lỗ hổng bảo mật không thể khắc phục về mặt lý thuyết.

---

## 3. Nguyên Nhân Gốc Rễ (Root Causes)

| Mã | Tên nguyên nhân gốc rễ | Dẫn chứng mã nguồn / Dữ liệu | Cơ chế gây lỗi / Điểm nghẽn |
|---|---|---|---|
| **RC-1** | **Vết nứt cách ly LBI do dung dưỡng C-FFI trong SAS** | `scripts/unsafe-allowlist.toml` (551 dòng), `docs/security-model.md:83-90` | Cho phép mã C và con trỏ thô chạy trực tiếp trong SAS mà không có hàng rào phần cứng (SATP/PMP) cô lập. |
| **RC-2** | **Nghịch lý chi phí IPC: Trap quá mức trong cùng SAS** | `kernel/src/task/syscall.rs`, `libs/ostd/src/ipc.rs` | Dù ở cùng không gian địa chỉ, IPC vẫn bị ép qua trap nhân, context switch và kiểm tra token phức tạp, làm mất ưu thế zero-copy. |
| **RC-3** | **Chiếm dụng bộ nhớ tĩnh thiếu tính co giãn (Memory Bloat)** | `perf-local-20260905...json` (79.69 MiB), `kernel/src/memory/mod.rs` (32 MiB heap) | Nhân phân bổ tĩnh 32 MiB heap và nạp sẵn toàn bộ cells nhúng vào RAM thay vì cấp phát theo nhu cầu (demand paging/dynamic slab). |
| **RC-4** | **Khoảng trống cấu trúc do thiếu vắng Tier 2** | `docs/system-architecture.md:138`, `docs/specs/18-cell-trust-tiers.md` | Thiết kế phân tầng lý thuyết (Tier 1/2/3) bị gãy ở giữa: không có cơ chế chuyển bảng trang nhẹ (SATP) cho native code chưa kiểm chứng. |
| **RC-5** | **Lộ trình phân tán nguồn lực & nợ phần cứng (Hardware Debt)** | `docs/project-roadmap.md`, `.agents/TODO.md` | Dàn trải đồng thời trên quá nhiều mặt trận: Desktop GUI (ViUI), VM Linux (Tier 3), Cloud Server (G2), Robot (G1) trong khi phần cứng thực tế bị nghẽn. |

---

## 4. Đánh Giá Các Chiều Rủi Ro Biên (Edge Cases Sweep)

1. **Scale (Quy mô bộ nhớ & Tải hệ thống) — Mức độ: CRITICAL**
   - Khi số lượng Cell tăng lên hoặc tải I/O lớn, hạn ngạch bộ nhớ tĩnh 79.69 MiB sẽ gây OOM tức thì trên các SoC nhúng có RAM $\le 128\text{ MiB}$.
2. **State Transitions (Chuyển đổi trạng thái & Phục hồi lỗi) — Mức độ: HIGH**
   - Trong mô hình LBI không có bảng trang riêng, nếu một Cell gặp lỗi `panic` (abort), việc dọn dẹp tài nguyên (Resource Cleanup) rất khó đảm bảo tính nguyên tử. Con trỏ thô của Grant đang chia sẻ (`grant_read.rs:347`) có thể trở thành Dangling Pointer sau khi Cell khởi động lại.
3. **Data Integrity (Tính toàn vẹn dữ liệu xuyên Cell) — Mức độ: CRITICAL**
   - Vi phạm bộ nhớ từ các đoạn mã `unsafe` trong Driver hoặc C-FFI ghi đè cấu trúc điều khiển của Kernel (bảng `CapTable` hoặc danh sách Task), gây mất mát dữ liệu thầm lặng (Silent Memory Corruption).
4. **Timing (Độ trễ thời gian thực & Jitter) — Mức độ: HIGH**
   - Vòng lặp điều khiển Robot (Control Loop P99) yêu cầu tính tiền định cao. Việc các Cell phải liên tục trap vào nhân để trao đổi IPC qua hàng đợi FIFO gây trôi độ trễ (Latency Jitter).
5. **Error Cascades (Hiệu ứng domino khi hỏng hóc) — Mức độ: HIGH**
   - Do thiếu sự phân tách phần cứng giữa các Driver ngoại vi, nếu bộ điều khiển phần cứng gặp sự cố trên bus PCIe, thao tác DMA sai lệch địa chỉ có thể xóa sạch bộ nhớ của toàn bộ hệ điều hành.

---

## 5. Ba Hướng Tiếp Cận Chiến Lược

### Phương án A: Pure LBI & Micro-MPU (Tối Thượng Cho Nhúng)
- Trục xuất C/FFI sang Wasm Micro-Runtime an toàn.
- Cắt giảm heap tĩnh xuống 4-8 MiB; cấp phát trang động (Demand Paging).
- Thay thế trap syscall bằng Lock-free SPSC Ring Buffer cho các cell tin cậy cùng Hart.

### Phương án B: Dual-Mode Kernel (Khuyên Nghị Lựa Chọn ⭐)
- **Kích hoạt Tier 2 (Paged Domain Engine)**: Chuyển đổi bảng trang nhẹ (`satp` trên RISC-V, `CR3` trên x86) cho mã C-FFI, Lua runtime và Drivers rủi ro cao.
- **Bảo toàn Real-time SAS cho Tier 1**: Giữ nguyên SAS zero-copy cho các Native Cells an toàn (Robot Control Loops, VFS Core).
- **Hybrid IPC**: Zero-trap Shared Memory cho Tier 1 $\leftrightarrow$ Tier 1; Syscall Bounded Copy cho Tier 2 $\leftrightarrow$ Tier 1 / Kernel.

### Phương án C: Hardware-Assisted Capability SAS
- Đón đầu phần cứng CHERI RISC-V (CHERIoT) hoặc RISC-V WorldGuard.
- Con trỏ năng lực 128-bit kiểm tra bounds và permissions ở cấp độ vi kiến trúc mà không cần đổi bảng trang.

---

## 6. Lộ Trình 4 Giai Đoạn & Cổng Nghiệm Thu (Hard Gates)

```text
Giai đoạn 1 (Bộ nhớ & Chữ ký): Heap 8MB, Footprint < 20MB, signing-required = ON.
      ↓
Giai đoạn 2 (Tier 2 Paged Domain): Bảng trang SATP/CR3 cho C/FFI, Negative Exploit Tests.
      ↓
Giai đoạn 3 (Zero-Trap Fast Path): Lock-Free Ring Buffer Tier 1, IPC P99 < 10µs.
      ↓
Giai đoạn 4 (Nghiệm thu Phần cứng): Đóng băng Desktop/Cloud, chốt G1 Robot trên RPi3 & VF2.
```

- **Gate 1 (Memory Budget)**: Khởi động lên Shell + Bench chiếm RAM $\le 20\text{ MiB}$ trên RISC-V QEMU.
- **Gate 2 (Tier 2 Containment)**: C-cell dereference NULL bị CPU page fault và nhân tiêu diệt an toàn mà toàn bộ SAS Tier 1 không crash.
- **Gate 3 (IPC Latency)**: 100.000 msg giữa 2 Tier 1 cells đạt throughput $\ge 500.000\text{ msg/s}$ và P99 $\le 10\ \mu\text{s}$.
- **Gate 4 (Physical Silicon)**: Kịch bản LAB-01 và CellosFS Native chạy thành công trên bo mạch thực tế RPi3 hoặc VisionFive 2.
