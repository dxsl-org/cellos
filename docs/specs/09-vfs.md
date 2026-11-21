# Cellos Architecture: VFS & Filesystems
**Version**: 0.6 (Mount-Table Layered Backends + phased `/srv` activation)
**Status**: Definitive
**Changed 2026-06-10**: Bỏ chiến lược Dual-VFS (viFS1 RedoxFS fork / viFS2 TFS — TFS upstream đã ngừng phát triển từ ~2018). Thay bằng mô hình MountTable một VFS service + nhiều backend cắm song song. Plan chi tiết: `.agents/260610-1202-vfs-mount-table-backends/`.
**Amended 2026-08-01**: D10 giữ RedoxFS-on-VirtIO làm G1 proof-of-function và tách riêng các gate G2 production theo ADR 0002.
**Amended 2026-11-16** (architecture remediation Phase 19): §1, §2, §3, §4, §5 và §6 được sửa cho khớp code.


## 1. VFS Contract (The Interface)

Có hai hợp đồng, và không cái nào là `Future` trên ranh giới Cell:

* **Giữa các Cell**: VFS là một Service Cell; App gọi nó bằng IPC message `VfsRequest`/
  `VfsResponse` (`libs/wire/src/ipc.rs`, lỗi ở `libs/wire/src/proto/vfs.rs`), qua directory/file
  handle (§2b, §2c). Không có trait object nào đi qua ranh giới Cell.
* **Trong VFS Cell**: mỗi backend cài trait `FsBackend` (`cells/services/vfs/src/backend.rs:85`), đồng
  bộ, mọi method trả `Result<_, FsError>`; `MountTable` giữ `Vec<Box<dyn FsBackend>>`
  (`cells/services/vfs/src/mount.rs:19-21`).

`abi::fs` (`libs/abi/src/fs.rs`) định nghĩa `ViFileSystem`/`ViFile` đồng bộ (`read(&mut [u8])`,
`write(&[u8])`, `seek`), cộng một `read_async` trả `BoxFuture` (`fs.rs:130-134`). Người dùng duy nhất
là kernel: BootFS `VIFS1` (`kernel/src/fs.rs:18`, `kernel/src/fs/fat.rs`) và file trong cap registry
(`kernel/src/cell/cap_registry.rs`). Trait không `#[repr(C)]`; chỉ struct dữ liệu như `Stat` mới là.


## 2. Mount-Table Layered Backends

Một VFS service duy nhất (`cells/services/vfs/`) sở hữu một **MountTable** (longest-prefix match, `mount.rs`). Mỗi backend phụ trách một prefix, **ngang hàng** chứ không xếp chồng. Backend mới cắm thêm qua `MountTable::add_backend(Box<dyn FsBackend>)` mà không đụng các backend hiện có.

```
VFS service (MountTable, longest-prefix match)
├── /bin, /etc   → BootFS  (initramfs — FAT16 nhúng kernel, read-only)
├── /tmp         → RamFS   (tmpfs, volatile, read-write)
├── /data        → littlefs (default-enabled persistent store)
├── /mnt/sd      → FAT32   (interop thẻ SD/PC)
└── /srv         → CellosFS Native (G1 functional; G2 production qualification pending)
```

### Vai trò từng backend

