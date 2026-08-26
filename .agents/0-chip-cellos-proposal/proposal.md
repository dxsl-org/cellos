# ĐỀ XUẤT ĐẦU TƯ
## Phát triển Chip RISC-V Tùy Chỉnh cho Hệ Điều Hành Cellos

**Phân loại:** Nội bộ — Cơ mật  
**Ngày:** 25/06/2026  
**Phiên bản:** 1.0  
**Chuẩn bị bởi:** DXSL

---

## 1. TÓM TẮT ĐIỀU HÀNH

Cellos là hệ điều hành Việt Nam dựa trên kiến trúc **Cellular SAS/LBI** (Single Address Space + Language-Based Isolation) — một mô hình kiến trúc mới căn bản, khác biệt hoàn toàn so với Linux/Windows truyền thống.

Đề xuất này xin vốn để thiết kế và tapeout **một vi xử lý RISC-V tùy chỉnh** với các tập lệnh mở rộng được tối ưu hóa đặc biệt cho kiến trúc Cellos. Bộ đôi **Cellos Chip + Cellos OS** sẽ tạo ra một sản phẩm không có đối thủ trực tiếp nào trên thị trường hiện tại:

> *"N-way hardware Cell isolation trong single address space, zero TLB overhead"*  
> — Khả năng này **không tồn tại** ở bất kỳ chip production nào trên thị trường tính đến tháng 6/2026.

**Cửa sổ cơ hội:** 2026–2028, trước khi các tiêu chuẩn RISC-V (SmMTT, RISC-V Worlds) được ratify và đến silicon.

**Thị trường mục tiêu chính:** Robotics/Tự động hóa công nghiệp + Edge AI bảo mật cao + **Máy chủ/PC an ninh chính phủ và quốc phòng**.

---

## 2. BỐI CẢNH: TẠI SAO BÂY GIỜ?

### 2.1 Vấn đề của kiến trúc bảo mật hiện tại

Mọi vi xử lý bảo mật trên thị trường ngày nay — từ ARM TrustZone đến Intel TDX — được thiết kế cho các **ngôn ngữ lập trình không an toàn** (C/C++), cần phần cứng MMU để cô lập các tiến trình khỏi nhau.

Hệ quả là mô hình bảo mật hiện tại có hai giới hạn cơ bản không thể khắc phục trong kiến trúc đó:

**Giới hạn 1 — Số lượng domain cô lập:**
ARM TrustZone (tiêu chuẩn ngành hiện tại) chỉ có **2 worlds** — Secure và Non-Secure. Khi một thiết bị cần cô lập 10, 20, hay 50 thành phần khác nhau (WiFi driver, motor controller, crypto key store, AI inference engine...), tất cả phải nhét vào một trong hai world. Một lỗ hổng trong WiFi driver = toàn bộ thiết bị bị compromise.

**Giới hạn 2 — Chi phí chuyển ngữ cảnh:**
Chuyển giữa Secure ↔ Non-Secure world tốn ~500 CPU cycles mỗi lần. Hệ thống real-time với hàng nghìn IPC operations/giây trả giá hiệu năng đáng kể.

### 2.2 Cellos đã giải quyết vấn đề này ở tầng phần mềm

Cellos dùng **Rust type system** thay thế MMU làm cơ chế cô lập — mỗi Cell (thành phần phần mềm) được cô lập khỏi Cell khác bởi trình biên dịch Rust, không cần page table riêng. Kết quả:
- N Cells cô lập nhau, không giới hạn số lượng
- Chuyển ngữ cảnh giữa Cells ~10ns (so với ~500 cycles của TrustZone)
- Zero TLB flush (không có per-process page table)

**Câu hỏi chiến lược:** Nếu phần mềm đã đúng, chip tùy chỉnh thêm gì?

---

## 3. GIÁ TRỊ CỦA CHIP TÙY CHỈNH

### 3.1 Nguyên tắc cốt lõi

Phần cứng mainstream được thiết kế cho ngôn ngữ unsafe → cần MMU để bảo vệ. Cellos dùng Rust → MMU là *overkill*. Với silicon tùy chỉnh, có thể làm điều tốt hơn: **phần cứng cộng sinh với type system**, thay vì thay thế nó.

### 3.2 Bốn tập lệnh mở rộng đề xuất

#### `Xcell` — Cell-ID Tagged Memory *(tác động cao nhất)*

Mỗi cache line (64 byte) mang một **Cell-ID tag** 12 bit. Mọi load/store đều được phần cứng kiểm tra tag này so với Cell hiện tại đang chạy.

```
┌─────────────┬──────────────────────────────────┐
│  Cell-ID    │  Data (64 bytes)                 │
│  (12 bit)   │                                  │
└─────────────┴──────────────────────────────────┘
     ↑ Hardware check tại memory bus
     Nếu sai → trap ngay lập tức
```

**Ý nghĩa:** Ngay cả khi kernel bị tấn công, hardware tags vẫn ngăn Cell này đọc trộm memory của Cell kia. Đây là điều **không thể làm với TrustZone** (chỉ 2 worlds) hay container (shared kernel).

**Overhead:** ~3% memory (tag per cache line).

#### `Xgrant` — Hardware Ownership Transfer *(IPC zero-copy cứng)*

Một instruction nguyên tử chuyển ownership vùng nhớ từ Cell này sang Cell khác — cập nhật Cell-ID tags đồng thời.

```asm
GRANT.CELL  rd, rs_ptr, rs_len, rs_dst_cell
; Chuyển [rs_ptr .. rs_ptr+rs_len] từ Cell hiện tại → Cell đích
; Atomic, hardware-verified: không thể grant memory không thuộc mình
; rd = grant handle để tracking
```

**Ý nghĩa:** Zero-copy IPC được đảm bảo bởi phần cứng. Loại bỏ hoàn toàn class confused-deputy attack.

