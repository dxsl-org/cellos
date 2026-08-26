# Phase 01 — VFS destructive-op authorization

## Context Links

- Plan: [plan.md](plan.md)
- Spec: `docs/specs/09-vfs.md`
- Law: CLAUDE.md → Law 6 (Vi naming), Scope Doctrine Gate 1
- Midori nguồn: no ambient authority — không có global namespace để bất kỳ ai cũng ghi/xoá

## Overview

- **Ưu tiên**: P1 (hạ từ P0 sau red-team — xem Key Insights)
- **Trạng thái**: **MERGED VÀO MAIN 2026-07-28** — PR #10 (`fix/vfs-destructive-authz`), merge commit
  `12a5df159`. Hai commit: `72f01d0d2` (fix VFS) + `b26a896bb` (sửa build tooling, mở khoá negative
  control). Mọi tiêu chí Success Criteria đã đạt — xem `## Evidence`.
- **Mô tả**: Ba op destructive của VFS bỏ qua `AccessTable` hoàn toàn. Bất kỳ cell nào cũng xoá
  được file dưới path gốc `/` (RamFS) dù rule `/` đặt `allow_write_all: false`. Phase này nối
  `can_write` vào mọi op có tác dụng phá huỷ.

> **Red-team correction**: bản draft lấy `Unlink("/bin/shell")` làm ví dụ động lực và xếp P0.
> Sai — `BinOverlay` từ chối mọi op phá huỷ ở tầng backend, nên `/bin/` đã có backstop. Lỗ thật
> nhỏ hơn nhưng vẫn thật: `/`-rooted path trên RamFS. Chi tiết trong Key Insights.

## Key Insights

