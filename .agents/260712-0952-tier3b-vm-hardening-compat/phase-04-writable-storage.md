# Phase 04 — virtio-blk RW: per-VM image-file backing + sector clamp

**Cửa sổ:** post-window (coding) · **Priority:** P1 · **Status:** pending · **Tier:** thinking · **Effort:** L · **Law 1:** no (chỉ cell manifest)

> ⚠️ **Sửa hướng sau Red Team (M1/A2/F4).** virtio-blk địa chỉ theo **sector**, không có path → path-guard CVE-2026-1386 nhắm SAI bề mặt. Rủi ro thật là backing-store isolation + sector bound.

## Context Links
- Dossier: `reports/dossier-writable-storage.md` (P03) · threat-model P02 (backing invariant)
- Code: `virtio_blk.rs:15,33` (16MiB Vec volatile), `:76` (sector guest-supplied), `:94,107` (`off>=disk.len()` clamp), `main.rs:13` (`block_io=false`)
- Red-Team M1/A2/F4

## Overview
Nâng virtio-blk lên RW + persist để `apt install` sống qua reboot VM — tiền đề cứng cho P05 (systemd cần root ghi được).

## Key Insights (từ Red Team, xác minh code)
- **M1 — path-guard nhắm sai:** không có per-request path trong virtio-blk (`virtio_blk.rs:75-76` chỉ có `type`+`sector`). CVE-2026-1386 là bài học virtio-**fs**/9p/jailer, không áp cho block. Bỏ path-guard khỏi P04 (giữ lại CHO virtio-fs nếu sau này có).
- **A2 — rủi ro thật = backing isolation:** nếu backing là shared cell-store (`PART_CELLSTORE`), guest ghi sector tùy ý → đè FAT/cell-table/ELF cell khác = **guest→host-disk escape** mà bounds-check RAM KHÔNG chặn. → backing PHẢI là **image-file/partition riêng từng VM**.
- **A2 — sector clamp:** `off>=disk.len()` break (`virtio_blk.rs:94,107`) phải tính lại theo **backing thật** khi chuyển từ Vec sang file; off-by-one cho ghi ra ngoài vùng backing.
- **F4 — manifest change:** cell hiện `block_io=false` + chỉ `OpenCap/ReadCap/CloseCap` (`main.rs:13,26`). Persist ra VFS cần write-cap + có thể `block_io=true` → đổi cell manifest (KHÔNG libs/api, nhưng mở surface → nêu rõ + review).

## Requirements
**Functional:**
- Backing = **image-file per-VM** (hoặc partition riêng VM), NEVER shared cell-store. Kích thước ≥ nhu cầu rootfs P05.
- virtio-blk RW: bỏ RO feature bit; `blk_write` (`virtio_blk.rs:81`) ghi xuống backing file qua VFS, persist.
- Sector-range clamp theo backing size thật ở biên VMM/VFS (mirror `registry.rs:311-317`).
- Cell manifest: thêm write-cap (+`block_io` nếu cần); ghi rõ surface mở thêm.
- Overlay tmpfs trong guest cho ghi tạm (option), backing file cho persist.
**Non-functional:** vẫn qua wrapper bounds-check; Law 2 (`Box<[u8]>` trước `.await` IPC VFS).

## Related Code Files
- Modify: `virtio_blk.rs` (RW bit, backing từ Vec→file-backed, sector clamp), `main.rs` (manifest write-cap), VFS-forward write path.
- Không có VFS `Rename` (memory) → ghi atomic bằng copy+.prev (chú ý TOCTOU + gấp đôi dung lượng — dossier chốt).

## Implementation Steps
1. Theo checklist dossier 3a: chọn image-file backing per-VM.
2. Cấp phát/nạp backing file riêng VM qua VFS; KHÔNG chạm cell-store.
3. Bật RW; `blk_write` → backing file; sector clamp theo size thật.
4. Cell manifest write-cap; review surface.
5. `haily-tester`: guest ghi → reboot VM → còn; test sector ngoài biên bị từ chối; test KHÔNG chạm được cell-store.

## Todo
- [ ] Backing = image-file per-VM (không shared cell-store)
- [ ] RW bit + blk_write→backing file persist
- [ ] Sector-range clamp theo backing thật
- [ ] Cell manifest write-cap + review surface
- [ ] Test persist qua reboot + test isolation cell-store
- [ ] `haily-reviewer` theo rubric P02 (backing invariant)

## Success Criteria
- `apk/apt add <pkg>` còn sau reboot VM.
- Guest ghi sector ngoài backing → từ chối; KHÔNG đọc/ghi được cell-store hay VM khác.
- Không regression suite hypervisor.

## Risk Assessment
- **Cao nhất về an toàn disk:** backing sai chỗ = host-disk escape. Mitigate: backing per-VM + test isolation bắt buộc + review rubric P02.
- FAT hỏng nếu ghi sai offset → test image throwaway.

## Security Considerations
- Backing-store isolation là invariant P02; qua `haily-reviewer` bắt buộc.

## Next Steps
- Mở khóa P05 (root-on-blk cần backing ghi được, ≥150MiB).