#### `Xprobe` — Per-Cell Hardware Counters *(never-die precision)*

Thay vì performance counters per-CPU (Linux model), có counters per-Cell-ID cập nhật liên tục.

| Counter | Ý nghĩa cho reliability |
|---------|------------------------|
| `cell_cycles[N]` | CPU cycles Cell N tiêu thụ |
| `cell_ipc_tx[N]` | Số IPC messages gửi |
| `cell_deadline_miss[N]` | Số lần miss RT deadline |
| `cell_faults[N]` | Số lần trap (signal bất thường) |

**Ý nghĩa:** Failure detection ở microsecond level (so với health check polling hiện tại ở giây level). Cellos Supervisor Cell có thể restart Cell lỗi trước khi người dùng nhận thấy.

#### `Xrt` — Hardware Deadline Registers *(RT guarantees cứng)*

Mỗi Cell có hardware deadline register. Nếu Cell không hoàn thành checkpoint trước deadline → hardware interrupt tới supervisor.

```
SETDEADLINE  rs_cell, rs_cycles   ; Kernel đặt deadline
CHECKPOINT                         ; Cell "tôi còn sống, gia hạn"
; Nếu CHECKPOINT không được gọi trước deadline →
; Hardware trap tới supervisor, không cần syscall polling
```

**Ý nghĩa:** RT hard guarantees không thể bị bypass bởi busy-loop, không có overhead. Đây là điều kiện tiên quyết để đạt IEC 62443 / ISO 26262 ASIL-D.

---

## 4. PHÂN TÍCH THỊ TRƯỜNG

### 4.1 Quy mô thị trường

| Segment | Quy mô 2030 | Tốc độ tăng |
|---------|-------------|-------------|
| Industrial IoT (IIoT) | >$100 tỷ USD | ~15%/năm |
| Automotive electronics | ~$400 tỷ USD | ~8%/năm |
| Edge AI chips | >$50 tỷ USD | ~25%/năm |
| Secure MCU (mục tiêu trực tiếp) | ~$10 tỷ USD | ~12%/năm |
| **Chính phủ & Quốc phòng IT (toàn cầu)** | **~$130 tỷ USD** | ~7%/năm |
| **RISC-V edge AI (ABI Research)** | **129 triệu units/năm vào 2030** | — |

Chỉ cần 0,1% thị trường Secure MCU = $10 triệu doanh thu. Với thị trường chính phủ/quốc phòng, hợp đồng đơn lẻ có thể đạt giá trị tương đương trong một lần ký kết.

### 4.2 Ba thị trường mục tiêu cụ thể

#### 🥇 Thị trường 1: Safety-Critical Robotics / Tự động hóa công nghiệp

**Vấn đề hiện tại:**
- Robots chạy Linux + ROS 2: powerful nhưng attack surface khổng lồ. Một lỗ hổng trong WiFi driver = compromise toàn bộ robot (bao gồm motor control)
- Robots chạy FreeRTOS/Zephyr: real-time nhưng C-based, MPU chỉ 8–16 regions, zero isolation giữa các thành phần
- Không có OS nào vừa RT + genuine hardware isolation + Rust-native

**Khoảng trống:** IEC 62443-4-1 yêu cầu cô lập giữa các security zones. TrustZone's 2-world model không đủ để satisfy auditor nghiêm túc với hệ thống phức tạp.

**Cellos Chip giải quyết:**
```
Cell: Motor Controller    (RT, Xrt deadline, exclusive MMIO)
Cell: Sensor Fusion       (RT, read-only sensor data)
Cell: WiFi Stack          (non-RT, KHÔNG thể access motor/sensor Cells)
Cell: OTA Update          (signed, capability-gated)
Cell: Crypto Key Store    (Xcell-tagged, không Cell nào extract được)
```
WiFi Stack bị compromise không thể kill motor. Hardware enforced.

**Competitor hiện tại:**
- NXP S32 (automotive): C/AUTOSAR, 2-world TrustZone
- TI Sitara: dual-core MCU+MPU, không có intra-domain isolation
- Nordic nRF9161: TrustZone, 2 worlds

#### 🥈 Thị trường 2: Secure Edge AI

**Vấn đề hiện tại:** Edge AI nodes (RK3588, Hailo-8, Jetson) đều chạy Linux. AI model = tài sản IP cực kỳ giá trị. Không có hardware boundary nào giữa networking code và AI inference engine.

**Kịch bản tấn công:** Compromise WiFi driver → đọc AI model weights → copy về → clone sản phẩm IP.

**Cellos Chip giải quyết:**
```
Cell: AI Inference        (model weights trong Xcell-tagged pages, CHỈ cell này đọc được)
Cell: Camera Input        (one-way Xgrant tới Inference cell)
Cell: Network Output      (chỉ nhận RESULTS, không nhận weights)
Cell: OTA + Signing       (hardware-measured, verified update)
```

**Thị trường cụ thể:** Industrial vision (kiểm tra chất lượng sản xuất), medical imaging devices, smart meter / grid edge.

#### 🥉 Thị trường 3: Post-Quantum Secure IoT Fleet

**Vấn đề hiện tại:** NIST PQC standards (ML-KEM, ML-DSA) finalized 2024. Toàn bộ fleet IoT hiện tại cần upgrade crypto. Software-only PQC trên Cortex-M4 ≈ 50ms/operation — quá chậm.

**Cơ hội:** Hardware NTT accelerator (core op của lattice crypto) + PUF-based device identity = chip làm ML-KEM trong microseconds với key không bao giờ rời silicon. Không ai đang làm điều này ở production MCU scale cho RISC-V.

#### ⭐ Thị trường 4: Máy Chủ & PC An Ninh Cao — Chính Phủ, Quân Đội, Tình Báo

