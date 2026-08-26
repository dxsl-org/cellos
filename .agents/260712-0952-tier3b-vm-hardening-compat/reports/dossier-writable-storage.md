# Dossier 3a — virtio-blk writable: per-VM image-file backing

**Cho:** P04 · **Nguồn:** research rootfs-build (a2267…) + code + red-team M1/A2/F4 · **Trạng thái:** ready-to-code

## Kết luận chốt — backing isolation là quyết định an toàn, không phải chi tiết
- **Backing = image-file (ext4) per-VM**, phục vụ qua VFS. **KHÔNG BAO GIỜ** shared cell-store (`PART_CELLSTORE`). Nếu dùng cell-store: guest ghi sector tùy ý → đè FAT/cell-table/ELF cell khác = **guest→host-disk escape** (bounds-check RAM không chặn). Đây là invariant P02.
- Image-file này CHÍNH là `debian-arm64.img` dossier 3b tạo (`mkfs.ext4 -m 0 -d` under fakeroot) → P04 và P05 dùng chung artifact.
- **Sector clamp:** hiện `off >= disk.len()` break (`virtio_blk.rs:94,107`) tính theo `DISK_SIZE=16MiB` Vec. Khi chuyển sang file-backed, clamp phải theo **kích thước backing thật** (query từ VFS); off-by-one = ghi ngoài vùng backing sang dữ liệu VFS kề.

## Manifest / capability (F4)
- Cell hiện: `block_io=false`, chỉ `OpenCap/ReadCap/CloseCap` (`main.rs:13,26`). Persist ghi ra VFS cần **write-cap** (+ có thể `block_io=true`). Đây là **cell manifest change, KHÔNG libs/api** → không Law 1, nhưng MỞ bề mặt → nêu rõ + `haily-reviewer`.

## Layering
- Persist: guest mount rw thẳng trên image (`root=/dev/vda1 rw`, dossier 3b) → `apk/apt add` sống qua reboot.
- Tạm: guest tự overlay tmpfs nếu muốn ghi phù du (guest-side, không cần VMM làm gì).
- Ghi atomic ra VFS: KHÔNG có `VfsRequest::Rename` (memory) → pattern copy + `.prev`. Lưu ý TOCTOU (cửa sổ giữa check-path và write) + gấp đôi dung lượng tạm. Chỉ cần nếu làm snapshot/atomic-swap; ghi thẳng vào image file thì không cần.

## KHÔNG làm (sửa hướng M1)
- **Bỏ path-traversal guard (CVE-2026-1386) khỏi P04.** virtio-blk địa chỉ theo *sector*, không có per-request path (`virtio_blk.rs:75-76` chỉ `type`+`sector`). Guard canonicalize/`..`/symlink là chuyện virtio-**fs**/9p — chỉ thêm NẾU sau này có virtio-fs, không phải bây giờ.

## Ready-to-code checklist (P04)
- [ ] Backing file per-VM đặt dưới thư mục VFS do VM sở hữu; cấp phát/nạp qua VFS; KHÔNG chạm cell-store.
- [ ] `virtio_blk.rs`: bỏ RO feature bit; `blk_write` (`:81`) → ghi backing file qua VFS (Law 2: `Box<[u8]>` trước `.await`).
- [ ] Sector-range clamp theo backing size thật (query VFS), mirror `registry.rs:311-317`.
- [ ] `main.rs` manifest: thêm write-cap (+`block_io` nếu cần); document surface mở.
- [ ] Test: (a) ghi → reboot VM → còn; (b) sector ngoài backing → từ chối; (c) guest KHÔNG đọc/ghi được cell-store hay image VM khác.
- [ ] `haily-reviewer` theo rubric P02 (backing-store isolation invariant).

## Rủi ro / mở
- FAT/ext4 hỏng nếu ghi sai offset → test image throwaway.
- copy+.prev TOCTOU nếu chọn atomic-swap — chốt có cần không (ghi in-place vào image thì bỏ qua).
