# Dossier 3d — virtio benchmark (batch-read chỉ nếu số chứng minh)

**Cho:** P07 · **Nguồn:** research §3 + code + red-team A1/F4/Mn4 · **Trạng thái:** ready-to-code

## Mục tiêu
Thay số ước lượng bịa trong doc (`tier3b-linux-vm.md`, đã gắn nhãn "estimate" ở P01) bằng số ĐO THẬT. Tối ưu batch-read chỉ làm NẾU baseline chứng minh nghẽn đáng kể ở tier vốn "boot-time/throughput không quan trọng".

## Phép đo (baseline trước, tối ưu sau)
- **Throughput blk:** đọc/ghi tuần tự + ngẫu nhiên qua virtio-blk (dd/fio trong guest hoặc guest-side microbench).
- **Throughput net:** iperf-style hoặc HTTP qua virtio-net → Net Cell.
- **Latency trap:** thời gian một vòng MMIO exit → emulate → resume (đo kernel-side hoặc qua counter).
- **Boot time:** power-on → shell prompt.
- **Caveat BẮT BUỘC ghi kèm:** QEMU TCG timing KHÔNG đại diện real-HW (nhắc lại caveat như bench suite hiện có). Số real-HW cần board thật.

## Nghẽn đã xác định (Research §3)
- Mỗi descriptor 16B = 1 `read_guest_memory` (1 syscall) + 1 `registry_lock().lock()` (`virtqueue.rs:61`, `registry.rs:307`). Chain 64 desc = 64 syscall + 64 lock.

## Batch-read — CHỈ nếu baseline chứng minh (A1/F4)
- **Đừng tối ưu mù.** Đo baseline trước; nếu 64-syscall/chain không phải nghẽn thực ở tier này → DỪNG, ghi kết luận "không đáng".
- Nếu làm: đọc cả desc table trong **một** `ReadGuestMemory` (len tùy ý — `vmm.rs:59`; table liền kề `desc_gpa+cur*16`). **CẤM syscall mới** (syscall mới = libs/api = Law 1 — F4). Tái dùng `ReadGuestMemory` = không đổi ABI.
- Batch mất per-access break của kernel → **bắt buộc clamp `cur < q_size`** (đã đưa vào P06) + full bounds-check cả vùng batch trước khi walk cục bộ.
- Ưu tiên "map read-only ring vào cell" (1 bounds-check lúc map cho vùng cố định) hơn table-copy, nếu batch được duyệt.

## Mn4 — correctness real-HW (không phải perf)
- Thiếu DMB trước store `used.idx` (`virtqueue.rs:86`, comment "TCG is SC"). ARM64 thật: guest có thể thấy idx tiến trỏ vào entry cũ. Trước khi công bố BẤT KỲ số real-HW, phải thêm barrier. Ghi vào doc + threat-model như prereq real-HW.

## Ready-to-code checklist (P07)
- [ ] Dựng phép đo (mở rộng `cells/apps/bench` hoặc guest-side); ghi baseline (blk/net/latency/boot) + caveat TCG.
- [ ] Cập nhật `tier3b-linux-vm.md` số THẬT (thay estimate).
- [ ] Quyết định batch dựa số: làm/không. Nếu làm → reuse `ReadGuestMemory`, clamp (P06), full bounds-check vùng batch; đo lại.
- [ ] Ghi Mn4 (DMB) vào doc/threat-model như prereq real-HW.

## Rủi ro / mở
- Batch validate không phủ hết vùng → OOB; giữ full bounds-check + clamp.
- Số real-HW chỉ tin sau khi có board + fix Mn4.
