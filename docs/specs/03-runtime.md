# Cellos Architecture: Runtime & SDK
**Version**: 0.4 (kernel-mediated IPC)
**Status**: Definitive
**Amended 2026-11-16** (architecture remediation Phase 19): §1, §2, §4 và §5 được sửa cho khớp code.

---

## 1. IPC: kernel-mediated message passing
Giữa các Cell không có lời gọi hàm trực tiếp qua Rust Trait. Cell Tier 1 có VA slot riêng
trong SAS, Cell Tier 2 có page table riêng ([Spec 18](18-cell-trust-tiers.md) §2), và mọi tương
tác giữa các Cell là **message passing qua kernel**. Hợp đồng chuẩn
là [Spec 17](17-ipc-wire-contract.md) §1 (amendment Phase 11, 2026-11-12):

* **Request/reply** là một exact operation: caller `IpcSubmit` rồi chờ đúng terminal của
  operation đó (`IpcWait`/`WaitCompletion(IPC)` → `IpcTake`); provider trả lời bằng `IpcReply`.
  SDK: `ostd::ipc::call_bytes`, `service_call*_bounded`, `ServiceRef::call`
  (`libs/ostd/src/ipc.rs`, `libs/ostd/src/call_outcome.rs`).
* **Event** là một post vào mailbox có giới hạn của đích (`ostd::ipc::post`, `SendGather`);
  mailbox đầy thì trả `Busy`, không bao giờ park.
* **Interface**: số syscall và kiểu ABI ở `libs/abi` (`libs/abi/src/syscall.rs`); hợp đồng
  dịch vụ (byte-0 discriminant, postcard message) ở `libs/wire` (`libs/wire/src/proto/`).
* **Data flow**: byte request và reply được kernel copy (`kernel/src/task/syscall/ipc.rs`).
  Dữ liệu lớn đi qua grant (`libs/ostd/src/grant.rs`): trong SAS là trang identity-mapped USER;
  với Tier-2 domain, grant được map tường minh vào page table của bên nhận
  ([Spec 17](17-ipc-wire-contract.md) §12). Không có tham chiếu Rust (`&T`, `Box<T>`) hay con
  trỏ thô nào đi qua ranh giới Cell. Ngoại lệ cũ `VfsResponse::DataPtr` (`GetFile`) đã bị bỏ
  ngày 2026-11-21; đọc file đi qua `ReadFileGrant` (Spec 17 §9).

## 2. Async/Await & Safety (Owned Buffers)
Cellos dùng mô hình bất đồng bộ của Rust **bên trong** một Cell (`ostd::executor`). Giữa các
Cell, bộ nhớ chỉ được chia sẻ qua grant, nên quy tắc dưới đây áp dụng cho grant.

### Quy tắc "Owned Buffers ONLY"
Để ngăn chặn lỗi ghi đè bộ nhớ khi một Cell bị unload đột ngột:
* **Quy tắc**: Một buffer mà thao tác async (DMA, ISR, service khác) còn ghi vào phải thuộc về
  một grant có chủ, không phải một tham chiếu mượn.
* **Giải pháp**: SDK trao quyền sở hữu buffer (`Box<[u8]>`/`Vec<u8>`, hoặc `GrantHandle`) cho
  thao tác và nhận lại khi thao tác kết thúc.

### Async Pinning Registry (Lá chắn Unload)
* **Cơ chế**: Khi một vùng nhớ tham gia Async/DMA, pin registry ghi owner, range và
  lifecycle (`kernel/src/memory/pin.rs`).
* **Bảo vệ**: Kernel không bao giờ trì hoãn cái chết của Cell. Khi vùng còn bị pin,
  `GrantFree`/`GrantUnregister` bị từ chối; nếu chủ chết, frame của vùng bị quarantine
  (`Quarantined(Pinned)`, vẫn mapped) cho tới khi driver xác nhận (`pin::acknowledge`), không bao
  giờ theo timer (`kernel/src/memory/pin.rs:1-35`; [Spec 02](02-memory.md) §3).

