# Cellos system context & design rules
**Last Updated**: 2026-11-16 (Phase 19 reconciliation: prime directive, §2 tree and Laws 1, 2, 4–8 checked against code)
**Audience**: Developers & AI Agents


## 🔴 PRIME DIRECTIVE
**Cellos uses Cellular SAS (Single Address Space) + Language-Based Isolation (LBI) for Tier 1 Cells**

- ❌ **NOT** traditional Linux/Unix process-based thinking
- ✅ **YES** Cellular architecture with bounded, kernel-copied IPC messages; zero-copy only through page-level Grants
- ✅ **YES** Rust type system for Tier 1 isolation; Tier 2 private roots and Tier 3 VMs add hardware isolation ([ADR-0015](../decisions/0015-dual-mode-hybrid-architecture.md), [Spec 18](18-cell-trust-tiers.md))

**Key Philosophy**: Software organized as **Cells** (not processes). Tier 1 Cells share one address space and are isolated by Rust's type system.

> **Reconciled 2026-11-16 (code check).** IPC is not zero-copy. The kernel copies each message into the receiver's recv buffer (`IPC_BUF_SIZE` = 4096, `libs/abi/src/kernel_ipc.rs:8`; [Spec 17](17-ipc-wire-contract.md)). The Tier-1 SPSC ring also copies message bytes ([Spec 20](20-unified-ipc-contract.md) §1). Zero-copy exists only for page-level Grants between SAS Cells (`libs/ostd/src/grant.rs`; Spec 17 §12.3).


## 1. Bản đồ tri thức (Source of Truth)
Trước khi code bất kỳ module nào, Agent **BẮT BUỘC** phải đọc file đặc tả tương ứng:
| Nếu nhiệm vụ liên quan đến... | Hãy đọc file này |
| --- | --- |
| Lấy code từ các dự án khác | [`27-fork.md`](27-fork.md) |
| Triết lý Cellular & Linker Linker | [`01-core.md`](01-core.md) |
| SAS layout, HHDM & focused ownership authorities | [`02-memory.md`](02-memory.md) |
| Async Safety & Owned Buffers Rule | [`03-runtime.md`](03-runtime.md) |
| Multi-Arch Trait (RV32/64/128) | [`04-hardware.md`](04-hardware.md) |
| Native vs Virtualization | [`05-application.md`](05-application.md) |
| Zero-copy Compositing & Input | [`06-graphics.md`](06-graphics.md) |
| User-space Stack (smoltcp) | [`07-networking.md`](07-networking.md) |
| Tickless Idle & Pointer Swizzling | [`08-power.md`](08-power.md) |
| Pluggable FS & Direct I/O | [`09-vfs.md`](09-vfs.md) |
| KUnit & Fault Injection | [`10-testing.md`](10-testing.md) |


## 2. Cấu trúc thư mục chuẩn

> **Cập nhật 2026-11-16 theo code.** Cây cũ đã lỗi thời do các thay đổi có chủ đích: `libs/api` được tách thành `libs/abi`, `libs/wire`, `libs/posix-shim`, `libs/kms-codec` rồi xoá (`2951404507`, `077909a812`); scheduler, SMP và hart-local chuyển từ `task/` sang `sched/` (`bee1c10dbb`); driver nhân chuyển sang `dev/` (`ce0540fdbe`); shell là Cellbox tại `cells/tools/cellbox` (`5043e2cf59`), `init` nằm ở `cells/tools/init`. Cây dưới đây chỉ liệt kê thư mục cấp cao; xem `ls` để biết đầy đủ.