| Backend | Prefix | Vai trò | Giai đoạn |
|---|---|---|---|
| **BootFS** (kernel `VIFS1`, `kernel_fs.img` FAT16 nhúng) | `/bin`, `/etc` | Initramfs: giải bài toán con gà–quả trứng (kernel load được binary VFS service trước khi VFS tồn tại). Loader fallback tại `kernel/src/loader/early.rs` | Có rồi |
| **RamFS** | `/tmp` | tmpfs chuẩn — scratch space volatile | Có rồi |
| **FAT32** (crate `fatfs`, có LFN/VFAT) | `/mnt/sd` | Interop thẻ SD ≤32GB / boot partition RPi / trao đổi dữ liệu với PC. **Không journaling — không dùng làm persistent store chính cho robot** | Shipped |
| **littlefs** | `/data` | Default-enabled persistent store for config/log/model. Backend and block adapter are shipped; QEMU functional and power-loss suites pass. Repeated real-board power-cut qualification remains required before production robot claims | Shipped software path; hardware qualification pending |
| **CellosFS Native** (`libs/cellos-fs`) | `/srv` | CoW extent engine, dual superblock ring, packed inode, checksum; thay RedoxFS theo [ADR 0016](../decisions/0016-cellosfs-native-replaces-redoxfs.md) (ADR cũ [09b](09b-vfs-native-fs-adr.md) đã superseded, chính sách kích hoạt theo [ADR 0002](../decisions/0002-phased-srv-redoxfs-activation.md)). Mount hiện tại format volume mới hoặc mount volume sẵn có, và degrade về empty/false nếu P5 thiếu | G1 functional; G2 pending |
| **exFAT** | `/mnt/sd` (mở rộng) | Thẻ SDXC >32GB nguyên bản (xuất xưởng exFAT, `fatfs` không đọc được). `FatBackend::mount()` tự detect OEM-Name `"EXFAT   "` và log cảnh báo rõ thay vì lỗi cryptic. Full support chỉ khi có nhu cầu thật | Theo nhu cầu |

### Quyết định đã chốt (2026-06-10)

* ❌ **Dual-VFS viFS1/viFS2 bị loại bỏ**: TFS upstream đã chết; RedoxFS port (~10K+ LOC) quá lớn so với nhu cầu G1 (YAGNI).
* ⚠️ **Xung đột tên đã gỡ**: thuật ngữ "viFS1" trong spec cũ (= RedoxFS fork) bị bỏ; `VIFS1` trong kernel (`kernel/src/fs.rs`) từ nay hiểu là **BootFS/initramfs**.
* 🔧 **Tech debt đã trả**: VFS từng `include_bytes!` lại các ELF `/bin` (nhúng trùng lặp với `kernel_fs.img`). Nay backend BootFS đọc `/bin` qua syscall FD của kernel trên `VIFS1` (`cells/services/vfs/src/backend_bootfs.rs:1-13`).


## 2b. Directory capabilities (2026-07-31)

Quyết định và lý lẽ nằm ở [ADR 09c](09c-vfs-directory-capabilities-adr.md); phần này
chỉ ghi mô hình đang chạy.

Một cell không còn phải đặt câu hỏi "tôi có được đọc path này không". Nó cầm một
**directory handle** (`abi::dir_handles::ViDirHandle` — kiểu riêng, **không phải**
`CapId`) và gửi `(handle, tên)`. Vì `tên` không được chứa separator, VFS resolve ra
đúng một mức bên trong thư mục của handle: **không có path nào ngoài handle mà cell
diễn đạt được**, nên không còn gì để check.

| Op | Ý nghĩa |
|---|---|
| `OpenRootDir { path }` | Op **duy nhất** còn nhận path. Bootstrap: cell xin handle cho các thư mục nó cần, rồi `SealPaths`. Vẫn qua `AccessTable` |
| `OpenDir { dir, name }` | Dẫn xuất handle hẹp hơn. Revoke `dir` revoke luôn nó |
| `ReadAt` / `WriteAt` / `StatAt` / `UnlinkAt` `{ dir, name }` | Thao tác trên một entry trong `dir` |
| `ListAt { dir }` | Liệt kê chính `dir`. Không nhận `name` — muốn liệt kê thư mục con thì phải cầm handle của nó |
| `CloseDir { dir }` | Trả lại `dir` **và mọi handle dẫn xuất từ nó** |
| `SealPaths` | Một chiều. Từ đây mọi request path-addressed của cell đó bị `Err(3)` |