## 3. Hot-Swap & State Transfer
Một Cell dịch vụ đang chạy có thể được thay bằng một bản mới cùng service id. Việc này do
**Supervisor Cell** điều phối (`cells/services/supervisor/src/hotswap.rs`); kernel chỉ cung
cấp các primitive (pause, freeze, `SpawnReplacement`, state stash, `CommitHotswap`, `KillCell`).

* **Ai yêu cầu**: chỉ chương trình đã ký `hotswap` (`hotswap <service> <elf>`). Supervisor nhận
  đúng hai đích: `config` và `hotswap-demo` (Cell demo).
* **Quy trình**:
    1. Supervisor tạm dừng đăng ký của dịch vụ và gửi envelope Snapshot `[0xAC, 0xF0, swap_id]`
       tới Cell cũ.
    2. Cell cũ cất trạng thái vào **state stash của kernel** dưới khóa `hotswap_key(swap_id)`.
       Một dịch vụ chạy `ostd::service::serve` làm việc này qua `Handler::on_snapshot`; `serve`
       chỉ nhận envelope hot-swap từ provider `SUPERVISOR` đang sống và không bao giờ trả lời nó.
    3. Supervisor đóng băng Cell cũ; `SpawnReplacement` nạp ELF mới, kernel niêm phong stash cho
       đúng bản thay thế đó.
    4. Envelope Restore `[0xAC, 0xF1, khóa thập phân]` → bản mới đọc stash, áp dụng
       (`Handler::on_restore`), xóa slot và gọi `HotSwapReady`.
    5. `CommitHotswap` đóng ingress cũ, chuyển hàng đợi và công bố tid mới; Supervisor `KillCell`
       Cell cũ (lý do thoát `0xAAAA_AAAA`). Init thấy lý do này cùng một provider mới đang sống,
       nên theo dõi tid mới thay vì khởi động lại dịch vụ.
    6. Một bước thất bại trước commit: Supervisor giết bản mới và khôi phục Cell cũ.
* **Đã chứng minh**: chỉ cho `config`, bằng lane `config-hotswap-x86` (khóa ghi trước swap đọc lại
  được sau swap, `config` có tid mới, init không khởi động lại nó).
* **Không có**: tráo đổi con trỏ hàm (symbol re-linking), kernel tự pause/serialize Cell, hay bất
  kỳ yêu cầu nào buộc các dịch vụ khác (vfs, net, compositor, input…) hỗ trợ hot-swap; chúng được
  nâng cấp bằng restart.

## 4. Boot Optimization (Snapshot)

> **Trạng thái (2026-09-28; sửa 2026-11-16)**: warm boot qua snapshot **chưa được bật trên bất kỳ ảnh nào**.
> Cổng qualify là feature build `snapshot-qualified`, tắt trong mọi ảnh shipping. Từ Phase 09
> (2026-11-11), kernel không có feature này không biên dịch engine (`kernel/src/snapshot_engine.rs`)
> mà dùng `kernel/src/snapshot_disabled.rs` (`kernel/src/main.rs:88-99`): không giữ vùng scratch
> lúc boot, capture trả `SnapshotError::GateClosed`, boot không bao giờ restore và xoá header một ảnh
> cũ để lại. Builtin `snapshot` của shell từ chối trước khi chạm Supervisor (`snapshot: not available from the suite shell, requires a lifecycle owner`).
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

## 5. Tooling: `ostd` & build
* **`ostd`**: Thư viện chuẩn thay thế `std` trong Cell, cung cấp allocator (`heap`), async
  executor (`executor`), IPC/grant và client dịch vụ (`libs/ostd/src/`).
* **Multi-Arch Build**: Không có subcommand `cargo-Cellos`. Kernel và Cell được build bằng cargo
  với `--target` cho từng kiến trúc; kiến trúc nào có module kernel, target toolchain và lane là
  bảng sinh tự động [`docs/generated/arch-status.md`](../generated/arch-status.md). Code 32-bit
  (RV32) nằm ở `kernel/src/arch/legacy` và không được build.