```text
Cellos/
├── kernel/src/               # Kernel (cell registry, ELF loader, memory, IPC, syscalls)
│   ├── boot/                 # Khởi tạo sơ khai (Handover từ OpenSBI/Limine)
│   ├── cell/                 # Metadata, Registry, vòng đời Cell
│   ├── loader/               # ELF loader, relocation, launch profiles
│   ├── memory/               # Frame allocator, paging, layout SAS
│   ├── ipc/                  # IPC phía kernel (copy message, grant)
│   ├── sched/                # scheduler.rs, smp/, hart_local/
│   ├── task/                 # tcb.rs, stack.rs, syscall/, user_copy/, self-tests
│   ├── dev/                  # Driver phía kernel
│   └── admission/, acpi/, arch/, fs/, hypervisor/, platform/, sync/
├── hal/                      # Tầng trừu tượng phần cứng
│   ├── core/                 # Glue Code (re-export traits & arch)
│   ├── arch/                 # riscv/ (common, rv32, rv64), arm/ (aarch32, aarch64), x86/ (x86_32, x86_64)
│   ├── soc/                  # arm-virt, bcm27xx, riscv, x86
│   └── traits/               # adc, arch, can, display, gpio, hypervisor, i2c, mmc, mte, paging, pwm, spi, uart
├── cells/                    # Các Cell (mỗi Cell là một ELF riêng, ký bởi cellos-sign)
│   ├── apps/                 # Ứng dụng (desktop, files, ocel, terminal, vm-runner, ...)
│   ├── demos/                # Demo và smoke cell
│   ├── drivers/              # Driver cells (Tier 1; unsafe chỉ qua allowlist)
│   ├── guests/               # silo-guest
│   ├── runtimes/             # lua, mesa-anv
│   ├── services/             # Dịch vụ hệ thống (vfs, net, net-broker, compositor, input, kms, supervisor, ...)
│   ├── tests/                # Test cells cho QEMU lanes
│   └── tools/                # init, cellbox (shell), net-tools, sys-tools, kms-ssh-admin, wasm
├── libs/
│   ├── abi/                  # ABI đóng băng: syscall table, record #[repr(C)], manifest, fs/block traits phía kernel
│   ├── wire/                 # Giao thức dịch vụ (service protocols)
│   ├── types/                # Các kiểu dữ liệu cốt lõi (CellId, VAddr, Error...)
│   ├── ostd/                 # Thư viện chuẩn dành cho Cells (heap, IPC, grant, service loop)
│   └── ...                   # posix-shim, kms-codec, nic-driver, viui, ...
├── authority/, boards/, scripts/, docs/
└── tests/
```

## 3. The Coding Laws
**Luật 1: Interface là "Thánh chỉ"**
- Mọi thay đổi trong libs/abi hoặc libs/types phải XÁC NHẬN 2 LẦN với User.
- ~~Các Trait trong api phải dùng #[repr(C)] để bảo đảm Stable ABI.~~ (Cập nhật 2026-11-16: `libs/api` không còn — xem §2. Trait Rust không có `#[repr(C)]`; ABI ổn định là số syscall và các record `#[repr(C)]` trong `libs/abi` (ví dụ `libs/abi/src/syscall.rs`, `libs/abi/src/manifest.rs`). Giữa các Cell chỉ có byte của message: giao thức dịch vụ nằm trong `libs/wire`, theo [Spec 17](17-ipc-wire-contract.md).)

**Luật 2: An toàn bộ nhớ SAS**
- Owned Buffers ONLY: Cấm truyền &mut [u8] qua ranh giới Async (bên trong một Cell; xem [Spec 03](03-runtime.md)).
- ~~Sử dụng Box<[u8]> để chuyển quyền sở hữu giữa các Cell.~~ (Cập nhật 2026-11-16: mỗi Cell có heap arena riêng (`libs/ostd/src/heap.rs`), nên một `Box` không thể đổi chủ sang Cell khác. Dữ liệu nhỏ đi bằng message do kernel copy (≤ `IPC_BUF_SIZE`, `libs/abi/src/kernel_ipc.rs:8`); dữ liệu lớn đi bằng Grant (`libs/ostd/src/grant.rs`). `GrantShare` chỉ cho quyền truy cập, không chuyển quyền sở hữu.)

**Luật 3: Đa kiến trúc (Multi-Arch)**
- Code trong kernel và libs/ostd không được giả định 32 hay 64-bit.
- Sử dụng các kiểu dữ liệu từ libs/types và hal/core.

**Luật 4: Quản lý Unsafe**
- **Cells**: #![forbid(unsafe_code)], ngoại trừ các entry đã review trong `scripts/unsafe-allowlist.toml` (cổng: `cellos-sign --check`; xem §5).
- **Kernel/HAL**: Chỉ dùng unsafe khi tương tác trực tiếp phần cứng và phải có # Safety documentation.

**Luật 5: Module Style (Modern Rust)**
> **Historical (2026-11-16).** Luật cấm `mod.rs` không được enforce và code không theo: git đang track 41 file `mod.rs` ngoài `third_party/` và `.agents/` (ví dụ `kernel/src/arch/mod.rs`, `kernel/src/task/syscall/mod.rs`, `libs/ostd/src/service/mod.rs`), và không có CI lint nào kiểm tra. Code mới nên ưu tiên `foo.rs` + `foo/`, nhưng `mod.rs` không bị chặn.
- **TUYỆT ĐỐI CẤM** sử dụng `mod.rs`.
- Sử dụng cấu trúc hiện đại: `foo.rs` nằm ngang hàng với thư mục `foo/`.
- Tên file/thư mục phải là snake_case.

