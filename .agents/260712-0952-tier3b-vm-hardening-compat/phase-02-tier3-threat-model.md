# Phase 02 — Tier-3 Threat Model → docs/specs/05-application.md

**Cửa sổ:** do-now (design doc, hợp Mythos) · **Priority:** P1 · **Status:** ✅ done (2026-07-12) · **Tier:** thinking · **Law 1:** no

## Context Links
- Research §2 (an toàn), §5 (so chiếu Firecracker/pKVM), §"Unresolved" #4
- Kernel boundary law: `CLAUDE.md` §Kernel Boundary Law, `docs/specs/15-kernel-boundary.md`
- Target: mục mới trong `docs/specs/05-application.md`

## Overview
Chưa có threat-model chính thức cho guest-escape ở Tier 3. Viết mục "Tier 3 Threat Model" liệt kê bề mặt tấn công, bất biến phòng thủ, và mô hình đối thủ — làm cơ sở đánh giá cho P04/P06/P08.

## Key Insights (mở rộng sau Red Team)
- Bề mặt tấn công guest→host xếp hạng: (1) parser desc chain `virtqueue.rs`, (2) wrapper `read/write_guest_memory`, (3) `inject_irq` intid, (4) backing-store isolation (writable virtio-blk, sector-addressed), (5) MMIO dispatch default arm, **(6) PSCI/HVC dispatch `psci.rs:44` (P09 CPU_ON index guest-controlled), (7) config-space reads, (8) resource-exhaustion**.
- **C1 (LIVE, Critical) — resource-exhaustion:** `inject_irq` push_back không cap độ sâu (`registry.rs:398`) + `avail_idx` delta không cap (`virtqueue.rs:46`). Guest mask IRQ + spam QueueNotify → kernel-heap/lock cạn → SAS nghĩa là **chết cả máy**, never-die supervisor không cứu được kernel OOM. Threat-model PHẢI liệt kê class này; đóng ở P06.
- **Backing-store isolation (M1/A2):** virtio-blk theo sector (không path) → CVE-2026-1386 path-guard KHÔNG áp. Invariant thật: backing = image-file per-VM, không shared cell-store; sector clamp theo backing thật. Ghi thành bất biến load-bearing.
- **Allowlist (M3/B5) — chỉ khuyến nghị, KHÔNG task:** cell đã `declare_syscalls!` hẹp (`main.rs:22-32`). Siết thêm là theater dưới LBI. Ghi rõ: giá trị duy nhất = giảm blast-radius của bug unsafe-dep (`alloc`) hoặc rustc miscompile (specs/16), KHÔNG phải "jailer".
- Cellos ĐÃ có lá chắn chính: kernel bounds-check tập trung (`registry.rs:311-317`) → miễn nhiễm class CVE-2026-5747 (Firecracker virtio OOB write). Threat-model phải ghi bất biến này là **load-bearing, không được bỏ**.
- pKVM (Google): state VM kernel-owned, TCB nhỏ ở EL2 — Cellos khớp. Ghi làm nguyên tắc.
- ~~Firecracker "jailer" allowlist~~ — **đã re-scope ở dòng 17**: cell đã có allowlist hẹp sẵn (`main.rs:22-32`); siết thêm CHỈ là khuyến nghị có-điều-kiện (giảm blast-radius unsafe-dep/rustc), KHÔNG phải task bắt buộc, KHÔNG gọi là "jailer-equivalent" (đó là mô hình process Linux, không áp cho cell LBI).
- ~~CVE-2026-1386 path-traversal guard cho VFS-forward~~ — **đã re-scope ở dòng 16**: virtio-blk địa chỉ theo sector, không có path per-request → guard này KHÔNG áp dụng bây giờ. Chỉ cần nếu sau này có virtio-fs.

## Requirements
**Functional — mục threat-model gồm:**
1. Tài sản cần bảo vệ (host RAM ngoài guest, kernel TCB, cell khác, CapSet).
2. Mô hình đối thủ (guest root độc hại; guest kernel bị khai thác).
3. Bảng bề mặt tấn công + bất biến phòng thủ hiện có + file:line chứng cứ.
4. Bất biến KHÔNG được vi phạm (bounds-check tập trung, cell không deref thô, shadow-GICD, single-thread vCPU assumption, Stage-2 SAS-isolation guard, resource ceiling).
5. Khoảng trống đã biết + phase đóng chúng (fuzz+IRQ-cap P06, backing-store isolation P04). Allowlist = khuyến nghị không bắt buộc (không phải phase riêng); path-guard KHÔNG áp dụng (loại khỏi scope).
6. Non-goal: nested VM. Ghi nhận ngoại lệ đã audit: GICV MMIO passthrough (vGIC) là passthrough phần cứng duy nhất, cố ý bypass SAS-isolation guard — không phải "0 passthrough tuyệt đối".

**Non-functional:** khớp giọng văn spec hiện có; cross-ref `15-kernel-boundary.md` và `16-rustc-tcb.md`.

## Related Code Files
- Modify: `docs/specs/05-application.md` (thêm mục "Tier 3 Threat Model").
- Đọc tham chiếu (không sửa): `cells/services/hypervisor/src/virtqueue.rs`, `kernel/src/hypervisor/registry.rs`, `kernel/src/memory/stage2.rs`.

## Implementation Steps
1. Đọc lại 5 file nguồn để trích file:line chính xác cho bảng bất biến.
2. Viết mục theo cấu trúc STRIDE-lite (Spoofing/Tampering/Info-disclosure/DoS/Elevation) áp cho guest-escape.
3. Bảng: mỗi bề mặt tấn công → phòng thủ hiện có (file:line) hoặc "GAP → phase X".
4. Ghi rõ 4 bất biến load-bearing + hậu quả nếu vi phạm.
5. Cross-ref sang 15/16 và plan này.

## Todo
- [ ] Trích file:line cho từng bất biến
- [ ] Viết mô hình đối thủ + tài sản
- [ ] Bảng bề mặt tấn công ↔ phòng thủ/GAP
- [ ] Mục khuyến nghị (không bắt buộc) thu hẹp syscall allowlist — ghi rõ lý do thật (blast-radius unsafe-dep/rustc), không phải "jailer"
- [ ] Cross-ref 15/16 + phase files

## Success Criteria
- Mỗi bề mặt tấn công có hoặc phòng thủ (file:line) hoặc phase đóng.
- `haily-reviewer` xác nhận không bỏ sót bề mặt so với virtqueue/registry code thực tế.
- Được dùng làm rubric đánh giá cho P04/P06/P08.

## Risk Assessment
- Rủi ro: threat-model bỏ sót một đường thoát → mitigate bằng đối chiếu trực tiếp với code (bước 1) thay vì viết trừu tượng.

## Security Considerations
- Đây LÀ tài liệu bảo mật; áp dụng "clarity override" — dùng câu đầy đủ, không viết tắt cảnh báo.

## Next Steps
- Cung cấp rubric cho dossier fuzz (P03) và thực thi P06.
