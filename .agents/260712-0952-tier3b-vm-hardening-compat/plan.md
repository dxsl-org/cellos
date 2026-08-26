---
title: "Tier 3b Linux VM — hardening & software-compatibility roadmap"
description: "Đóng 3 khoảng cách của Tier 3b (doc-vs-reality, an toàn guest-escape, độ phủ phần mềm) rồi mở rộng: storage ghi được, guest glibc, fuzz+bench virtio, tiếp tục x86 SVM."
status: pending
priority: P2
effort: 8 phases (3 do-now analysis + 5 post-window coding)
branch: main
created: 2026-07-12
source_research: .agents/reports/research-260712-1010-tier3b-linux-vm-safety-perf-compat.md
tags: [tier3b, hypervisor, security, compat, virtio, x86-svm, docs]
---

# Tier 3b Linux VM — Hardening & Compatibility

Chuyển các phát hiện trong `research-260712-1010` thành lộ trình có phân pha.
**Ranh giới cửa sổ Mythos (analysis-only, tới 2026-07-14):** P01-P03 là docs/threat-model/design
→ **làm được ngay**. P04-P08 là coding → **hoãn tới sau 2026-07-14**.

## Quyết định người dùng đã chốt (2026-07-12)
- **Guest OS:** thêm rootfs **glibc (Debian/Ubuntu minimal)** song song Alpine → độ phủ rộng nhất.
- **Storage:** **virtio-blk writable + overlay tmpfs** → package install persist.

## Phases

| # | Phase | Cửa sổ | Effort | Tier | Law 1? | Depends |
|---|-------|--------|--------|------|--------|---------|
| 01 | ✅ [Doc reconciliation (x86 Planned, apt→apk, LOC/perf)](phase-01-doc-reconciliation.md) | **do-now** | S | fast | no | — |
| 02 | ✅ [Tier-3 threat model → docs/specs/05 (+C1 IRQ-DoS, PSCI, config-space, backing invariant)](phase-02-tier3-threat-model.md) | **do-now** | M | thinking | no | — |
| 03 | ✅ [Design dossiers (storage/glibc-boot/fuzz-refactor/bench)](phase-03-design-dossiers.md) | **do-now** | L | thinking | no | 02 |
| 04 | [virtio-blk RW: per-VM image-file backing + sector clamp + write-cap (NOT shared cell-store)](phase-04-writable-storage.md) | post-window | L | thinking | no (manifest) | 03 |
| 05 | [**glibc guest = root-on-blk boot rework** (disk-image, RAM bump, init, RTC+RNG, network)](phase-05-glibc-guest-rootfs.md) | post-window | **XL** | thinking | no | 01,**04** |
| 06 | [virtqueue+IRQ fuzz & harden: **process_notify backend refactor**, cur<q_size clamp, C1 IRQ cap, buf.writable assert](phase-06-virtqueue-fuzz-hardening.md) | post-window | L | thinking | no | 03 |
| 07 | [virtio **benchmark only** (batch-read cut); reuse ReadGuestMemory; note real-HW DMB](phase-07-virtio-benchmark-optimize.md) | post-window | S | medium | no | 06 |
| 08 | [x86 SVM continuation — **boot parallel, gate on P06 only**](phase-08-x86-svm-continuation.md) | post-window | XL | thinking | YES | 06 |

## Dependency Graph (REVISED sau Red Team)

```
do-now │  01 (docs) ─┐
       │  02 (threat +C1) ─► 03 (dossiers) ─┬─► 04 (RW image backing) ─► 05 (glibc root-on-blk) ─┐
       │                                    ├─► 06 (refactor+fuzz+cap) ─► 07 (bench)             │
post-  │                                    │                          └─► 08 (x86 SVM boot) ────┤
window │  01 ───────────────────────────────┘   (x86 world-switch song song 04/05)               │
       │             x86 *compat* milestone (writable+glibc trên x86) mới gate 04+05 ─────────────┘
```

