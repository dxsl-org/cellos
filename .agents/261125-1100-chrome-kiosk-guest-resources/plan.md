# Plan — Chrome guest: only Chromium in the window, then CPU, memory, GPU, files, sound, clipboard

Goal: the taskbar's Chrome opens a window that shows Chromium and nothing else of its Linux
guest, and then the guest gets what Chromium needs to run smoothly: faster VM entry and a real
timer, more vCPUs, more memory, wider CPU features, and host integration (pointer shape,
clipboard, files, sound). GPU acceleration is decided, not assumed.

User request (2026-11-25): "kiểm tra để chrome chạy được khi bấm vào icon trên Cellos. vm chỉ chạy
duy nhất chrome ở tình trạng toàn cửa sổ và không hiển thị các thành phần khác của linux. Sau đó mở
rộng số lượng cpu/ram/gpu/file/sound/clipboard để chrome chạy mượt mà hơn."
Recon: [scout-report.md](scout-report.md). Builds on `.agents/261111-1700-tier3-chrome/` (done).
ADR-0017 §5e records the direction; ADR-0025 §3.2 stands (the VMM is the Chrome guest's, not a
general-purpose hypervisor).

## Why this order

Measured on the i7-14700 under nested KVM (scout-report §1): the same guest image shows
Chromium 6.9 s after cage starts under plain QEMU/KVM with one vCPU and 5.9 s with four; under
the CellOS hypervisor it took about 22 s, with 1, 2 or 4 CPUs in the outer machine alike. Of
25.0 s from the click to Chromium's UI the guest ran for 12.6 s; the rest was 60,000 trips
between the guest, the kernel and the Cell, more than half of them console bytes and a
quarter 8259 ports, and an idle Chromium makes 2,000 HLT trips a second. The guest's one vCPU
is slow at being a vCPU, so the cost of a trip, the interrupt controller and the timer come
before vCPU count.

## Phases

| # | Phase | Slice outcome | Status |
|---|---|---|---|
| 01 | [Only Chromium in the window](phase-01-kiosk-window.md) | no blank guest screen, guest pointer, window buttons, warning bar or Linux console; quitting Chromium closes the window | done, checkpoint (Chrome lane 21/21) |
| 02 | [Smoothness baseline](phase-02-smoothness-baseline.md) | a bench page and VM-exit statistics give numbers every later phase must move | done (bench lane, stats lines) |
| 03 | [VM entry fast path](phase-03-vm-entry-fast-path.md) | a `RunVcpu` call stops paying for work that changes nothing, and fewer are made | done, checkpoint (84 -> 38 µs per call) |
| 04 | [Guest APIC and timer](phase-04-guest-apic-timer.md) | in-kernel local APIC with a TSC-deadline timer; high-resolution guest timers; an idle guest sleeps | done (timer p95 1.6 ms; 10-min soak) |
| 05 | [More vCPUs](phase-05-vcpus.md) | the Chrome guest boots with N vCPUs on N host CPUs | done, checkpoint (1 vCPU default by owner decision; 4 via `/chrome-guest-vcpus`) |
| 06 | [More memory](phase-06-memory.md) | 3072 MiB guest when the machine has it; 2 MiB nested-page leaves | done (2983 MiB; 5 table pages; range-crossing carve fixed) |
| 07 | [AVX2 for the guest](phase-07-guest-xsave.md) | XSAVE state switched; Chromium's software raster uses AVX2 | done, checkpoint (paint +4-9 %; +30-40 µs per call under nesting; CP2 approved 2026-11-26) |
| 08 | [Pointer shape](phase-08-pointer-shape.md) | the host pointer takes the guest's cursor image over the window | done (arrow over page, I-beam over omnibox; lane check) |
| 09 | [Clipboard](phase-09-clipboard.md) | text copies between Chromium and CellOS apps | done (Chromium <-> Terminal both ways; lane checks) |
| 10 | [Files](phase-10-files.md) | a host folder is Chromium's Downloads and upload source | done (downloads on the data disk; VFS FAT rename under `/mnt/disk/`) |
| 11 | [Sound](phase-11-sound.md) | Chromium's audio plays through a CellOS audio service | done, checkpoint (tone recorded 5.008 s, 439.3 Hz; lane flaky on today's host, see log) |
| 12 | [GPU decision](phase-12-gpu-decision.md) | an ADR picks software-only, a virgl proxy, or waiting for a native driver | done (ADR-0037: software only) |

