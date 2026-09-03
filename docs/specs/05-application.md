# Cellos Architecture: Application Tiers
**Version**: 1.0 (canonical tier/runtime/SDK taxonomy)
**Status**: Definitive — updated 2026-08-26 (ADR 0003; Silo API and ADR-0006 production gate)

---

## 1. Application tier taxonomy

Cellos uses **tier** only for application execution and isolation classes.
Do not use `layer` for these classes; `layer` is reserved for architecture
stacks and hardware-isolation layers. Do not use numbered SDK tiers.

| Tier | Canonical name | Runtime profiles | Isolation | Status |
| :--- | :--- | :--- | :--- | :--- |
| **Tier 1** | Trusted SAS Cell | `rust-no-std`, future `rust-std`, `ffi-posix`, `lua` | Shared SAS + LBI; fleet posture depends on signing/admission | Shipped for current native Cells; future `rust-std` is G4 |
| **Tier 2** | Native Domain Cell | Same native Cell shape as Tier 1 | Private MMU domain, explicit mapped grants, copied/domain-aware IPC | Accepted design, **not implemented** |
| **Tier 3** | VM Guest | `linux-guest` | Hypervisor / Stage-2 | ARM64 guest path exists; broader platform work tracked separately |

Legacy aliases:

- **Tier 1b** now means a Tier 1 runtime profile, usually `ffi-posix`
  (C/C++/Zig through POSIX shim or mlibc) or `lua`.
- **Tier 3b** now means the Tier 3 `linux-guest` profile.

The SDK is one family of named modules: Foundation, Runtime profiles, Service
clients, UI/graphics, Middleware/helpers, Tooling, and Guest integration. The
SDK is not split into Tier 1/Tier 2/Tier 3 SDKs.

Code note: Manifest v2 currently has a field named `tier` for x86 PKU
protection-key selection. That is a **protection class**, not this application
tier taxonomy; compatibility aliases must be preserved if the code is renamed.

---

## 2. Tier 1: Trusted SAS Cells

Dành cho kernel, drivers, services, RT control — bất cứ thứ gì cần hiệu năng tuyệt đối hoặc quyền truy cập hardware.

- Rust `.o`, chạy trong SAS (Single Address Space)
- Isolation: Rust type system (Language-Based Isolation)
- Bắt buộc: `#![forbid(unsafe_code)]` cho Cells; `unsafe` chỉ trong kernel/HAL
- Không giới hạn file count — full Cargo crate với submodules
- Runtime profile mặc định hiện tại: `rust-no-std`; `rust-std` là G4 và phải đi
  qua pure-Rust PAL/custom target, không đi qua mlibc.

### 2.1 Hardware-Assisted Infrastructure Used by Tier 1 Services

Some trusted services may use a hypervisor mechanism without changing the
application-tier taxonomy. That mechanism is not automatically a public Tier 1
API or a production isolation claim.

#### Development Silo Provider

The former public `ostd::silo` interface has been removed. Applications cannot
initialize Silo keys, request generic signing or ECDH, select raw commands, or
connect directly to the Silo service.

The current Silo is an optional KMS-internal `DEV_REFERENCE` provider restricted
to the signed AArch64 virtualized-QEMU `test-hooks` lane. Its private protocol
implements only one-time development initialization and the exact TLS 1.3
client `CertificateVerify` purpose already governed by KMS. Only the live KMS
instance is authorized; readiness registration is exact-instance and
test-hooks-only.

The locked guest is size- and SHA-256-admitted before launch, and faults or
resets permanently fail the current instance closed. Stage-2 proves a useful
software containment path, but the Cellos EL2 host still constructs and loads
the guest and supplies its disposable development seed. This is therefore not
hardware custody, kernel-compromise-resistant storage, an HSM, or production
qualification.

