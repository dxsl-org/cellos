# Phase 06 — Directory capability thay path string

## Context Links

- Plan: [plan.md](plan.md) · Phụ thuộc: [phase-02](phase-02-vfs-read-gating.md)
- Spec: `docs/specs/09-vfs.md`, `docs/specs/17-ipc-wire-contract.md`
- Midori nguồn: **không có global namespace**. App không mở được `/etc/passwd` vì không có
  filesystem toàn cục để mở — nó chỉ có những handle được truyền vào

## Overview

- **Ưu tiên**: P3 — thay đổi API sâu nhất trong plan; làm sau khi phase 02 đã chứng minh mô hình ACL
- **Trạng thái**: Planned — **cần 2× confirmation (Law 1: `VfsRequest` nằm trong libs/api)**
- **Mô tả**: Chuyển VFS từ "cell gửi path string, VFS kiểm tra path đó có được phép không" sang
  "cell gửi `(dir_handle, tên tương đối)`, VFS chỉ resolve trong phạm vi handle nó đã phát". Đây là
  bước biến ACL (kiểm tra) thành capability (không có gì để kiểm tra vì không thể diễn đạt path
  ngoài phạm vi).

## Key Insights

- Khác biệt bản chất giữa phase 02 và phase 06: ACL trả lời "cell này có được đọc path kia không";
  capability làm cho **path kia không tồn tại trong từ vựng của cell**. Midori chọn cái sau vì cái
  trước luôn có lỗ (confused deputy, path traversal, TOCTOU giữa check và use).
