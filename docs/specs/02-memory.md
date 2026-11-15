# Cellos Architecture: Memory Model
**Version**: 0.3 (Universal SAS & Resource Governance)
**Status**: Definitive

---

## 1. Universal SAS Layout (Trait-Based)
Thay vì hardcode địa chỉ 64-bit, Cellos dùng bộ khung **Virtual Memory Layout** trừu tượng thông qua `hal-core`.

### Layout Segments
| Segment | RV32 (Sv32) | RV64 (Sv39/48) | Đặc điểm |
| :--- | :--- | :--- | :--- |
| **Trap Zone** | Low 4KB | Low 4KB | Unmapped để bắt lỗi NULL pointers. |
| **HHDM** | Offset-based | High-half | Ánh xạ trực tiếp RAM vật lý. |
| **Kernel Static** | Fixed High | Fixed High | Code/Data của Nano Kernel. |
| **Global Heap** | Remaining | Dynamic | Vùng nhớ cấp phát cho các Cell. |

## 2. Global Allocator & Resource Governance
Hệ thống sử dụng **Hybrid Allocator** để cân bằng giữa tốc độ và chống phân mảnh.

### Quota-based Allocation (Chống "Tham")
* **Cơ chế**: Mỗi Cell có `MemoryQuota`.
* **Thực thi**: Bộ cấp phát (`GlobalAlloc`) truy vấn `CallerID` (thông qua Program Counter range) để trừ vào quỹ RAM của Cell đó.
* **OOM Policy**: Trả về `Result::Err(OutOfMemory)` thay vì panic toàn hệ thống.

### Real-Time Pool (TLSF)
`rlsf 0.2.3` is linked and a 256 KiB static pool is initialised at boot, but no runtime
allocation path currently calls the `alloc` / `dealloc` wrappers. Stacks still come from
`Stack::new_kernel` and `Stack::new_user`, so Cellos has not qualified end-to-end TLSF
WCET or latency on the current system.

## 3. Focused ownership authorities

**Amended 2026-11-15** (architecture remediation Phase 16): một kiểu chủ sở hữu frame
(`FrameRun`), một hợp đồng release, một record `Region` cho các bảng grant/SHM/DMA export.

Cellos không có một "Metadata Registry" toàn cục. Quyền sở hữu được giữ tại authority
nhỏ nhất có thể kiểm tra và thu hồi đúng vòng đời:

* Frame vật lý: mỗi chủ sở hữu giữ frame của mình dưới dạng `FrameRun`
  (`kernel/src/memory/frame_run.rs`): stack, cell segment (`memory/cell_segments.rs`), grant,
  SHM, domain backing, bảng trang EPT/NPT/Stage-2, guest RAM và VMCB/VMCS/MSR/IO bitmap
  (`PrivateRun`). Ngoài `memory/frame*.rs` không còn lời gọi free frame thô nào trong code
  production. Quota nằm ở `cell_quota`.
* Grant, SHM và DMA export: `PAGE_GRANT_TABLE` (kèm các SHM handle), `REG_GRANT_TABLE` và DMA
  `EXPORTS` đều lưu một `Region` (`kernel/src/memory/region.rs`): owner (task + cell
  incarnation), rights, receiver, pin state, record vòng đời domain nếu có, và frame của
  nó — `FrameRun` mà region sở hữu từ lúc cấp phát, hoặc dải frame của một grant mà export
  khóa. Id mà syscall nhìn thấy (địa chỉ base của grant/SHM frame, export id) không đổi. Chỉ
  ai lấy được row ra khỏi bảng mới release frame của nó. Trong bảng `GrantAlloc`, chỉ syscall
  SHM thấy row SHM và syscall grant không thấy chúng.
* Async/DMA lifetime nằm trong pin registry: một run còn bị pin, lease hoặc export giữ thì
  bị giữ lại, vẫn mapped (`Quarantined(Pinned)`), cho tới khi key của nó kết thúc.
* MMIO/resource exclusivity nằm trong resource registry tương ứng; một window mà việc hạ
  quyền USER chưa được xác nhận vẫn bị giữ (sentinel quarantined) và được thử lại.

