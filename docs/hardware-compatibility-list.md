# Cellos Hardware Compatibility List (HCL)

**Status:** no physical x86 machine is qualified. The machine table below is
empty on purpose.
**Owner:** `x86_64 PC lane` plan, `.agents/261004-1957-x86-pc-lane/` (phase 07
fills rows; phase 01 defines the model).
**Read with:** `docs/roadmap/hardware-tracks.md` (gate inventory X86-PC-0..7) and
`docs/specs/04-hardware.md` §7 (x86 PC driver order).

**Current direction (2026-10-08):** [ADR-0022](decisions/0022-intel-x86-64-c2c-only-direction.md)
limits qualification to the headless Intel x86-64 Cell-to-Cell Anywhere profile.
Select one exact machine first; qualify it before a separately authorized second
node. Reusing its configuration is preferred, not required; each different
configuration needs independent qualification. AMD/new ARM/RISC-V expansion is parked,
except Apple Silicon, which is an active secondary lane under
[ADR-0032](decisions/0032-apple-silicon-secondary-lane.md) with its own requirements in
[§7](#7-apple-silicon-aarch64-group-3). Sections 1–6 cover x86 only.
This list and completed QEMU gates authorize neither purchase nor remote/production
activation; phase 07 and the existing security/authority gates still apply.

## 1. How to read this list

This is a closed list. A machine not in the table is **not supported** — it is
not "probably works". QEMU, compile and host results are regression evidence and
never qualify a machine; only a capture bound to one exact machine can create a
row.

The contract a machine is qualified against is the **generic**
`boards/pc/x86_64-pc` descriptor, which the single x86_64 image binds at boot
when the Limine command line names no `cellos.profile=` (ADR-0029). It
declares a compatibility contract, not facts guaranteed for every PC. It now
lists COM1/16550, IOAPIC, HPET, PCIe ECAM, NVMe, AHCI, xHCI, e1000 and `igb`;
the driver phases and DMAR/multi-port serial have completed at QEMU only.
The physical profile requires DMA isolation; QEMU's optional-remapping profile
is not an acceptable substitute.

### Pre-physical test requirement (3 environments)

Trước khi nạp và đo kiểm trên bất kỳ board phần cứng x86 thật nào (mức `S2` / `S3`),
bản build kernel và image phải hoàn thành kiểm thử thành công trên cả 3 môi trường ảo hoá:

1. **QEMU TCG** (`qemu64,+pdpe1gb`, software-only): kiểm tra tính đúng đắn kiến trúc,
   không phụ thuộc vào phần cứng hay cờ CPU của máy chủ phát triển.
2. **QEMU/KVM** (`-accel kvm -cpu host`): kiểm tra tương tác tăng tốc phần cứng,
   CPUID thực tế, các cờ mở rộng và timing thực của CPU x86.
3. **VMware** (VMware Workstation / ESXi): kiểm tra tính tương thích với hypervisor
   công nghiệp chuẩn doanh nghiệp (ACPI, APIC routing, bộ điều khiển lưu trữ và mạng).

Chỉ khi bản build vượt qua kiểm chứng trên cả 3 môi trường này thì mới được chuyển sang
bước nạp trên board vật lý.

| Evidence level | Meaning | May appear in the machine table? |
|---|---|---|
| `S1 — qemu` | Exercised against a device model; useful regression only | No |
| `S2 — physical, development` | Captured on one exact machine, unqualified | Yes, as `S2` |
| `S3 — physical, qualified` | A governance decision binds the capture to a support claim | Yes, as `S3` (none exist yet) |
| `Not supported` | Fails a mandatory requirement, or no driver exists for its controller family | Recorded only for machines that were deliberately rejected and why |

A row in the machine table requires **every** mandatory requirement (R1–R9) to
have passed a capture. A machine that fails any mandatory item is recorded in
[§6 Rejected machines](#6-rejected-machines-record-and-why) instead of the
table. Within a row the evidence level is per-capability, and gaps are allowed
only for the **optional** capabilities in §2 — record each one in `Notes` rather
than leaving a blank cell.

## 2. Mandatory requirements (class level)

Any row in the machine table must satisfy all of these. Each is verified by
capture, not by the datasheet. A machine that fails any of them is recorded in
[§6](#6-rejected-machines-record-and-why), not in the machine table.

| # | Requirement | Why | Verified how |
|---|---|---|---|
| R1 | A **console**: a 16550-compatible UART at a SPCR/COM address (or BMC serial-over-LAN), or GOP linear framebuffer + PS/2/USB keyboard | Early boot outputs to COM1 best-effort; after ACPI/command-line evaluation the active console binds to SPCR (I/O or MMIO, GSI), explicit `cellos.console=` (`ttyS0..3`, `mmio@0x...`, `fb`), probed COM1, or the Limine linear framebuffer text console. Input merges UART RX, PS/2 and USB HID. | An **exact-resource** report for that machine — e.g. `setserial -g /dev/ttyS0` or the `ttyS0` sysfs/ACPI device record, or SPCR table dump, plus a Cellos-side exercised-console marker. Reaching `Cellos >` proves console output and input end to end. |
| R2 | A **reliable clocksource**: invariant TSC, kvmclock (guests), ACPI **HPET**, or the FADT ACPI PM timer in port I/O space | `init_timers` (`hal/arch/x86/src/x86_64.rs`) picks the first available of TSC (invariant or hypervisor-published frequency), kvmclock (stable host TSC), HPET, PM timer (`hal/arch/x86/src/x86_64/clock/mod.rs`, plan 261103-2218 phase 02). The tick uses TSC-deadline when the CPU supports it, otherwise the LAPIC periodic timer calibrated against that source. Without any source the boot prints `[x86-gate] timer CLOSED: no clocksource (TSC, kvmclock, HPET or PM timer)` and the UART RX IRQ stays closed. The G1 private lane still requires HPET | The `[x86-gate] timer: clock=… clockevent=…` boot line, plus ACPI tables (live OS) or the Cellos lines `[acpi] gates: … hpet=… fadt=…` and `[acpi] FADT: … pm_timer=…` |
| R3 | **Secure Boot can be disabled** (or ships off) | There is no signed or measured x86 boot path; code-signing/secure-boot is a separate Security-track item. A board that locks Secure Boot on **cannot be made compatible** — there is no exemption path. The machine pin (`/srv/.cellos-machine/pin`, ADR-0029 §3) is change detection only, **not** a boot-integrity control: the profile is selected by `cellos.profile=` in `limine.conf`, which nothing measures or verifies today, and a raw-disk or offline writer can delete or replace the pin. It becomes meaningful against a capable attacker only once Secure Boot (signed Limine EFI + `limine enroll-config`) or a measured boot covers `limine.conf` and the kernel | Firmware setup screen; record the menu path and BIOS version |
| R4 | Storage controller in **AHCI mode** (or an NVMe device), with **Intel VMD disabled**; a hardware RAID controller (PERC/MegaRAID, HPE Smart Array) only in HBA/pass-through mode that exposes NVMe or AHCI | NVMe ships and AHCI phases 02a/02b completed at QEMU only; physical persistence still needs capture. RST/RAID-only firmware mode fails closed. Cellos has no VMD, MegaRAID, mpt3sas or smartpqi driver: with VMD on, NVMe devices are hidden behind the VMD endpoint and are not found ([ADR-0031](decisions/0031-x86-platform-breadth-kvm-vmware-intel.md) §3) | Firmware SATA/VMD/RAID mode plus Cellos mount → write → reboot → read capture |
| R5 | Firmware boot of the Cellos ISO: legacy BIOS **or** UEFI (Limine) | Both images are produced (`build/make-iso.sh`, "fresh BIOS shell boot" + OVMF lanes exist) | Boot the ISO and capture the UART log |
| R6 | Network controller has an **exact device ID accepted by a shipped driver** | `e1000` admits `8086:100e` (82540EM) and `8086:100f` (82545EM copper); `e1000e` admits `8086:10d3` (82574L); `igb` admits `8086:10c9` (QEMU 82576) and `8086:1533` (flash-backed i210). The 82545EM and 82574L are the default NICs of VMware guests and the 82574L is q35's default NIC; their evidence is QEMU DHCP/VT-d (plan 261103-2218 phase 08), with VMware rows pending phase 09. QEMU or VMware DHCP/VT-d evidence is not actual NIC qualification; the i210/i211 family target does not admit every SKU, i211 or flashless i210, and I219 (`e1000e` `Target` IDs) stays unimplemented | `lspci -nn` + a Cellos vendor:device bind and DHCP Tx/Rx capture |
| R7 | Board facts recorded: vendor/model, chipset/PCH, CPU family, BIOS version **and date** | A row bound to "a model" instead of a machine is not evidence | Capture record |
| R8 | **VT-d and Interrupt Remapping enabled**, usable ACPI DMAR with INTR_REMAP, DMA isolation and IR active before device DMA | `x86_64-pc` requires isolation and interrupt remapping; DMAR without INTR_REMAP or unit without IR support fails closed | Firmware settings, DMAR record with INTR_REMAP, IRTA tables and Cellos isolation/IR-before-traffic capture |
| R9 | **Intel x86-64 CPU with VT-x and EPT exposed and enabled** | Required hardware prerequisite for the sole C2C target's Tier-3 path; VMX execution is still incomplete, and SVM results cannot substitute | Exact CPU/model plus firmware/feature capture; this establishes availability only, **not** guest execution or C2C guest-bridge readiness |

Optional (record, do not require): xHCI/HID (phases 03/03b have QEMU evidence),
multi-port COM/RS485 (phase 06 QEMU only; DE/RE timing unclaimed), mSATA/SATA DOM,
SIM/Mini-PCIe, audio, PS/2, Super-I/O GPIO/watchdog. Optional inventory is not an
authorization to launch peripheral work. Intel VMX execution and the explicit
C2C guest adapter require their own evidence even after R9 passes.

## 3. Machine table

Empty. No `S2`/`S3` row exists yet. Phase 07 first captures one exact Intel
machine. Only after first-machine qualification and separate procurement
approval may it add a second Intel node. Matching the first configuration is
preferred to reduce bring-up work, not mandatory; every unit needs its own
capture, and a different model must meet the same applicable gates. No Intel/AMD pair is planned.

| Machine / serial | Chipset / CPU | BIOS (version, date) | R1 COM1/SOL | R2 clocksource | R4 storage | R5 ISO boot | R6 NIC / PCI ID | R9 Intel VT-x / EPT | R8 VT-d / DMAR | R3 Secure Boot off | Profile / pin digest | Level | Captured | Log | Notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| _(none)_ | | | | | | | | | | | | | | | |

**Profile / pin digest:** the bound catalog profile and its source from the
`[x86-gate] board=… source=…` line, and the first 12 hex digits of the pinned
machine facts from `Init: machine pin: first boot — recorded …` (or `machine
pin` at the console). A later `mismatch` on that machine means its firmware,
identity, VT-d state or PCI set changed; record the new digest and the reason
after re-confirmation.

## 4. Acquisition checklist (pre-purchase)

Use this for selection, **not purchase authorization**. Phase 07 requires its
existing QEMU prerequisites and an explicit procurement decision; a "no" answer
is a stop, not a caveat.

1. COM1 present and **enabled in firmware**, standard address `0x3F8`, IRQ 4
   (header or DB9 — both fine). Ask which Super-I/O chip exposes it if the
   vendor knows.
2. A reliable clocksource (R2): invariant TSC (nearly every current Intel CPU),
   or ACPI HPET, or a port-I/O PM timer. Record which one Cellos selects.
3. SATA mode selectable to **AHCI** (not "RST only" / "RAID only"), or the
   device is NVMe.
4. **Secure Boot can be disabled**; record the menu path.
5. Intel x86-64 CPU, **VT-x/EPT and VT-d all exposed and enabled**; require
   usable firmware DMAR. Marketing family names do not establish these facts.
6. Exact NIC PCI ID known before purchase: current `igb` physical candidate is
   **flash-backed i210 `8086:1533`**, not generic i210/i211. `8086:10c9`,
   `8086:100e`, `8086:100f` and the discrete 82574L `8086:10d3` have QEMU
   evidence only; I219 (PCH LAN), `igc`/i225-i226 and Realtek are not
   implemented. No actual NIC is physically qualified.
7. Record USB/xHCI if present; QEMU HID evidence does not qualify the physical
   controller or replace the mandatory headless COM1 path.
8. Firmware mode: legacy BIOS or UEFI, and whether CSM can be enabled.
9. Record BIOS version/date and the first machine's exact configuration;
   a second node must match and wait for first-machine qualification and approval.
10. Record industrial extras only; multi-port serial QEMU evidence does not
    claim physical RS485 timing, watchdog or GPIO support.
11. If the machine is also the GPU target (ADR-0033): record the iGPU PCI ID and
    generation. Prefer an `xe`-official platform (Lunar Lake, Panther Lake,
    Wildcat Lake). ADL-N (N100/N150), MTL and ARL are force-probe in `xe`, a
    recorded risk. Also record whether the ACPI DMAR has a graphics DRHD and a
    stolen-memory RMRR. No GPU is qualified, and GPU support is not an HCL requirement.

## 5. Adding or changing a row

1. Capture on the exact machine: UART log for boot, storage
   (mount → write → reboot → read), network (DHCP Tx/Rx), and required VT-d DMA
   isolation before traffic. A failure of any mandatory requirement (R1–R9)
   disqualifies the row — record it in §6 instead; optional failures go in
   `Notes`.
2. Store logs under `.agents/<plan>/evidence/` (phase 07) and record the file
   hash.
3. Fill the row with R1–R9 (each must have passed its specified capture), the
   optional capabilities, and the evidence level (`S2` unless governance
   promotes it); record optional gaps in `Notes`. R9 feature presence is not
   VMX execution evidence, and an HCL row is not C2C/production admission.
4. Do not copy a row's conclusions to a different machine, revision, or BIOS
   version — that is a new row and a new capture.
5. Do not cite QEMU, compile, or host results in a row.

## 6. Rejected machines (record and why)

Empty. A machine rejected for a mandatory requirement (most likely R3, R1, R2 or
R4) is listed here with the reason, so the same mistake is not repeated.

## 7. Apple Silicon (aarch64 group 3)

**Owner:** `.agents/261104-0711-apple-silicon-group/` (phase 07 fills rows).
**Authority:** [ADR-0032](decisions/0032-apple-silicon-secondary-lane.md), a lower-priority
lane than Intel. Same closed-list rule as §1: a Mac that is not in the table is not
supported. The admitted chip order is M1 (t8103), then M2 (t8112), then
M1/M2 Pro/Max/Ultra. M3 needs a permissive reference first; M4 and later are parked.

Evidence levels for this section: `S1-qemu` and `S1-hvf` (Cellos as a guest of
macOS on Apple Silicon) are virtual-hardware results and never create a row.
`S2-hv` (bare metal under the m1n1 hypervisor) is development evidence for a phase
and does not create a row either. A row needs a native `S2` capture or `S3`.

| # | Requirement | Why | Verified how |
|---|---|---|---|
| A1 | Exact machine on the admitted chip list, with an Asahi installer-supported model | Cellos follows Asahi's hardware coverage; every chip is its own image (ADR-0029 §5) | Model identifier, SoC, RAM, serial and system-firmware (iBoot) version in the capture |
| A2 | Boot volume set to Permissive Security by the Asahi installer/1TR, with macOS and its recovery left intact | No signed or measured Cellos boot exists. The machine pin is change detection only (same limit as R3) | Installer transcript and the 1TR steps used |
| A3 | Bootloader versions recorded: m1n1 stage 1/2, and U-Boot + Limine when the EFI path is used | m1n1 builds the FDT the kernel trusts; an unrecorded loader makes the row unreproducible | Versions and hashes of each stage |
| A4 | A console: s5l UART through a USB-C debug adapter, or a framebuffer plus USB keyboard | No COM1 equivalent exists; the m1n1 vUART only exists under the hypervisor | Capture taken through the stated console |
| A5 | Every DMA-capable device in use behind a DART with translation on and bypass refused; internal storage only under the phase 06 SART decision | `DmaIsolation::Required` (ADR-0032 §8) | Boot log of DART binding plus the DART fault test |
| A6 | Internal NVMe (ANS2): mount → write → reboot → read on the Cellos partition only; macOS still boots afterwards | The disk also holds macOS, recovery and iSC partitions | Storage capture plus a macOS boot check |
| A7 | Wired Ethernet with a captured PCI ID admitted by a shipped driver | Exact-ID admission, as R6 | `lspci -nn` under Asahi + Cellos bind and DHCP Tx/Rx capture |
| A8 | All cores online (P and E clusters), timer and IPIs by FIQ | Spin-table SMP and AIC FIQ are Apple-specific paths | Boot log hart table and tick/IPI counters |

Never write to the SPI NOR, SEP, system-firmware or iSC partitions, and never
change boot policy beyond the installer's own steps. A machine that fails A1–A8
goes into the rejected list below with the reason.

| Machine / serial | SoC / RAM | System firmware | m1n1 / loader | A4 console | A5 DART | A6 storage | A7 NIC / PCI ID | A8 cores | Profile / pin digest | Level | Captured | Log | Notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| _(none)_ | | | | | | | | | | | | | |

Rejected Apple machines: none.