**Kiểm tra tên làm trên byte thô, trước mọi normalize** (`wire::dir_name`): rỗng, `.`,
`..`, mọi `/` hoặc `\`, byte điều khiển/NUL, quá dài, và UTF-8 không hợp lệ đều bị từ
chối. Không normalize trước — normalize chính là nơi sinh ra lỗ traversal.

**Vòng đời & thu hồi** (`cells/services/vfs/src/dirs/`): handle thuộc về
`(CellId, generation)`. Thấy generation cao hơn nghĩa là instance cũ đã chết → purge
toàn bộ handle của nó, nên **handle không sống qua hot-swap**. Thu hồi là **bắc cầu**:
mỗi entry ghi lại handle nó được dẫn xuất từ đâu, kể cả qua ranh giới spawn.

**Kế thừa lúc spawn**: kernel chỉ là người đưa thư — nó chép tập handle từ spawner
sang con và khai báo *ai* đã nêu tập đó (`QueryDirHandles`), chứ không diễn giải handle
nào. VFS mới là nơi kiểm: lần đầu tiếp xúc với một cell, VFS kéo bản khai của kernel và
chỉ bind nếu spawner **thực sự giữ đủ** mọi handle được nêu — **all-or-nothing**. Thiếu
một cái thì không bind cái nào. Hệ quả đã biết và chấp nhận: một yêu cầu quá rộng
**hỏng ở lần gọi filesystem đầu tiên của con, không phải lúc spawn**. Một cell có tập
kế thừa được seal ngay, kể cả khi bind thất bại — nếu không, thất bại sẽ *mở rộng*
quyền so với thành công.

`AccessTable` vẫn chạy trên path đã resolve, nhưng lùi về vai trò **defense in depth**:
handle quyết định *thư mục nào*, bảng vẫn nói *hệ thống cho phép gì ở đó* (một handle
tới mount read-only không được biến thành quyền ghi).

**Đường di trú**: các op path-string vẫn còn, chuyển từng cell một. Cell tiên phong là
`/bin/vfs-test`. Chừng nào chưa xoá hẳn các variant path-string, guarantee chỉ đúng với
cell đã seal.


## 2c. File handles (2026-08-09)

File handles are a VFS-owned, service-local capability. `ViVfsFileHandle` is a
distinct type, not a widened `CapId`, and the file-handle path uses the ordinary
attested message IPC path only.

| Op | Ý nghĩa |
|---|---|
| `OpenFileAt { dir, name }` | Resolve one entry under a directory handle and return `VfsResponse::FileHandle` |
| `ReadFileHandle { file, offset, max }` | Read from an issued file handle, bounded by `max` (`u32`) and the inline reply cap |
| `CloseFile { file }` | Close the issued file handle |

Semantics that matter for the ABI:

- The owner key includes both `CellId` and `generation`, so a respawned cell must
  re-acquire its file handles after hot-swap.
- `OpenFileAt` / `ReadFileHandle` / `CloseFile` are re-authorized on every
  attested request; stale or wrong-owner handles fail closed.
- `CloseFile` and parent-dir revocation both remove the handle from the VFS table;
  generation purge does the same when a newer instance replaces the old one.
- The file-handle table is not serialized across hot-swap. Open handles are
  session-scoped and are intentionally re-acquired.
- Replies stay inline and bounded: this path uses the 4000-byte `Data` cap
  (`IPC_BUF_SIZE - 96`) rather than any grant-backed or pointer-based transfer.
- No async, grant, or raw-pointer lifetime is introduced by this phase.


## 3. Đường dữ liệu: copy và grant
Dữ liệu file đi qua ba bước, mỗi bước là một copy:

1. **Block driver -> VFS**: mỗi sector đi qua `blk_router` tới block Driver Cell bằng một exact
   operation có hạn (§6) và vào sector cache của VFS (`cells/services/vfs/src/block_stream.rs`,
   `page_cache.rs`).
2. **VFS -> App, inline**: reply ngắn nằm trong IPC message, tối đa 4000 byte `Data` (§2c).
3. **VFS -> App, grant**: với `ReadHandleGrant`/`ReadGrant`, App chia sẻ một grant nó sở hữu cho
   VFS; VFS copy byte vào đó qua `sys_grant_copy_from_slice`, tối đa `VFS_GRANT_CHUNK` = 64 KiB mỗi
   lần (`libs/wire/src/ipc.rs:26-33`). Trong lúc copy, VFS giữ một lease trên grant: `GrantFree`/
   `GrantUnregister` của chủ bị từ chối và chủ chết thì frame bị quarantine
   (`cells/services/vfs/src/grant_read.rs:1-13`). Ghi theo chiều ngược lại (`WriteHandleGrant`).

Quyền sở hữu buffer không chuyển sang VFS hay Driver Cell, và Driver không DMA vào buffer của App.
Không có ngoại lệ con trỏ: `GetFile`/`DataPtr` cũ (con trỏ thô vào heap của VFS) nay là tombstone
trả `Err(6)` (Spec 17 §9, 2026-11-21). Đọc cả file theo path dùng `ReadFileGrant`: VFS copy
`min(file_len, max, grant_len)` byte vào grant App sở hữu (`cells/services/vfs/src/grant_read.rs`).


## 4. Page Cache

Cache là sector cache LRU nằm hoàn toàn trong VFS Cell, một chủ, không lock dùng chung; ngân sách
là một phần tư heap của Cell (`cells/services/vfs/src/page_cache.rs:1-30`). Không có page cache
dùng chung trong SAS, và không backend nào trả con trỏ metadata cho App.

* **LRU & OOM**: VFS Cell owns page-cache replacement/eviction policy. Kernel supplies
  quota and reclaim mechanisms but does not choose filesystem eviction policy.


## 5. Large File Support (LFS)
Offset trên wire và trong `FsBackend` là `u64` (`read_at`/`write_at`, `backend.rs`). Code 32-bit
(RV32) không được build; kiến trúc nào có lane là bảng
[`docs/generated/arch-status.md`](../generated/arch-status.md).


## 6. Fault Isolation (Luật 4 & Panic Recovery)
Mỗi FS Cell được bao bọc bởi cơ chế an toàn của Kernel:

* **Isolation**: Mọi backend chạy trong một VFS Cell; backend panic làm chết cả VFS Cell, nên mọi
  App đang dùng VFS đều thấy lỗi cho tới khi VFS được khởi động lại.
* **Recovery**: Kernel không reload Cell. Init khởi động lại VFS theo cấu hình boot; cache và mọi
  handle của instance cũ mất theo nó (handle gắn với `(CellId, generation)`, §2b, §2c).
* **Bounded block driver (Phase 12, 2026-11-13)**: mỗi request tới block Driver Cell là một exact operation có hạn `BLOCK_REPLY_TICKS = 3_500` (35 s = budget 30 s của driver + 5 s). Driver đã nhận mà không trả lời → `Indeterminate` (write không bao giờ replay); chưa nhận → `NotIssued`. Sau một lần hết hạn, device bị đánh dấu *stalled* và mọi request tới nó bị từ chối ngay cho đến khi route trỏ sang provider khác, nên một driver bị treo chỉ tốn đúng một hạn. Witness: `a_frozen_block_driver_does_not_hang_vfs_x86` (`tests/integration/tests/nvme-x86.rs`, fault hook `F12-FREEZE`).
* **Heartbeat**: VFS chạy trên `ostd::service::serve` với heartbeat 60 s (`liveness::VFS_HEARTBEAT_TICKS`), re-arm mỗi vòng và trước mỗi block request; VFS treo thật (lock cycle, chờ vô hạn) bị kernel kill và init restart.
* **Typed backend errors**: mọi method của `FsBackend` trả `Result<_, FsError>`; lỗi thiết bị giữa chừng một FAT read là `FsError::Io`, không còn là short read trông như thành công. Wire byte không đổi (`Unsupported` → 6, còn lại → 1).
* **Trust store**: SSH known-hosts không còn nằm trong VFS; kms giữ nó tại `/srv/cellos/kms/ssh_known_hosts`. `VfsRequest::Trust*` là tombstone trả `Err(6)` (Spec 17 §9, 2026-11-13).