Không scan con trỏ tổng quát và không cập nhật OwnerID bằng heuristic. Hibernate/hot-swap
phải dùng typed, subsystem-owned serialization.

### 3.1 Hợp đồng release của `FrameRun`

Một `FrameRun` là một dải frame liên tục có owner, kind (`Leaf`, `PageTable`, `VmStructure`),
kernel alias (`Intact`, `Removed`, `User`) và trạng thái
`Mapped | Unpublished | Quarantined(reason) | Free`. Thứ tự release không đổi:

1. Holder dài hạn (pin, lease, DMA export) được kiểm tra khi run còn `Mapped`; còn holder thì
   run thành `Quarantined(Pinned)` và PTE giữ nguyên, vì holder vẫn dereference VA.
2. Gỡ mọi publication: leaf trong kernel root và trong private root.
3. Flush một lần cho cả operation (`Batch`): cell chết là một operation, chi phí là
   O(operation) broadcast, không phải O(trang).
4. Chờ xác nhận có giới hạn. Không xác nhận thì run thành `Quarantined(TagUnconfirmed |
   SharedUnconfirmed | RfenceFailed)`, được log, không bao giờ được dùng lại và không bao giờ
   gây panic. Reaper trên hart 0 giải phóng run khi mọi epoch per-hart đã yêu cầu được publish;
   không có gì bị bỏ rơi.
5. Drain các copy window ngắn (SAS copy).
6. Normalise: RV64/AArch64 khôi phục identity alias kernel RWX khi alias khác `Intact`;
   x86_64 không bao giờ thêm alias mà prune các bảng trang lower-half vào cùng batch.
7. Free dưới allocator guard.

Một VA đã giải phóng (cell VA slot, stack reservation) vẫn reserved cho tới khi flush của nó
được xác nhận (`VaHold`). Flush trong lúc giữ `FRAME_ALLOCATOR` không biểu diễn được:
`frame::lock` mượn `FlushCap` suốt đời guard (E0499), có witness
`scripts/check-frame-guard-compile-fail.sh` chạy trong CI.

Hạ quyền (W^X, thu hồi MMIO) không thể quarantine nên fail closed: spawn bị từ chối và frame
của nó bị quarantine; window MMIO vẫn bị giữ cho tới khi lần hạ quyền được xác nhận.

### 3.2 Giới hạn quarantine

Run đang chờ invalidation (`TagUnconfirmed`, `SharedUnconfirmed`, `RfenceFailed`) được đếm.
Khi chúng vượt 1/64 bộ nhớ được quản lý, mọi cấp phát frame mới bị từ chối và trả về kết quả
hết bộ nhớ sẵn có của đường gọi (đếm ở `frame_run.refused`, log theo lũy thừa của 2): node
xuống cấp chứ không hỏng. Pin hold và `AliasUnrestored` không được tính. Frame bảng trang
(`allocate_frame_uncapped`) được miễn, để giới hạn không bao giờ biến một quarantine thành
panic.

## 4. Stack Safety (Guard Pages)
* **Cơ chế**: Mọi Stack của Task/Cell được bao bọc bởi một trang **Unmapped 4KB (Guard Page)**.
* **Hành vi**: Stack Overflow sẽ kích hoạt `Page Fault` ngay lập tức. Kernel sẽ cô lập Task đó thay vì để nó phá nát dữ liệu Cell lân cận.

## 5. Protection Policy (W^X)

Dù dùng chung bộ nhớ, hardware page-level protection vẫn được bật cho segment của cell:

* **Text**: Read + Execute (RX).
* **Data**: Read + Write (RW).
* **Read-only** (`.rodata`, RELRO): Read (R).

Loader phải map mọi trang WRITE trong lúc áp `.rela.dyn`, rồi **hạ về đúng `p_flags`
trước khi cell chạy lệnh đầu tiên**. Cơ chế: `docs/specs/19-hardware-isolation-layers.md`
§2 Layer A. Verify runtime 2026-07-31 (`tests/integration/tests/wx-text-write.rs`: cell ghi
vào `.text` của chính nó → fault → cell bị terminate, kernel tiếp tục chạy).

### Bảo đảm này KHÔNG bao gồm