Đây là thị trường có **rào cản chính trị cao nhất** và **động lực mua mạnh nhất**: chủ quyền công nghệ. Không một quốc gia nào muốn hệ thống máy tính nhà nước — đặc biệt quân sự và tình báo — phụ thuộc hoàn toàn vào chip nước ngoài với khả năng có backdoor không kiểm soát được.

**Bối cảnh toàn cầu:**
- Nga → Astra Linux (Linux fork cho quân đội), chạy trên x86 nước ngoài
- Trung Quốc → UOS/Kylin (Linux fork), chạy trên Loongson/Zhaoxin — nhưng vẫn kiến trúc x86 clone
- Mỹ → SELinux/NSA, nhưng không chia sẻ cho đồng minh đầy đủ
- Việt Nam → **chưa có giải pháp tự chủ**

**Vấn đề căn bản của mọi giải pháp hiện tại — kể cả Linux hardened:**

```
Linux (dù hardened đến đâu):
  - ~5 triệu dòng code C trong kernel → hàng trăm CVE/năm
  - Nếu kernel exploit thành công → TẤT CẢ process bị expose
  - SELinux/AppArmor: policy phức tạp, lỗi cấu hình = rủi ro
  - Container: vẫn share kernel → kernel exploit = game over

Windows:
  - Closed source (US) → không thể audit backdoor
  - Lịch sử: PRISM (NSA mass surveillance, Snowden 2013)
  - Không phù hợp cho tài liệu mật cấp cao
```

**Mô hình triển khai Cellos cho chính phủ/quân đội:**

Điểm mấu chốt: **không cần viết lại toàn bộ phần mềm hiện có**. Cellos hỗ trợ hai lớp song song:

```
┌─────────────────────────────────────────────────────────────┐
│                    Cellos trên Cellos Chip                   │
│                                                             │
│  [Linux VM Cell]          [Native Rust Cells]               │
│  Chạy toàn bộ              ┌──────────────────┐             │
│  phần mềm Linux            │ Crypto Key Cell  │← Xcell tag  │
│  hiện có:                  │ (không ai đọc    │  hardware   │
│  - Office, email           │  được ngoài cell)│             │
│  - Trình duyệt             ├──────────────────┤             │
│  - Ứng dụng nghiệp vụ      │ Auth Cell        │             │
│  - Database                │ (xác thực định   │             │
│                            │  danh người dùng)│             │
│  Cô lập bởi               ├──────────────────┤             │
│  Stage-2 MMU              │ Audit Log Cell   │             │
│  (hypervisor)             │ (append-only,    │             │
│                            │  không ai xóa)   │             │
│                            └──────────────────┘             │
│                                                             │
│              Cellos Kernel (minimal TCB ~10K LOC)           │
└─────────────────────────────────────────────────────────────┘
```

**Tại sao mô hình này khác biệt căn bản:**

Với Linux/Windows hiện tại: nếu kernel bị exploit → attacker đọc được RAM của **tất cả** tiến trình, kể cả khóa mật mã, dữ liệu phân loại, session đang mở.

Với Cellos + Xcell hardware tags: ngay cả khi kernel exploit thành công, hardware tags **vẫn từ chối** mọi memory access trái phép ở tầng vật lý — bởi vì check xảy ra tại memory bus, không qua kernel code. Đây là điều không thể đạt được với bất kỳ OS nào trên chip mainstream hiện nay.

**Multi-Level Security (MLS) tự nhiên:**

Hệ thống phân loại thông tin (Mật / Tối Mật / Tuyệt Mật) có thể map trực tiếp vào Cell isolation:

| Cấp độ bảo mật | Hiện tại | Cellos + Xcell |
|----------------|----------|----------------|
| Tuyệt Mật | Máy vật lý riêng biệt (air-gap) | Native Rust Cell, Xcell-tagged, không kết nối mạng (capability-gated) |
| Tối Mật | Máy riêng hoặc VLAN nghiêm ngặt | Rust Cell với network cap giới hạn, Xcell boundary |
| Mật | SELinux label trên Linux | Linux VM Cell, policy-gated IPC |
| Nội bộ | Linux thường | Linux VM Cell, unrestricted |

Trên một máy vật lý duy nhất, **phần cứng đảm bảo** thông tin Tuyệt Mật trong một Cell không bao giờ rò rỉ sang Cell Mật hay Nội bộ — kể cả khi user bị phishing, malware, hay insider threat.

**Insider threat — mối lo ngại hàng đầu của quân đội:**

Kịch bản: cán bộ được cấp quyền truy cập hệ thống, nhưng cố tình copy tài liệu mật ra ngoài.

- **Linux hiện tại:** Root access = đọc được toàn bộ RAM. Sudo = bypass hầu hết controls.
- **Cellos + Xcell:** Tài liệu mật trong Cell A. Người dùng trong Cell B. Ngay cả với root/sudo trong Cell B, hardware tags ngăn đọc Cell A memory. Để exfiltrate, phải qua IPC capability-gated channel — **log được toàn bộ**.

**Chủ quyền công nghệ — lợi thế không thể mua:**

```
Hệ thống hiện tại:
  Phần mềm: Windows (Microsoft, Mỹ)
           hoặc Linux (US-originated)
  Phần cứng: Intel/AMD (Mỹ) hoặc ARM (Anh/Nhật)
  
  Rủi ro: Supply chain attack, hardware backdoor,
           export control (ITAR/EAR) cắt nguồn cung

Cellos Chip + Cellos OS:
  Phần mềm: Cellos (Việt Nam, open source, auditable)
  Phần cứng: RISC-V custom (Việt Nam, IP owned)
  
  Không dependency vào bất kỳ chính phủ nước ngoài nào
  Không royalty, không export license
  Có thể audit toàn bộ từ gate-level đến OS
```

