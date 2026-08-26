# Phase 06 — virtqueue+IRQ fuzz & hardening (backend refactor + live-bug fixes)

**Cửa sổ:** post-window (coding) · **Priority:** P1 · **Status:** pending · **Tier:** thinking · **Effort:** L · **Law 1:** no

> ⚠️ **Re-scoped sau Red Team (F3/M3/A1/C1/Mn2/Mn3).** Fuzz đòi một refactor production BẮT BUỘC; "syscall allowlist" bị CUT (đã tồn tại + theater dưới LBI).

## Context Links
- Dossier: `reports/dossier-virtqueue-fuzz.md` (P03) · threat-model P02 (rubric + C1)
- Code: `cells/services/hypervisor/src/virtqueue.rs`, `virtio_blk.rs`; `kernel/src/hypervisor/registry.rs`
- Precedent refactor: `loader_image.rs:68` (`place_images<W>` thread closure)

## Overview
Parser desc chain là bề mặt tấn công #1 (class CVE-2026-5747). Làm fuzzable được (đòi refactor), fuzz, sửa các bug LIVE tìm được + đã biết, cap tài nguyên kernel (C1).

## Key Insights (từ Red Team, xác minh code)
- **F3 — fuzz đòi refactor production:** `process_notify` gọi thẳng `crate::vmm::read/write_guest_memory` (`virtqueue.rs:43,49,61,82,86`); trên host trả `usize::MAX` (`vmm.rs:25-27`) → `!= 16` → break → **0 coverage**. Phải đổi chữ ký nhận **memory-backend** (trait/closure) + sửa mọi caller (blk/net/console). Production phải chạy CÙNG parser fuzzer test.
- **A1 — clamp thiếu:** `cur` (next-index, u16 tới 65535) KHÔNG clamp `< q_size` (`virtqueue.rs:67,71`); hiện an toàn nhờ kernel trả `!=16`→break, nhưng batch (P07) mất backstop đó → thêm clamp `cur < q_size` như sửa parser độc lập.
- **C1 — LIVE kernel DoS:** `inject_irq` push_back không cap (`registry.rs:398`) + `avail_idx` delta không cap (`virtqueue.rs:46`) → guest làm cạn kernel-heap/lock. Cap cả hai.
- **M3/B5 — allowlist CUT:** cell đã `declare_syscalls!` hẹp (`main.rs:22-32`); siết thêm là theater dưới LBI + sẽ hỏng cell. Không làm; chỉ ghi khuyến nghị vào P02.
- **Mn2 — invariant trigger:** contract comment phải nêu đủ: no-SMP + no-async-vcpu + same-core sync (`registry.rs:182-217`).
- **Mn3 — bug giòn:** `blk_write` sentinel `usize::MAX` (`virtio_blk.rs:111`); `blk_read` bỏ return của `write_guest_memory` (`:96`) → im lặng drop vẫn báo OK.

## Requirements
**Functional:**
1. Refactor `process_notify` (+device models) nhận memory-backend → fuzzable host. Production dùng backend syscall-thật.
2. Fuzz harness host; targets: chain vòng, `next`/`cur` ngoài biên, len tràn, `avail_idx` nhảy, writable-flag mismatch.
3. Thêm clamp `cur < q_size`.
4. Thêm assert `buf.writable` khớp chiều device (thiếu hiện tại, `virtqueue.rs:69` chỉ ghi flag).
5. Cap độ sâu IRQ queue (`registry.rs:398`, kernel-side) + cap `avail_idx` delta ≤ `q_size`.
6. Sửa Mn3: `blk_read` xử lý return; `blk_write` không phụ thuộc sentinel giòn (hoặc document + test).
7. Contract comment single-thread invariant đủ trigger (Mn2).

**Non-functional:** cell vẫn `#![forbid(unsafe_code)]`; production và fuzz chia sẻ cùng parser.

## Related Code Files
- Modify: `virtqueue.rs` (backend param, clamp, writable assert, avail cap, contract), `virtio_blk.rs`/console/net (caller update, blk_read/write fix), `kernel/src/hypervisor/registry.rs` (IRQ queue cap — kernel một dòng).
- Create: fuzz harness (vd `cells/services/hypervisor/fuzz/` theo dossier).

## Implementation Steps
1. Refactor backend param (F3) — production build xanh, cùng parser.
2. Harness + chạy fuzz tới ngưỡng dossier.
3. Triage + sửa; thêm clamp + writable assert + Mn3 fixes.
4. Cap IRQ queue + avail delta (C1); test guest spam không làm OOM.
5. Contract comment (Mn2).
6. `haily-reviewer` theo rubric P02.

## Todo
- [ ] Refactor process_notify memory-backend (production, F3)
- [ ] Fuzz harness + chạy
- [ ] clamp cur<q_size (A1)
- [ ] assert buf.writable direction
- [ ] cap IRQ queue depth + avail delta (C1)
- [ ] fix blk_read return + blk_write sentinel (Mn3)
- [ ] contract comment invariant (Mn2)
- [ ] review theo rubric P02

## Success Criteria
- Fuzz chạy trên CÙNG parser production dùng; 0 panic chưa xử lý tới ngưỡng dossier.
- Guest spam QueueNotify/avail không làm cạn kernel (C1 đóng).
- clamp + writable assert + Mn3 vào code; contract comment đủ trigger.

## Risk Assessment
- Refactor hot-path #1 attack-surface → phải regression suite hypervisor đầy đủ; precedent `place_images<W>` giảm rủi ro.
- Nếu parser không tách được → fallback fuzz in-guest (chậm, ít giá trị) — dossier P03 phải chốt tính khả thi trước.

## Security Considerations
- Phase an toàn cốt lõi; bất biến bounds-check (`registry.rs:311-317`) tuyệt đối giữ nguyên qua refactor.
- C1 là bug LIVE — ưu tiên cap ngay cả khi fuzz chưa xong.

## Next Steps
- P07 benchmark trên parser đã hardened; P08 x86 tái dùng parser này.
