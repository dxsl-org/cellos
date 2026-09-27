# Cellos Architecture: Runtime & SDK
**Version**: 0.3 (Zero-Copy Inter-Cell Communication)
**Status**: Definitive

---

## 1. IPC: Direct Method Calls
Trong Cellos, khái niệm IPC truyền thống bị loại bỏ. Mọi tương tác giữa các Cell là **gọi hàm trực tiếp (Direct Call)** thông qua Rust Traits.

* **Performance**: Chi phí tương đương một lời gọi hàm ảo (~2-3 chu kỳ CPU).
* **Interface**: Định nghĩa trong crate `libs/api`. Sử dụng `#[repr(C)]` cho các cấu trúc dữ liệu ở biên giới (Boundaries) để đảm bảo **Stable ABI**.
* **Data Flow**: Mặc định là **Zero-copy**. Dữ liệu được truyền dưới dạng tham chiếu (`&T`) hoặc quyền sở hữu (`Box<T>`).

## 2. Async/Await & Safety (Owned Buffers)
Cellos tận dụng triệt để mô hình lập trình bất đồng bộ của Rust để tối ưu I/O.

### Quy tắc "Owned Buffers ONLY"
Để ngăn chặn lỗi ghi đè bộ nhớ khi một Cell bị unload đột ngột:
* **Quy tắc**: Cấm truyền `&mut [u8]` qua ranh giới Async giữa các Cell.
* **Giải pháp**: Phải truyền `Box<[u8]>` hoặc `Vec<u8>`. Quyền sở hữu (Ownership) được chuyển giao hoàn toàn cho Driver.

### Async Pinning Registry (Lá chắn Unload)
* **Cơ chế**: Khi một vùng nhớ tham gia Async/DMA, pin registry ghi owner, range và
  lifecycle; quarantine ngăn reclaim trước khi cancellation/unpin hoàn tất.
* **Bảo vệ**: Kernel sẽ từ chối lệnh `unload` của Cell sở hữu ban đầu cho đến khi tác vụ Async hoàn tất và quyền sở hữu được trả về hoặc giải phóng.

## 3. Hot-Swap & State Transfer
Cellos hỗ trợ nâng cấp phần mềm mà không cần ngừng hệ thống (Live Update).

* **Protocol**: Các Cell quan trọng phải thực thi Trait `StateTransfer`.
* **Quy trình**:
    1. Kernel đóng băng (Pause) các luồng thực thi của `OldCell`.
    2. Gọi `serialize_state()` để trích xuất dữ liệu trạng thái.
    3. Nạp `NewCell` và gọi `deserialize_state(blob)`.
    4. Tráo đổi con trỏ hàm (Symbol Re-linking) và giải phóng `OldCell`.

## 4. Boot Optimization (Snapshot)

> **Trạng thái (2026-09-28)**: warm boot qua snapshot **chưa được bật trên bất kỳ ảnh nào**.
> Cổng qualify là một feature build, `QUALIFICATION_ENABLED = cfg!(feature = "snapshot-qualified")`,
> mặc định tắt trong mọi ảnh shipping: capture và restore đều từ chối trước khi chạm đĩa hoặc
> RAM, và hợp đồng shell/Supervisor giữ nguyên (`snapshot: unavailable on this platform`).
> **Chưa có phép đo warm-boot nào và chưa có witness trên board** — con số `<100 ms` trước đây
> không có witness đứng sau. Định dạng v2 ở §4.3 là hợp đồng **nội bộ** đã có code + unit test
> (nửa thiết bị-độc-lập của phase 07), không phải một tuyên bố sẵn sàng.

### 4.1 Mục tiêu
- **Cold boot** (lần đầu, sau update, hoặc khi ảnh snapshot bị từ chối): parse ELF + link +
  init cells — đây là đường khởi động duy nhất hiện đang chạy.
- **Warm boot** (snapshot `COMMITTED` hợp lệ): replay đúng các frame vật lý đã lưu rồi resume
  scheduler. Thời gian **chưa được đo**; chỉ chạy khi một profile đã qualify bật feature này.

### 4.2 Cơ chế hoạt động

Ảnh snapshot nằm trong partition **P3** dành riêng của ảnh đĩa MBR (`disk_v3.img`), **không**
phải một file `system.img` trên FAT16: sector 0 là header, sector 1 trở đi là inventory rồi
payload. Kernel chỉ ghi/đọc vùng P3 đó.

```
Capture (chỉ khi feature bật):
  freeze/park mọi task + hart           ← phase 07 step 3, CHƯA có
        ↓
  ghi header WRITING + flush            (vô hiệu hoá ảnh cũ trước khi ghi payload)
        ↓
  ghi inventory (pa, frame_count) → payload → flush
        ↓
  ghi header COMMITTED + flush

Restore (chỉ khi feature bật):
  đọc + kiểm tra toàn bộ ảnh            (chưa ghi một byte RAM nào)
        ↓
  ghi CONSUMING + flush                 (bền vững; reboot từ đây từ chối ảnh)
        ↓
  replay chính xác từng PA trong inventory
        ↓
  ghi CONSUMED
```

### 4.3 Snapshot Format (internal v2)

Header đúng một sector 512 byte tại sector đầu của P3. Mọi trường little-endian.

