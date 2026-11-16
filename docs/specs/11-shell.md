# Incremental Native Shell (Shell bản địa tăng trưởng)

**Status**: Historical design sketch, not a normative specification. Current-source
behavior and [Spec 09](09-vfs.md) remain relevant to existing code. The accepted
next shell/tooling direction is [ADR-0028](../decisions/0028-cellbox-cli-first-structured-pipelines-and-tui.md)
and the [Cellbox CLI design](../cellbox-cli.md); its
[implementation plan](../../.agents/261020-1750-cellbox-cli/plan.md) is closed at its stated
ceilings with deferred items. The shell is now Cellbox at `cells/tools/cellbox` (`5043e2cf59`).
The old tiny-shell recommendations below do not override that accepted design.

**Code check (2026-11-16):** the "no copy" claims in this sketch (§1 borrowing from the VFS
page cache, §3 zero-copy loading, the `Box<[u8]>` borrow in strategy item 4, and the `ls`
flow) do not match the code. Shell, VFS and drivers are separate Cells with separate heaps;
data crosses them as kernel-copied IPC messages (Spec 00 Law 2, Spec 17). §1, §3, item 4
and the `ls` flow carry dated notes on what the code does; the rest is the historical sketch.


### 1. Giai đoạn 1: Built-in Commands (Tier 1 Cell)

> **Historical (2026-11-16).** Shell hiện là Cellbox (`cells/tools/cellbox`): một Tier 1 Cell (manifest LEGACY, `scripts/unsafe-allowlist.toml` entry `app-cellbox`) là một file ELF riêng, được `init` khởi chạy (`cells/tools/init/src/pin_gate.rs:22`), không phải code nạp thẳng vào kernel. Nó có bộ lệnh multicall lớn hơn nhiều so với 4 lệnh dưới đây (`cells/tools/cellbox/src/executor.rs`). `cat` **không** mượn Page Cache của VFS: dữ liệu đến qua reply IPC hoặc `sys_read_cap` (`libs/ostd/src/fs.rs:110-111`), cả hai đều do kernel copy vào buffer của Cellbox (Spec 00 Luật 2).

Mày đừng coi Shell là một file ELF nằm trên đĩa vội. Hãy để nó là một **Tier 1 Native Cell** được nạp thẳng vào SAS.

* **Lệnh cơ bản**: Chỉ cần làm 4 lệnh: `help`, `ls`, `cat`, và `clear`.
* **Tại sao**:
* `ls` và `cat` sẽ kiểm tra xem tầng **VFS Dispatcher** và **viRamDisk** có thực sự thông chưa.
* Nó giúp mày thực hiện đúng **Luật 8 (Borrowing)**: Shell khi nhận dữ liệu từ `cat` không cần copy vào buffer riêng, mà chỉ cần mượn (`borrow`) trực tiếp từ Page Cache của VFS trong SAS.


### 2. Giai đoạn 2: Syscall Stress Test

Biến Shell thành công cụ "hành hạ" Trap Handler mày vừa viết.

* **Tận dụng `ecall**`: Mọi lệnh trong Shell phải đi qua Syscall. Ví dụ: Thay vì gọi hàm `print` của Kernel, Shell phải gọi `sys_log` để kiểm tra xem `stvec` và `sepc` có hoạt động chuẩn không.
* **Async Shell**: Áp dụng cái `ViAsyncFileSystem` mày vừa tút lại. Shell sẽ không bị treo (block) khi đợi đọc RAM Disk, giúp mày test luôn con **Async Executor** sau này.


### 3. Giai đoạn 3: External ELF Execution (The Real Test)

> **Cập nhật 2026-11-16 theo code.** Cellbox chạy chương trình ngoài bằng `sys_spawn_from_path` (`cells/tools/cellbox/src/executor.rs:2902-2932`, `libs/ostd/src/syscall.rs:576`), không qua `ViVmRuntime` — trait đó (`libs/abi/src/vm.rs:99`) là giao diện VM Tier 3 và chỉ có implementor trong test. Việc nạp **có copy**: sau khi VFS lên, ELF được đọc từ VFS vào một Grant (`libs/ostd/src/syscall.rs:577-584`), rồi loader copy từng segment `PT_LOAD` vào frame mới (`kernel/src/loader/elf.rs:369-373`); không ánh xạ trực tiếp file từ RAM Disk.