**Competitor analysis cho thị trường này:**

Không có quốc gia nào hiện tại có bộ đôi **OS + chip tự sản xuất** với kiến trúc novel (không phải Linux fork trên chip nước ngoài). Đây là khoảng trống thực sự ở cấp quốc gia.

- Nga: Astra Linux (Linux fork) trên x86/ARM nước ngoài → không tự chủ phần cứng
- Trung Quốc: UOS trên Loongson → chip tự làm nhưng là x86 clone, không có novel isolation
- Hàn Quốc: không có OS tự chủ
- Nhật Bản: không có OS tự chủ cho quân sự

---

## 5. PHÂN TÍCH ĐỐI THỦ CẠNH TRANH

*(Dựa trên nghiên cứu thị trường thực tế, cross-verified từ 27 nguồn, tháng 6/2026)*

### 5.1 CHERIoT / SCI Semiconductor ICENI

**Tổng quan:** CHERIoT là extension RISC-V của Microsoft/Arm cho IoT bảo mật. ISA frozen tháng 12/2024, spec 1.0 ra tháng 11/2025. SCI Semiconductor ICENI là chip thương mại đầu tiên, đang scale mass production 2026.

**Điểm mạnh của CHERIoT:**
- Ngăn buffer overflow / use-after-free ở cấp độ con trỏ
- Backing từ Microsoft và Arm
- Đầu tiên ra thị trường (Q1 2026)

**Điểm yếu căn bản:**
- **Chỉ 32-bit** — chính nhóm phát triển xác nhận: "would be very hard to scale to big out-of-order cores" → không bao giờ lên được robotics hay server 64-bit
- **Pointer-level capabilities ≠ Cell-level isolation** — CHERIoT ngăn lỗi lập trình, không cung cấp N-way domain isolation
- **+57% core area overhead** (33 kGE trên Ibex core) — marketing nói "3%" là đo toàn SoC, không phải core
- **C-language focused** — không tận dụng Rust type system
- Không có hardware RT deadline registers
- Không có per-Cell performance counters

**Kết luận:** CHERIoT là đối thủ trong **embedded 32-bit IoT** — không phải ở 64-bit robotics, server hay PC. Không chồng lấp thị trường mục tiêu chính của Cellos Chip.

### 5.2 SiFive WorldGuard (2nd Gen Intelligence Series)

**Tổng quan:** SiFive's X160/X280/X390 AI cores hỗ trợ WorldGuard với **tối đa 4 worlds**. Available for licensing hiện tại.

**So sánh với Cellos Chip:**
- 4 worlds so với **N-way** (hàng chục tới hàng trăm Cells)
- Không có hardware ownership transfer (Xgrant)
- Không có per-Cell hardware counters (Xprobe)
- Không có hardware deadline registers (Xrt)
- Không có per-cell crypto binding

**Kết luận:** SiFive WorldGuard là closest production competitor — nhưng 4 worlds vẫn quá ít cho systems phức tạp, và thiếu các primitives cho zero-copy IPC và never-die.

### 5.3 RISC-V Worlds / SmMTT (tiêu chuẩn đang phát triển)

**Tổng quan:** Các đề xuất tiêu chuẩn hóa N-way isolation cho RISC-V:
- RISC-V Worlds: hỗ trợ tới 128 WIDs (World IDs)
- SmMTT: hỗ trợ tới 64 domains

**Thực trạng (verified từ paper tháng 2/2026, RISC-V Summit Europe):**
- **Chưa được ratify** — SmMTT target ratification tháng 12/2026
- **Chưa có commercial silicon** — chỉ có FPGA validation
- **Interoperability, scalability, RT suitability "remain insufficiently understood"** (trích dẫn trực tiếp)
- **IOPMP gap nghiêm trọng:** IOPMP chỉ check target-side memory — initiator-side DMA tagging được để là "implementation-specific" (mỗi vendor tự implement khác nhau)
- Silicon thực tế: sớm nhất **2027–2028**
- Automotive RISC-V full releases: **Q2/2028–Q2/2029**

**Kết luận:** Đây là cạnh tranh tiềm năng nhưng **2–3 năm sau**. Cửa sổ cơ hội của Cellos Chip là 2026–2028.

### 5.4 Intel TDX / AMD SEV-SNP / ARM CCA

**Tổng quan:** Confidential Computing cho server/cloud — isolation ở VM granularity.

**Tại sao không phải đối thủ trực tiếp:**
- Granularity: toàn bộ VM (không phải per-Cell/per-process)
- Target: cloud multi-tenancy (bảo vệ tenant từ cloud provider)
- Không áp dụng cho embedded/robotics
- Context switch overhead: ~10µs (so với ~10ns của Cellos)

**RISC-V CoVE:** Draft v0.7 RC2 (tháng 8/2024), chưa ratified, không có shipping hardware. VM-granularity only.

### 5.5 Bảng so sánh tổng hợp

| Tiêu chí | TrustZone-M | CHERIoT ICENI | SiFive WorldGuard | Cellos Chip |
|----------|-------------|---------------|-------------------|-------------|
| **Số domain cô lập** | 2 | N/A (pointer) | **4** | **N (không giới hạn)** |
| **Kiến trúc** | 32/64-bit | **32-bit only** | 64-bit | 64-bit |
| **Ngôn ngữ tối ưu** | C/ASM | C | C/Rust | **Rust-native** |
| **Context switch** | ~500 cycles | ~50 cycles | ~50 cycles | **~10ns** |
| **Memory overhead** | ~0% | ~1% SoC level | Thấp | **~3%** |
| **IPC zero-copy** | Không | Không | Không | **Có (Xgrant)** |
| **RT deadline HW** | Không | Không | Không | **Có (Xrt)** |
| **Per-component PMU** | Không | Không | Không | **Có (Xprobe)** |
| **DMA initiator tagging** | Không | N/A | Không | **Có** |
| **Production ready** | Có | Có (2026) | Có (IP) | **2027–2028** |
| **Giá** | Royalty ARM | SCI pricing | SiFive licensing | **IP owned** |