Production is `BLOCKED_BY_ADR_0006`.
[ADR-0006](../decisions/0006-block-production-root-pending-exact-product-evidence.md)
closed Phase 6 NO-GO and selected no production root product. No exact product,
procurement path, OTP/provisioning plan, or board/AP integration is approved.
Phase 4 remains product-independent and blocked only on real protected
persistence, authenticated time, and a distinct reviewed pending-key binding
under the frozen KMS ABI. Phase 5 is `DEV_REFERENCE`; Phases 7–8 remain blocked.
Reopening requires one vendor-signed package binding all eight ADR-0006 criteria
to one proposed deployment; receipt permits review, not approval, and every item
must pass without inference before a superseding GO ADR selects the product.

This backend is infrastructure behind KMS, not an application tier or App SDK
module. See the [development Silo guide](../guides/tier1-silo.md) for the exact
boundary.

#### Hardware Isolation Layers

LBI remains load-bearing for trusted Tier-1 cells: `rustc` is part of the TCB, unsafe or
ambient-authority code can break the language wall, and shared microarchitectural state can
still expose timing or speculative side channels.

The hardware-isolation taxonomy and implementation status are owned by
[Spec 19](19-hardware-isolation-layers.md): Layer A W^X is implemented; Layer B per-domain
page tables are the future hardware boundary for untrusted native cells and optional
defense-in-depth for selected Tier-1 cells; Layer C mechanisms such as MTE or MPK are
hardware-gated bonuses and never load-bearing. RISC-V PMP is not available to the Cellos
S-mode runtime without a separate M-mode firmware owner (see Spec 12 §2).

MTE and MPK are not Spectre/Meltdown mitigations. Side-channel controls require their own
threat model, implementation, and verification.

---

## 3. Tier 1 `ffi-posix` profile (legacy alias: Tier 1b)

Dành cho **nhúng thư viện C/C++ vào Rust cell** — link trực tiếp vendor SDK, legacy firmware, hoặc thư viện C không có Rust equivalent mà không cần rewrite.

**Use case chính:**
- Vendor NPU SDK (RKNN, Hailo, K230 KPU) — không có Rust alternative
- Camera ISP library từ silicon vendor
- Validated/certified C codebase (DO-178, IEC 62443) — rewrite phá cert
- Legacy robot firmware C/C++ (10K+ LOC) — rewrite cost quá cao
- Complex C apps: DOOM, FFmpeg, SQLite, mbedTLS (usually require the mlibc POSIX profile)

### 3.1 Two-profile C library strategy (G2)

**Profile A — POSIX services shim** (default, embedded/simple cells, no build overhead):

| | Profile A: POSIX services shim | Profile B: mlibc |
|---|---|---|
| Binary size | Small | Larger (Grisu3, slab alloc) |
| printf float | Limited | Grisu3 (correct %.15g) |
| malloc | Bump arena | frg::slab_allocator |
| Build | Rust only | WSL2 Meson build first |
| Default | Yes | Opt-in via feature |

**CRITICAL mutual exclusion:** `api = { features = ["mlibc"] }` suppresses the POSIX services shim. Forget the feature while using mlibc-shim → duplicate-symbol link error. **Never link both.**

See `docs/mlibc-build.md` for the full mlibc build guide.

**Cách hoạt động:** Rust cell link statically với C library. Các lời gọi POSIX bên trong C code (`malloc`, `open`, `read`...) được resolve sang `Cellos-libc` (Newlib + POSIX shim) tại link time — chạy native trong SAS, 0ms overhead.

```
[Tier 1 ffi-posix link flow - mlibc profile:]
  cell.rs (Rust, owns the cell)
    └── api = { features = ["mlibc"] }  ← POSIX services shim suppressed
    └── mlibc-shim                      ← links third_party/mlibc/build/libc.a
    └── extern "C" { fn rknn_init(...); }   ← FFI bindings
         ↓ links statically
        librknn_api.a  (vendor SDK, C/C++)
         ↓ malloc/open/read → resolve to
        libc.a  (mlibc profile — sysdeps/Cellos → Cellos_syscall)
         ↓ → ViSyscall (VFS IPC, Net IPC, GetTime, GetRandom)
```

**Implementation hiện tại** (`libs/api/src/services/posix.rs`, feature flag `posix`):

