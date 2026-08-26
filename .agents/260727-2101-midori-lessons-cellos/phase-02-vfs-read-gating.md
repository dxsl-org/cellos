# Phase 02 — Read gating + handle owner-check + ACL theo danh tính

## Context Links

- Plan: [plan.md](plan.md) · Phụ thuộc: [phase-01](phase-01-vfs-destructive-authz.md)
- Bối cảnh: `.agents/260712-1903-thread-cellid-quota-fix/plan.md` — kernel-side **đã xong** (validation
  V1); phần VFS-side đã gộp vào phase này (validation D1). Không còn là blocking dependency.
- Spec: `docs/specs/09-vfs.md`, `docs/specs/17-ipc-wire-contract.md`
- Midori nguồn: không có ambient filesystem — app không thể `open("/etc/passwd")` vì không có
  namespace toàn cục để mở; mọi thẩm quyền được truyền vào

## Overview

- **Ưu tiên**: P1
- **Trạng thái**: Runtime-closed under user-approved amended criteria (2026-08-05) — Law 1 đã được confirm 2×; QEMU hiện chứng minh metadata-only governed message-path `GetFile` positive+deny và `ReadFileGrant` clamp/nonzero/deny. **Không** claim real `ReadGrant` producer hay Tier-1 direct fast-IPC proof trong phase này.
- **Mô tả**: VFS hiện cho **mọi cell đọc mọi path**, `can_read` chưa được nối vào đâu, và **cả hai
  bảng handle của VFS không so owner** nên đọc chéo cell được ngay cả khi ACL hoàn thành. Đây là lỗ
  ambient-authority lớn nhất còn lại trong Cellos.

> **Red-team correction**: bản draft chọn "Phương án A (không cần ABI)" — VFS tra `GetProcs`/
> `GetProcs2` để map CellId → path — làm mặc định. **Phương án đó bị reject**: nguồn danh tính đó
> không tồn tại, và thứ gần nhất thì kẻ tấn công tự đặt được. Phase này giờ Law 1-gated.

## Key Insights