---

## 6. CỬA SỔ CƠ HỘI

```
2024    2025    2026    2027    2028    2029    2030
  │       │       │       │       │       │       │
  │       │   CHERIoT ICENI mass prod ←   │       │
  │       │       │       │       │       │       │
  │       │  SiFive WorldGuard (4 worlds) │       │
  │       │       │       │       │       │       │
  │       │       │ SmMTT ratification ↑  │       │
  │       │       │       │  Silicon ↑    │       │
  │       │       │       │       │  Automotive   │
  │       │       │       │       │  RISC-V ready │
  │       │       │       │       │       │       │
  │       │   ╔═══════════════════════╗   │       │
  │       │   ║  CỬA SỔ CELLOS CHIP  ║   │       │
  │       │   ║  N-way isolation,    ║   │       │
  │       │   ║  không đối thủ       ║   │       │
  │       │   ╚═══════════════════════╝   │       │
```

**Sau 2028:** SmMTT silicon xuất hiện → cạnh tranh tăng. Nhưng Cellos Chip có 2 lợi thế bền vững:
1. **Software moat:** Cellos OS là hệ sinh thái, không chỉ là chip
2. **Complete stack:** Chip + OS + toolchain + ecosystem = barrier to entry cao

---

## 7. LỢI THẾ BỀN VỮNG CỦA BỘ ĐÔI CHIP + CELLOS

### 7.1 Tại sao chip đơn thuần không đủ

Nếu chỉ làm chip RISC-V thêm extensions mà không có OS tương ứng, đó là **commodity silicon** — giống ESP32-C3 ở $1/chip. Giá trị nằm ở **complete stack**.

### 7.2 Tại sao OS đơn thuần có hạn chế

Cellos OS hiện tại dùng Rust type system (LBI) làm cơ chế cô lập. Nếu kernel bị tấn công (kernel exploit), LBI bị phá vỡ hoàn toàn. Hardware Xcell tags thì không — chúng được check tại memory bus, không qua kernel code.

**Kết hợp lại:**

```
Cellos OS + Cellos Chip:
  
  Threat 1: Bug trong app Cell
    → Rust type system ngăn lan rộng  (OS layer)
    → Xcell tags ngăn memory access   (hardware layer)
    → Double barrier
    
  Threat 2: Kernel exploit
    → Xcell tags vẫn hoạt động       (hardware, không qua kernel)
    → Kẻ tấn công vẫn bị chặn
    → Điều này KHÔNG CÓ ở bất kỳ OS nào khác
    
  Threat 3: Hung/crashed Cell
    → Xprobe detect microsecond      (hardware event)
    → Supervisor restart Cell         (Cellos never-die)
    → User không nhận thấy downtime
```

### 7.3 IP ownership — lợi thế dài hạn

Các đối thủ phụ thuộc vào ARM royalty (TrustZone) hoặc SiFive licensing (WorldGuard). Cellos Chip với RISC-V open ISA + custom extensions = **IP hoàn toàn sở hữu** — không royalty, không licensing fee, không bị ARM/Intel kiểm soát roadmap.

---

## 8. LỘ TRÌNH KỸ THUẬT VÀ TÀI CHÍNH

### Giai đoạn 0: FPGA Prototype (3–6 tháng)

**Mục tiêu:** Validate các tập lệnh mở rộng trước khi invest vào tapeout.

**Hoạt động:**
- Port Cellos kernel lên CVA6 / VexRiscv RISC-V core (open-source)
- Thêm Xcell extension vào RTL (synthesizable Verilog/SpinalHDL)
- Thêm Xgrant extension
- Deploy lên FPGA Xilinx UltraScale+ hoặc Intel Cyclone V
- Chạy Cellos benchmark: IPC throughput, Cell isolation violation tests, RT latency
- **Go/No-go gate:** Nếu không chứng minh được 10x improvement vs software-only → revisit design

**Ngân sách ước tính:**
- FPGA board (Xilinx ZCU102 hoặc tương đương): ~$3,000–5,000
- Engineering time: 2–3 kỹ sư × 6 tháng
- Tổng Giai đoạn 0: **~$100,000–150,000** (chủ yếu nhân lực)

### Giai đoạn 1: Open PDK Tapeout (6–12 tháng)

**Mục tiêu:** Silicon thực sự, chứng minh khả năng sản xuất.

**Hoạt động:**
- Skywater 130nm Open PDK (Google-sponsored MPW shuttle program)
- Cost: **$0** cho first shuttle (Google/Efabless free program)
- Tiny Tapeout cho proof-of-concept nhỏ: ~$300/tile
- Timeline từ tape-out đến silicon: 6–12 tháng
- Mục tiêu: Validate Xcell + Xgrant trên real silicon, không nhất thiết high-speed

**Ngân sách ước tính:**
- Tapeout cost: ~$5,000–20,000 (Tiny Tapeout hoặc small MPW run)
- Engineering time: 3–4 kỹ sư × 12 tháng
- EDA tools (OpenROAD open-source flow): $0
- Tổng Giai đoạn 1: **~$300,000–500,000**

### Giai đoạn 2: Commercial ASIC (18–24 tháng sau Giai đoạn 1)

**Mục tiêu:** Production-grade silicon cho commercial deployment.

**Hoạt động:**
- GlobalFoundries 22FDX hoặc TSMC 28nm
- Full pipeline: Xcell + Xgrant + Xcap + Xprobe + Xrt + Xcrypto-cell
- Target: embedded/robot G1 use case (ARM64 compatible core + custom extensions)
- Volume: 1,000–10,000 units cho early adopter program

