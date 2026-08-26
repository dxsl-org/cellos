# Phase 07 — virtio benchmark (batch-read cut from default)

**Cửa sổ:** post-window (coding) · **Priority:** P2 · **Status:** pending · **Tier:** medium · **Effort:** S · **Law 1:** no

> ⚠️ **Re-scoped sau Red Team (A1/B5/F4/Mn4).** Batch-read CUT khỏi default (mâu thuẫn logic + premature). Còn lại: đo thật, thay số bịa trong doc.

## Context Links
- Dossier: `reports/dossier-virtio-benchmark.md` (P03) · Research §3 · Red-Team A1/F4/Mn4
- Code: `virtqueue.rs:61,86`, `registry.rs:307`

## Overview
Đo hiệu năng thật thay số ước lượng trong doc. Batch-desc-read chỉ làm NẾU benchmark chứng minh nghẽn đáng kể ở một tier vốn "boot-time/throughput không quan trọng".

## Key Insights (từ Red Team)
- **A1/B5 — batch-read mâu thuẫn + premature:** "giữ per-access backstop" chính là chi phí N-syscall batch định bỏ; không thể batch (1 syscall) VÀ per-access-validate (N syscall) cùng lúc. Batch cũng mất clamp `cur<q_size` (đã chuyển sang P06). Không có baseline thì tối ưu là premature.
- **F4 — ABI trap:** nếu batch thì PHẢI tái dùng `ReadGuestMemory` (len tùy ý, `vmm.rs:59`; desc table liền kề `desc_gpa+cur*16`) đọc `q_size*16` byte một lần — KHÔNG thêm syscall (syscall mới = libs/api = Law 1).
- **Mn4 — real-HW correctness:** thiếu DMB trước store `used.idx` (`virtqueue.rs:86`, comment "TCG is SC"); trên ARM64 thật guest có thể thấy idx tiến trỏ vào entry cũ. Benchmark "80% native real-HW" ride trên path chỉ đúng dưới TCG.

## Requirements
**Functional:**
- Bộ đo: throughput blk/net, latency trap, boot time (ghi caveat QEMU TCG).
- Cập nhật số THẬT vào `tier3b-linux-vm.md` (thay ước lượng).
- Ghi nhận Mn4 (DMB) như correctness real-HW cần trước khi công bố số real-HW.
- **Batch-read chỉ khi:** baseline chứng minh 64-syscall/chain là nghẽn thật; NẾU làm → tái dùng `ReadGuestMemory`, thêm clamp (P06), giữ full bounds-check cả vùng batch.
**Non-functional:** không phá bounds-check.

## Related Code Files
- Modify: `docs/guides/tier3b-linux-vm.md` (số perf thật). Có thể mở rộng `cells/apps/bench`.
- (Chỉ nếu batch được duyệt) `virtqueue.rs` — reuse `ReadGuestMemory`.

## Implementation Steps
1. Dựng phép đo theo dossier 3d; ghi baseline.
2. Cập nhật doc số thật.
3. Đánh giá: nghẽn có đáng tối ưu ở tier này? Nếu KHÔNG → dừng, ghi kết luận. Nếu CÓ → batch qua reuse `ReadGuestMemory` + clamp (P06), đo lại.
4. Ghi Mn4 vào doc/threat-model như prereq real-HW.

## Todo
- [ ] Baseline benchmark (blk/net/latency/boot)
- [ ] Cập nhật doc số thật
- [ ] Quyết định batch: cần hay không (dựa số)
- [ ] (Nếu cần) batch reuse ReadGuestMemory, không syscall mới
- [ ] Ghi nhận DMB real-HW (Mn4)

## Success Criteria
- Doc có số benchmark thật thay ước lượng.
- Kết luận rõ về batch (làm/không, dựa số) — không tối ưu mù.
- Mn4 ghi nhận cho tính đúng real-HW.

## Risk Assessment
- Nếu batch được duyệt: validate không phủ hết vùng batch → OOB; giữ full bounds-check + clamp P06.

## Security Considerations
- Mọi thay đổi truy cập guest-mem giữ bất biến `registry.rs:311-317`.

## Next Steps
- Số thật làm cơ sở SLA G2 + so ARM64/x86 (P08).