**Luật 6: Cellos Naming Convention**
- Mọi thành phần trong mã nguồn phải tuân thủ quy tắc đặt tên để phân biệt giữa **Hợp đồng Cellos (Contract)**, **Thực thi (Implementation)** và **Mã nguồn Fork**.
- **Chống đụng độ:** Không được đặt tên trùng với các thư viện fork (ví dụ: cấm đặt tên Trait là `FileSystem` nếu đã fork RedoxFS).
- ~~**Định danh SAS:** Các Trait bắt đầu bằng `Vi` phải hỗ trợ cơ chế chuyển giao sở hữu (Ownership) hoặc Lease/Grant để tối ưu hóa Single Address Space.~~ (**Historical (2026-11-16):** code không theo — nhiều trait `Vi` chỉ là giao diện thiết bị/UI không có ownership hay Lease/Grant, ví dụ `ViUart`, `ViGpio`, `ViSpi` (`hal/traits/`) và `ViTheme`, `ViWidget` (`libs/viui`). Tiền tố `Vi` hiện chỉ là quy ước đặt tên.)
- **Refactor hàng Fork:** Khi đưa mã nguồn ngoại lai vào, phải giữ nguyên logic gốc nhưng phải bọc (wrap) hoặc implement lại các Trait theo chuẩn `Vi` để Kernel có thể gọi.

| Đối tượng | Tiền tố / Quy tắc | Ví dụ |
| --- | --- | --- |
| **Public Trait (ABI)** | **Vi** + PascalCase | `ViFileSystem`, `ViFile`, `ViDriver` |
| **Core Types / Errors** | **Vi** + PascalCase | `ViResult`, `ViError`, `ViConfig` |
| **Hệ thống tập tin** | retired `viFS1` / `viFS2`; active boot FS `VIFS1` | `VIFS1` (kernel BootFS/initramfs) |
| **Địa chỉ (Multi-Arch)** | **VAddr / PAddr** | `VAddr`, `PAddr` (Bắt buộc dùng từ `libs/types`) |
| **Internal Modules** | **snake_case** | `task/tcb.rs`, `memory/paging.rs` |


**Luật 7: Quản lý Trait & Tính Linh Hoạt (Polymorphism)**
> **Cập nhật 2026-11-16 theo code.** Trait object chỉ dùng **bên trong một binary**: kernel giữ BootFS/FAT sau `Box<dyn ViFileSystem>` / `Box<dyn ViFile + Send + Sync>` (`kernel/src/fs.rs:18`, `libs/abi/src/fs.rs:40`). Không có trait object nào đi qua ranh giới Cell: mỗi Cell là một ELF riêng với heap riêng, và giao diện giữa các Cell là giao thức message trong `libs/wire` (Spec 17). Nạp/gỡ Cell động đi qua loader và service registry của kernel, không qua `dyn Trait`. Ví dụ `Arc<dyn ViDriver + Send + Sync>` là historical: `ViDriver` (`libs/wire/src/driver.rs:10`) không có implementor nào.
- **Trait Object**: Sử dụng `dyn Trait` thay cho Generics tại các interface hệ thống bên trong một binary (ví dụ VFS phía kernel). ~~để hỗ trợ nạp/gỡ Cell động~~
- **Bắt buộc Bounds an toàn**: Mọi trait object được lưu trữ hoặc chuyển giao giữa các Task phải chỉ định rõ Send + Sync (ví dụ: `Box<dyn ViFile + Send + Sync>`, `libs/abi/src/fs.rs:21`).
- **Box vs Arc**: Sử dụng `Box` cho các đối tượng có chủ sở hữu duy nhất và Arc cho các tài nguyên dùng chung bên trong một Cell hoặc trong kernel (không chia sẻ giữa các Cell).


**Luật 8: Quản lý tài nguyên & LBI (Memory Safety)**
- **Tối ưu hóa tham chiếu (Borrowing over Heap)**: Sử dụng tham chiếu (`&` / `&mut`) thay vì Ownership (`Box`) khi chỉ cần truy cập dữ liệu tạm thời để giảm áp lực cấp phát lên heap. *(Cập nhật 2026-11-16: không có "Global Heap" chung cho Cells; mỗi Cell có arena riêng, mặc định 1 MiB (`libs/ostd/src/heap.rs`), kernel có heap riêng.)*
- **Lifetime**: Mọi struct chứa tham chiếu phải chỉ định rõ `lifetime` để ngăn chặn dangling references trong Single Address Space.
- **Drop**: Bắt buộc triển khai `Drop` trait cho các cấu trúc quản lý tài nguyên để đảm bảo thu hồi tài nguyên ngay lập tức khi đối tượng ra khỏi phạm vi (scope). *(Cập nhật 2026-11-16: các `Drop` hiện có: `FileHandle` (`libs/abi/src/fs.rs:196`), `ostd::fs::File` và `ReadDir` (`libs/ostd/src/fs.rs:217`, `:38`), `GrantHandle` (`libs/ostd/src/grant.rs:223`). `Lease::drop` (`libs/abi/src/fs.rs:212`) vẫn rỗng — chưa có Lock Manager. `DriverContext` không tồn tại.)*


