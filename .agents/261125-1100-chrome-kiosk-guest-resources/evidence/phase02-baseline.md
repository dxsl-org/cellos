# Phase 02 — smoothness baseline (2026-11-25)

Lane: `scripts/qemu-tier3-chrome-bench-x86.sh` (driver `scripts/tier3_chrome_bench.py`, page
`tests/guests/chrome-bench/bench.html`). Raw records: `phase02-bench.json`, `phase02-bench.log`.

- Machine: i7-14700, WSL2 (Hyper-V -> KVM -> CellOS -> guest), QEMU 11.1.3.
- `baseline`: the desktop image of this tree at phase 02 (`loglevel=4` already in), q35
  `-smp 4 -m 3G`; the Chrome guest has 1 vCPU and 1544 MiB. The ISO is kept at
  `~/.cache/cellos-chrome-bench/phase02-baseline.iso` so later phases can run it in the same
  rounds as their own image (`BENCH_BASELINE_ISO`).
- `reference`: the same `chrome-guest.sqfs`, kernel and initramfs straight under QEMU/KVM
  (microvm, virtio-mmio), 1 vCPU, 1536 MiB. One nesting level less; no CellOS.

## Host state: loaded

While these rounds ran, a Windows process that is not part of this work (test binaries
`haily_app-…`, then `haily_core-…`) held 24-25 of the 28 logical CPUs (`Get-Counter`:
`% Processor Time` 98 %). `host_ms` is a fixed Python loop the driver times before every run:
176 ms was the lowest seen today, 381-464 ms during these rounds.

The figures in `scout-report.md` §1 (reference 6.9 s, CellOS 21-22 s from click to UI) were
taken before that load started. They do not compare with this table. Only columns of one table
compare, which is why the driver runs its targets in turns.

## Bench (times in ms unless marked)

| metric | baseline 1 | baseline 2 | median | reference 1 | reference 2 | reference 3 | median | baseline / reference |
|---|---|---|---|---|---|---|---|---|
| `host_ms` | 464 | 401 | 432 | 437 | 409 | 381 | 409 | |
| launch -> Chromium UI (s) | 71.0 | 75.1 | 73.0 | 19.1 | 16.1 | 15.1 | 16.1 | 4.5x |
| navigation -> report (s) | 28.2 | 27.0 | 27.6 | 18.3 | 18.2 | 18.3 | 18.3 | 1.5x |
| `raf` frames in 5 s | 256 | 278 | 267 | 376 | 377 | 377 | 377 | 0.71x |
| `raf` interval p50 / p95 | 16.7 / 33.4 | 16.7 / 16.8 | 16.7 / 25.1 | 13.3 / 13.4 | 13.3 / 13.4 | 13.3 / 13.4 | 13.3 / 13.4 | |
| `timer` lateness p50 / p95 | 10.5 / 24.0 | 9.1 / 19.9 | 9.8 / 21.9 | 0.2 / 0.4 | 0.1 / 0.2 | 0.1 / 0.2 | 0.1 / 0.2 | ~100x |
| `js1` | 1232 | 1311 | 1272 | 505 | 470 | 504 | 504 | 2.5x |
| `jsN` (4 workers) | 2539 | 2230 | 2385 | 1442 | 1342 | 1387 | 1387 | 1.7x |
| `scroll` frames in 5 s | 87 | 80 | 83.5 | 333 | 378 | 369 | 369 | 0.23x |
| `scroll` interval p95 | 147.9 | 144.7 | 146.3 | 23.8 | 18.9 | 19.4 | 19.4 | 7.5x |
| `paint` frames in 5 s | 71 | 89 | 80 | 352 | 373 | 371 | 371 | 0.22x |
| `paint` interval p95 | 139.8 | 100.1 | 120.0 | 24.7 | 20.5 | 19.7 | 20.5 | 5.9x |

"Launch" is the click on the taskbar's Chrome for `baseline` and the start of QEMU for
`reference`; both include the guest's boot.

Third `baseline` round: no report. Chromium gave up 55 s after cage started
(`GPU process launch failed: error_code=1002`, `GPU process isn't usable. Goodbye.`,
`CHROME_GUEST_EXITED:133`). Not seen on the quiet host (Chrome lane 21/21, several runs).
Two runs are what the `baseline` medians rest on.

## VM statistics (kernel and Cell, the two `baseline` runs, click to Close, about 105 s)

| | run 1 | run 2 |
|---|---|---|
| `RunVcpu` calls | 58,030 | 60,078 |
| in the run scope (ms) | 61,159 | 61,205 |
| in the guest (ms) | 48,133 | 47,444 |
| around the guest, per call (µs) | 224 | 229 |
| exits: port I/O | 32,415 | 33,243 |
| exits: EPT violation (virtio-mmio) | 10,668 | 10,727 |
| exits: HLT | 9,735 | 10,794 |
| exits: MSR (served in the kernel) | 8,057 | 8,214 |
| exits: host interrupt | 5,212 | 5,314 |
| exits: CPUID (kernel) | 1,474 | 1,474 |
| exits: CR (kernel) | 2 | 2 |
| Cell: 8259 ports | 29,445 | 30,273 |
| Cell: UART / RTC / ACPI PM / PIT / other ports | 2,756 / 148 / 30 / 12 / 24 | 2,756 / 148 / 30 / 12 / 24 |

VMX controls offered to CellOS here (kernel log):
`preemption-timer=true tpr-shadow=true msr-bitmaps=true vpid=true apic-access=false
apic-registers=false virtual-interrupts=false vmcs-shadowing=true`.

## What the numbers say

- Timers are the worst row. The guest boots with `nohz=off highres=off` and takes its ticks
  from the PIT, which the Cell raises only when its loop comes round: `setTimeout(4)` is 10 ms
  late at the median. Frame pacing, scrolling and painting follow from it. Phase 04.
- Half of all trips are 8259 ports, about four per delivered interrupt. Phase 04.
- A call costs about 225 µs around the guest on this loaded host (76 µs on the quiet one):
  `INVEPT`, `VMPTRLD`, 17 host-state `VMWRITE`s, `VMREAD`s, `VMCLEAR`. Phase 03.
- `js1` does no I/O and still takes 2.5x the reference. Not explained by this phase's data;
  phase 03 tests two candidates (no VPID: every entry and exit flushes the guest's
  translations; an `INVEPT` before every entry).
- `apic-access`, `apic-registers` and `virtual-interrupts` are not offered under this stack,
  so a guest APIC here is trap-and-emulate whatever a physical machine would allow.

## Limits

Triple-nested virtualization on a loaded host: these numbers rank changes made on this
machine. They are not what a physical Intel machine shows; no such run exists.
