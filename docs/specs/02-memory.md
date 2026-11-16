# Cellos Architecture: Memory Model
**Version**: 0.3 (Universal SAS & Resource Governance)
**Status**: Definitive
**Amended 2026-11-16** (architecture remediation Phase 19): §1, §4 và §6 được sửa cho khớp code;
trạng thái theo kiến trúc nằm ở bảng sinh tự động [`docs/generated/arch-status.md`](../generated/arch-status.md).

---

## 1. SAS Layout
Không có trait layout trong `hal-core`. Bố cục là hằng số của kernel
(`kernel/src/loader/va_alloc.rs`) cộng với alias RAM của HAL từng kiến trúc.

### Layout Segments (kernel 64-bit)
| Segment | Ở đâu | Đặc điểm |
| :--- | :--- | :--- |
| **RAM alias của kernel** | `hal::paging::phys_to_virt` | Identity (VA = PA) hoặc HHDM tùy kiến trúc: cột "Kernel RAM alias" của [arch-status](../generated/arch-status.md). Kernel ghi vào bộ nhớ Cell qua alias này. |
| **RAM được quản lý** | dưới `IDENTITY_RAM_END = CELL_VA_START` (`va_alloc.rs:107`) | RAM từ 64 GiB trở lên không được dùng và được log (`memory::frame::plan_managed_ranges`). |
| **Cell slots** | từ `CELL_VA_START = 0x10_0000_0000` (64 GiB), bước `CELL_VA_STRIDE` 32 MiB (256 MiB với `large-cell-va`), `MAX_SLOTS = 512` (`va_alloc.rs:76-96`) | Code, data và heap riêng của một Cell; heap được trừ quota (§2). |
| **Stack arena** | `STACK_VA_START = CELL_VA_START + MAX_SLOTS * CELL_VA_STRIDE` (`va_alloc.rs:385`) | User stack có guard page không có frame (§4). |
| **Kernel image** | ảnh của kernel | Nơi duy nhất kernel fetch lệnh (§3.1 bước 6). |

Kernel 32-bit (RV32, x86, arm) nằm ở `kernel/src/arch/legacy` và không được build.

## 2. Global Allocator & Resource Governance
Hệ thống sử dụng **Hybrid Allocator** để cân bằng giữa tốc độ và chống phân mảnh.

### Quota-based Allocation (Chống "Tham")
**Amended 2026-11-15** (architecture remediation Phase 17): heap được trừ vào một chủ tường
minh, không còn vào "Cell đang chạy".

* **Cơ chế**: Mỗi Cell incarnation có `MemoryQuota`. Chủ của một lần trừ là một `Charge`
  (`CellId` + generation của incarnation); `Charge::KERNEL` không bị giới hạn và không được đếm.
* **Thực thi**: Bộ cấp phát (`GlobalAlloc`) trừ mỗi lần cấp phát vào `Charge` mà hart đang mở
  qua `heap::ChargeScope`. Nó không suy chủ từ Cell đang được dispatch hay từ Program Counter.
  Chỉ ba loại code mở scope:
  1. Syscall entry mở `Charge::dispatched()`: việc của syscall được tính cho Cell gọi. Đây là
     nơi duy nhất dispatch identity trở thành một charge.
  2. Cấp phát có chủ chỉ định: payload IPC wire tính cho Cell gửi, payload mailbox lớn hơn
     32 byte tính cho Cell nhận, completion queue tính cho Cell sở hữu.
  3. Scope kernel (`Charge::KERNEL`) cho bookkeeping của kernel, kể cả phần tăng của mailbox
     (bị giới hạn bởi độ sâu mailbox).

  Mọi interrupt entry mở scope kernel làm lệnh đầu tiên, và `yield_cpu` giữ scope kernel qua
  mỗi lần chuyển ngữ cảnh, nên không scope nào đi qua một interrupt hay một context switch.
  Ngoài mọi scope, chủ là kernel. Mỗi block ghi lại incarnation đã bị trừ, và `dealloc` hoàn
  lại đúng incarnation đó, bất kể ai giải phóng; incarnation đã bị thay thì không được hoàn.
  Đối tượng có quota riêng (pipe ring, thread stack, SHM, DMA) trừ trực tiếp qua
  `cell_quota::Reservation`.