**Ngân sách ước tính:**
- NRE (Non-Recurring Engineering) cho commercial fab: $500K–2M
- Engineering team (5–8 người): ~$1M/năm
- Testing, packaging, validation: $200K–500K
- Tổng Giai đoạn 2: **$2M–4M**

### Tổng đầu tư 3 giai đoạn

| Giai đoạn | Thời gian | Ngân sách | Milestone |
|-----------|-----------|-----------|-----------|
| **0: FPGA Prototype** | 6 tháng | $150K | Go/No-go validation |
| **1: Open PDK** | 12 tháng | $500K | First real silicon |
| **2: Commercial ASIC** | 24 tháng | $3–4M | Production-grade chip |
| **Tổng** | ~3.5 năm | **$3.5–4.5M** | Commercial product |

---

## 9. RỦI RO VÀ BIỆN PHÁP GIẢM THIỂU

| Rủi ro | Mức độ | Biện pháp |
|--------|--------|-----------|
| **SmMTT ratify sớm hơn dự kiến** (trước 2027) | Trung bình | Cellos Chip vẫn có software moat + complete stack. Chuyển focus sang differentiation ở toolchain/ecosystem |
| **Chi phí NRE vượt dự toán** | Cao | Gate tại Giai đoạn 0 và 1. Không commit Giai đoạn 2 trước khi có revenue từ Giai đoạn 1 |
| **Ecosystem lock-in** | Thấp | RISC-V open ISA = không bị ARM/Intel lock. Extensions là custom nhưng documented và publishable |
| **SCI Semiconductor ICENI chiếm thị trường IoT** | Thấp | Không overlap: ICENI là 32-bit IoT, Cellos Chip target 64-bit robotics/industrial |
| **CHERIoT scale lên 64-bit** | Thấp | Chính team CHERIoT xác nhận: "very hard to scale to big out-of-order cores" |
| **Procurement chính phủ chậm** | Cao | Không depend vào doanh thu chính phủ ở Giai đoạn 0–1. Build commercial product trước, chính phủ là bonus không phải điều kiện |
| **Thiếu nhân lực chip design trong nước** | Cao | Partner với ĐHBK Hà Nội/HCM + design house ở Đài Loan (TSMC ecosystem). RISC-V open-source toolchain giảm barrier to entry |

---

## 10. ÁP DỤNG THỰC TẾ: USE CASES CỤ THỂ

### Use Case 1: Robot Công Nghiệp

**Scenario:** Robot hàn tự động trong nhà máy, kết nối WiFi để cập nhật firmware, có cảm biến lực/hình ảnh, điều khiển 6 trục servo.

**Với ARM TrustZone (hiện tại):**
- WiFi stack và motor control trong cùng Non-Secure world
- Hack WiFi → có thể điều khiển motor
- 1 lỗi software crash motor control toàn bộ robot

**Với Cellos Chip:**
- WiFi Cell, Motor Control Cell, Sensor Cell, OTA Cell — hoàn toàn cô lập hardware
- Hack WiFi Cell: Xcell trap bất kỳ attempt access Motor Cell memory
- Motor Control Cell bị crash: Xprobe detect microsecond → Supervisor restart → Robot tiếp tục hoạt động
- Xrt deadline register: Motor Control không respond trong 2ms → hardware escalate → safety shutdown

### Use Case 2: Camera AI Kiểm Tra Chất Lượng

**Scenario:** Camera line kiểm tra lỗi trong nhà máy điện tử, AI model trị giá hàng triệu USD phát triển qua nhiều năm.

**Với Linux + GPU hiện tại:**
- Hack qua network → đọc AI model weights → copy → competitor clone sản phẩm

**Với Cellos Chip:**
- AI Inference Cell: model weights trong Xcell-tagged pages, chỉ Cell này đọc được
- Network Cell: chỉ nhận inference results, không thể đọc weights
- Firmware update: hardware-measured, signed, chỉ update được qua capability-gated OTA Cell

### Use Case 3: Máy Trạm An Ninh Chính Phủ

**Scenario:** Cán bộ cấp cao làm việc với tài liệu có nhiều cấp mật khác nhau trên cùng một máy trạm. Yêu cầu: không thể copy-paste giữa tài liệu Tuyệt Mật và email thông thường, không thể exfiltrate qua USB/network, có audit log đầy đủ.

**Với Windows/Linux hiện tại:**
- Phải dùng nhiều máy vật lý riêng biệt (air-gap) — tốn kém, bất tiện
- Hoặc dùng SELinux (phức tạp, lỗi policy = lỗ hổng, không có hardware backing)
- Root exploit hoặc kernel exploit = toàn bộ dữ liệu bị expose

**Với Cellos Chip:**
```
Cell: Linux VM (email, office)          ← không có cap đọc Cell bên dưới
Cell: Tài liệu Mật (Linux VM riêng)    ← isolated hoàn toàn
Cell: Tài liệu Tuyệt Mật (Rust native) ← Xcell-tagged, không network cap
Cell: Crypto & PKI                      ← key không bao giờ rời Cell
Cell: Audit Logger                      ← append-only, không ai xóa được
```
- Clipboard cross-Cell: chỉ qua capability-gated IPC → **logged và controlled**
- USB access: chỉ qua USB driver Cell với cap kiểm soát → có thể block hoàn toàn
- Kernel exploit: hardware Xcell tags vẫn ngăn cross-Cell memory access
- **Một máy vật lý = nhiều mức độ bảo mật, hardware enforced**

**Kịch bản insider threat:**
Attacker có quyền root trong Linux VM Cell → thử đọc RAM của Tài liệu Tuyệt Mật Cell → Xcell hardware trap → bị chặn tại memory bus → alert tới Audit Logger Cell.