Dependencies: 01 -> 02 -> 03 -> 04 -> 05; 06 and 07 after 03; 08-11 after 01 (each independent);
12 after 07. Phases 08-11 share `wire::vmm_session` and the x86 MMIO slot table: one at a time.

## Owner approvals (Law 1, kernel authority)

**CP1 given by the user on 2026-11-25** for every row below, and the hand-over of the VMX
kernel/HAL files for phases 02-07 (plan checkpoint form: "Duyệt CP1 cho mọi mục trong bảng";
CP2 after each phase, on the exact delta and its evidence). The same answers fixed: this phase
order; GPU stays software until the phase-12 ADR; shared files live on a second data disk;
sound comes last; nothing is committed until the user has read the diff. Each phase writes its
record in `docs/abi-boundary-records.md` with the exact bytes before the edit.

| Phase | Delta | Class |
|---|---|---|
| 03-05 | edits to `hal/arch/x86/src/x86_64/vmx*`, `kernel/src/arch/x86_64/hypervisor/svm_registry.rs` (lock inventory: Intel VMX/EPT, P09) | owner handoff |
| 04 | `InjectIrq` on x86: the vector stays pending until the guest can take it (today it is dropped) | `libs/abi` text |
| 05 | `CreateVcpu`: a VM's second and later vCPUs start waiting for INIT/SIPI; the hypervisor's row gains thread spawn | `libs/abi` text, catalog |
| 06 | `/bin/hypervisor` `guest_memory` 1544 MiB -> 3080 MiB | catalog |
| 08 | compositor `SET_POINTER_IMAGE`; `VMM_EVT_POINTER` | `libs/wire` |
| 09 | compositor clipboard opcodes and event; `vmm_session` clipboard frames | `libs/wire` |
| 11 | service id `AUDIO`; `wire::audio`; catalog rows for the audio Cells | `libs/abi`, `libs/wire`, catalog |

## Cross-cutting rules

- Every phase reports the phase-02 numbers before and after; a phase that moves nothing says so.
- No fabricated content: status text is real, failures show the real error.
- Kernel/HAL changes are isolated per phase, keep the SVM/TCG CI lanes green
  (`qemu-x86-hypervisor-boot`, `-tier3-python`, `-tier3-virtio-e2e`, `-tier3-hostile`) and are
  committed separately from Cell changes.
- Device models are written from the virtio/OASIS specifications, the 9P2000.L protocol
  description and the Intel SDM (ADR-0030); no Linux, KVM or QEMU source is consulted for them.
- Guest packages are added only through the pinned builder (`scripts/build-chrome-guest-image.sh`).
- Every phase updates `docs/project-changelog.md` and `docs/guides/tier3b-linux-vm.md`.

## Top risks

1. Triple nesting here (Hyper-V -> KVM -> CellOS -> guest) makes every VMX instruction CellOS
   executes a trap; numbers on this machine overstate entry cost against bare metal, where no
   evidence exists (HCL has no physical x86 row).
2. Phases 03-05 change the VMX backend, which no CI lane runs (CI is SVM under TCG); the Chrome
   and display lanes are the only gate and need nested VMX.
3. `/mnt/sd` is a fixed 256 MiB partition filled by the 246 MiB guest root: guest packages for
   sound and clipboard must fit in about 10 MiB, or the disk layout (Law 1) changes first.
4. No host audio stack and no host clipboard exist; phases 09 and 11 build both ends.

## Evidence

`.agents/261125-1100-chrome-kiosk-guest-resources/evidence/` (PNG tracked, logs ignored).