```
Offset  Size  Field
0x00    4     magic = 0x5543_4956 ("VICU" khi đọc byte)
0x04    2     version = 2  (header v1 bị từ chối → cold boot, không migration)
0x06    1     state: EMPTY=0, WRITING=1, COMMITTED=2, CONSUMING=3, CONSUMED=4
0x07    1     flags (phải bằng 0)
0x08    8     kernel_hash (8 hex đầu của git SHA lúc build)
0x10    8     ram_base   ┐ bố cục RAM managed tại thời điểm capture
0x18    8     ram_end    ┘
0x20    4     sector_size
0x24    4     run_count
0x28    4     frame_count (tổng số frame 4096 byte)
0x2C    4     inventory_sectors
0x30    4     image_sectors (header + inventory + payload)
0x34    4     padding
0x38    8     payload_lba
0x40    4     crc32   ← 4 byte này bị đặt về 0 khi tính checksum
0x44    4     reserved
0x48  440     reserved (zero)
```

**Inventory** (`run_count` entry, mỗi entry 16 byte, zero-pad hết sector):

```
Offset  Size  Field
0x00    8     pa (physical address, căn 4096)
0x08    4     frame_count (≥ 1)
0x0C    4     flags (phải bằng 0)
```

Mỗi run là `frame_count` frame 4096 byte **liền nhau** bắt đầu tại PA tường minh `pa`. Frame
không thuộc run nào thì không có trong ảnh — reader replay đúng `runs[*].frame_pa(i)` và
**không** bao giờ dựng lại một dải dense kiểu `pa_base + index * 4096`.

**Payload**: các frame của từng run, ghi tại đúng PA của chúng, theo thứ tự inventory.

**Checksum** — một định nghĩa duy nhất dùng chung cho writer và reader:
`crc32(header.canonical_bytes() || inventory_sectors || payload_sectors)`, trong đó
`canonical_bytes()` là 512 byte header với 4 byte `crc32` (offset `0x40`) đặt về 0.

**Invalidation / từ chối** (fallback cold boot, không migration):
- magic sai, version không phải 2, hoặc `state` không đọc được;
- `kernel_hash` đổi (kernel được build lại) hoặc bố cục RAM (`ram_base`/`ram_end`) đổi;
- geometry/identity sai: run rỗng, lệch căn, ngoài RAM, trùng hoặc chồng lấn; ảnh vượt P3;
  `sector_size ≠ 512`; tổng frame trong inventory khác header;
- checksum mismatch (corruption);
- state `WRITING` / `CONSUMING` / `CONSUMED` gặp lại sau reboot.

### 4.4 Ràng buộc triển khai

| Ràng buộc | Lý do |
|-----------|-------|
| VirtIO/thiết bị **không** được snapshot | MMIO register reset sau power cycle; phải reinit, và transport MMC còn phải bị loại trừ khi capture |
| MMIO region bị loại khỏi inventory | Chỉ frame thuộc managed RAM vào inventory; ghi vào MMIO có side effect (gửi packet, eject disk) |
| Snapshot dùng **physical address** tường minh (PA trong inventory) | VA không được lưu; reader replay theo PA, nên bố cục RAM lúc restore phải trùng lúc capture |
| **Không** có relocation table | Layout/identity mismatch bị **từ chối** ở preflight, không được patch; thay đổi layout/KASLR ⇒ cold boot |
| Mọi lần đổi state đều `flush` bền vững | Header `WRITING`/`COMMITTED`/`CONSUMING`/`CONSUMED` là thứ tự durable; reboot giữa chừng phải thấy ảnh chưa-commit hoặc đã-consume |
| Replay đã bắt đầu ⇒ không quay lại cold boot | Lỗi giữa chừng trả `RestoreOutcome::FatalMixedRam` và reset (`halt_mixed_ram`), không chạy tiếp trên RAM hỗn hợp |
| Closure chưa được chứng minh | Inventory từ frame allocator **không** chứng minh phủ hết `.data`/`.bss` của kernel, allocator metadata/lock, page table, task record và hart-local (phase 07 step 2) |

### 4.5 Prerequisites trước khi bật

- [x] Định dạng nội bộ v2: inventory PA tường minh, một checksum canonical, state machine
      `EMPTY → WRITING → COMMITTED → CONSUMING → CONSUMED` với thứ tự flush bền vững,
      preflight capacity/identity, và `FatalMixedRam` + reset thay vì cold boot trên RAM hỗn hợp
- [x] Fake block device trong bộ test (`#[cfg(test)]`) cùng ma trận corruption/reset
      (26 test snapshot; `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` → 145 passed)
- [ ] All-hart quiescence + acknowledged safe-root freeze trước capture
- [ ] Coherent staging (COW hoặc write-protection) để byte không đổi giữa lúc đọc và lúc ghi
- [ ] Closure đầy đủ (mutable kernel-image root, allocator metadata/lock, page table, task record, hart-local)
- [ ] Authenticated monotonic epoch / freshness lấy từ thiết bị lưu trữ tin cậy
- [ ] Witness thật `save → reset → restore → resume` trên board có block device, kèm loại trừ transport MMC
- [ ] Phép đo warm boot trên đúng thiết bị — không con số nào được claim trước khi đo

## 5. Tooling: `ostd` & `cargo-Cellos`
* **`ostd`**: Thư viện chuẩn thay thế `std`, cung cấp các interface cho Allocator, Async Runtime và Logging.
* **Multi-Arch Build**: `cargo-Cellos` hỗ trợ biên dịch song song cho nhiều Target (RV32, RV64) từ cùng một mã nguồn thông qua cơ chế `hal/core`.
