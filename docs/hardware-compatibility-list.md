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
configuration needs independent qualification. AMD/new ARM/RISC-V expansion is parked.
This list and completed QEMU gates authorize neither purchase nor remote/production
activation; phase 07 and the existing security/authority gates still apply.

## 1. How to read this list

This is a closed list. A machine not in the table is **not supported** — it is
not "probably works". QEMU, compile and host results are regression evidence and
never qualify a machine; only a capture bound to one exact machine can create a
row.

The contract a machine is qualified against is the **generic**
`boards/pc/x86_64-pc` descriptor (build with `--features board-x86-pc`). It
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
| R1 | A 16550-compatible UART at COM1 `0x3F8`, IRQ 4 — a DB9 port, a mainboard COM header, an add-in card mapped there, or BMC serial-over-LAN | The only working x86 log **and** input path today (`kernel/src/main.rs:157`, `kernel/src/task/drivers/console_drv.rs:139-145`). Polled RX survives a closed IRQ (`:160,711-716`), but no UART means a silent boot | An **exact-resource** report for that machine — e.g. `setserial -g /dev/ttyS0` or the `ttyS0` sysfs/ACPI device record — showing base `0x3F8` **and** IRQ 4, **plus** a Cellos-side exercised-RX marker with the IRQ gate open. Reaching `Cellos >` alone is insufficient: polled RX works with the IRQ gate closed, so it proves neither the address nor the IRQ |
| R2 | ACPI **HPET** exposed | `x86_timer_ready` requires `lapic != 0 && ioapic != 0 && hpet_base != 0` (`kernel/src/main.rs:421`); there is no LPIT/TSC-deadline fallback. Without HPET the UART RX IRQ stays closed | ACPI tables (live OS) or the Cellos ACPI gate line `[acpi] gates: madt=… hpet=… mcfg=…` |
| R3 | **Secure Boot can be disabled** (or ships off) | There is no signed or measured x86 boot path; code-signing/secure-boot is a separate Security-track item. A board that locks Secure Boot on **cannot be made compatible** — there is no exemption path | Firmware setup screen; record the menu path and BIOS version |
| R4 | Storage controller in **AHCI mode** (or an NVMe device) | NVMe ships and AHCI phases 02a/02b completed at QEMU only; physical persistence still needs capture. RST/RAID-only firmware mode fails closed | Firmware SATA mode plus Cellos mount → write → reboot → read capture |
| R5 | Firmware boot of the Cellos ISO: legacy BIOS **or** UEFI (Limine) | Both images are produced (`build/make-iso.sh`, "fresh BIOS shell boot" + OVMF lanes exist) | Boot the ISO and capture the UART log |
| R6 | Network controller has an **exact device ID accepted by a shipped driver** | e1000 supports 82540EM; `igb` admits `8086:10c9` (QEMU 82576) and `8086:1533` (flash-backed i210). QEMU DHCP/VT-d evidence is not actual NIC qualification; the i210/i211 family target does not admit every SKU, i211 or flashless i210 | `lspci -nn` + a Cellos vendor:device bind and DHCP Tx/Rx capture |
| R7 | Board facts recorded: vendor/model, chipset/PCH, CPU family, BIOS version **and date** | A row bound to "a model" instead of a machine is not evidence | Capture record |
| R8 | **VT-d enabled**, usable ACPI DMAR and DMA isolation active before device DMA | `x86_64-pc` requires isolation; phase 05 is complete at QEMU only, not physical firmware qualification | Firmware settings, DMAR record and Cellos isolation-before-traffic capture; missing remapper fails closed |
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

| Machine / serial | Chipset / CPU | BIOS (version, date) | R1 COM1/SOL | R2 HPET | R4 storage | R5 ISO boot | R6 NIC / PCI ID | R9 Intel VT-x / EPT | R8 VT-d / DMAR | R3 Secure Boot off | Level | Captured | Log | Notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| _(none)_ | | | | | | | | | | | | | | |

## 4. Acquisition checklist (pre-purchase)

Use this for selection, **not purchase authorization**. Phase 07 requires its
existing QEMU prerequisites and an explicit procurement decision; a "no" answer
is a stop, not a caveat.

1. COM1 present and **enabled in firmware**, standard address `0x3F8`, IRQ 4
   (header or DB9 — both fine). Ask which Super-I/O chip exposes it if the
   vendor knows.
2. ACPI **HPET** advertised (some newer chipsets disable it).
3. SATA mode selectable to **AHCI** (not "RST only" / "RAID only"), or the
   device is NVMe.
4. **Secure Boot can be disabled**; record the menu path.
5. Intel x86-64 CPU, **VT-x/EPT and VT-d all exposed and enabled**; require
   usable firmware DMAR. Marketing family names do not establish these facts.
6. Exact NIC PCI ID known before purchase: current `igb` physical candidate is
   **flash-backed i210 `8086:1533`**, not generic i210/i211. `8086:10c9` has
   QEMU evidence only; `e1000e`/I219, `igc`/i225-i226 and Realtek are not
   implemented. No actual NIC is physically qualified.
7. Record USB/xHCI if present; QEMU HID evidence does not qualify the physical
   controller or replace the mandatory headless COM1 path.
8. Firmware mode: legacy BIOS or UEFI, and whether CSM can be enabled.
9. Record BIOS version/date and the first machine's exact configuration;
   a second node must match and wait for first-machine qualification and approval.
10. Record industrial extras only; multi-port serial QEMU evidence does not
    claim physical RS485 timing, watchdog or GPIO support.

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