Khi cái Shell nội bộ đã ổn, lúc đó mới làm tính năng nạp file ELF từ `/bin/`.

* ~~**Sức mạnh của `Vi` prefix**: Lúc này Shell sẽ gọi `ViVmRuntime` để yêu cầu Kernel tạo một không gian thực thi mới cho ứng dụng.~~
* ~~**Zero-copy Loading**: Nhờ SAS, Shell chỉ cần gửi đường dẫn file, Kernel (Loader) sẽ ánh xạ (map) trực tiếp file đó từ RAM Disk vào vùng thực thi mà không cần copy dữ liệu.~~


### 🛠️ Chiến thuật cho dev và Agent (Áp dụng các Luật)

1. ~~**Luật 5 (No `mod.rs`)**: Shell phải nằm trong `cells\apps\shell`.~~ *(Cập nhật 2026-11-16: không có lệnh cấm `mod.rs` — git track 41 file `mod.rs` của dự án (ngoài `third_party/`) và không có CI lint, clippy lint hay script nào kiểm tra; module mới nên dùng `foo.rs` + `foo/` nhưng đó chỉ là guideline ([code-standards Law 5](../code-standards.md), Spec 00 Luật 5). Shell hiện là Cellbox tại `cells/tools/cellbox`.)*
2. **Luật 6 (Naming)**: Mọi interface giao tiếp với Kernel phải dùng tiền tố `Vi` (ví dụ: `ViSyscall`).
3. **Luật 8 (Resource)**: Cấm dùng `String` hay `Vec` vô tội vạ cho việc parse lệnh. Hãy dùng `&str` và split bằng iterator để giảm áp lực cho Heap của robot nano.
4. ~~**Luật 8 (Borrowing)**: Khi Shell thực hiện lệnh cat, nó sẽ nhận về một Box<[u8]> từ VFS. Theo Luật 8, Shell nên mượn (borrow) dữ liệu này để in ra UART thay vì copy sang một buffer trung gian, tận dụng tối đa lợi thế của Single Address Space (SAS).~~ *(Historical 2026-11-16: một `Box` không thể đi từ VFS sang Cellbox vì mỗi Cell có heap riêng; Cellbox nhận byte do kernel copy vào buffer của chính nó, xem §1 và Spec 00 Luật 2.)*


### Luồng chạy của lệnh `ls` trong Shell (Kết nối 3 khu vực)

*Cập nhật 2026-11-16 theo code.* Khi mày gõ lệnh `ls` trong Cellbox (`cells/tools/cellbox`), dữ liệu **được copy** qua ranh giới Cell; không có đường nào "không cần `memcpy`":

1. **App Cell (`cellbox`)**: `ls_collect` (`cells/tools/cellbox/src/commands.rs:167`) gửi `VfsRequest::ListDir` (`libs/wire/src/ipc.rs`) tới VFS service qua IPC (`vfs_list_dir_details`, `cells/tools/cellbox/src/cmd_fs.rs:969`). Kernel copy request vào recv buffer của VFS (≤ `IPC_BUF_SIZE` = 4096, `libs/abi/src/kernel_ipc.rs:8`).
2. **Service Cell (`vfs`)**: tra MountTable ([Spec 09](09-vfs.md)), đọc thư mục từ backend, rồi trả reply; kernel lại copy reply vào buffer reply 512 byte trên stack của `cellbox` (`cmd_fs.rs:974-975`), nên thư mục lớn bị cắt bớt (`cmd_fs.rs:907-908`).
3. **Kernel BootFS (VIFS1)**: với `/` và các đường dẫn fallback, `cellbox` còn đọc BootFS qua `ostd::fs::read_dir` (`libs/ostd/src/fs.rs:45`); mỗi `sys_read_dir` copy một `DirEntry` vào buffer trên stack của caller.
4. **Kết quả**: `cellbox` gộp, loại trùng, sắp xếp danh sách trên heap của chính nó rồi in ra console.