| Nhóm | Functions | Status |
|---|---|---|
| Memory | `malloc/free/realloc/calloc` | ✅ Done (AllocHeader, 16-byte align) |
| Strings | `memcpy/memmove/memset/strlen/strcpy/strcmp` | ✅ Done |
| Files | `_open/_read/_write/_close/_lseek` → ViSyscall | ✅ Done |
| Time | `_time/_gettimeofday` → ViSyscall::GetTime | ✅ Done |
| Exit | `_exit` → ViSyscall::Exit | ✅ Done |
| Entropy | `getentropy/arc4random_buf` → ViSyscall::GetRandom | ✅ Done |
| Network | `connect/send/recv/close` → Net IPC | ✅ Done |
| Process | `_fork/_execve/_kill/_wait` | ❌ Returns -1 (SAS incompatible) |
| Memory map | `_sbrk` | ❌ Returns NULL (Rust allocator used) |

**Limitations (by design — không fix):**
- `fork()` = -1 — thư viện C không cần fork; app cần fork → Tier 3
- `mmap(MAP_ANONYMOUS)` = không support — Rust allocator quản lý heap
- Dynamic linking = không support — statically-linked only
- Signals/kill = không support — thư viện C hiếm khi dùng signals

**C libraries phù hợp:**
- ✅ RKNN SDK, Hailo SDK, K230 KPU (NPU inference)
- ✅ mbedTLS, wolfSSL (TLS, sau khi có entropy)
- ✅ SQLite (embedded database)
- ✅ libopus, libvpx (codec, không cần fork)
- ✅ Vendor sensor calibration/fusion libraries
- ❌ Libraries dùng `dlopen` (dynamic plugins)
- ❌ Libraries fork subprocess (libgit2 hooks, ffmpeg filters)

**Tier 1 ffi-posix vs Tier 3 linux-guest — khi nào dùng cái nào:**

| | Tier 1 ffi-posix: C library link | Tier 3 linux-guest: Linux VM |
|---|---|---|
| Overhead | 0ms — native SAS | 2-10s boot |
| Isolation | LBI (Rust type system) | Hardware Stage-2 MMU |
| fork/exec | ❌ By design | ✅ Full Linux |
| Phù hợp | Vendor SDK, validated C lib | Full Linux ecosystem, fork-heavy apps |
| Trust requirement | Must be trusted (cùng SAS với kernel) | Untrusted OK (hardware fence) |

---

## 4. Tier 3: Virtualization (Linux Ecosystem)

### 4.1 Tại sao cần Tier 3

Tier 1 native profiles tốt cho code tin cậy nhưng thiếu ecosystem. G2 target (server/PC) cần:
- nginx, PostgreSQL, Node.js, Python full, Java — không port được hết lên Cellos
- **Giải pháp**: Chạy Linux VM bên trong Cellos như 1 Tier 1 Hypervisor Cell

Analogy: WSL2 trên Windows — chạy Windows + Linux side-by-side, Linux disk/net nối vào Windows.

> **Note**: The development Silo described in §2.1 is infrastructure behind KMS, not Tier 1 or Tier 3 application API. Its AArch64 QEMU Stage-2 lane is `DEV_REFERENCE` software containment, not a hardware fence or production custody claim.

### 4.2 Tier 3 `linux-guest` profile (legacy alias: Tier 3b) [G2]

```
Mục đích: Chạy Linux ecosystem (`apk add nginx` trên Alpine hiện tại)
Guest: Linux kernel + userspace, khởi động bình thường
Interface: VirtIO devices (disk, net, console) → forward sang Cellos services
Boot time: 2-10 giây (Linux init)
```

Diagram:
```
Cellos (HS-mode)
├── Tier 1 cells (HU-mode) — vfs, net, shell, drivers, trusted runtime profiles
└── Hypervisor Cell (Tier 1, HS-mode capable)
    ├── Cellos_hv/ (minimal VMM, ~9K LOC Rust)
    │   sys_create_vm / sys_create_vcpu
    │   sys_map_guest_memory → Stage-2 setup
    │   sys_run_vcpu (blocking until VM exit)
    │   sys_vcpu_get/set_regs / sys_inject_irq
    └── VirtIO backends (MMIO bus, no PCI)
        virtio-blk  → sys_send(VFS_ENDPOINT, ...)
        virtio-net  → sys_send(NET_ENDPOINT, ...)
        virtio-console → serial output
        virtio-gpu  → sys_send(COMPOSITOR, ...) [G2+]

    └── Linux Guest (VS-mode, trong Stage-2 fence)
            apk add nginx; nginx; → works
```