### Use Case 4: Máy Chủ Tình Báo / Command & Control

**Scenario:** Hệ thống máy chủ xử lý thông tin từ nhiều nguồn khác nhau (SIGINT, HUMINT, OSINT) với yêu cầu compartmentalization — nhân viên phân tích SIGINT không được biết nguồn HUMINT và ngược lại.

**Với Linux + containers hiện tại:**
- Container isolation: shared kernel → một kernel CVE = toàn bộ compartment bị expose
- VMs: hypervisor bug = game over. VMware/KVM có US origin — supply chain risk
- Kubernetes: massive attack surface, phức tạp, nhiều CVE/năm

**Với Cellos Chip:**
```
Cell: SIGINT Analysis (Rust native)  ─┐
Cell: HUMINT Processing (Rust native) ─┼─ Xcell hardware boundaries
Cell: OSINT Aggregation (Linux VM)   ─┘  Không Cell nào đọc Cell khác
Cell: Fusion Engine (Rust native)        Nhận data qua Xgrant (hardware-verified ownership)
Cell: Network Gateway                    Capability-gated: chỉ output được approve
```

- Xprobe per-Cell counters: phát hiện bất thường (ví dụ: Cell đột nhiên tăng IPC với Cell không được phép liên lạc)
- Xrt deadlines: đảm bảo real-time processing không bị stall
- Failure recovery microsecond: một Cell crash không ảnh hưởng các Cell khác

**Lợi thế chủ quyền:** Toàn bộ stack — từ gate-level silicon đến OS kernel đến application — có thể được audit bởi đội ngũ trong nước. Không phụ thuộc vào vendor nước ngoài cho security updates.

### Use Case 5: Gateway IoT Fleet

**Scenario:** 10,000 thiết bị IoT trong một nhà máy, mỗi thiết bị cần unique identity, không thể clone, phải support post-quantum crypto.

**Với Cellos Chip + PUF:**
- Mỗi chip có PUF-derived unique key, không bao giờ rời silicon
- Ed25519 / ML-DSA hardware acceleration → signing trong microseconds
- Xcell-bound key storage: ngay cả OS compromise không extract được key

---

## 11. VÌ SAO ĐÂY LÀ CƠ HỘI CHO VIỆT NAM

### 11.1 RISC-V là cơ hội bình đẳng

ARM và x86 có 30 năm IP tích lũy, patent fortress, và hàng nghìn tỷ USD đầu tư. **RISC-V là lần đầu tiên** một kiến trúc open ISA cho phép bất kỳ tổ chức nào — kể cả từ Việt Nam — thiết kế chip mà không cần license từ ARM hay Intel.

### 11.2 Khoảng trống thị trường là thực

Như research xác nhận: **không có chip nào trên thị trường** (tính đến 6/2026) cung cấp N-way hardware Cell isolation + DMA initiator-side tagging + zero-copy ownership transfer cho 64-bit RISC-V. Đây không phải khoảng trống nhỏ — đây là khoảng trống mà **cả RISC-V International đang nỗ lực chuẩn hóa** (SmMTT, RISC-V Worlds), nhưng chưa xong.

### 11.3 Cellos đã có foundation

Cellos OS không phải bắt đầu từ đầu:
- Kernel SAS/LBI đã hoạt động
- Driver Cells (VirtIO, NIC, MMC, GPIO, UART) đã implement
- Cell signing, capability system đã có
- Test suite 30+ integration tests

Chip tùy chỉnh là **lớp tiếp theo** xây trên foundation đã có, không phải bắt đầu lại.

### 11.4 Chủ quyền công nghệ — lợi thế quốc gia không thể mua

Đây là điểm quan trọng nhất với người ra quyết định ở cấp nhà nước:

**Không quốc gia nào hiện tại có bộ đôi OS + chip tự chủ với kiến trúc novel:**

| Quốc gia | OS tự chủ | Chip tự chủ | Kiến trúc novel |
|----------|-----------|-------------|-----------------|
| Nga | Astra Linux (Linux fork) | Không — dùng x86/ARM | Không |
| Trung Quốc | UOS/Kylin (Linux fork) | Loongson (x86 clone) | Không |
| Hàn Quốc | Không | Không | Không |
| Nhật Bản | Không có cho quân sự | Không | Không |
| **Việt Nam + Cellos** | **Có (Cellos)** | **Có (Cellos Chip)** | **Có (SAS/LBI)** |

Cellos Chip KHÔNG phải Linux fork trên chip nước ngoài — đó là một kiến trúc hoàn toàn mới được thiết kế từ đầu. Đây là lợi thế **không thể mua từ nước ngoài**, phải tự build.

**Về rủi ro ITAR/EAR:** Luật kiểm soát xuất khẩu của Mỹ (ITAR/EAR) có thể hạn chế Việt Nam tiếp cận chip tiên tiến trong các kịch bản địa chính trị xấu. Chip RISC-V tự thiết kế + Skywater/GlobalFoundries (không bị ITAR) = **không thể bị cắt nguồn cung bởi quyết định chính trị của nước ngoài**.

### 11.5 Định vị chiến lược

> **"Cellos là hệ điều hành đầu tiên trên thế giới được thiết kế đồng bộ từ OS đến silicon, tối ưu cho Rust và N-way isolation trong single address space — và là bộ đôi chip+OS tự chủ duy nhất với kiến trúc novel, không phải fork từ nước ngoài"**

Đây là positioning không ai có thể copy nhanh — vì để copy, họ phải đồng thời có OS + chip + toolchain + ecosystem.

---

## 12. KẾT LUẬN VÀ KIẾN NGHỊ

### Kết luận từ phân tích

