# 🛸 Cellos - FORK & REFERENCE STRATEGY

**Status**: Non-normative reference strategy. Binding subsystem architecture lives in
the owning numbered specs; for filesystems, Spec 09 supersedes §6 below.

File này định hướng việc tái sử dụng mã nguồn và ý tưởng từ các dự án Open Source để tối ưu hóa thời gian phát triển Cellos.

Mã nguồn đã tải về thư mục .reference/, nếu không tìm thấy thì tải từ github.


## 1. Tầng HAL & Drivers (The Hands)

*Mục tiêu: Chạy được trên phần cứng càng nhanh càng tốt.*

| Thành phần | Nguồn tham khảo | Hành động (Action) | Ghi chú |
| --- | --- | --- | --- |
| **VirtIO Drivers** | `virtio-drivers` crate | **Copy & Wrap** | Sử dụng crate này làm nền tảng cho `virtio_blk`, `virtio_gpu`. |
| **UART 16550** | `blog_os` / `Redox` | **Copy** | Logic ghi/đọc UART gần như là chuẩn chung, bê về và đóng gói vào Spinlock. |
| **RISC-V SBI** | `rustsbi` | **Refer** | Tham khảo cách gọi SBI call để hoàn thiện `hal/sbi.rs`. |


## 2. Memory & SAS (The Field)

*Mục tiêu: Xây dựng cánh đồng SAS an toàn mà không tốn công debug bảng trang.*

| Thành phần | Nguồn tham khảo | Hành động (Action) | Ghi chú |
| --- | --- | --- | --- |
| **Page Table Walk** | `TheseusOS` / `Redox` | **Fork** | Bê logic duyệt cây thư mục bảng trang, nhưng phải sửa để hỗ trợ cả SV32 (RV32) và SV39 (RV64). |
| **Heap Allocator** | `linked_list_allocator` | **Copy** | Dùng luôn làm Kernel Heap để có `Box` và `Vec` nhanh chóng. |
| **LBI Logic** | `TheseusOS` | **Refer** | Học cách Theseus ép kiểu an toàn để thực hiện cách ly bộ nhớ mà không cần MMU. |


## 3. Task & IPC (The Heart)

*Mục tiêu: Hiện thực hóa cơ chế giao tiếp siêu tốc kiểu Hubris.*

| Thành phần | Nguồn tham khảo | Hành động (Action) | Ghi chú |
| --- | --- | --- | --- |
| **IPC ABI** | `Hubris` (Oxide) | **Fork** | Chép bộ Verb `Send/Recv/Reply`. Sửa lại để truyền địa chỉ trong SAS thay vì copy qua lại. |
| **Grant System** | `TockOS` | **Refer** | Tham khảo cách Tock quản lý quyền truy cập bộ nhớ của Kernel cho App (Grants) để hoàn thiện `task/tcb.rs`. |
| **Context Switch** | `Redox` / `xv6-riscv` | **Fork** | Bê bộ assembly lưu/phục hồi thanh ghi, nhưng phải tạo 2 bản cho RV32 và RV64. |


## 4. Loader & Cells (The Soul)

*Mục tiêu: Nạp linh hồn cho Cell và vá địa chỉ chuẩn SAS.*

| Thành phần | Nguồn tham khảo | Hành động (Action) | Ghi chú |
| --- | --- | --- | --- |
| **ELF Parsing** | `xmas-elf` crate | **Copy** | Dùng thư viện này để đọc cấu trúc ELF, đừng tự parse bằng tay tốn thời gian. |
| **Relocation** | `TheseusOS` | **Fork** | ĐÂY LÀ PHẦN QUAN TRỌNG. Phải học cách Theseus vá địa chỉ để nạp nhiều Cell vào cùng một SAS. |


## 5. Networking (The Voice)

*Mục tiêu: Có mạng để Jarvis có thể giao tiếp với bên ngoài mà không cần viết driver TCP/IP phức tạp.*

| Thành phần | Nguồn tham khảo | Hành động (Action) | Ghi chú |
| --- | --- | --- | --- |
| **TCP/IP Stack** | `smoltcp` crate | **Copy & Wrap** | Đây là bộ stack mạng viết bằng Rust "thuần khiết", không dùng `std`, cực kỳ hợp với Cellos. |
| **Network Cell** | `TheseusOS` | **Refer** | Tham khảo cách Theseus tách Network thành một Cell độc lập để xử lý song song. |