### 4.3 VMM: Minimal VMM (custom, ~2.9K LOC shipped; ~9K planned at full device coverage)

**Hypervisor Cell là Tier 1 Rust cell bình thường** — cùng spawn/lifecycle/IPC/restart pattern với vfs/net/shell cells. Điểm khác duy nhất: có `HypervisorCap` capability token, được kernel dùng để gate hypervisor syscalls và switch HS-mode khi dispatch.

**Capability gating** (theo pattern hiện có tại `kernel/src/task/cap.rs` và `tcb.rs:148-153`):
```rust
// Follows same ZST token pattern as BlockIoCap, NetworkCap, SpawnCap
pub struct HypervisorCap;

// In Task struct:
hypervisor_cap: Option<HypervisorCap>,
// syscall_allowlist bitmap gates: sys_create_vm, sys_create_vcpu,
// sys_map_guest_memory, sys_run_vcpu, sys_vcpu_regs, sys_inject_irq
```

**Restart semantics:** Hypervisor Cell chết → NotifyOnExit (204) wakes init → init respawns cell → Linux guest boot lại. Linux RAM state lost (ephemeral), disk state survive qua VirtIO blk → VFS. Identical với cách init restart vfs/net/shell hôm nay.

**IPC pattern (VirtIO backend → Cellos cells):**
```
Linux guest MMIO write (disk I/O)
  → sys_run_vcpu() returns VmExit::MmioWrite
  → Hypervisor Cell: sys_send(VFS_ENDPOINT, read_req)   ← cell-to-cell IPC
  → VFS Cell processes → sys_send(HYPERVISOR_TID, resp)
  → Hypervisor Cell injects VirtIO completion into guest
```

**Multi-instance:** N Hypervisor Cells = N độc lập Linux VMs. Không có gì ngăn spawn nhiều instance — kernel treat chúng như N Tier 1 cells bình thường. Trong G2: thường 1 instance (Option A). Cho isolated workloads: N instances (Option B, Firecracker-style).

Cellos tự viết VMM tối giản thay vì fork crosvm (~75K LOC thực tế, kéo theo tokio + mmap dependencies).

**Thiết kế VMM:**
- Rust-native Tier 1 cell, không có tokio, không mmap, không libc
- Target: `microvm` profile — MMIO bus only, không PCI bus emulation
- VirtIO: `virtio-blk`, `virtio-net`, `virtio-console` over MMIO
- VirtIO backends forward về Cellos IPC (VFS Cell, Net Cell) — không cần implement storage/net stack riêng
- Stage-2 page table: dùng lại primitives từ `kernel/src/memory/`

**Tại sao không fork crosvm:**
- crosvm thực tế ~75K LOC (không phải ~20K như estimate ban đầu)
- Depends tokio (async runtime) + mmap — cả hai không fit SAS cell
- Upstream drift: crosvm thay đổi thường xuyên theo ChromeOS
- microvm profile không cần 90% features của crosvm (VFIO, USB, balloon, etc.)

**Tại sao không QEMU:** ~1M LOC C, cần JIT/mmap/fork — không fit Tier 1 cell.
**Tại sao không Firecracker:** thiếu GPU/display backend — chỉ cho serverless, không G2 desktop.

