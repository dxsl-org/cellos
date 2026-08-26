# Phase 03 — Design Dossiers (storage / glibc / fuzz / bench)

**Cửa sổ:** do-now (design artifacts, hợp Mythos) · **Priority:** P1 · **Status:** ✅ done (2026-07-12) · **Tier:** thinking · **Law 1:** no

## Context Links
- Research §2, §3, §4, §5 · threat-model P02 (rubric)
- Outputs feed: P04, P05, P06, P07

## Overview
Bốn dossier thiết kế chi tiết (không code) để phase coding sau cửa sổ Mythos thực thi thẳng. Mỗi dossier lưu trong `reports/` của plan này.

## Key Insights
- Dossier = ranh giới an toàn của cửa sổ Mythos: quyết định thiết kế, giao diện, sơ đồ dữ liệu, thứ tự thao tác — nhưng KHÔNG viết code sản phẩm.
- Storage writable rủi ro thật là **backing-store isolation** (per-VM image-file, không shared cell-store) + sector clamp — KHÔNG phải path-guard (virtio-blk địa chỉ theo sector, không có path per-request; CVE-2026-1386 không áp — đã re-scope sau Red Team M1/A2, xem P02).
- Fuzz dossier phải chọn được harness chạy host (virtqueue parser tách được khỏi syscall? cần shim `read_guest_memory` giả lập buffer).

## Requirements — 4 dossier

### 3a. `dossier-writable-storage.md`
- virtio-blk đã ghi được hôm nay (`blk_write` xử lý BLK_T_OUT) nhưng backing là 16MiB Vec **volatile** — mục tiêu P04 là PERSIST, không phải "bật RW" (đã bật).
- Backing: image-file per-VM (KHÔNG shared cell-store — invariant an toàn, xem P02) + sector clamp theo backing thật; overlay tmpfs layering trong guest cho ghi tạm.
- KHÔNG cần path-guard (virtio-blk theo sector, không path — CVE-2026-1386 không áp). Nêu API VFS cần cho persist (`VfsRequest` nào; có Rename không — theo memory là KHÔNG → dùng copy+.prev nếu cần atomic-swap).
- Bất biến: vẫn qua wrapper bounds-check; không deref thô.

### 3b. `dossier-glibc-guest.md` (⚠️ mở rộng sau Red Team F1/F2/B6 — đây là boot rework, không phải script)
- **Boot model:** chuyển initramfs→shell (`dtb.rs:22 rdinit=/bin/sh`) sang **root-on-blk** (`root=/dev/vda init=/sbin/init`). Chốt bootargs + DTB node.
- **Disk backing:** rootfs image ≥150MiB thật (qua P04 image-file backing) — không phải 16MiB Vec (`virtio_blk.rs:15`). Xác định kích thước + cách nạp.
- **Guest RAM:** bump >128MiB (`main.rs:55`); chốt số.
- **Init system:** MVP `init=/bin/sh` trước, rồi systemd; liệt kê device systemd đòi.
- **RTC + virtio-rng (B6, tiền đề chạy TLS software):** thiết kế wire Goldfish RTC + virtio-rng device vào guest. KHÔNG có = P05 fail mục tiêu.
- **Device gap analysis:** Debian kernel probe gì mà VMM chưa emulate? (nhiều PCI/ACPI/RTC MMIO) — nếu lớn, đề xuất tách phase con.
- Build: debootstrap vs mmdebstrap; kernel ELF (ARM64 entry, x86 PVH vmlinux).
- Số rootfs: 1 (Debian-only) hay 2 (giữ Alpine) — chờ user (B4).

### 3c. `dossier-virtqueue-fuzz.md` (⚠️ phải CHỨNG MINH tính khả thi refactor — Red Team F3)
- **Refactor production bắt buộc:** `process_notify` gọi thẳng `crate::vmm::read/write_guest_memory` (`virtqueue.rs:43,61`), host trả usize::MAX → 0 coverage. Dossier PHẢI thiết kế đổi chữ ký nhận **memory-backend** (trait/closure), cập nhật mọi caller (blk/net/console). Precedent `loader_image.rs:68 place_images<W>`. Production chạy CÙNG parser fuzzer test — nếu không tách được, đề xuất fuzz in-guest (fallback, ghi rõ giá trị thấp hơn).
- Fuzz targets: desc chain vòng, `next`/`cur` ngoài biên (clamp `cur<q_size`), len tràn, `avail_idx` nhảy (cap delta), writable-flag mismatch.
- Property invariants từ threat-model P02 (gồm C1 resource-exhaustion + Mn3 blk_read/write robustness).
- Chọn công cụ: `cargo fuzz` (libFuzzer) hay proptest (no_std-friendly?).

### 3d. `dossier-virtio-benchmark.md`
- Nghẽn đã xác định: 1 syscall + 1 lock/descriptor (`virtqueue.rs:61`, `registry.rs:307`).
- Thiết kế phép đo: throughput blk/net, latency trap, boot time — trên QEMU (ghi caveat TCG).
- Tối ưu đề xuất: batch-read cả desc table trong 1 syscall; đo trước/sau.
- Mở rộng `cells/apps/bench` hay guest-side benchmark.

**Non-functional:** mỗi dossier có "Ready-to-code checklist" để P0x chỉ việc thực thi.

## Related Code Files
- Create: `reports/dossier-writable-storage.md`, `reports/dossier-glibc-guest.md`, `reports/dossier-virtqueue-fuzz.md`, `reports/dossier-virtio-benchmark.md` (trong thư mục plan).
- Đọc: virtio_blk.rs, virtqueue.rs, virtio_mmio.rs, registry.rs, loader_image.rs, scripts build hiện có.

## Implementation Steps
1. Spawn `haily-researcher` song song cho câu hỏi mở mỗi dossier (debootstrap vs mmdebstrap; cargo-fuzz no_std; FAT32-RW an toàn).
2. Viết 4 dossier; mỗi cái kết bằng "Ready-to-code checklist" + file:line điểm sửa.
3. Đối chiếu từng dossier với rubric threat-model P02 (dossier storage & fuzz phải trả lời mọi GAP liên quan).

## Todo
- [ ] dossier-writable-storage (VFS path-guard là mục bắt buộc)
- [ ] dossier-glibc-guest (build path + rootfs selection)
- [ ] dossier-virtqueue-fuzz (host harness + targets)
- [ ] dossier-virtio-benchmark (đo + batch-read design)
- [ ] Đối chiếu rubric P02

## Success Criteria
- 4 dossier đủ chi tiết để P04-P07 code không cần quyết định kiến trúc mới.
- Mỗi dossier có checklist + điểm sửa file:line.

## Risk Assessment
- Dossier fuzz rủi ro nhất: nếu parser không tách được khỏi syscall thì harness host bất khả thi → dossier phải chứng minh tách được (thiết kế trait shim) hoặc đề xuất fuzz in-guest.

## Security Considerations
- Dossier storage & fuzz trực tiếp phục vụ đóng GAP an toàn của P02.

## Next Steps
- Hết cửa sổ Mythos → P04/P05/P06/P07 thực thi theo dossier.