- **Critical path độ-phủ phần mềm:** 03 → 04 (RW image backing) → 05 (root-on-blk glibc + RTC/RNG/network). **P05→P04 là edge cứng** (systemd cần root ghi được).
- **Critical path an toàn:** 02 (+C1) → 03 (fuzz-refactor dossier) → 06 (refactor + fuzz + cap C1).
- **P08 gate = chỉ P06** (parser đã hardened để x86 tái dùng). Boot Alpine-x86 KHÔNG cần writable/glibc → x86 world-switch chạy song song P04/P05.

## Red-Team Adjudication (2026-07-12 — 3 reviewer: Security Adversary · Assumption Destroyer · Failure-Mode/Scope)
*Verdict: CAUTION — 3 plan-breaker (F1/F2/F3). Mọi finding xác minh bằng code trước khi accept.*

> **Mythos verdict layer (2026-07-12):** `.agents/260712-1836-mythos-g123-analysis/dossier-6-tier3b-verdicts.md`
> sharpens three resolutions below with concrete mechanism:
> - **C1** → pending-IRQ = **bounded coalescing bitset** (1 bit/INTID, idempotent inject) — strictly correct
>   (no legit IRQ dropped, only redundant re-injections collapse) + smaller than a ring. Write the
>   "bound every guest-triggered kernel queue, coalesce where semantics allow" **invariant** into P02;
>   IRQ set is its first instance. Pair with `cur<q_size` + `avail_idx` delta bounds.
> - **M1/A2** → per-VM image backing is mandatory **the moment backing becomes shared-OR-persistent**
>   (today's single volatile Vec + single VM is not yet exploitable, but P04 must not ship a shared-store shortcut).
> - **F1/F2 (P05 scope)** → freezing P05's graduation deliverable at **Alpine/musl** and making Debian/glibc a
>   **separate sequenced phase** is compatible with the user's "keep both rootfs" decision — it is a sequencing
>   split (security work on fixed scope; glibc stretch at its own pace), not a drop of Debian.

| ID | Sev | Finding | Resolution |
|----|-----|---------|------------|
| F1/F2 | **Crit** | P05 "Debian boot như Alpine" SAI: VMM hiện boot initramfs→`/bin/sh` (dtb.rs:22), RAM 128MiB (main.rs:55), disk 16MiB Vec volatile (virtio_blk.rs:15). Debian cần root-on-blk + init + ≥150MiB persistent + RAM lớn hơn. | **Accept** — P05 re-scope M→**XL**: root-on-blk path, disk-image backing, RAM bump, init system, +edge cứng P05→P04. |
| F3 | **Crit** | P06 fuzz dựa trên refactor chưa chứng minh: `process_notify` gọi thẳng syscall wrapper (virtqueue.rs:43,61), host trả usize::MAX → 0 coverage. | **Accept** — refactor memory-backend thành **bước production BẮT BUỘC** P06; production chạy CÙNG parser fuzzer test. Precedent `loader_image.rs:68`. |
| C1 | **Crit** | LIVE bug: `inject_irq` push_back không cap độ sâu (registry.rs:398); guest mask IRQ + spam QueueNotify → kernel OOM → chết cả máy (SAS). | **Accept** — P02 liệt kê resource-exhaustion; P06 cap độ sâu queue (kernel-side). |
| M1/A2 | **Major** | P04 path-guard nhắm SAI: virtio-blk theo sector không path (virtio_blk.rs:76). Rủi ro thật: sector→offset bound; backing KHÔNG được là shared cell-store (ghi đè FAT/cell-table/ELF cell khác = disk escape). | **Accept** — P04: backing = **image-file per-VM** + sector clamp; bỏ path-guard trừ khi virtio-fs. Nâng backing-isolation thành invariant P02. |
| M3/B5 | **Major** | "jailer allowlist" (P06) là theater dưới LBI VÀ **đã tồn tại** (main.rs:22-32). Siết "220-227+VFS/Net" sẽ HỎNG cell (thiếu Recv/Log/GetTime). | **Accept** — **CUT** khỏi P06; demote thành khuyến nghị có lý do trong P02 (chỉ giảm blast-radius unsafe-dep/rustc). |
| A1/B5 | **Major** | P07 batch-read mâu thuẫn: "per-access backstop" chính là chi phí batch định bỏ; batch mất clamp `cur<q_size`. | **Accept** — **CUT batch-read** khỏi default P07; clamp `cur<q_size` → sửa parser độc lập trong P06. |
| F4 | **High** | (a) P07 "wrapper nội bộ"→syscall mới = Law 1 (mislabel). (b) P04 ẩn manifest change: `block_io=false`, chưa có write-cap persist VFS. | **Accept** — P07 bắt buộc tái dùng `ReadGuestMemory`, CẤM syscall mới. P04 nêu rõ manifest/write-cap. |
| A3 | **Med** | P08 gate P04+P05+P06 quá bảo thủ; Alpine-x86 chỉ cần P06. | **Accept** — P08 gate = P06; world-switch song song. |
| Mn1-4 | **Minor** | PSCI/HVC+config-space thiếu ở P02; single-thread trigger chưa đủ; blk_write sentinel giòn + blk_read bỏ return; thiếu DMB trước used.idx (real-HW). | **Accept** — Mn1/2→P02+P06; Mn3→P06; Mn4→P07. |
| B4 | **Med (USER)** | 2 rootfs = gánh CI/bảo trì không tính; "boot-time không quan trọng" triệt lợi thế Alpine. | **Defer** — §Câu hỏi mở. |
| F5 | **Low (USER)** | Cửa sổ Mythos: P01/P02 sửa docs/specs (committed tree) ≠ `.agents/` artifact. | **Defer** — §Câu hỏi mở. |

## Quyết định user (2026-07-12, sau Red Team)
1. **Số rootfs (B4): GIỮ CẢ Alpine + Debian.** ⇒ P05 nhận nợ 2 lane CI + 2 kernel config; ghi rõ chủ sở hữu bảo trì. Alpine ca nhẹ, Debian-glibc độ phủ.
2. **Phạm vi Mythos (F5): docs/specs TÍNH LÀ analysis.** ⇒ P01 + P02 + P03 đều **do-now**; chỉ P04-P08 (coding) hoãn sau 2026-07-14.

## Key Cross-Cutting Invariants (REVISED)
- **Không phá Law 1:** chỉ P08 chạm `libs/api` (ViVmExit x86 variants, plan 260711-1917). P04-P07 KHÔNG đổi ABI — P07 bắt buộc tái dùng `ReadGuestMemory`, CẤM syscall mới; P04 chỉ đổi cell manifest (write-cap), không libs/api.
- **Bảo toàn bất biến bounds-check:** mọi truy cập guest-mem vẫn qua wrapper kernel đã `checked_add` (`registry.rs:311-317`). Refactor fuzz (P06) và mọi tối ưu (P07) phải chạy production qua CÙNG parser đã bounds-check; `cur<q_size` clamp bù cho việc batch bỏ per-access break.
- **Backing-store isolation (M1/A2):** virtio-blk RW backing = **image-file/partition riêng của từng VM**, KHÔNG BAO GIỜ shared cell-store (`PART_CELLSTORE`); sector→offset clamp theo backing thật. Đây là ranh giới chống guest→host-disk escape, độc lập với bounds-check RAM.
- **Single-thread vCPU invariant:** `write_guest_memory` giả định không vCPU nào chạy đồng thời VÀ RunVcpu là đồng bộ same-core (`registry.rs:182-217,321-323`); mọi thay đổi đa luồng/SMP/async-vcpu phải ghi lại + thêm quiesce.
- **Resource-exhaustion (C1):** guest không được làm cạn tài nguyên kernel — cap độ sâu IRQ queue (`registry.rs:398`) + cap `avail_idx` delta theo `q_size`.

## Nguồn & tham chiếu
- Research: `.agents/reports/research-260712-1010-tier3b-linux-vm-safety-perf-compat.md`
- ARM64 VMM (done): `.agents/260613-2134-tier3b-vmm-arm64-el2/`
- x86 SVM (plan): `.agents/260711-1917-tier3b-x86-vtx/`

## Cook Handoff
Sau khi hết cửa sổ Mythos, chạy coding từng phase:
`/hc-cook d:\Cellos\.agents\260712-0952-tier3b-vm-hardening-compat\phase-04-writable-storage.md`