**Cấu trúc `cells/services/hypervisor/` (shipped ARM64, ~2.9K LOC cell+kernel; ~9K là mục tiêu khi phủ đủ device):**
```
src/
  run_loop.rs       — VmExit dispatch loop (MMIO/HVC/WFI/Preempted/Shutdown)
  vmm.rs            — create_vm / create_vcpu / map_guest / run_vcpu wrappers
  loader_image.rs   — ARM64 Image header parser + guest RAM placement
  dtb.rs            — FDT builder (10 nodes: RAM/CPU/PSCI/GIC/timer/chosen/UART + virtio×3)
  pl011.rs          — PL011 UART emulator
  gicd.rs           — GICv2 GICD shadow-register emulator
  psci.rs           — PSCI 1.0 handler (SYSTEM_OFF/CPU_ON/…)
  timer.rs          — armv8-timer virtual IRQ injection
  virtio_mmio.rs    — virtio-mmio transport (QueueNotify, feature negotiation)
  virtqueue.rs      — split virtqueue (avail/used ring, descriptor chain walk)
  virtio_console.rs — virtio-console (slot 0, SPI 16)
  virtio_blk.rs     — virtio-blk → VFS IPC (slot 1, SPI 17)
  virtio_net.rs     — virtio-net → Net IPC, MAC demux (slot 2, SPI 18)
  net_backend.rs    — L2Send/L2Recv IPC helpers to Net Cell
  vgic.rs           — GICH/GICV hardware vGIC (Phase 09)
  loader_image.rs   — guest image placement helper
```

### 4.3.1 Tier-3 Threat Model (guest-escape)

> **Scope:** cô lập giữa **Linux guest ↔ Cellos host** (kernel + cell khác). KHÔNG bàn bảo mật *bên trong* guest (đó là việc của guest OS). Bổ sung [15-kernel-boundary.md](15-kernel-boundary.md) (kernel TCB) và [16-rustc-tcb.md](16-rustc-tcb.md) (LBI).
> **Ratified:** 2026-07-12 (research `.agents/reports/research-260712-1010`, red-team `.agents/260712-0952`).

**Tài sản cần bảo vệ:** (1) host RAM ngoài vùng guest; (2) kernel TCB + CapSet; (3) cell khác + dữ liệu của chúng; (4) host disk (cell-store, ELF cell khác); (5) tính sẵn sàng của cả máy (SAS — một địa chỉ, kernel OOM = chết tất cả).

**Mô hình đối thủ:** guest root độc hại, hoặc guest kernel bị khai thác, điều khiển hoàn toàn: nội dung mọi vùng RAM guest, giá trị MMIO/PIO ghi ra, mọi trường trong virtqueue (địa chỉ/độ dài descriptor, chỉ số ring), HVC/PSCI function-id + tham số, tần suất QueueNotify.

**Bề mặt tấn công ↔ phòng thủ:**