- Doc comment của module tự thừa nhận: *"all authenticated cells may read all paths"*
  ([access.rs:10](../../cells/services/vfs/src/access.rs#L10)).
- `can_read` tồn tại nhưng là `#[allow(dead_code)]` ([access.rs:92](../../cells/services/vfs/src/access.rs#L92))
  — chưa từng được gọi. Field `allow_read_all` cũng vậy ([access.rs:20-22](../../cells/services/vfs/src/access.rs#L20-L22)).
- Cả `can_write` và `can_read` nhận `_cell: CellId` rồi **bỏ đi**
  ([access.rs:80](../../cells/services/vfs/src/access.rs#L80), [:93](../../cells/services/vfs/src/access.rs#L93)).
- **Phương án A không khả thi (đã xác minh)**: `ProcessInfo`/`ProcessInfoV2` chỉ mang `id`, `state`,
  `name[32]` + counters — **không path, không cell_id**
  ([syscall.rs:769-786](../../libs/api/src/abi/syscall.rs#L769-L786)). Trường duy nhất giống path
  là `name`, dẫn xuất bằng `path.rsplit('/').next()`
  ([loader.rs:177](../../kernel/src/loader.rs#L177)) từ **path_hint do caller truyền** vào
  `sys_spawn_from_elf`. Bất kỳ SpawnCap holder gọi `sys_spawn_from_elf(elf, "/bin/vfs")` là con nó
  tên `vfs` → ACL của phase 02 cấp cho con đó allowlist `/srv/`. `CapSet` an toàn ở đây (ceiling
  bound nó) nhưng ACL phía VFS thì không — nó tin một string kẻ tấn công chọn.
- **CellId không phải khoá bền vững**: `owner = types::CellId(sender as u64)` với `sender` là tid;
  kernel cũng đặt `cell_id = CellId(tid as u64)` ([loader.rs:190](../../kernel/src/loader.rs#L190)).
  Init respawn service `Permanent` bị crash bằng một `sys_spawn_from_path` mới → tid mới → CellId
  mới. Nên bảng static keyed theo CellId **không viết được lúc build** và tự trỏ lại sau lần
  auto-restart đầu tiên. → **Rule phải key theo `path`**, tid→path resolve mỗi request.
- **VFS đang BỊA `CellId` từ tid** (validation V2, finding mới — cả 4 red-team reviewer đều nói sai
  về chỗ này): kernel đã vá thread-CellId(0), thread giờ **inherit `cell_id` của cell cha** nhưng
  nhận **tid riêng** ([syscall.rs:1415-1450](../../kernel/src/task/syscall.rs#L1415-L1450), có comment
  tường minh *"it must never fall back to CellId(0), which is exactly the quota-escape this closes"*).
  VFS thì dựng `CellId(sender as u64)` từ tid ([dispatch.rs:49](../../cells/services/vfs/src/dispatch.rs#L49),
  `:113`, `:124`). ⇒ Với cell do loader spawn, `CellId(tid) == cell_id` **tình cờ** đúng; với **thread**,
  VFS bịa ra một CellId không ứng với cell nào → quota ghi vào sổ ảo thay vì sổ cell cha.
  **LATENT, không live**: `sys_spawn` có trong ostd ([syscall.rs:233](../../libs/ostd/src/syscall.rs#L233))
  nhưng chưa cell nào gọi. Nó sẽ cắn cell đầu tiên dùng thread.
  ⇒ Đây là **lý lẽ mạnh nhất cho quyết định Law 1**: fix đúng là VFS nhận `cell_id` do kernel attest,
  và cùng một ABI change phục vụ **cả ACL lẫn quota accounting**. Vì chưa cell nào dùng thread, luật
  "unknown identity → deny" không phá gì hôm nay.
- **Hai bảng handle không so owner (bypass hoàn chỉnh)**:
  - `PendingTable` không có field owner, `next_id` tuần tự từ 1
    ([pending.rs:24-43](../../cells/services/vfs/src/pending.rs#L24-L43)). Cell A gửi
    `Poll{handle: n}` với n = 1..N → `slots.remove(n)` trả nguyên nội dung file mà cell B đã
    `ReadAsync`, **không qua `can_read` lần nào**, và B mất data (DoS).
  - `HandleEntry` **có** field `owner` nhưng doc ghi *"for quota accounting"*
    ([handle_table.rs:12-14](../../cells/services/vfs/src/handle_table.rs#L12-L14)) và
    `get_mut(cap)` chỉ tra `cap.0`, **không so** `entry.owner`
    ([handle_table.rs:54-56](../../cells/services/vfs/src/handle_table.rs#L54-L56));
    `dispatch.rs:173` cũng không so. Doc comment "Per-cell open file handle table"
    ([handle_table.rs:2](../../cells/services/vfs/src/handle_table.rs#L2)) là sai.
- Op đọc không gate — **7 op, không phải 6** (draft bỏ sót `Poll`): `GetFile`
  ([:26](../../cells/services/vfs/src/dispatch.rs#L26)), `ListDir` ([:37](../../cells/services/vfs/src/dispatch.rs#L37)),
  `Stat` ([:42](../../cells/services/vfs/src/dispatch.rs#L42)), `ReadAsync` ([:137](../../cells/services/vfs/src/dispatch.rs#L137)),
  **`Poll`** ([:145](../../cells/services/vfs/src/dispatch.rs#L145)), `ReadGrant` ([:162](../../cells/services/vfs/src/dispatch.rs#L162)),
  `ReadFileGrant` ([:221](../../cells/services/vfs/src/dispatch.rs#L221)).
- `GetFile` trả `DataPtr { ptr, len }` — **con trỏ thô**. Trong SAS đây là thẩm quyền đọc vĩnh viễn,
  đã phát là không thu hồi được. Gate `GetFile` quan trọng hơn mọi op còn lại.
- **⚠ Đường fast-IPC KHÔNG mang danh tính caller nào cả** (phát hiện 2026-07-30 khi làm bước 3,
  xác minh trực tiếp): `ostd::fast_ipc::call_vfs` ([fast_ipc.rs:134](../../libs/ostd/src/fast_ipc.rs#L134))
  gọi `vfs_fast_handler(req, out)` ([vfs/src/main.rs:95](../../cells/services/vfs/src/main.rs#L95)) —
  signature **không có tham số sender** — và **bypass hoàn toàn `handle_request`**, nơi mọi gate của
  phase này sống. `TrustedHandle<VfsCell>` không phải là control: doc của nó tự ghi *"it does not
  enforce this at runtime"* ([fast_ipc.rs:133](../../libs/ostd/src/fast_ipc.rs#L133)).
  ⇒ Và op duy nhất mà fast path đang phục vụ chính là **`GetFile`** — đúng cái op mà bullet trên
  xếp ưu tiên cao nhất, vì nó phát `DataPtr` thô không thu hồi được.
  ⇒ **Hệ quả cho thiết kế**: bước 8 như viết hiện tại **không gate được `GetFile`** — cell nào có
  fast path vẫn lấy `DataPtr` không qua `can_read`. Bước 4 (thiết kế ABI) **phải phủ cả fast-IPC**,
  không chỉ đường message; nếu không thì gate `GetFile` chỉ là trang trí đúng theo nghĩa mà mục
  Security Considerations đã cảnh báo về owner-check. Chốt cùng lúc với thiết kế `cell_id` attest.

## Requirements

**Functional**

1. **Law 1**: kernel truyền `cell_id` (hoặc `cell_id` + tier) của caller kèm mỗi IPC tới VFS. VFS
   không bao giờ tự suy ra danh tính từ dữ liệu do caller cung cấp, và **không bao giờ dựng `CellId`
   từ `sender`** (tid) nữa. Theo `docs/specs/17-ipc-wire-contract.md`.
   `cell_id` này phục vụ **cả ACL lẫn quota accounting** — một ABI change, hai chỗ dùng (validation D1).
2. Cả hai bảng handle key theo `(cell_id, handle)` hoặc so `entry.owner == caller` trước khi trả
   dữ liệu / trước khi `remove`. `PendingTable` phải có field owner.
3. Cả 7 op đọc gọi `can_read(caller, path)` trước khi trả dữ liệu; fail → `Err(3)`.
4. `AccessTable` hỗ trợ rule theo `(cell path, prefix)` — **key theo path, không theo CellId** —
   với prefix rule làm fallback. Danh tính không resolve được → **deny**, không fallback permissive.
5. Quota `release` credit đúng owner đã charge (fix open finding của phase 01): VFS ghi lại writer
   theo path, và một caller chỉ release được quota nó giữ.
6. **Xoá mọi `types::CellId(sender as u64)`** trong `dispatch.rs` — thay bằng `cell_id` kernel attest.
   Đây là fix cho V2 (quota của thread ghi vào sổ ảo). Grep là success criterion.

**Non-functional**

6. Deny-by-default giữ nguyên; unknown identity = deny.
7. Không tăng số IPC round-trip.
8. `access.rs` giữ dưới 200 dòng — tách `access/rules.rs` khi thêm bảng per-cell.

## Architecture

```
kernel ──(IPC + cell_id đã attest)──► VFS
                                       │
                                       ├─ handle table: so owner == cell_id  ◄── req 2
                                       │
                                       └─ AccessTable.can_read/can_write(cell_id → path, path)
                                              ├─ per-path entry (nếu có)  ─┐
                                              └─ prefix rule (fallback)  ──┴─► allow / deny
                                                     (không resolve được → deny)
```

`cell_id → path`: VFS giữ map, nạp từ **một nguồn tin cậy duy nhất**. Vì `cell_id` giờ do kernel
attest, VFS resolve path bằng cách nào cũng an toàn hơn draft — nhưng vẫn phải xử lý CellId tái sử
dụng sau cell death: invalidate entry khi kernel báo cell chết, hoặc kèm generation counter vào
`cell_id`. **Chốt trong bước 1 cùng thiết kế ABI**, không để mở.

## Related Code Files

| File | Hành động |
|------|-----------|
| `libs/api/src/abi/...` (IPC caller identity) | Modify — **Law 1, cần 2× confirmation** |
| `kernel/src/task/syscall.rs` | Modify — attach cell_id vào đường IPC tới service |
| `docs/specs/17-ipc-wire-contract.md` | Modify — ghi field danh tính mới |
| `cells/services/vfs/src/access.rs` | Modify — dùng danh tính, bỏ 2 `#[allow(dead_code)]`, rule per-path |
| `cells/services/vfs/src/access/rules.rs` | Create (khi access.rs vượt 200 LOC) |
| `cells/services/vfs/src/pending.rs` | Modify — thêm owner, so owner trong `poll` |
| `cells/services/vfs/src/handle_table.rs` | Modify — `get_mut`/`remove` so owner; sửa doc comment sai |
| `cells/services/vfs/src/dispatch.rs` | Modify — gate 7 op đọc |
| `cells/services/vfs/src/quota.rs` | Modify — release credit đúng owner đã charge |

## Implementation Steps

1. **Gate 0 (blocking) — forgeability test, không phải stability test.** Spawn một cell qua
   `sys_spawn_from_elf` với `path_hint = "/bin/vfs"`, rồi khẳng định VFS **không** cấp cho nó ACL
   của vfs. Draft chỉ kiểm CellId có bền vững giữa thread và cell chủ — sai câu hỏi (và câu hỏi đó
   đã có đáp án: kernel-side đã vá, xem Key Insights). Cùng bước này: chốt cách xử lý CellId tái
   sử dụng (invalidate-on-death vs generation counter).
2. Xin 2× confirmation cho việc kernel attest cell_id kèm IPC (Law 1), kèm kết quả Gate 0 làm cơ sở.
3. **Owner-check hai bảng handle TRƯỚC khi gate op đọc** — đây là bước có ROI cao nhất và không
   cần chờ ABI: thêm owner vào `PendingTable`, so owner trong `HandleTable::get_mut`/`remove`, sửa
   doc comment. Test: cell A quét `Poll{1..N}` → `Err`.
4. Attach cell_id vào IPC (sau confirmation), cập nhật spec 17.
5. Mở rộng rule shape sang per-path + prefix fallback; bỏ `#[allow(dead_code)]` trên `allow_read_all`.
6. Hiện thực `can_read` thật: per-path entry trước, prefix fallback sau, unknown identity → deny.
7. Gate `Stat` + `ListDir` trước (rẻ, ít caller) → boot thử. Đây là phép thử xem đường boot có phụ
   thuộc read ambient hay không.
8. Gate `GetFile` (trả con trỏ thô — ưu tiên cao nhất), rồi `ReadAsync`, `Poll`, `ReadGrant`,
   `ReadFileGrant`.
9. Allowlist khởi đầu **rộng rồi siết**: mọi cell đọc `/bin/` + `/tmp/` + `/data/`; siết `/srv/` về
   đúng vfs/net/shell. `/bin/` giữ read-all — siết `/bin/` là việc của phase 06 (loader/pkg đọc ELF
   qua đó).
10. Fix quota release (req 5) — cần danh tính, nên phải sau bước 4.
11. Negative + positive test; clippy + build + full suite 3 arch.

## Todo List

- [x] **Gate 0: forgeability test** — kết luận bằng đọc code, không cần spawn: chữ ký cell ký trên
      *bytes*, không bind vào path ([loader.rs:122-151](../../kernel/src/loader.rs#L122)), và `name`
      lấy từ `path_hint` do caller chọn ([:182](../../kernel/src/loader.rs#L182)) ⇒ **mọi ACL keyed
      theo tên cell là forge được**. Hệ quả: rule table KHÔNG được phân biệt theo cell (xem dev log).
- [x] Chốt xử lý CellId tái sử dụng → **generation counter** (`Task::cell_generation`)
- [x] 2× confirmation Law 1 (cell_id kèm IPC) — đã nhận từ orchestrator
- [x] **Owner-check `PendingTable` + `HandleTable`** (không chờ ABI) + sửa doc comment sai
- [x] Test: cell A quét `Poll{1..N}` bị chặn
- [x] Attach cell_id vào IPC + cập nhật spec 17 (§11 mới)
- [x] Rule shape per-path + prefix fallback, bỏ 2 `#[allow(dead_code)]`
- [x] `can_read` thật; unknown identity → deny
- [x] Gate `Stat`, `ListDir` (boot thử: KHÔNG chạy được — không có QEMU)
- [x] Gate `GetFile` (cả ecall + fast-IPC), `ReadAsync`, `Poll`, `ReadGrant`, `ReadFileGrant`
- [~] Allowlist: giữ `/bin/` read-all ✔; **KHÔNG siết `/srv/`** — xem dev log
- [x] Quota release credit đúng owner đã charge
- [x] **Xoá mọi `CellId(sender as u64)` trong `dispatch.rs`** (fix V2) — grep = 0
- [~] Full suite 3 arch pass — check ×3 arch + aarch64 `build` pass; suite runtime UNVERIFIED (no QEMU)

## Success Criteria

**Done khi**

- Governed message-path `GetFile("/tmp/volatile.txt")` trả `DataPtr` non-null với `len == 8`
  trước `SealPaths`; phase này chỉ claim response metadata, không dereference raw pointer.
- `GetFile`, `Stat`, `ListDir`, `Unlink`, `Mkdir`, `ReadAsync`, và `OpenRootDir` đều bị từ chối sau
  `SealPaths`.
- `ReadFileGrant` chứng minh đủ ba marker runtime: clamp đúng chiều dài grant, copy nonzero bytes, và
  bị từ chối sau `SealPaths`.
- `ReadGrant` hiện chỉ được claim ở mức fail-closed/zero-byte khi dùng cap không có producer thật;
  probe runtime là unknown-cap + valid shared grant, không giả làm EOF của một file mở được.
- Không path nào dùng `DataPtr` được mô tả như Tier-2-safe; direct fast-IPC `GetFile` proof **không**
  thuộc Phase 02 closure.

**Validation**

- Test-hooks QEMU lane `tests/integration/tests/vfs-quota.rs` chờ đủ các marker:
  `dircap: GetFile returns a nonempty pointer before sealing`,
  `grant: ReadFileGrant clamps to grant length`,
  `grant: ReadFileGrant copies nonzero bytes`,
  `grant: ReadFileGrant is refused after sealing`,
  và `[vfs-test] ALL TESTS PASSED`.

## Risk Assessment

| Rủi ro | Xác suất | Ảnh hưởng | Giảm thiểu |
|--------|----------|-----------|------------|
| Law 1 confirmation bị từ chối | Trung bình | Phase mất nền tảng | Bước 3 (owner-check) không cần ABI và tự nó đã đóng một lỗ đọc-chéo thật → phase vẫn giao được giá trị nếu ABI bị chặn |
| Đụng spec 17 IPC wire contract → discriminant/framing collision | Cao | IPC sai âm thầm | Đã có tiền lệ (`project-input-ipc-protocol-collision`); test wire-level, thêm field ở cuối |
| Thread nhận tid riêng → resolve thất bại → deny → cell dùng thread mất quyền đọc, **âm thầm** (mỗi op chỉ trả `Err(3)`) | Thấp hôm nay | Cell vỡ khó chẩn đoán | Kernel đã cho thread inherit `cell_id` của cha, và req 1 làm VFS **nhận** cell_id đó thay vì bịa từ tid → thread tự động resolve về cell chủ. **Chưa cell nào dùng thread** (grep `cells/`) nên rủi ro hôm nay thấp; nhưng phải test bằng fixture tự dựng, không đợi cell thật xuất hiện. Log mọi deny kèm cell_id + path |
| CellId tái sử dụng sau cell death → cell mới thừa hưởng ACL cũ | Trung bình | Escalation | Chốt ở bước 1; generation counter là phương án an toàn hơn invalidate |
| Gate `GetFile` phá đường load ELF của loader/pkg | Cao | Boot vỡ | `/bin/` giữ read-all ở phase này |
| access.rs vượt 200 LOC | Cao | Vi phạm chuẩn | Tách `access/rules.rs` ngay khi thêm bảng per-path |

## Security Considerations

- **`GetFile` trả con trỏ thô là thẩm quyền vĩnh viễn, không phải một lần đọc.** Trong SAS một
  `DataPtr` đã phát là không thu hồi được. Về dài hạn phải thành grant có thời hạn (phase 06).
- **Bảng handle không so owner là lỗ tương đương ambient authority, chỉ ẩn hơn.** Nó vô hiệu hoá
  mọi ACL đặt ở tầng path. Đây là lý do bước 3 đứng trước các bước gate: gate path mà không so
  owner là ACL trang trí.
- Danh tính phải do **kernel attest**. Bất kỳ danh tính dẫn xuất từ dữ liệu caller cung cấp (path
  hint, name, self-reported id) là forge được — nguyên tắc này áp cho cả các service khác, không
  riêng VFS.
- Unknown identity → **deny**, không fallback prefix rule. Fallback permissive biến mọi lỗi resolve
  thành một bypass.
- Siết `/srv/` trước: đó là RedoxFS service volume — giá trị cao nhất, ít caller hợp lệ nhất.
- Chiến lược "rộng rồi siết" là có chủ ý; siết tất cả cùng lúc sẽ vỡ boot và dẫn tới nới lại toàn bộ.

## Deviation Log

Bước 3 (owner-check hai bảng handle) đã thực hiện — 2026-07-30. Các bước còn lại vẫn Planned
(chờ ABI Law 1). Quyết định trong lúc làm:

- **Danh tính**: giữ nguyên `types::CellId(sender as u64)` (req 6 chưa được duyệt), nhưng gom **6 chỗ
  dẫn xuất trong `dispatch.rs` về 1 chỗ duy nhất** ở đầu `handle_request`; các arm dùng biến `caller`.
  Đổi sang cell_id do kernel attest sau này = sửa 1 dòng. Grep `CellId(sender` giờ trả **1** kết quả
  (dòng 34), không phải 0 — success criterion đó thuộc bước 4.
- **Deny-by-default tại boundary**: `sender == 0` → `Err(3)` ngay, không dựng CellId. `main.rs` vốn đã
  lọc `sender > 0`, đây là guard tại chỗ dẫn xuất.
- **So owner TRƯỚC khi remove** ở cả `PendingTable::poll` và `HandleTable::remove`. Nếu remove trước
  rồi so sau thì việc quét handle vẫn phá được dữ liệu của cell khác dù không đọc được — DoS vẫn còn.
- **Không phân biệt lỗi**: wrong-owner trả cùng mã với stale/unknown (`Err(4)` cho Poll,
  `GrantDone{bytes:0}` cho ReadGrant) để keyspace tuần tự không thành existence oracle. Wire contract
  của spec 17 không đổi.
- **`insert_ro` đổi thứ tự tham số** → `(owner, cap, ...)` cho khớp `get_mut(caller, cap)` /
  `remove(caller, cap)`. An toàn: `insert_ro` hiện **không có caller nào** — nên `HandleTable` hôm nay
  luôn rỗng và `ReadGrant` luôn trả 0 bytes. Owner-check ở đó là phòng ngừa cho lúc write path được nối.
- **Test không chạy được trong repo**: crate là bin `no_std`/`no_main`, không có target test host nào;
  đã xác minh `--cfg test` (không kèm `--test`) **strip** hẳn các fn `#[test]` nên không command nào
  trong repo typecheck được `#[cfg(test)] mod tests` (tiền lệ: `cells/services/net/src/tls/*`).
  8 test đã chạy thật qua harness host ngoài repo (`#[path]`-include đúng 2 file đó, dep thật vào
  `libs/types` + `libs/api`); mutation-check: 5/8 fail khi hoàn nguyên owner-check.
  → **Follow-up cần thiết**: một target test host cho các bảng này, hoặc case tương ứng trong
  `tests/integration/vfs-*`.
- **Ghi nhận, không sửa**: đường fast-IPC (`ostd::fast_ipc::call_vfs` → `vfs_fast_handler` ở
  `main.rs`) **không mang danh tính caller nào cả**. Hôm nay chỉ phục vụ `GetFile` (không chạm bảng
  handle) nên bước 3 không ảnh hưởng, nhưng bước 8 (gate `GetFile`) **không có gì để gate trên đường
  đó** — phải tính vào thiết kế ABI ở bước 4.

### Bước 4-10 (ABI + gating) — 2026-07-30

- **Cơ chế attest: trailer ở CUỐI recv buffer, opt-in qua a3 của `Recv`.** Không thêm variant/field
  vào `VfsRequest` (byte 0 là discriminant postcard → chính là rủi ro framing collision phase này
  phải tránh), không thêm syscall number, không thêm allowlist bit. `Recv` a3 trước đây **không dùng**
  và mọi caller cũ truyền 0, nên opt-in không đổi hành vi của bất kỳ receiver nào khác — blast radius
  bằng 0. Kernel ghi trailer **sau** khi copy payload ⇒ sender pad message full buffer cũng không
  đặt trước được trailer giả. Type: `api::caller_identity::CallerIdentity`
  (`libs/api/src/abi/caller_identity.rs`, module mới dưới `abi/` thay vì nhồi vào `syscall.rs` 786
  dòng). Spec 17 §11 ghi lại toàn bộ contract.
- **`TryRecv` / `RecvTimeout` / `RecvScatter` KHÔNG attest**: `RecvTimeout` đã dùng a3 cho deadline,
  `RecvScatter` không có một buffer-tail duy nhất. Service nào cần authorize phải recv bằng `Recv`.
- **CellId tái sử dụng → generation counter** (không phải invalidate-on-death). Lý do: invalidate cần
  một kênh kernel→VFS báo cell chết = thêm ABI surface, và **fail-open trong khoảng thời gian giữa
  lúc chết và lúc VFS nhận tin**. Generation mint 1 lần/cell trong `Task::new`, thread ghi đè bằng
  generation của cell cha trong `Scheduler::spawn_thread` (nếu không thì thread và cell của nó là hai
  principal khác nhau và thread mất quyền vào state của chính cell mình). Đã xác minh `next_task_id`
  chỉ tăng, chưa bao giờ recycle ⇒ hôm nay generation là defense-in-depth, không phải bản vá một lỗ
  đang chảy; nó là thứ giữ đảm bảo nếu cách cấp tid đổi.
- **Fast-IPC lấy danh tính từ kernel, không từ tham số.** `kernel::fast_ipc::call_vfs` là *kernel
  code* (loader resolve import của cell về symbol này, `resolve_export`), nên nó tự resolve identity
  từ scheduler cho task đang chạy trên hart — trước khi `SieGuard::disable()`, vì resolve lấy
  SCHEDULER lock và giữ lock đó qua handler sẽ deadlock backend VFS. `VfsFastHandler` đổi signature
  (thêm `caller: Option<CallerIdentity>` ở **đầu**), `vfs_fast_handler` gate `GetFile` bằng
  `can_read` y như đường ecall. Bản copy trong `libs/ostd/src/fast_ipc.rs` (mỗi cell link riêng, con
  trỏ luôn null cho client nên thực tế không bao giờ tới handler) truyền `None` ⇒ fail-closed.
- **Cả 7 op đọc đi qua `can_read`.** `Poll` và `ReadGrant` không mang path trong request, nên
  `PendingRead` + `HandleEntry` giờ lưu path và hai op đó **re-authorize** path đã lưu — quyết định
  lúc mở chỉ chứng minh policy *lúc đó*, handle sống lâu hơn một lần đổi rule. `insert_ro` đổi
  signature (thêm `path`); an toàn vì nó vẫn chưa có caller nào.
- **Mã lỗi**: deny trả `Err(3)`. Ngoại lệ có chủ ý, giữ nguyên từ bước 3: `Poll` với handle
  không-phải-của-mình vẫn trả `Err(4)` (giống stale) và `ReadGrant` với cap không-phải-của-mình vẫn
  trả `GrantDone{bytes:0}` (giống cap lạ) — đổi sang `Err(3)` sẽ biến keyspace tuần tự thành
  existence oracle. Deny **vì rule** (cap/handle đúng là của caller nhưng path bị chặn) trả `Err(3)`,
  không leak gì caller chưa biết.
- **Quota release theo path→writer** (`QuotaTracker::{set_writer, record_writer, writer_of,
  release_path}`). `Write` (ghi đè toàn file) → `set_writer`; `Append` → `record_writer`
  (insert-if-absent, vì append không chuyển sở hữu bytes cũ). `RmdirRecursive` release **từng file**
  (`subtree::files_under` trả `(path, size)`) vì hai file trong một cây có thể do hai cell trả tiền.
  Bound đã ghi trong doc `quota.rs`: file do 2 cell ghi thì writer đầu bị credit toàn bộ khi xoá; map
  in-memory nên không sống qua hot-swap (sau hot-swap xoá file không credit ai — lệch về phía
  over-charge, không phát quota free). Pre-check `can_charge` nay dùng delta **chỉ khi** caller chính
  là writer cũ, ngược lại phải đủ cho full size.
- **KHÔNG siết `/srv/` (req 7 nửa sau) — có chủ ý, không phải bỏ sót.** Siết về "vfs/net/shell" cần
  một binding cell→program mà kernel bảo chứng. Không có: chữ ký ký trên bytes chứ không bind path,
  `name` lấy từ `path_hint` do spawner chọn (Gate 0), và `shell` không có service id để tra qua
  registry. Hai lựa chọn còn lại đều xấu hơn hiện trạng: (a) key theo tên cell = ACL forge được, đúng
  cái Gate 0 cấm; (b) chỉ cho VFS/NET (tra qua `LookupService`) → vỡ `ls /srv` từ shell và
  `cells/tests/srv-test`. Đã ship *shape* để thêm row là sửa data: `EXACT_RULES` (whole-path, ưu tiên
  trước) + `PREFIX_RULES` (fallback). `EXACT_RULES` rỗng — populate được ngay khi có identity attest
  cho cell.
- **Root `/` giữ read-all.** Định siết (đó mới là "deny by default" thật) nhưng ramfs root chứa path
  ngoài mọi prefix mà cell đọc lúc khởi động — `net-broker` đọc `/etc/cellos/cluster.cfg`
  (`cells/services/net-broker/src/identity.rs:24`). Siết sẽ vỡ cluster boot bằng một
  `PermissionDenied` mờ. Để permissive và ghi lại.
- **`kernel/src/task/{tcb.rs, scheduler.rs}` và `kernel/src/fast_ipc.rs`, `libs/ostd/src/fast_ipc.rs`,
  `libs/ostd/src/syscall.rs` nằm ngoài bảng File Ownership** nhưng bắt buộc phải sửa: generation
  counter cần `Task` field + thừa kế ở `spawn_thread`, và req "fast-IPC phải mang danh tính" chỉ sửa
  được ở hai file `fast_ipc.rs`. Không đụng `kernel/src/{main.rs, policy.rs, loader*}` hay
  `kernel/src/task/cap.rs` — một phase song song đang giữ chúng (đã xác nhận qua `git status`).
- **Test**: 5 test `caller_identity` chạy **thật in-tree** (`cargo test -p api`, libs/api build được
  trên host). 20 test VFS (`access`, `caller`, `pending`, `handle_table`) chạy qua **harness host
  ngoài repo** — cell là bin `no_std`/`no_main`, không command nào trong repo compile được
  `#[cfg(test)]` của nó. Mutation-check: 6/6 mutation nhắm đích (bỏ generation khỏi principal, đổi
  deny-by-default thành permissive fallthrough, bỏ owner-filter khỏi `path_of`/`owned_path`) làm fail
  đúng test tương ứng; 2/2 mutation trên `caller_identity` (bỏ magic, bỏ chặn cell_id 0) cũng fail.
  **Không có kết quả runtime nào** — không QEMU, không boot.

## Next Steps

- Phase 06 thay path string bằng directory capability; **owner-check của bước 3 là prerequisite** —
  phase 06 draft giả định "hạ tầng handle đã có", điều đó chỉ đúng sau phase này.
- Future Law 1 phase riêng phải thiết kế **real `ReadGrant` producer** qua `OpenAt`/file-handle/close;
  `HandleTable::insert_ro` test-only không đủ để mở lại claim đó.
- Future Tier-1 transport phase riêng phải quyết định **direct fast-IPC `GetFile`** theo bridge/transport
  mới; runtime closure ở đây chỉ bao phủ governed message path.