1. **Khoảng trống thị trường là thực và được xác minh:** Không có chip production nào cung cấp N-way hardware isolation cho 64-bit RISC-V tính đến 6/2026. Các tiêu chuẩn RISC-V liên quan chưa ratified, silicon sớm nhất 2027–2028.

2. **Cửa sổ cơ hội 2–3 năm:** Trước khi SmMTT/RISC-V Worlds ra silicon. Cellos Chip cần tapeout Giai đoạn 1 trong 2026–2027 để có vị thế.

3. **CHERIoT không phải mối đe dọa trực tiếp:** 32-bit only, pointer-level (không phải Cell-level), không scale lên robotics/server. Thị trường khác nhau.

4. **SiFive WorldGuard là closest competitor nhưng có gap lớn:** Chỉ 4 worlds, thiếu Xgrant/Xprobe/Xrt.

5. **Regulatory tailwind thực sự:** IEC 62443, ISO 26262, FDA cybersecurity guidance đều đang push outcome-based isolation requirements. Software-only có thể comply, nhưng hardware backing làm compliance dễ chứng minh hơn và tạo differentiation.

6. **Thị trường chính phủ/quân đội là thị trường chiến lược đặc biệt:** Cellos + Chip là bộ đôi **OS + chip tự chủ duy nhất** không phải Linux fork trên chip nước ngoài. Không quốc gia nào có điều này. Đây là lợi thế chủ quyền không thể mua bằng tiền — phải build. Với chính phủ Việt Nam, đây đồng thời là sản phẩm chiến lược quốc gia, có thể nhận hỗ trợ từ ngân sách an ninh quốc gia.

7. **ROI path:** Giai đoạn 0–1 ($650K) là low-risk validation. Chỉ invest Giai đoạn 2 ($3–4M) sau khi có evidence từ prototype và early adopter interest.

### Kiến nghị

**Xin phê duyệt ngân sách Giai đoạn 0: $150,000**

Giai đoạn 0 là FPGA prototype để:
- Validate kỹ thuật (Xcell + Xgrant trên CVA6 core)
- Benchmark vs software-only baseline
- Attract early adopter partner (robotics company, industrial automation)
- **Go/No-go decision** trước khi commit vốn lớn hơn

Rủi ro Giai đoạn 0 là giới hạn trong $150K — nếu technical validation fail, có thể dừng mà không mất vốn lớn. Nếu thành công, đây là foundation cho product roadmap 3–5 năm với potential exit trong thị trường chip bảo mật IoT/robotics đang tăng trưởng mạnh.

---

## PHỤ LỤC A: Nguồn Tham Khảo

1. CHERIoT 1.0 Specification — cheriot.org (November 3, 2025)
2. *"Area Comparison of CHERIoT and PMP in Ibex"* — Riedel et al., ETH Zurich/lowRISC, arXiv:2505.08541 (May 2025)
3. *"System-Level Isolation for Mixed-Criticality RISC-V SoCs: A 'World' Reality Check"* — arXiv:2602.05002v1 (February 2026, RISC-V Summit Europe 2026)
4. CoVE Specification v0.7 RC2 — riscv-non-isa/riscv-ap-tee, GitHub (August 2024)
5. SiFive 2nd Gen Intelligence RISC-V AI CPUs — CNX Software (September 2025)
6. Axelera Metis 214 TOPS — armdevices.net (March 2026)
7. Tenstorrent TT-QuietBox 2 — design-reuse.com (May 2025)
8. Nuclei NA900 ISO 26262 ASIL-D — design-reuse.com
9. SCI Semiconductor ICENI press release — scisemi.com
10. ABI Research: RISC-V Edge AI 129M shipments by 2030 — design-reuse.com

## PHỤ LỤC B: Thuật Ngữ Kỹ Thuật

| Thuật ngữ | Giải thích |
|-----------|-----------|
| **SAS** | Single Address Space — toàn bộ OS và apps chia sẻ một address space |
| **LBI** | Language-Based Isolation — cô lập bằng Rust type system thay MMU |
| **Cell** | Đơn vị isolation của Cellos, tương đương process nhưng trong SAS |
| **Xcell** | Tập lệnh mở rộng đề xuất: Cell-ID tagged memory |
| **Xgrant** | Tập lệnh mở rộng đề xuất: hardware ownership transfer |
| **TrustZone** | Công nghệ bảo mật của ARM: 2 worlds (Secure/Non-Secure) |
| **CHERIoT** | Capability Hardware Enhanced RISC Instructions for IoT — 32-bit |
| **SmMTT** | Supervisor Memory-mapped Transaction Table — RISC-V N-way isolation proposal (chưa ratified) |
| **IOPMP** | Input/Output Physical Memory Protection — RISC-V DMA isolation (có gap) |
| **CoVE** | Confidential VM Extension for RISC-V (chưa ratified, VM-level) |
| **PUF** | Physical Unclonable Function — hardware unique key generation |
| **NTT** | Number Theoretic Transform — core operation của post-quantum crypto |
| **ASIL-D** | Automotive Safety Integrity Level D — mức cao nhất trong ISO 26262 |
| **MLS** | Multi-Level Security — hệ thống xử lý thông tin ở nhiều cấp mật trên cùng phần cứng |
| **Tier 3b** | Mức triển khai Linux VM Cell trong Cellos — chạy Linux app không cần viết lại |
| **TCB** | Trusted Computing Base — phần code tối thiểu phải tin tưởng trong hệ thống |
| **ITAR/EAR** | Luật kiểm soát xuất khẩu vũ khí/công nghệ của Mỹ — có thể hạn chế mua chip nước ngoài |

---

*Đề xuất này được chuẩn bị bởi nhóm DXSL, dựa trên nghiên cứu thị trường cross-verified từ 27 nguồn học thuật và thương mại (tháng 6/2026). Các số liệu thị trường và timeline đối thủ được cập nhật theo thực trạng tính đến ngày viết.*