| # | Bề mặt | Phòng thủ hiện có (file:line) | Trạng thái |
|---|--------|-------------------------------|-----------|
| 1 | Wrapper `read/write_guest_memory` (mọi truy cập RAM guest đi qua đây) | Kernel bounds-check GPA vào cửa sổ guest-RAM: `checked_sub`/`checked_add`, `end > guest_pages*PAGE_SIZE` → `InvalidInput` (`kernel/src/hypervisor/registry.rs:311-317, 358-364`). Cell `#![forbid(unsafe_code)]` — không deref con trỏ thô. | ✅ **Load-bearing.** Miễn nhiễm class CVE-2026-5747 (Firecracker virtio OOB write). |
| 2 | Parser descriptor chain (`virtqueue.rs`) | Mọi đọc desc/ring qua wrapper #1; `MAX_CHAIN=64` chống chain vô hạn (`virtqueue.rs:23`). | ⚠️ **GAP:** `cur` (next-index) chưa clamp `< q_size`; `avail_idx` delta chưa cap; assert `buf.writable` khớp chiều device còn thiếu → **fuzz + fix ở phase P06**. |
| 3 | `inject_irq` intid | Syscall layer validate `intid ≤ 1019`; IRQ chỉ vào hàng đợi vCPU của chính guest đó (`registry.rs:390-398`). | ⚠️ **GAP (C1, Critical, LIVE):** `push_back` không cap độ sâu (`registry.rs:398`) → guest mask IRQ + spam QueueNotify = kernel-heap OOM = chết cả máy. **Cap độ sâu ở P06.** |
| 4 | Backing-store virtio-blk | Ghi **đã hoạt động hôm nay** (BLK_T_OUT xử lý bởi `blk_write`, `virtio_blk.rs:81,103-116`) nhưng backing là **16MiB Vec volatile, zero-filled** (`:15,33`) — không load từ ảnh thật, mất khi cell restart; sector clamp `off>=disk.len()` (`:94,107`). | ⚠️ **GAP:** khi P04 thêm persist, backing PHẢI là **image-file/partition per-VM, KHÔNG shared cell-store** (nếu không: guest ghi sector tùy ý → đè FAT/cell-table/ELF cell khác = host-disk escape); sector clamp phải theo backing thật (không phải hằng số 16MiB). **Invariant + fix ở P04.** virtio-blk theo *sector*, không path → path-traversal guard KHÔNG áp (chỉ áp nếu sau này có virtio-fs). |
| 5 | PSCI/HVC dispatch | Hiện an toàn: CPU_ON trả DENIED, AFFINITY_INFO chỉ so `mpidr==0`, không index (`psci.rs:72-83`). | ⚠️ **GAP tương lai:** P09 SMP sẽ implement CPU_ON với target-CPU/entry guest-controlled → phải bounds-check khi làm. Ghi nhận trước. |
| 6 | MMIO dispatch default arm | Run-loop có default arm (log + error) cho IPA chưa đăng ký. | ✅ (m2 red-team ARM64 đã đóng). |
| 7 | Config-space reads | `config_read` trả hằng số (`virtio_blk.rs:41-47`). | ✅ hiện tại; giữ bất biến "config trả giá trị không guest-influenced". |
| 8 | Resource-exhaustion tổng quát | — | ⚠️ **GAP (C1):** xem #3 + `avail_idx` delta (#2). Nguyên tắc: guest KHÔNG được làm cạn tài nguyên kernel; mọi hàng đợi kernel do guest kích phải có trần. |

**Bất biến load-bearing (vi phạm = mất cô lба):**
1. **Bounds-check tập trung** — mọi truy cập RAM guest qua wrapper kernel đã `checked_add` (`registry.rs:311-317`). Refactor/tối ưu (P06/P07) phải chạy production qua CÙNG đường này.
2. **Cell không deref thô** — hypervisor cell `#![forbid(unsafe_code)]`; guest memory chỉ chạm qua syscall.
3. **Backing-store isolation** — virtio-blk RW backing = image-file/partition per-VM, không bao giờ shared cell-store.
4. **Shadow-GICD** — `gicd.rs` chỉ giữ shadow register, không ghi GICD vật lý (chống corrupt IRQ routing host).
5. **Single-thread/sync-vCPU** — `write_guest_memory` an toàn nhờ không vCPU nào chạy đồng thời VÀ RunVcpu đồng bộ same-core (`registry.rs:182-217, 321-323`); mọi hướng SMP/đa-luồng/async-vCPU phải thêm quiesce.
6. **Resource ceiling** — mọi hàng đợi kernel guest kích (IRQ queue, avail delta) có trần.
7. **Stage-2 SAS-isolation guard** — `stage2::map()` từ chối map một IPA guest-RAM vào HPA nằm ngoài vùng đã `carve_guest_ram` cho VM đó, trả `SasViolation` (`kernel/src/memory/stage2.rs:222-224`). Đây là cơ chế THẬT chặn `map_guest_memory` map bừa RAM host vào guest — khác với bất biến #1 (bounds-check trên đường copy runtime).

**Non-goal:** nested VM (guest không tạo sub-VM). **Ngoại lệ đã audit (không phải "không có passthrough" tuyệt đối):** `create_vm` cài đặt **GICV MMIO hardware passthrough** (GIC CPU interface HPA→IPA, cho vGIC) qua `map_mmio_passthrough`, cố ý bypass CẢ MMIO-hole guard lẫn SAS-isolation guard (`registry.rs:79-84`, `stage2.rs:279-286`) — đây là passthrough phần cứng duy nhất, read-only về phía guest, cô lập theo VMID, dùng Device-nGnRnE. Mọi device khác (blk/net/console/PL011/timer/PSCI) đều emulate, buffer copy qua wrapper #1. Không hỗ trợ VFIO/IOMMU passthrough tùy ý ⇒ nếu thêm sau phải mở threat-model.

