# Phase 08 — x86 SVM continuation (đồng bộ với plan 260711-1917)

**Cửa sổ:** post-window (coding) · **Priority:** P2 · **Status:** pending · **Tier:** thinking · **Law 1:** YES (ViVmExit x86 variants)
**Depends:** P06 (parser hardened) — boot chạy SONG SONG P04/P05

> ⚠️ **Gate nới sau Red Team (A3).** Boot Alpine-x86 KHÔNG cần writable(P04)/glibc(P05) — chỉ cần parser đã hardened (P06). x86 world-switch chạy song song P04/P05; P04/P05 chỉ gate mốc *compat trên x86* (writable+glibc x86), không gate boot.

## Context Links
- Plan gốc: `.agents/260711-1917-tier3b-x86-vtx/plan.md` (10 phase, SVM-first, PVH boot, no-LAPIC MVP)
- Research §4 #4-#5 · dossier-glibc-guest (P03) cho vmlinux x86

## Overview
x86_64 hiện **0 dòng code** dù doc từng ghi "Working" (P01 đã sửa). Phase này KHÔNG lặp lại plan 260711-1917 mà đồng bộ tiền đề: hạ tầng an toàn (fuzz P06), storage ghi được (P04), glibc guest (P05) phải xong để x86 kế thừa, rồi khởi động plan gốc.

## Key Insights
- SVM-first vì QEMU TCG chỉ emulate SVM (không VMX) → CI chạy được; VMX ở lane KVM/HW thật (non-blocking).
- PVH boot cần trích `vmlinux` (note PHYS32_ENTRY chỉ có trong ELF chưa nén) — chung vấn đề với dossier glibc-guest.
- Law 1: P04 của plan gốc thêm `#[repr(C,u8)]` variants vào `ViVmExit` + bump VERSION 1→2 → cần xác nhận 2×.
- Device model/virtqueue/run-loop đã arch-generic → x86 chỉ thêm "personality", tái dùng parser đã hardened ở P06.

## Requirements
**Functional:**
- **Gate boot = chỉ P06** (parser hardened + memory-backend đã refactor để x86 tái dùng). x86 world-switch bring-up chạy SONG SONG P04/P05.
- Khởi động plan 260711-1917 theo thứ tự (01→05 critical path tới boot Alpine x86).
- Mốc *compat x86* (writable + glibc trên x86) mới gate P04+P05.
- Tái dùng parser đã fuzz (P06) cho x86; **KHÔNG** tái dùng "path-guard" (đã bỏ ở P04 — sai bề mặt).
- **SVM preemption-timer gap:** SVM không có preemption timer → cần host one-shot timer + INTR intercept (open question plan gốc). Nêu rõ như **entry gate P08**, không kế thừa im lặng.
**Non-functional:** vendor-neutral trait boundary (Law 7); **bounds-check EPT/NPT phải ngang Stage-2 (`checked_add`)** trước khi coi x86 an toàn.

## Related Code Files
- Theo `.agents/260711-1917-tier3b-x86-vtx/` phase files (kernel x86 virt + cell x86 personality).
- Law 1 touch: `libs/api/src/abi/hypervisor.rs`.

## Implementation Steps
1. Kiểm **P06** đã merge (gate boot). P04/P05 KHÔNG chặn boot.
2. Chạy plan gốc 260711-1917 P01 (vendor detect + SVM enable) — song song P04/P05.
3. Chốt cơ chế SVM budget (host timer + INTR) trước world-switch (P03 plan gốc).
4. Ở P04 plan gốc (ViVmExit ABI) → dừng xin Law 1 confirm 2×.
5. Boot Alpine x86 (SVM/TCG). glibc guest x86 = mốc compat, sau P04+P05.
6. Bounds-check EPT/NPT qua fuzz (tái dùng harness P06).

## Todo
- [ ] Gate boot: P06 xong (KHÔNG chờ P04/P05)
- [ ] Chốt SVM budget mechanism (entry gate)
- [ ] Khởi động 260711-1917 P01, song song P04/P05
- [ ] Law 1 confirm ở ViVmExit x86
- [ ] Boot Alpine x86 (SVM/TCG)
- [ ] Fuzz EPT/NPT bounds path (harness P06)
- [ ] glibc guest x86 (mốc compat, sau P04+P05)

## Success Criteria
- x86 boot Alpine tới shell dưới SVM/TCG (đúng milestone M2 plan gốc).
- Doc x86 chuyển từ "Planned" → "Working" CÓ bằng chứng (test suite xanh), không lặp lỗi doc-vs-reality.

## Risk Assessment
- XL, rủi ro cao nhất là world-switch (dossier-p03 plan gốc). Đây là lý do gate sau khi hạ tầng an toàn ARM64 đã fuzz.
- SVM không có preemption timer → cần host one-shot timer + INTR intercept (open question plan gốc).

## Security Considerations
- Law 1 (ABI) — xác nhận 2×. Bounds-check EPT/NPT phải ngang Stage-2 (checked_add) trước khi coi x86 an toàn.

## Next Steps
- x86 xong = độ phủ phần mềm đầy đủ (phần lớn phần mềm là x86_64) → đạt mục tiêu "chạy hầu hết phần mềm thông dụng".