* **OOM Policy**: Khi vượt quota hoặc hết heap, bộ cấp phát trả về null. Lời gọi fallible
  (`try_reserve`, `heap::try_box`) nhận `OutOfMemory`. Lời gọi infallible đi vào
  `alloc_error_handler`, làm hart đó dừng (idle) chứ không panic toàn hệ thống.

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
6. Normalise: RV64/AArch64 khôi phục identity alias kernel RW+NX (đọc/ghi, không bao giờ
   thực thi; cùng leaf mà boot map cấp cho mọi frame `Usable`) khi alias khác `Intact`;
   x86_64 không bao giờ thêm alias mà prune các bảng trang lower-half vào cùng batch.
   Không có gì thực thi từ một frame tự do hay từ identity alias của RAM: kernel chỉ fetch
   từ image của chính nó (`MemoryType::Kernel`).
7. Free dưới allocator guard.

Một VA đã giải phóng (cell VA slot, slot của stack arena, stack reservation) vẫn reserved cho tới khi flush của nó
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
* **Cơ chế**: Đáy mỗi stack kernel và user có `STACK_GUARD_PAGES = 2` trang guard (`kernel/src/task/stack.rs:26-27`); guard của user stack trong stack arena không có frame (`kernel/src/loader/va_alloc.rs:387`).
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
  SMEP/SMAP trên x86_64 (bật khi CPU có) không đổi điều đó: SMAP chỉ chặn *kernel* chạm trang USER
  ngoài user-copy window (`task/user_copy`, nơi duy nhất set RFLAGS.AC); kernel ghi vào bộ nhớ cell
  (nạp segment, zero grant và stack) qua alias HHDM/identity. Giữa các Cell trong SAS, SMAP không chặn
  gì. RV64 giữ `sstatus.SUM` suốt đời kernel, AArch64 chạy không có PAN: lớp chặn kiểu SMAP chỉ có trên x86_64.
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
* **Arch 32-bit không có page table.** `wx::enforce` của riscv32, x86_32 và arm32 chỉ ghi log khoảng
  trống (`kernel/src/loader/wx.rs:213-227`); các target này không được build
  ([arch-status](../generated/arch-status.md)). Câu "protection vẫn được bật" chỉ đúng với các arch có MMU.

## 6. Addendum: Tier 2 Paged Domain Memory Architecture (Layer B)

**Amended 2026-09-19** (ADR-0015 & Spec 22 Gate); sửa 2026-11-16 cho khớp code:
Tier 2 cung cấp ranh giới cô lập phần cứng hoàn toàn (Hardware MMU Isolation) cho mã không tin cậy (C/C++ FFI, mlibc, Lua, hoặc unsigned binaries). Kiến trúc nào admit Tier 2 là bảng [arch-status](../generated/arch-status.md):

1. **Bảng trang riêng (`AddressSpace`)**:
   - Mỗi Tier 2 Cell sở hữu một gốc bảng trang riêng (`satp` Sv39 trên RV64, `TTBR0_EL1` trên AArch64, `CR3` trên x86_64), được gán tag (ASID/PCID) độc lập từ pool `PRIVATE_ASIDS` (`kernel/src/memory/tag_pool.rs`, `address_space.rs:353`; tối đa 1 tag mỗi incarnation; tag chỉ được cấp lại khi có bằng chứng `Invalidated` rằng mọi hart đã invalidate tag đó).
   - **User space**: Chỉ ánh xạ private ELF segments, stack, heap và grant của chính cell đó; mọi leaf user là non-global (`address_space.rs:1553-1571`). Toàn bộ không gian bộ nhớ của các Cell khác (bao gồm cả Tier 1 SAS) đều **unmapped**; bất kỳ hành vi đọc/ghi/thực thi trái phép nào ra ngoài đều gây page fault (RV64: `scause = 0xd, 0xf, 0xc`) và cell bị terminate an toàn mà không ảnh hưởng tới kernel.
   - **Kernel space**: Ánh xạ supervisor-only (không có cờ `USER`). U-mode không thể truy cập RAM alias hay kernel text/data.
2. **Quản lý Vòng đời & Teardown**:
   - Chuyển trạng thái `Live` → `Dying` theo thế hệ (`generation`).
   - Thu hồi toàn bộ khung trang trung gian và trang lá khi cell kết thúc.
3. **Cơ chế Grant liên Domain (Domain Grants)**:
   - Grant page (`Syscall::GrantRegister` / `Syscall::GrantSlice`) được ánh xạ vào bảng trang riêng của domain qua `map_grant_page` theo quyền của grant (`ReadOnly` → R+NX, `ReadWrite` → RW+NX) và được thu hồi qua `unmap_grant_page`, với invalidation tag được xác nhận trước khi frame được dùng lại ([Spec 22](22-native-domain-cell-implementation-gate.md) §2.5).