**Khuyến nghị (KHÔNG bắt buộc) — thu hẹp syscall allowlist của hypervisor cell:**
Cell đã khai báo allowlist hẹp (`cells/services/hypervisor/src/main.rs:22-32`). Siết thêm KHÔNG phải "jailer-equivalent" như Firecracker: dưới LBI + `#![forbid(unsafe_code)]` + bounds-check tập trung, cell không thể bị chiếm quyền thực thi như một tiến trình VMM Linux. Giá trị *duy nhất* của việc siết thêm = giảm blast-radius nếu (a) một dependency `unsafe` (vd `alloc`) có lỗi, hoặc (b) rustc miscompile ([16-rustc-tcb.md](16-rustc-tcb.md)). Cân nhắc theo chi phí/lợi ích, không coi là hàng rào cô lập chính.

### 4.4 Kernel H-extension requirements (RISC-V)

**Privilege mode change khi H-extension detect:**
```
Không có H-ext (hiện tại):   M-mode → S-mode (kernel) → U-mode (cells)
Có H-ext (Tier 3 ready):     M-mode → HS-mode (kernel) → HU-mode (cells)
                                                         → VS/VU-mode (guest)
```

SBI tự detect và delegate vào HS-mode thay vì S-mode khi H-ext có.
Cells chạy HU-mode — transparent, không thay đổi cell code.

**Kernel changes:**
```
hal/arch/riscv/hypervisor.rs  (~200 LOC)
  H-extension detection (misa CSR bit 'H')
  HS-mode boot path (transparent fallback to S-mode if no H-ext)
  New CSRs: hstatus, hgatp, hedeleg, hideleg, hip, hie

kernel/src/hypervisor/         (~800 LOC, new module)
  VM struct + Stage-2 page table management
  vCPU struct + run loop + VM exit dispatch

kernel/src/syscall/hypervisor.rs  (~300 LOC)
  sys_create_vm, sys_create_vcpu, sys_map_guest_memory
  sys_run_vcpu (blocking), sys_vcpu_regs, sys_inject_irq
```

**Không đụng**: scheduler, IPC, memory quota, normal cell lifecycle.

### 4.5 Multi-arch HAL trait

```rust
/// Hardware virtualization interface — one impl per arch (hal/traits/hypervisor/).
pub trait ViHypervisor {
    type Vm; type Vcpu; type Stage2Table;
    fn create_vm(&self) -> ViResult<Self::Vm>;
    fn create_vcpu(&self, vm: &mut Self::Vm) -> ViResult<Self::Vcpu>;
    fn map_guest(&self, table: &mut Self::Stage2Table,
                 ipa: u64, hpa: u64, pages: usize, writable: bool) -> ViResult<()>;
    fn run_vcpu(&self, vcpu: &mut Self::Vcpu) -> ViResult<ViVmExit>;
    fn inject_irq(&self, vcpu: &mut Self::Vcpu, intid: u32) -> ViResult<()>;
}
```

| Arch | Mechanism | HAL crate | Status |
|---|---|---|---|
| **ARM64** | EL2 non-VHE (HCR_EL2, VTTBR_EL2, Stage-2, GICH) | `hal-arm` | **✅ G1 shipped** (P01–P10) |
| RISC-V | H-extension (HS-mode, hgatp Stage-2) | unsupported path | ⏳ Pending — H-ext absent on current boards |
| x86_64 AMD | SVM (VMCB, NPT) | `hal-x86` + owner-scoped kernel registry | 🚧 Implemented MVP; production hardware qualification pending |
| x86_64 Intel | VMX | `hal-x86` root-operation plumbing | 🚧 VMXON implemented; VMCS/EPT/guest execution pending |