## 6. Filesystem Evolution (The Data Field)

*Mục tiêu: Đa dạng hóa khả năng lưu trữ từ thẻ nhớ nhỏ đến các "cánh đồng" dữ liệu SAS.*

> Historical note: the active filesystem architecture now lives in Spec 09 (MountTable).
> This section preserves the retired fork/reference strategy only; uppercase `VIFS1`
> remains the valid kernel BootFS/initramfs name.

| Thành phần | Nguồn tham khảo | Hành động (Action) | Ghi chú |
| --- | --- | --- | --- |
| **FAT32** | `fatfs` crate | **Copy & Wrap** | Dùng cho các phân vùng boot nhỏ, thẻ nhớ đời cũ. |
| **exFAT** | `exfat-rs` / `vfat` | **Fork & Fix** | Bắt buộc phải có để đọc thẻ nhớ dung lượng lớn (>32GB) trên các robot nano. |
| **viFS1 (Classic)** | Retired design name; see Spec 09 MountTable. | POSIX-compliant, dựa trên Node và Path truyền thống. | Historical only; không còn là policy hiện hành. |
| **viFS2 (Modern)** | Retired design name; see Spec 09 MountTable. | **B-tree based**, tối ưu tìm kiếm, hỗ trợ snapshot và ghi đè an toàn. | Historical only; không còn là policy hiện hành. |
| **SAS-based Global Page Cache** | `TheseusOS` | **Refer** | Cách Theseus tận dụng SAS để cache dữ liệu từ đĩa mà không cần copy qua lại giữa các tiến trình. |


## 7. Async Runtime (The Pulse)

*Mục tiêu: Chạy hàng nghìn Task/Cell mà không tốn RAM cho Context Switch.*

| Thành phần | Nguồn tham khảo | Hành động (Action) | Ghi chú |
| --- | --- | --- | --- |
| **Waker/Executor** | `embassy` / `async-task` | **Fork** | Cellos dùng Async Rust nên cần một bộ Executor cực nhẹ. Bê logic từ Embassy (dành cho hệ nhúng) về là tối ưu nhất. |
| **Inter-cell Waker** | `TheseusOS` | **Refer** | Cách đánh thức một Task ở Cell B khi Cell A đã hoàn thành công việc trong môi trường SAS. |


## 🛠️ STRATEGY

### A. Quy tắc "Library-First"

Bất cứ khi nào có thể, hãy ưu tiên dùng các **no_std crates** có sẵn trên `crates.io` (như `smoltcp`, `fatfs`, `xmas-elf`). Đừng bắt lão dev viết lại logic nghiệp vụ, hãy để lão tập trung vào việc **"Vá" (Link)** các thư viện đó vào SAS.

### B. Bẫy "Địa chỉ tuyệt đối"

Khi copy code HAL từ các dự án khác, hãy cảnh giác với các địa chỉ MMIO được hardcode (như `0x10000000`).

* **Cellos Rule**: Mọi địa chỉ cứng phải được đưa vào file cấu hình của từng thiết bị hoặc lấy từ Device Tree để đảm bảo Multi-Arch.

### C. Cơ chế "Panic Guard"

Khi mượn code từ OS dùng `panic!` nhiều, chuyển failure có thể phục hồi sang `ViResult`
ở boundary thích hợp. Panic còn lại abort Cell và đi qua terminate → reap → notify →
supervisor; không dùng `catch_unwind` trong no_std abort-on-panic builds.

### D. ⚠️ DO'S & DON'TS

1. **CẤM COPY `mod.rs**`: Thấy dự án nào dùng `mod.rs`, khi bê về Cellos phải refactor ngay sang **Modern Style (Luật 5)**.
2. **KIỂM TRA `unsafe**`: Các đoạn code copy từ Redox hay Theseus có nhiều `unsafe`. Phải bọc lại bằng Interface an toàn của Cellos và giải trình rõ.
3. **HÓA THÂN MULTI-ARCH**: Mọi đoạn code copy liên quan đến `u64` hoặc địa chỉ cứng phải được chuyển sang `VAddr`/`usize` để chạy được cả RV32.