## 4. Agent Workflow
- **Check Spec**: Đọc file trong design/ để hiểu "Tại sao".
- **Interface First**: Định nghĩa Trait trong libs/wire (hợp đồng dịch vụ) hoặc libs/abi (ABI kernel) trước khi code phần thực thi.
- **Thực thi**: Code logic, chú ý xử lý Result thay vì panic! để hỗ trợ Panic Recovery.
- **Verification**: Viết test KUnit cho mọi logic quan trọng.


## 5. Trusted Computing Base (TCB)

> **Full spec**: [`docs/specs/16-rustc-tcb.md`](./16-rustc-tcb.md) — authoritative reference for TCB guarantees, limits, risk mitigations, and operational policies. This section is a summary.

Cellos dùng **Language-Based Isolation (LBI)** làm cơ chế bảo mật chính thay cho hardware MMU per-process. Điều này có một hệ quả quan trọng: **`rustc` là load-bearing TCB**.

### Tại sao `rustc` là TCB

- `#![forbid(unsafe_code)]` trên mọi Cell được enforce bởi `rustc` tại compile time — không phải hardware.
- Borrow checker ngăn một Cell giữ live reference vào vùng nhớ của Cell khác — đây là ranh giới cách ly chính.
- Nếu `rustc` bị compromise (compiler supply-chain attack hoặc soundness hole), toàn bộ Cell isolation guarantee sụp đổ.

**Prior art:** Microsoft Singularity/Midori xác định CLR/.NET runtime là load-bearing TCB của họ với cùng pattern. Cellos thay CLR bằng `rustc` — đạt cùng guarantee nhưng không cần GC. Xem `docs/research/research-singularity-midori.md`.

> *"One interesting aspect of relying on type safety was that your compiler becomes part of your TCB."*
> — Joe Duffy, Safe Native Code (2015)

### Tại sao LBI thay vì hardware MMU per-Cell (số đo)

Singularity (MSPC 2006) đo trực tiếp:

| Isolation mechanism | Overhead |
|---|---|
| SIP (Language-Based, Ring 0, no mode-switch) | **<5%** |
| Hardware isolation (added on top) | **25–33%** avg, **37.7%** trên WebFiles macro-benchmark |

Cellos sẽ không dùng per-Cell SATP vì lý do này. Các Cell chia sẻ address space, `rustc` enforce ranh giới tại compile time.

**IPC cost comparison:**

| System | IPC cost |
|---|---|
| Singularity channel (pointer hand-off) | ~1,200 cycles |
| Cellos typed message round trip | **48,5 µs p50 / 86,6 µs p99 trên QEMU TCG** (D1 measurement) |
| Linux syscall | ~80–150 cycles |

Direct dispatch là mục tiêu rewrite chỉ cho Tier 1, chưa phải đường chạy hiện hành và chưa có số
đo. Message path hiện tại tuân theo Spec 17; Grant API (syscalls 208–212) là analogue của exchange
heap cho large data (>page).

### Thành phần TCB (nhỏ nhất → lớn nhất)

| Component | Vai trò | Ước lượng |
|---|---|---|
| `rustc` (nightly Rust compiler) | Enforce LBI — forbid unsafe, ownership/borrow rules | ~3–5M LOC |
| Cellos kernel | SAS memory allocator, scheduler, IPC, ELF loader | [generated nLOC](../code-metrics.generated.md) |
| `libs/abi` + `libs/types` | Stable ABI giữa kernel và Cells | ~2K LOC |

### Không nằm trong TCB (by design)

- **Cell code** — `#![forbid(unsafe_code)]` nghĩa là kernel không trust Cell code; violation bị caught bởi rustc trước khi link.
- **C/C++ thư viện qua Tier 1 `ffi-posix` profile** — trusted code shares the caller's SAS; kernel validate tất cả grant/IPC boundaries.
- **Lua and Tier 3 VM guests** — Lua is a trusted Tier 1 runtime profile with interpreter restrictions; VM guests are sandboxed by hypervisor Stage-2 paging.

### Mitigation: giảm thiểu rủi ro rustc-as-TCB

Cellos tốt hơn Singularity/Midori ở điểm này vì ba lý do:

1. **rustc là open-source** — cộng đồng lớn, thường xuyên audit; Bartok (Singularity/Midori) là closed-source.
2. **Ferrocene** — ISO 26262 ASIL-D certified subset của rustc; đủ điều kiện cho automotive safety market.
   - ⚠️ **Qualification lanes**: RV64 is the reference/QEMU CI lane, not a qualified
     bare-metal Ferrocene target. `aarch64-unknown-none` is the first qualification
     candidate under Ferrocene's documented host/build conditions. Toolchain evidence
     does not certify the Cellos product; see Spec 16.
3. **miri** — interprets MIR bytecode, phát hiện unsoundness trong unsafe code. Chạy trong CI cho kernel unsafe paths.

### Policies từ LBI/TCB (phải follow)

| Policy | Rationale |
|---|---|
| **Không link GC runtime vào RT-critical Cells** | Midori's unsolved problem — GC pauses phá RT guarantees. Rust RAII = deterministic destruction là lợi thế cấu trúc quyết định. |
| **`static mut` trong kernel chỉ qua `Spinlock<Option<T>>`** | Mutable statics = "ambient authority" (F6, Duffy). Cells đã block qua `forbid(unsafe_code)`. Kernel là gap còn lại. |
| **`#![forbid(unsafe_code)]` trên mọi Cell, tuyệt đối ngoài allowlist đã review** | Compiler không thể verify safety nếu unsafe được phép — một unsafe line đủ để vô hiệu hóa LBI guarantee của cell đó. Driver cần MMIO/DMA nên allowlist tồn tại; mỗi entry **là một lỗ trên tường LBI**, có reason/approver/date và bị báo khi quá `review_by`. Nguồn sự thật: `scripts/unsafe-allowlist.toml`, cổng: `cellos-sign --check`. |
| **Upgrade `rustc` ngay khi CVE soundness hole** | Soundness hole trong borrow checker hoặc codegen = P0 breach — không có patch nào khác ngoài upgrade compiler. |

### Hệ quả với security review

Bất kỳ lỗ hổng nào trong `rustc`'s unsafe checker, borrow checker, hoặc codegen cho phép unsafe code trong crate `#![forbid(unsafe_code)]` là **P0 kernel security breach** với Cellos — cần upgrade compiler ngay lập tức.

**Khi review Cell code**, không cần audit memory safety (rustc đã làm). Tập trung vào:
- Logic bugs và incorrect IPC protocol usage
- Grant misuse (phải kiểm tra `!Copy + !Clone` invariant trên owner handle)
- Capability escalation qua IPC (Cell request syscall không có trong manifest)

### So sánh prior art (Singularity / Midori / Cellos)

| Dimension | Singularity | Midori | Cellos |
|---|---|---|---|
| **Isolation mechanism** | Sing# + MSIL verifier | M# type system | Rust ownership + borrow checker |
| **IPC cost** | ~1,200 cycles (channel) | Not published | Message path measured on QEMU; direct Tier-1 target unmeasured |
| **Large data transfer** | Exchange heap (linear types) | Isolated object graph | Grant API (page-level, runtime) |
| **MMU isolation** | Eliminated (Ring 0) | Eliminated | Eliminated (Cellular SAS) |
| **Mutable statics** | Allowed | Banned by language | Convention only (`Spinlock<Option<T>>`); no lint |
| **GC / memory** | GC (Bartok) | GC (CLR-derived); RT-unsolved | **RAII, no GC** |
| **Real-time capable** | No | No | **Yes** |
| **Compiler TCB** | Bartok (closed, unverified) | Bartok + M# (closed) | rustc (open-source, Ferrocene) |
| **Channel contracts** | Full FSM verification | Typed async RPC | Interface types only (YAGNI) |
| **Production shipped** | No | One workload (Bing speech) | In progress |

**Ghi chú về mutable statics (F6):** Midori ban `static mut` tại language level — Cellos chỉ enforce bằng convention. Cells đã block qua `#![forbid(unsafe_code)]`. Kernel unsafe code là gap còn lại. TODO: xem xét custom clippy lint cho kernel non-HAL paths.

**Bài học tổ chức (F8):** Midori bị cancel vì lý do organizational/political, không phải technical failure (performance đạt parity với C/C++). Regret lớn nhất của team: không open-source ngay từ đầu, không publish papers. Cellos nên duy trì public GitHub presence sớm.

*Sources: Hunt & Larus ACM 2007; Deconstructing Process Isolation MSPC 2006; Joe Duffy "Safe Native Code" (2015). Full analysis: `docs/research/research-singularity-midori.md`*