Kernel syscall dispatch (`kernel/src/hypervisor/registry.rs`) selects the ARM64 registry
or the x86 SVM registry by target. Unsupported architectures return `NotSupported`.
The x86 label must remain backend-specific: AMD guest execution exists; Intel guest
execution does not.

### 4.6 Implementation status

**ARM64 EL2 VMM — ✅ COMPLETE (G1, 2026-06-16)**
```
Phases 01–10 shipped in cells/services/hypervisor/:
  P01: HAL ViHypervisor trait + ARM64 stay-at-EL2 boot + EL2 MMU/vectors
  P02: Stage-2 builder + guest-RAM carve (128 MiB) + VTTBR/VTCR
  P03: vCPU world-switch + trap decode + bare-metal guest smoke
  P04: Syscalls 220-227 (CreateVm/CreateVcpu/MapGuest/RunVcpu/VcpuRegs/InjectIrq/WriteGuest/ReadGuest)
  P05: Hypervisor cell: guest-load + DTB + PSCI + PL011 + GICD emul → BOOTS ALPINE
  P06: virtio-mmio transport + split virtqueue + virtio-console
  P07: virtio-blk → VFS Cell → mounts rootfs
  P08: virtio-net → Net Cell (L2 MAC-bridge, DHCP → 10.0.2.15, apt works)
  P09: Full GICH/GICV hardware vGIC upgrade (IRQ throughput)
  P10: ARM64 CI smoke + unsupported RISC-V path + this docs update
```

**RISC-V H-extension — ⏳ Pending**
```
Current RISC-V boards (SG2042, SG2044, K230) lack H-extension.
ENOSYS stubs in hal-riscv/src/hypervisor.rs + registry.rs are in place.
Impl unblocks when H-ext hardware is available.
```

**x86_64 AMD SVM — 🚧 Implemented MVP**
```
SVM root enablement, owner-scoped VM/vCPU registry, NPT guest mapping,
world-switch/exit conversion, IRQ injection, and the x86 Hypervisor Cell loop exist.
This is not production qualification: real-hardware lifecycle, isolation, and stress
gates remain open.
```

**x86_64 Intel VMX — 🚧 Root operation only**
```
VMX feature/firmware checks and VMXON are implemented. VMCS lifecycle, EPT, vCPU
world-switch, and guest execution remain pending.
```

---

## 5. Platform Profiles

| Profile | Tiers | Hardware | Use case |
|---|---|---|---|
| **Cellos-Nano** | Tier 1 | RV32, <512KB | MCU, motor/sensor control |
| **Cellos-Standard** | Tier 1 + 1b + 3a | RV64/ARM64 SBC | Robot brain, edge AI |
| **Cellos-Server** | Tier 1 + 1b + 3a + 3b | x86_64 / ARM64 | Server, PC, cloud node |

---

## 6. Những đường sai cần tránh (Wrong Paths)

1. **Type-1 hypervisor**: Tier 3 phải chạy ON TOP of Cellos, không phải thay thế kernel. Cellos kernel = Type-2 host. ✅ Xác nhận: hypervisor cell là Tier 1 cell bình thường với HypervisorCap.
2. **Port QEMU**: Quá lớn (~1M LOC C, cần JIT/mmap) — không fit Tier 1 cell.
3. **Fork crosvm**: ~75K LOC thực tế (không phải ~20K), kéo theo tokio + mmap — không fit SAS cell. ✅ Build minimal VMM từ scratch (~9K LOC) — đã shipped ARM64 EL2 (P01-P10).
4. **Gộp Security Silo và Linux VM**: Hai use case khác nhau — implement riêng, reuse Stage-2 primitives.
5. **Assume H-ext mọi nơi**: RV32 không có H-ext. ARM dùng EL2. x86 splits AMD SVM
   and Intel VMX. Phải per-arch HAL. ARM64 EL2 shipped; AMD SVM is an MVP; Intel VMX
   guest execution and RISC-V H-extension remain pending.
6. **Android G1**: Android cần GPU passthrough + camera HAL + binder IPC — G2+ only, đừng để Android shape G2 design sớm.
7. **WASI Preview 1**: Deprecated (2019 spec), bỏ qua hoàn toàn.
