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

Cellos không có một "Metadata Registry" toàn cục. Quyền sở hữu được giữ tại authority
nhỏ nhất có thể kiểm tra và thu hồi đúng vòng đời:

* Task/Cell frame ownership và quota nằm ở task-owned frame lists + `cell_quota`.
* Grant ownership/lease nằm trong per-task grant tables; reaper thu hồi khi task chết.
* Async/DMA lifetime nằm trong pin registry và quarantine; frame chỉ được reclaim sau
  cancel/unpin hoặc quarantine completion.
* MMIO/resource exclusivity nằm trong resource registry tương ứng.

Không scan con trỏ tổng quát và không cập nhật OwnerID bằng heuristic. Hibernate/hot-swap
phải dùng typed, subsystem-owned serialization.

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

* **Chỉ là code integrity, không phải data confidentiality.** Stack, heap, grant page và
  MMIO window vẫn `USER+RW` **xuyên cell**: một cell có `unsafe` vẫn đọc/ghi được *dữ liệu*
  của cell khác. Tường cho dữ liệu là Layer B (per-domain page table, Tier 2 — chưa hiện
  thực). Điều §5 bảo đảm là không cell nào sửa được **code hoặc hằng** của cell nào.
* **Cross-hart / cross-core closure còn phụ thuộc arch.**
  - `RV64`: W^X order PTE update, local `sfence.vma`, rồi SBI RFENCE tới mọi hart online từ xa;
    firmware không probe được RFENCE phải giữ kernel single-hart. QEMU 8.2/OpenSBI đã PASS oracle
    hai hart 5/5 với cả positive RFENCE và negative control; hardware RV64 thật vẫn là host gate.
  - `x86_64`: `invlpg` chỉ local; chưa có SMP IPI shootdown path, nên cửa sổ stale entry trên core
    khác vẫn là giới hạn thật.
  - `AArch64`: HAL đã phát `dsb ishst; tlbi vaae1is; [vae2is nếu EL2]; dsb ish; isb`, nên stage-1
    invalidate là broadcast trong inner-shareable domain theo code hiện tại. Tuy vậy repo vẫn thiếu
    witness runtime 2 PE, nên chưa được tuyên bố là D7 hoàn tất.
* **Arch bare-physical không enforce.** riscv32 Nano, x86_32, arm32 chạy không page table;
  `wx::enforce` ghi log khoảng trống thay vì áp đặt. Câu "protection vẫn được bật" chỉ đúng
  với các arch có MMU.