Ba giới hạn dưới đây là giới hạn của *bảo đảm*, không phải chi tiết hiện thực — đọc §5 mà
thiếu chúng sẽ dẫn tới kết luận sai về mức cô lập:

* **Chỉ là code integrity, không phải data confidentiality trong SAS.** Stack, heap, grant page và
  MMIO window trong SAS vẫn `USER+RW` **xuyên cell** đối với các Cell cùng chia sẻ `KERNEL_ROOT`.
  Tường cách ly dữ liệu vật lý là Layer B (per-domain page table, Tier 2 — đã hiện thực hóa theo Spec 22).
* **Cross-hart / cross-core closure còn phụ thuộc arch.**
  - `RV64`: W^X order PTE update, local `sfence.vma`, rồi SBI RFENCE tới mọi hart online từ xa;
    RFENCE lỗi thì quay về đường IPI epoch; firmware không probe được RFENCE phải giữ kernel
    single-hart. QEMU 8.2/OpenSBI đã PASS oracle
    hai hart 5/5 với cả positive RFENCE và negative control; hardware RV64 thật vẫn là host gate.
  - `x86_64`: hạ quyền dùng shared flush (`memory::tlb_shootdown`): flush local mọi PCID rồi
    IPI epoch tới mọi CPU online, chờ có giới hạn. CPU không xác nhận thì W^X fail closed (spawn
    bị từ chối, frame bị quarantine) thay vì panic. Fixture `S22-X86-QUARANTINE-SILENT-CPU` giữ
    một CPU với ngắt bị che quá giới hạn chờ và chứng minh điều đó trên QEMU TCG và KVM.
  - `AArch64`: HAL đã phát `dsb ishst; tlbi vaae1is; [vae2is nếu EL2]; dsb ish; isb`, nên stage-1
    invalidate là broadcast trong inner-shareable domain theo code hiện tại. Tuy vậy repo vẫn thiếu
    witness runtime 2 PE, nên chưa được tuyên bố là D7 hoàn tất.
* **Arch bare-physical không enforce.** riscv32 Nano, x86_32, arm32 chạy không page table;
  `wx::enforce` ghi log khoảng trống thay vì áp đặt. Câu "protection vẫn được bật" chỉ đúng
  với các arch có MMU.

## 6. Addendum: Tier 2 Paged Domain Memory Architecture (Layer B)

**Amended 2026-09-19** (ADR-0015 & Spec 22 Gate):
Tier 2 cung cấp ranh giới cô lập phần cứng hoàn toàn (Hardware MMU Isolation) cho mã không tin cậy (C/C++ FFI, mlibc, Lua, hoặc unsigned binaries):

1. **Bảng trang riêng (`AddressSpace`)**:
   - Mỗi Tier 2 Cell sở hữu một gốc bảng trang Sv39 (`satp`) riêng biệt, được gán ASID độc lập (`AsidLease`).
   - **User space**: Chỉ ánh xạ private ELF segments, stack và heap của chính cell đó. Toàn bộ không gian bộ nhớ của các Cell khác (bao gồm cả Tier 1 SAS) đều **unmapped**; bất kỳ hành vi đọc/ghi/thực thi trái phép nào ra ngoài đều kích hoạt Store/Load/Instruction Page Fault (`scause = 0xd, 0xf, 0xc`) và cell bị terminate an toàn mà không ảnh hưởng tới kernel.
   - **Kernel space**: Ánh xạ supervisor-only (không có cờ `USER`). U-mode không thể truy cập HHDM hay kernel text/data.
2. **Quản lý Vòng đời & Teardown**:
   - Chuyển trạng thái `Live` → `Dying` theo thế hệ (`generation`).
   - Thu hồi toàn bộ khung trang trung gian và trang lá khi cell kết thúc.
3. **Cơ chế Grant liên Domain (Domain Grants)**:
   - Zero-copy grant pages (`Syscall::GrantRegister` / `Syscall::GrantSlice`) được ánh xạ trực tiếp vào bảng trang riêng của domain qua `map_grant_page` với cờ `USER+RW` và được thu hồi qua `unmap_grant_page` khi gọi `Syscall::GrantUnregister`.