- `Write`/`Append`/`Mkdir` **có** gọi `can_write` ([dispatch.rs:50](../../cells/services/vfs/src/dispatch.rs#L50),
  [:75](../../cells/services/vfs/src/dispatch.rs#L75), [:92](../../cells/services/vfs/src/dispatch.rs#L92)).
  `Rmdir` ([:101](../../cells/services/vfs/src/dispatch.rs#L101)),
  `Unlink` ([:110](../../cells/services/vfs/src/dispatch.rs#L110)),
  `RmdirRecursive` ([:123](../../cells/services/vfs/src/dispatch.rs#L123)) thì **không**. Đây là
  bỏ sót, không phải quyết định thiết kế — cùng file, cùng hàm, cùng pattern.
- **`/bin/` KHÔNG khai thác được** (red-team, đã xác minh): `BinOverlay` trả `false` cho `write`,
  `append`, `mkdir`, `rmdir`, `unlink`, `rmdir_recursive`
  ([backend_bin_overlay.rs:73-90](../../cells/services/vfs/src/backend_bin_overlay.rs#L73-L90)),
  nên `Unlink("/bin/shell")` hôm nay trả `Err(1)` — backend từ chối, không phải authorization từ
  chối. `/bin/` là defense-in-depth sẵn có.
- **Lỗ thật ở `/`**: `mounts.mount("/", ram, false)`
  ([manager.rs:54](../../cells/services/vfs/src/manager.rs#L54)) và RamFS **có** hiện thực
  `unlink`/`rmdir_recursive` ([backend_ramfs.rs:234](../../cells/services/vfs/src/backend_ramfs.rs#L234),
  [:251](../../cells/services/vfs/src/backend_ramfs.rs#L251)). Rule `/` là `allow_write_all: false`
  ([access.rs:62-66](../../cells/services/vfs/src/access.rs#L62-L66)) nhưng không được tham chiếu
  trên đường xoá → xoá thành công. Đây là ví dụ động lực đúng.
- **Authorization đang nhân đôi ở hai nơi**: `MountEntry::writable` (đối số thứ 3 của `mount`)
  là `#[allow(dead_code)]` và doc tự nhận *"Informational until AccessTable rules are mount-driven"*
  ([mount.rs:16-19](../../cells/services/vfs/src/mount.rs#L16-L19)). Hai nguồn sự thật cho cùng
  một câu hỏi = một trong hai sẽ lệch. Phải enforce hoặc xoá.
- `RmdirRecursive("/")` là nguy hiểm nhất: xoá đệ quy 32 tầng, có release quota, không có
  authorization.
- `WriteGrant` ([:195](../../cells/services/vfs/src/dispatch.rs#L195)) hiện là stub (`let _ = data`)
  nên chưa khai thác được, nhưng TODO trong đó nói sẽ wire cap→path ở Phase 04 — nếu wire mà
  không thêm `can_write` thì mở lại đúng lỗ này qua đường grant.
- `owner` được dựng bằng `types::CellId(sender as u64)` với `sender` là **tid**. Phase 01 KHÔNG
  phụ thuộc vào chất lượng của danh tính đó: rule hiện tại là path-prefix và bỏ qua CellId, nên
  chỉ cần bảng rule được *tham chiếu* là đã chặn được `/bin/`. Việc siết theo danh tính là phase 02.

## Requirements

**Functional**

1. `Rmdir`, `Unlink`, `RmdirRecursive` trả `VfsResponse::Err(3)` (PermissionDenied) khi
   `can_write(owner, path)` là false, **trước** khi thực hiện bất kỳ mutation hay quota op nào.
2. `WriteGrant` gọi `can_write` ngay khi path routing được wire (thêm assertion/TODO có ràng buộc).
3. Không đổi hành vi cho path đang được phép ghi (`/data/`, `/tmp/`, `/mnt/sd/`, `/srv/`).
4. `MountEntry::writable` hoặc được enforce (kiểm cùng `AccessTable`, deny nếu một trong hai từ
   chối) hoặc bị xoá khỏi struct. Không để lại field authorization không ai đọc.

**Non-functional**

4. Không đổi `VfsRequest`/`VfsResponse` → không đụng libs/api → không kích hoạt Law 1.
5. Không thêm allocation trên happy path (`can_write` là vòng lặp prefix trên static slice).

## Architecture

Không có thay đổi kiến trúc. `dispatch.rs` đã là "cross-cutting policy layer" theo chính doc
comment của nó ([dispatch.rs:4](../../cells/services/vfs/src/dispatch.rs#L4)); phase này làm cho
comment đó thành đúng.

Điểm cần cẩn thận về thứ tự: `RmdirRecursive` gọi `collect_dir_bytes` (walk subtree) **trước**
delete. Check quyền phải đặt trước cả `collect_dir_bytes` — không phải chỉ trước `rmdir_recursive` —
để một cell không được phép cũng không dùng được op này làm kênh do thám kích thước thư mục.

## Related Code Files

| File | Hành động |
|------|-----------|
| `cells/services/vfs/src/dispatch.rs` | Modify — thêm `can_write` vào 3 arm + WriteGrant |
| `cells/services/vfs/src/access.rs` | Không đổi ở phase này |
| `cells/services/vfs/src/vfs_test.rs` (hoặc test cell tương ứng) | Modify — thêm case negative |
| `tests/integration/tests/` | Modify — scenario "cell thường không xoá được /bin" |

## Implementation Steps

1. Xác định module test hiện có của VFS (`vfs-test` cell, 8 scenario theo
   `project-vfs-m21-status`) và điểm chèn scenario mới.
2. `Rmdir` arm: dựng `owner`, check `can_write`, return `Err(3)` sớm.
3. `Unlink` arm: giống trên. Lưu ý `file_size` đang được lấy trước `unlink` cho quota — check
   quyền phải đứng trước cả bước đó.
4. `RmdirRecursive` arm: check quyền **trước** `collect_dir_bytes`.
5. `WriteGrant` arm: thêm `can_write` gate; nếu path chưa resolve được từ cap thì fail-closed
   (`Err(3)`) thay vì stub-success — hiện tại nó trả `GrantDone` cho mọi thứ.
6. Enforce hoặc xoá `MountEntry::writable` (req 4) — quyết định trong bước này, không để mở.
7. Negative test **trên `/`**: cell ghi file `/x` (RamFS root, hiện `Err(3)` cho `Write` nhưng
   thử qua đường khác nếu cần dựng fixture), rồi `Unlink("/x")` → phải `Err(3)`, và `Stat("/x")`
   vẫn OK sau đó. Test phải **fail trước khi fix** — nếu nó pass trước fix thì fixture sai.
8. Secondary test `/bin/`: `Unlink("/bin/hello-cell")` → `Err(3)` thay vì `Err(1)`. Đây là
   defense-in-depth (BinOverlay vẫn là backstop), ghi rõ trong test comment để người sau không
   nhầm là bằng chứng của lỗ.
9. Positive regression: `Unlink("/tmp/x")` và `RmdirRecursive("/tmp/d")` vẫn hoạt động.
10. `cargo clippy -- -D warnings`, build + boot QEMU, chạy suite VFS.

## Todo List

- [x] Khảo sát test harness VFS hiện có, chọn điểm chèn (`vfs-test` cell, `test_access_control`)
- [x] Gate `Rmdir`
- [x] Gate `Unlink` (trước cả `file_size`, không chỉ trước `unlink`)
- [x] Gate `RmdirRecursive` (trước `collect_dir_bytes`)
- [x] Gate `WriteGrant` → fail-closed `Err(3)`, refuse trước khi chạm grant
- [x] Xoá `MountEntry::writable` (chọn xoá, không enforce — xem Evidence → Quyết định)
- [x] Negative test **trên `/`** (RamFS): `/readme.txt`
- [ ] ~~Negative test phải fail trước fix~~ — **CHƯA CHỨNG MINH**, xem Evidence → Chưa đạt
- [x] Secondary test `/` non-existent → `Err(3)` thay `Err(1)` (chứng minh thứ tự check)
- [x] Positive regression: `/tmp/`, `/data/`, `/srv/` không đổi hành vi (36/36 suite pass)
- [x] clippy `-D warnings` + build + boot QEMU + suite VFS pass

## Success Criteria

**Done khi**

- Mọi arm trong `handle_request` có tác dụng mutation đều đi qua `can_write`; grep
  `VfsRequest::` vs grep `can_write` trong `dispatch.rs` khớp số lượng arm mutation.
- Negative test fail trước khi fix, pass sau khi fix (chứng minh test thực sự bắt được lỗi).

**Validation**

- Suite VFS pass đủ số scenario cũ + scenario mới.
- Boot QEMU tới shell prompt bình thường (không self-inflicted PermissionDenied trên đường boot —
  đặc biệt: `pkg` shell built-in và bất kỳ chỗ nào ghi `/srv/` hoặc dọn `/tmp/`).

## Risk Assessment

| Rủi ro | Xác suất | Ảnh hưởng | Giảm thiểu |
|--------|----------|-----------|------------|
| Đường boot/tool nội bộ đang dựa vào việc xoá không bị chặn | Trung bình | Boot vỡ | Chạy full boot + suite trước khi commit; log `Err(3)` kèm path để lộ ra ngay |
| **Cross-plan**: `.agents/260712-1000-cell-package-distribution/phase-01-writable-cell-store.md` muốn mở `/bin/` writable — đụng đúng `access.rs:33` + `backend_bin_overlay.rs:63-68` mà phase này siết | Cao | Hai plan đẩy ngược nhau trên cùng dòng | Precedence đã chốt trong `plan.md` → Dependencies: pkg dùng **rule per-cell** cho `/bin/` (cần rule shape của phase 02), KHÔNG flip `allow_write_all` toàn prefix. `pkg` là **shell built-in**, không phải cell riêng — nên ngoại lệ nếu có sẽ thuộc `/bin/shell`, chính cell mà phase 04 đang deprivilege. Không cấp ngoại lệ trong phase 01. |
| Không có mechanism để cấp ngoại lệ trong phase 01: `can_write(_cell, path)` bỏ qua danh tính (`access.rs:80-87`) | Chắc chắn | Ngoại lệ không diễn đạt được | Chấp nhận: phase 01 chỉ gate, không cấp ngoại lệ. Mọi ngoại lệ chờ rule shape per-cell của phase 02. |
| Thay `WriteGrant` stub sang fail-closed làm vỡ caller đang gọi | Thấp | Test vỡ | Grep caller của `WriteGrant`; hiện là stub nên khả năng thấp |

## Security Considerations

- **Đây là phase sửa lỗ, không phải phase thêm feature** — thứ tự "check trước mutation" là bất
  biến phải giữ, không được đổi thành check-sau-cho-tiện.
- Fail-closed là mặc định: `can_write` đã trả `false` khi không match rule nào
  ([access.rs:86](../../cells/services/vfs/src/access.rs#L86)) — giữ nguyên tính chất đó.
- Không log nội dung path ở mức có thể rò rỉ; log path là chấp nhận được (path không phải secret
  trong Cellos), nhưng không log content.
- **Open finding (KHÔNG được fix ở phase này, và bản draft claim sai)**: quota release ghi vào sổ
  của *caller* (`release(owner, size)`, [quota.rs:60-64](../../cells/services/vfs/src/quota.rs#L60-L64))
  còn charge thì ghi cho người *đã ghi file* ([dispatch.rs:66](../../cells/services/vfs/src/dispatch.rs#L66)).
  Draft claim rằng phase 01 "thu hẹp kênh làm sai lệch quota" — **sai**: `can_write` bỏ qua danh
  tính cell và `/tmp`, `/data`, `/mnt/sd`, `/srv` đều `allow_write_all: true`, nên sau phase 01
  cell A vẫn xoá được file của cell B ở các prefix đó và vẫn được credit vào sổ của A (A tiến tới
  budget vô hạn, B bị charge cho file không còn tồn tại và cuối cùng `Err(2)` mọi lần ghi). Cả hai
  hướng đều **âm thầm** — không denial, không audit event. Fix thật cần danh tính per-cell → thuộc
  phase 02.

## Evidence

**Diff**: 4 file, +107/−35 trên branch `feat/vfs-destructive-authz`
(`cells/services/vfs/src/{dispatch,mount,manager}.rs` + `cells/tests/vfs-test/src/main.rs`).

**Đã đạt**

| Tiêu chí | Bằng chứng |
|----------|-----------|
| Mọi arm mutation đi qua `can_write` | `Rmdir`/`Unlink`/`RmdirRecursive` gate trước mọi mutation VÀ trước mọi bước đọc phụ (`file_size`, `collect_dir_bytes`) |
| `WriteGrant` fail-closed | Trả `Err(3)` trước khi chạm grant; khối `unsafe` cũ bị xoá hẳn (nó đọc buffer rồi bỏ). 0 caller thật — `ostd::fs::write_all` không cell nào gọi |
| clippy `-D warnings` | `service-vfs` exit 0, `app-vfs-test` exit 0, với `.cargo/config.toml` đã copy vào worktree (không có nó thì build thiếu `relocation-model=pic`) |
| Suite VFS | **36 PASS / 0 FAIL**, `[vfs-test] ALL TESTS PASSED`. 9427 byte serial **tôi tự capture** qua TCP probe (mirror `tests/integration/src/lib.rs::boot_rv64`), không lấy theo báo cáo agent |
| 5 assertion mới | `unlink /readme.txt → PermissionDenied` · `/readme.txt survived the refused unlink` · `rmdir under / → PermissionDenied` · `rmdir_recursive under / → PermissionDenied (before subtree walk)` · `WriteGrant is fail-closed` |
| Không hồi quy | 31 assertion cũ vẫn pass (lifecycle, dir ops, async read, RamFS, edge cases, quota, rmdir-recursive quota) |

**Negative control — ĐÃ ĐẠT (2026-07-28, sau khi sửa build tooling)**

Lúc commit `72f01d0d2` thì tiêu chí *"negative test fail trước khi fix"* **chưa** chứng minh được:
revert gate rồi rebuild cho ra image kernel không đọc được (`/bin/vfs not in VIFS1`), và lỗi đó tái
hiện cả khi restore fix → lỗi tooling, không phải regression.

Nguyên nhân gốc tìm ra ở `b26a896bb`: dưới Git Bash, **MSYS rewrite mọi argument trông giống POSIX
path** trước khi native Windows exe nhìn thấy nó, nên tham số đích `/bin/...` truyền cho
`mkfat32.py` đến nơi thành `C:/Program Files/Git/bin/...`. Image là FAT16 hợp lệ, `mkfat32` exit 0,
root dir chứa một thư mục tên `C:` — chỉ là **không có `/bin`**. Kernel boot, mount VIFS1, rồi báo
mọi cell "not found". `build-shell-test-ci.sh` đã có guard từ trước; ba script còn lại thì chưa.

Với tooling đã sửa, negative control chạy được: revert `dispatch.rs` → **31 PASS / 5 FAIL**, đúng
5 assertion mới, trong đó có `/readme.txt survived the refused unlink` FAIL — tức file bị xoá thật,
lỗ hổng là reachable chứ không phải lý thuyết. Restore → 36/36.

Hệ quả rộng hơn: `qemu-boot-test.sh` chỉ assert "FAT16 mounted" chứ không assert có cell nào load
được, nên lane boot rv64 đã PASS nhiều tháng trên các image **không chứa cell nào**.

**Quyết định trong lúc build**

- `MountEntry::writable` → **xoá** (không enforce). Giá trị của nó trùng gần khít `AccessTable`, và
  giữ nguồn thẩm quyền thứ ba nửa vời dễ lệch mà vẫn tạo cảm giác đang enforce. Read-only mang tính
  *cấu trúc* đã do backend lo (`BinOverlay`). Xoá luôn tham số thứ 3 của `mount()` (6 call site) —
  để lại một tham số bị bỏ là cùng mùi ở tầng trên. DRY.
- Negative test dùng path **không tồn tại** cho `Rmdir`/`RmdirRecursive` thay vì dir thật: nếu ai
  chạy suite trên build chưa gate, một `RmdirRecursive("/")` thật sẽ xoá sạch filesystem. Err(1)→Err(3)
  đủ chứng minh thứ tự check mà không mang rủi ro phá huỷ.

## Next Steps

- ~~**Follow-up mới (từ phase này)**: script image không dùng được trên Windows~~ → **ĐÃ SỬA**
  ở `b26a896bb`: cả 4 script nhận `MSYS2_ARG_CONV_EXCL='*'` quanh lời gọi `mkfat32`, probe
  `PYTHON_BIN` (tên trần `python3` trên Windows là Microsoft Store alias stub), `mktemp` dưới
  `target/` thay vì `/tmp` (POSIX temp path native Windows Python không mở được) + cleanup bằng EXIT
  trap, và **assertion sau build** dùng `inspect_fat.py` kiểm tra image thật sự có `/bin` + cell mà
  lane đó cần. Chưa phủ: `scripts/mksrv-img.sh` (ghi dưới `build/`, bị guard chặn) — lane redoxfs-srv
  do CI Linux chạy, nơi không có MSYS conversion.
- ~~**Papercut**: `tools/__pycache__` không có trong `.gitignore`~~ → đã thêm ở `b26a896bb`.
- **Worktree gotcha**: `.cargo/config.toml` bị gitignore nên `git worktree add` không mang theo →
  worktree build **âm thầm** thiếu `relocation-model=pic` + `CC_riscv64gc_unknown_none_elf`. Phải
  copy tay sau khi tạo worktree.
- Phase 02 nối read gating + ACL theo danh tính cell (Law 1: kernel attest `cell_id`), và mang theo
  fix cho open finding quota ở trên.
- Ghi nhận cho phase 06: sau khi có directory capability, `AccessTable` prefix rule trở thành
  lớp phòng thủ thứ hai, không phải lớp duy nhất.