- Trong SAS chi phí rất thấp: một handle = một index vào bảng per-cell.
- **Hạ tầng handle hiện có KHÔNG dùng lại được nguyên trạng** (red-team correction — draft claim
  "Cellos đã có đúng hạ tầng này" là sai): `HandleEntry` có field `owner` nhưng doc ghi *"for quota
  accounting"* ([handle_table.rs:12-14](../../cells/services/vfs/src/handle_table.rs#L12-L14)) và
  `get_mut(cap)` **không so** `entry.owner` ([handle_table.rs:54-56](../../cells/services/vfs/src/handle_table.rs#L54-L56));
  `PendingTable` không có owner nào cả ([pending.rs:24-27](../../cells/services/vfs/src/pending.rs#L24-L27)).
  Doc comment "Per-cell open file handle table" ([handle_table.rs:2](../../cells/services/vfs/src/handle_table.rs#L2))
  là sai. **Owner-check là prerequisite từ [phase 02](phase-02-vfs-read-gating.md) bước 3** — xây
  capability model lên một bảng handle đọc chéo được là vô nghĩa.
- `GetFile` trả `DataPtr { ptr, len }` — con trỏ thô ([dispatch.rs:26-35](../../cells/services/vfs/src/dispatch.rs#L26-L35)).
  Trong SAS, một khi đã phát thì **không thu hồi được**: capability model không hoàn chỉnh nếu
  `GetFile` còn tồn tại ở dạng này. Chuyển sang grant có thời hạn là một phần của phase.
- Mô hình tương đương đã được kiểm chứng: Fuchsia namespace (mỗi component nhận một namespace do
  parent dựng), Capsicum `openat`-only, Plan 9 per-process namespace.
- Kế thừa: khi cell A spawn cell B, A truyền cho B tập handle con của handle mình có → monotonic
  downgrade cho *filesystem*, đúng cùng nguyên lý mà `CapSet` đã áp cho device
  ([loader.rs:263-290](../../kernel/src/loader.rs#L263-L290)). Tính đối xứng này là lý lẽ mạnh nhất
  cho phase: Cellos đã có mô hình đúng cho thiết bị, chỉ chưa áp cho file.

## Requirements

**Functional**

1. `ViDirHandle` — handle trỏ tới một thư mục, do VFS phát, lưu trong bảng per-cell của VFS.
2. Op mới nhận `(handle, name)` với `name` **không được chứa** `..`, `/` dẫn ra ngoài, hay path
   tuyệt đối. VFS resolve tương đối handle; không thể diễn đạt path ngoài phạm vi.
3. Handle gốc được cấp lúc spawn từ một namespace spec (nguồn: bảng static như phase 02, hoặc
   POLICY.BIN mở rộng).
4. `sys`-level: handle có thể truyền cho cell con lúc spawn, chỉ được thu hẹp (subdirectory), không
   mở rộng.
5. `GetFile` chuyển sang grant có thời hạn hoặc bị loại bỏ; con trỏ thô vĩnh viễn không tương thích
   với capability model.
6. Đường di trú: giữ op path-string cũ song song — **không big-bang**. Cell chưa chuyển vẫn chạy,
   chịu ACL của phase 02.
7. **Per-cell handle-only flag** (red-team): bảng handle mang một cờ, đặt khi cell đã được di trú;
   `dispatch.rs` **từ chối** op path-string từ cell có cờ, ngay ở entry. Không có cờ này thì
   "cell không thể diễn đạt path ngoài handle" là sai với **mọi** cell suốt cả quá trình migration
   và không có bước nào làm nó thành đúng — deprecation warning không phải enforcement.
8. **Xoá** (không phải deprecate) các variant path-string là step cuối tường minh, với ABI
   confirmation riêng.

**Non-functional**

7. Không tăng số IPC round-trip cho op thường (handle đã có sẵn ở client).
8. Tuân `docs/specs/17-ipc-wire-contract.md`: framing, recv-mask, discriminant phải theo contract —
   thêm variant vào `VfsRequest` là đổi discriminant, phải kiểm tra kỹ (đã có tiền lệ lỗi:
   `project-input-ipc-protocol-collision`).
9. `IPC_BUF_SIZE` 512 byte: `(handle, name)` nhỏ hơn path tuyệt đối → có lợi, nhưng phải xác nhận
   postcard envelope vẫn khớp (đã có tiền lệ: cap 480 byte tại
   [dispatch.rs:148-152](../../cells/services/vfs/src/dispatch.rs#L148-L152)).

## Architecture

```
Trước:  cell ──VfsRequest::Write{ path: "/srv/x", .. }──► VFS ──can_write(cell, path)?──► backend
        (cell diễn đạt được MỌI path; an toàn phụ thuộc vào việc check đúng)

Sau:    spawn: VFS phát handle H = "/srv/app-a"  ──►  cell chỉ giữ H
        cell ──VfsRequest::WriteAt{ dir: H, name: "x", .. }──► VFS ──resolve(H) + reject("..")──► backend
        (cell KHÔNG diễn đạt được path ngoài H; không còn gì để check)
```

Bảng handle: `HashMap<(CellId, HandleId), ResolvedDir>` trong `VfsManager`. Vòng đời: handle chết khi
cell chết (đã có pattern reaper cho grant — `project-large-buffer-ipc`). Handle phát cho cell con lúc
spawn phải là entry mới, không share entry của cha (để revoke cha không revoke con ngoài ý muốn — hoặc
ngược lại nếu đó là ngữ nghĩa mong muốn; **cần chốt bằng ADR trước khi code**).

Điểm phải chốt trước khi hiện thực (đây là lý do phase này là P3, không phải P1):
1. Ngữ nghĩa revoke: revoke handle cha có revoke handle con không?
2. Handle có bền vững qua hot-swap của cell không (`StateTransfer`)?
3. `ViDirHandle` là `api::cap::CapId` mở rộng hay type mới?
4. **Handle set của con được authenticate thế nào lúc spawn?** (red-team) Req 4 đòi kế thừa
   thu-hẹp-only, nhưng kernel **không phơi lineage**: `Task` không có field parent/spawner
   (`kernel/src/task/tcb.rs`), `ProcessInfoV2` không có parent id
   ([syscall.rs:777-786](../../libs/api/src/abi/syscall.rs#L777-L786)), và `SpawnFromPath` ABI chỉ có
   `a0 = path_ptr, a1 = path_len` ([syscall.rs:31-33](../../libs/api/src/abi/syscall.rs#L31-L33)) —
   không có carrier để truyền handle. Nên VFS chỉ có thể **tin lời con** rằng "cha cho tôi H", đúng
   lỗ confused-deputy mà phase này tồn tại để đóng. Ba lựa chọn: (a) kernel-mediated transfer (thêm
   ABI → Law 1 thứ hai); (b) VFS phát một **sealed token** cha truyền cho con làm argument; (c) bỏ
   req 4, đổi thành "con xin handle con-thư-mục, cha grant qua IPC khi cả hai còn sống".
   **Không code trước khi chốt điểm này** — nó quyết định phase có cần Law 1 thứ hai hay không.

## Related Code Files

| File | Hành động |
|------|-----------|
| `libs/api/src/abi/...` (VfsRequest/ipc) | Modify — **Law 1, cần 2× confirmation** |
| `libs/api/src/abi/syscall.rs` + `kernel/src/task/tcb.rs` | Modify **CHỈ NẾU** ADR item 4 chọn phương án (a) kernel-mediated transfer — **Law 1 thứ hai**. Liệt kê ở đây để chi phí ABI hiện ra trước khi xin confirmation, không phát hiện giữa lúc code |
| `cells/services/vfs/src/dispatch.rs` | Modify — arm mới cho op theo handle |
| `cells/services/vfs/src/manager.rs` | Modify — bảng handle per-cell + vòng đời |
| `cells/services/vfs/src/handles.rs` (nếu có) hoặc mới | Modify/Create |
| `libs/ostd/src/...` (VFS client API) | Modify — `ctx.vfs()` phơi API theo handle |
| `docs/specs/09-vfs.md` | Modify — ghi mô hình mới |
| `docs/specs/17-ipc-wire-contract.md` | Modify — discriminant mới |
| ADR mới | Create — ngữ nghĩa revoke/kế thừa handle |

## Implementation Steps

0. **Prerequisite**: owner-check hai bảng handle đã land (phase 02 bước 3). Không bắt đầu trước đó.
1. **Viết ADR trước**: chốt 4 điểm ở Architecture (revoke · hot-swap · type · **authenticate handle
   set lúc spawn**). Không code trước khi có ADR.
2. Xin 2× confirmation cho thay đổi `VfsRequest` (Law 1), kèm ADR làm cơ sở. Nếu ADR item 4 chọn
   phương án (a) thì xin **cả hai** ABI change trong cùng một lần, không chia nhỏ để lách.
3. Hiện thực bảng handle trong `VfsManager` + per-cell handle-only flag + `Drop`/reaper theo cell
   death (Law 8).
4. Thêm op theo handle **song song** op cũ: `OpenDir`, `ReadAt`, `WriteAt`, `StatAt`, `ListAt`,
   `UnlinkAt`. Reject mọi `name` chứa `..`, `/`, hoặc rỗng — test riêng cho từng dạng.
5. Cấp handle gốc lúc spawn từ namespace spec.
6. Chuyển 1 cell tiên phong (đề xuất: một demo cell nhỏ, không phải shell) sang API handle, **đặt cờ
   handle-only cho nó**, và khẳng định nó nhận `Err(3)` khi gửi `Write { path }`. Đây là bằng chứng
   duy nhất cho thấy guarantee đã đạt — test traversal trên `*At` không chứng minh được điều đó.
7. Chuyển `GetFile` sang grant có thời hạn, hoặc xoá nếu không còn caller.
8. Chuyển dần các cell còn lại, đặt cờ handle-only cho từng cell khi chuyển xong.
9. **Xoá** các variant path-string khỏi `VfsRequest` (không phải deprecate) — step cuối, cần ABI
   confirmation riêng. Chừng nào chưa xoá, guarantee chỉ đúng với cell có cờ.
10. Cập nhật `docs/specs/09-vfs.md` + `17-ipc-wire-contract.md`.

## Todo List

- [ ] **Prerequisite**: owner-check hai bảng handle (phase 02 bước 3) đã land
- [ ] ADR: revoke · hot-swap · type · **authenticate handle set lúc spawn** (4 điểm)
- [ ] 2× confirmation cho thay đổi libs/api (Law 1) — gộp cả ABI thứ hai nếu ADR chọn (a)
- [ ] Bảng handle per-cell + **cờ handle-only** + vòng đời theo cell death
- [ ] Op `*At` song song op cũ
- [ ] Reject `..` / `/` / rỗng trong `name` — test từng dạng
- [ ] Cấp handle gốc lúc spawn từ namespace spec
- [ ] Cell tiên phong + cờ handle-only + **test nhận `Err(3)` cho `Write{path}`**
- [ ] `GetFile` → grant có thời hạn hoặc xoá
- [ ] Chuyển các cell còn lại, đặt cờ từng cell
- [ ] **Xoá** variant path-string khỏi `VfsRequest` (step cuối, ABI confirmation riêng)
- [ ] Cập nhật spec 09 + 17

## Success Criteria

**Done khi**

- Cell tiên phong (có cờ handle-only) nhận `Err(3)` khi gửi `VfsRequest::Write { path }`. **Đây là
  criterion chính** — không phải "test traversal trên `*At` pass", vì cái đó pass được trong lúc cell
  vẫn gửi path tuyệt đối qua op cũ.
- Một cell đã chuyển **không thể diễn đạt** path ngoài handle của nó: mọi biến thể `..`/absolute bị
  reject ở tầng decode/resolve, VÀ op path-string bị từ chối cho cell đó.
- `GetFile` không còn trả con trỏ thô vĩnh viễn.
- Spec 09 + 17 phản ánh mô hình mới.

**Validation**

- Suite 3 arch pass. Test path-traversal riêng (`..`, `../..`, `/abs`, `a/../../b`, `.`, rỗng, UTF-8
  lạ) cho mọi op `*At`.
- Cell chưa chuyển vẫn chạy (đường di trú không phá backward compat).

## Risk Assessment

| Rủi ro | Xác suất | Ảnh hưởng | Giảm thiểu |
|--------|----------|-----------|------------|
| Đổi discriminant `VfsRequest` gây collision như bug input trước đây | Cao | IPC sai âm thầm | Theo spec 17 nghiêm ngặt; thêm variant ở cuối enum; test wire-level |
| Chuyển đồng loạt → vỡ nhiều cell | Cao | Không boot | Op song song + chuyển từng cell; đây là lý do phase là P3 |
| Ngữ nghĩa revoke chốt sai → phải làm lại | Trung bình | Rework lớn | ADR trước, code sau (bước 1 là blocking) |
| **Migration window vô hiệu hoá guarantee**: op path-string còn sống, `handle_request` không có per-cell mode (`dispatch.rs:25-240`) → phase có thể được mark complete với mục tiêu an toàn chưa đạt | Cao | Guarantee giả | Cờ handle-only (req 7) + criterion `Err(3)` cho `Write{path}` + xoá variant là step cuối tường minh (req 8) |
| **ADR item 4 phát hiện cần Law 1 thứ hai giữa lúc code** | Cao nếu bỏ qua bước 1 | Confirmation lần hai, rework | Item 4 là blocking trong ADR; kernel/ABI surface đã đưa vào Related Code Files để chi phí hiện ra trước |
| Handle leak khi cell chết bất thường | Trung bình | Giữ tài nguyên | Reaper theo cell death, có tiền lệ grant reaper |
| Path traversal qua unicode/normalization | Trung bình | Escape namespace | Reject theo byte, không normalize; whitelist ký tự cho `name` |

## Security Considerations

- **Đây là phase biến "check" thành "không thể diễn đạt"** — giá trị an toàn nằm ở chỗ đó, không ở
  việc thêm một lớp kiểm tra nữa. Nếu hiện thực xong mà cell vẫn gửi được path tuyệt đối qua op mới
  thì phase thất bại về mục tiêu dù test pass.
- Reject `..` phải làm ở tầng **resolve trong VFS**, không tin vào client. Và làm theo byte thô,
  không normalize trước — normalize là nơi sinh ra lỗ traversal.
- `GetFile` con trỏ thô là lỗ không thu hồi được trong SAS. Nếu không xử lý được trong phase này thì
  phải ghi thành finding riêng, không được im lặng bỏ qua.
- Kế thừa handle lúc spawn phải **chỉ thu hẹp**, đối xứng với `CapSet::intersect`. Cho phép mở rộng
  dù chỉ một trường hợp là mở lại đúng lỗ escalation mà P-TRUST đã đóng cho device cap.

## Deviation Log

Kernel half of the spawn carrier (2026-07-31).

- **Decision — carrier is a staging syscall, not a spawn argument.** `SpawnFromElf`
  (a0..a3 = grant_id/len/path_hint_ptr/path_hint_len) and `SpawnPinned`
  (a0..a3 = path_ptr/path_len/priority/core_id) already use all four argument
  registers, and `ostd::syscall` fixes the register file at four across all three
  arches. A pointer in a spare register would have reached only `SpawnFromPath`
  and `SpawnFromMem` — and `sys_spawn_from_path` prefers the `SpawnFromElf` route
  whenever VFS is registered, so the primary post-boot spawn path would silently
  carry nothing. `SpawnSetDirs` (240) stages a versioned `#[repr(C)]` carrier on
  the caller's own task; the next spawn consumes and clears it. Existing spawn
  ABIs are byte-identical, so a caller that stages nothing gets an empty set.
- **Decision — `ViSpawnArgs` was NOT widened.** Growing it would make the kernel
  read `size_of::<ViSpawnArgs>()` bytes from stack frames laid out by prebuilt
  cell binaries (`kernel/src/embedded*/`) compiled against the narrower struct,
  turning adjacent stack bytes into a handle pointer.
- **Decision — attestation is pulled, not pushed.** The 32-byte
  `CallerIdentity` trailer cannot hold a variable-length set. `QueryDirHandles`
  (241) has the kernel write the record into the service's own buffer during the
  service's own syscall; a buffer shorter than `DIR_ATTESTATION_LEN` is an error,
  never a partial write.
- **Deviation — narrowing-only is NOT enforced at spawn time.** The ADR says an
  over-broad spawn fails the spawn. The kernel cannot evaluate that predicate: it
  is not the authority, and a spawner may legitimately pass a handle it acquired
  from the VFS *after* its own spawn, so no kernel-side subset check is even
  correct. The kernel enforces only the structural half (bound, version, no zero,
  no duplicate — all-or-nothing). The authority half lands in the VFS half of the
  phase, as an all-or-nothing bind. Detail in
  `.agents/reports/phase-06-kernel-handle-carrier-260731.md`.
- **Surprise — `boot` suite baseline is 53/54, not 54/54.** `bench_all_pass`
  fails with "FATAL: Failed to spawn bench-probe". Reproduced at HEAD with all
  phase-06 changes reverted and the kernel rebuilt, so it is pre-existing and
  unrelated. Not fixed here (outside this phase's file ownership).

VFS half — the service as the authority (2026-07-31).

- **Decision — two operations beyond the six named, plus one response variant.**
  The brief named `OpenDir`/`ReadAt`/`WriteAt`/`StatAt`/`ListAt`/`UnlinkAt`. Three
  more were unavoidable and all are appended at the end. `OpenRootDir { path }`
  is the bootstrap: `OpenDir` derives from a handle the caller already holds, and
  with no way to acquire the first one the whole interface is unreachable —
  Capsicum's shape, where a process opens its directories and then calls
  `cap_enter()`. `SealPaths` is the per-cell flag; without a way to set it the
  phase's primary criterion cannot be reached at all. `CloseDir { dir }` gives
  ADR point 1 a caller: transitive revocation with no operation that triggers it
  is an untested claim, and it is now asserted end-to-end in the pioneer.
  `VfsResponse::DirHandle` was needed because `PendingHandle(u32)` cannot carry a
  `ViDirHandle`.
- **Decision — `ListAt` takes no name.** Listing a subdirectory means holding a
  handle to it. A `name` here would be a second resolution path to keep in step
  with the first, for an operation `OpenDir` already covers.
- **Decision — the per-cell flag is set two ways, one of them not self-imposed.**
  A cell seals itself with `SealPaths`, and any cell the kernel attests an
  inherited handle set for is sealed on first contact whether or not the bind
  succeeded. The second matters: a spawner handing over handles has placed the
  child in the capability world, and if a *refused* bind left path strings open
  the failure would widen the child's reach relative to success.
- **Decision — the fast-IPC path declines a cell it has not met.** Deciding
  whether a cell is sealed needs the kernel's provenance record, and pulling it
  is a syscall `vfs_fast_handler` cannot make with interrupts disabled. It now
  returns 0 for an unseen cell, which `call_vfs` callers already treat as "fast
  path unavailable" and retry as an ecall. Cost: one ecall per cell, once.
  Leaving it would have served a path read to a cell that should have been
  refused one.
- **Decision — name validation lives in `libs/api/src/services/dir_name.rs`.**
  It is called at resolve time inside the VFS, but the predicate itself is a pure
  function, and `libs/api` is the only place in this repo where a test actually
  runs on the host. 26 of them do, covering each traversal shape separately.
- **Deviation — the pioneer is `/bin/vfs-test`, not a demo cell.** The brief said
  "a small demo, NOT the shell". No demo cell is spawned during any suite that
  runs here (init's demo list is on-demand from the shell), so migrating one
  would have produced no observable evidence without also editing the image build
  scripts — which another phase owns. `vfs-test` is small, is not the shell, is
  already auto-spawned, and is asserted by two suites. It acquires its
  directories, works through handles, seals itself, and then proves
  `Write { path } → Err(3)`.
- **Surprise — `VfsRequest` now runs past byte-0 `0x0F` into the range spec 17 §3
  assigns to `INPUT_EVENT_OPCODE` (`0x10`), `NET_READY` (`0x11`) and
  `REACTOR_WAKE` (`0x12`).** Safe by receiver, not by value: the VFS is never a
  focus target, declares `network = false`, and runs a plain recv loop rather than
  a reactor. Recorded in §3 with the standing obligation to re-check before
  variant 23.
- **Not done, deliberately.** `GetFile`'s raw pointer is still permanent
  authority, and the path-string variants still exist. Both were named out of
  scope; findings in the phase report.

## Next Steps

- Sau phase này, "no ambient authority" của Midori được hiện thực đầy đủ cho filesystem; `AccessTable`
  của phase 02 lùi về vai trò defense-in-depth.
- Cân nhắc áp cùng mô hình cho service registry (`sys_lookup_service` hiện là namespace toàn cục).