### Bộ Dispatcher "Zero-copy" (Tuân thủ Luật 8)

Điểm mấu chốt ở đây là **KHÔNG** tạo thêm `String` hay `Vec<String>`. Chúng ta sẽ "mượn" trực tiếp từ buffer nhập vào.

```rust
// user/src/shell.rs
use crate::api::ViResult; // Luật 6

pub struct ViShell<'a> {
    prompt: &'a str, // Luật 8: Dùng lifetime mượn thay vì sở hữu
}

impl<'a> ViShell<'a> {
    pub fn dispatch(&self, line: &'a str) -> ViResult<()> {
        // Luật 8: Sử dụng Iterator và &str slice để parse, không cấp phát Heap
        let mut parts = line.trim().split_whitespace();
        let cmd = parts.next().ok_or(ViHalError::InvalidInput)?;

        match cmd {
            "ls" => self.cmd_ls(parts),
            "cat" => self.cmd_cat(parts),
            "help" => self.cmd_help(),
            _ => {
                println!("Cellos: command not found: {}", cmd);
                Ok(())
            }
        }
    }
}
```


### Tận dụng VFS qua Syscall (Tuân thủ Luật 7)

Mày hãy nhìn cách lệnh `ls` tận dụng cái **Trait Object** mày vừa xây dựng:

```rust
// user/src/shell/commands.rs
impl<'a> ViShell<'a> {
    fn cmd_ls(&self, mut args: core::str::SplitWhitespace<'a>) -> ViResult<()> {
        let path = args.next().unwrap_or("/");
        
        // Gọi Syscall mở thư mục - trả về một Trait Object
        // Luật 7: Box<dyn ViFile + Send + Sync>
        let dir = crate::fs::open(path)?; 
        
        // Giả sử backend lịch sử được thay bằng MountTable hiện hành của Spec 09
        for entry in dir.read_dir()? {
            println!("{}", entry.name());
        }
        Ok(())
    }
}
```


### Tại sao cấu trúc này lại "đáng đồng tiền bát gạo"?

1. **Hiệu suất Robot Nano**: Nhờ **Luật 8 (Borrowing)**, việc mày parse một câu lệnh dài dằng dặc cũng không tốn thêm một byte nào trên Global Heap. Điều này cực kỳ quan trọng khi Cellos chạy trên các thiết bị có RAM giới hạn.
2. **Tính linh hoạt của Cell**: Vì mày dùng **Luật 7 (Trait Objects)**, cái Shell này không cần biết nó đang `ls` trên backend nào. Trong policy hiện hành, việc chọn backend được Spec 09 MountTable quyết định; `viFS1`/`viFS2` chỉ còn là tên thiết kế đã retired.
3. **An toàn tuyệt đối (LBI)**: Nếu Shell bị lỗi (ví dụ: parse sai địa chỉ), cơ chế **Panic Recovery** của Cellos sẽ chỉ làm sập cái Shell Cell đó. Kernel bắt được trap, hồi sinh Shell, và mày lại thấy dấu nhắc lệnh `Cellos >` mà không phải reboot cả hệ thống.


### 🏁 Lời khuyên "Chốt hạ":
**"Code từ từ nhưng phải Native"**.

* **Bước 1**: Làm một cái vòng lặp `loop { print!("Cellos > "); input(); dispatch(); }` đơn giản nhất.
* **Bước 2**: Thực thi các lệnh bằng cách gọi trực tiếp vào `ViFileSystem` trait qua syscall.

Đừng copy `Busybox` hay `Rush` ngay lúc này, vì mày sẽ mất cả tuần để sửa lỗi tương thích. Tự code một cái Shell tí hon theo đúng "vibe" của Cellos sẽ giúp mày hiểu sâu hơn về cái "long mạch" Trap/Syscall mày vừa khai thông.
