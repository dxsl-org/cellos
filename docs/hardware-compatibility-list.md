# Cellos Hardware Compatibility List (HCL)

**Status:** no physical x86 machine is qualified. The machine table below is
empty on purpose.
**Owner:** `x86_64 PC lane` plan, `.agents/261004-1957-x86-pc-lane/` (phase 07
fills rows; phase 01 defines the model).
**Read with:** `docs/roadmap/hardware-tracks.md` (gate inventory X86-PC-0..7) and
`docs/specs/04-hardware.md` §7 (x86 PC driver order).

## 1. How to read this list

This is a closed list. A machine not in the table is **not supported** — it is
not "probably works". QEMU, compile and host results are regression evidence and
never qualify a machine; only a capture bound to one exact machine can create a
row.

The contract a machine is qualified against is the **generic**
`boards/pc/x86_64-pc` descriptor (build with `--features board-x86-pc`). It
declares only facts that hold for any PC and only drivers whose cells exist, so
as of phase 01 it lists COM1/16550, IOAPIC, HPET, PCIe ECAM, NVMe and e1000 —
nothing about AHCI, xHCI or `igb` until those drivers ship.

| Evidence level | Meaning | May appear in the machine table? |
|---|---|---|
| `S1 — qemu` | Exercised against a device model; useful regression only | No |
| `S2 — physical, development` | Captured on one exact machine, unqualified | Yes, as `S2` |
| `S3 — physical, qualified` | A governance decision binds the capture to a support claim | Yes, as `S3` (none exist yet) |
| `Not supported` | Fails a mandatory requirement, or no driver exists for its controller family | Recorded only for machines that were deliberately rejected and why |

A row in the machine table requires **every** mandatory requirement (R1–R7) to
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
| R4 | Storage controller in **AHCI mode** (or an NVMe device) | A driver exists for NVMe (`cells/drivers/nvme`); AHCI is roadmap gate X86-PC-1. RST/RAID-only firmware mode fails closed | Firmware setup — the SATA mode option, not the marketing spec |
| R5 | Firmware boot of the Cellos ISO: legacy BIOS **or** UEFI (Limine) | Both images are produced (`build/make-iso.sh`, "fresh BIOS shell boot" + OVMF lanes exist) | Boot the ISO and capture the UART log |
| R6 | Network controller belongs to a **driver family Cellos ships** | e1000 (82540EM) today; `igb` i210/i211 is phase 04. Other Ethernet classes fail closed (`kernel/src/task/drivers/pcie_ecam.rs:894`) | `lspci -nn` on a live OS + a Cellos boot log line naming vendor:device |
| R7 | Board facts recorded: vendor/model, chipset/PCH, CPU family, BIOS version **and date** | A row bound to "a model" instead of a machine is not evidence | Capture record |

Optional (record, do not require): VT-x (needed for Tier 3 on Intel — backend
P09), VT-d (needed for DMA isolation — phase 05), xHCI (phase 03), multi-port
COM/RS485 (phase 06), mSATA/SATA DOM, SIM/Mini-PCIe (no driver planned), audio,
PS/2, Super-I/O GPIO/watchdog (no driver planned).

## 3. Machine table

Empty. No `S2`/`S3` row exists yet; phase 07 creates the first two (one Intel,
one AMD).

| Machine | Chipset / CPU | BIOS (version, date) | R1 COM1/SOL | R2 HPET | R4 storage | R5 ISO boot | R6 NIC | VT-x | VT-d | Secure Boot off | Level | Captured | Log | Notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| _(none)_ | | | | | | | | | | | | | | |

## 4. Acquisition checklist (pre-purchase)

Print this and verify before buying; a "no" answer is a stop, not a caveat.

1. COM1 present and **enabled in firmware**, standard address `0x3F8`, IRQ 4
   (header or DB9 — both fine). Ask which Super-I/O chip exposes it if the
   vendor knows.
2. ACPI **HPET** advertised (some newer chipsets disable it).
3. SATA mode selectable to **AHCI** (not "RST only" / "RAID only"), or the
   device is NVMe.
4. **Secure Boot can be disabled**; record the menu path.
5. VT-x exposed; prefer VT-d (Whiskey-Lake-class and later U-series usually have
   it, Haswell-ULT U-series does not).
6. NIC family known before purchase (`igb`/i210/i211 is the phase-04 target;
   `e1000e`/I219, `igc`/i225-i226 and Realtek families are not implemented yet).
7. USB controller type (xHCI) and whether a PS/2 or USB keyboard is available —
   USB input needs phase 03.
8. Firmware mode: legacy BIOS or UEFI, and whether CSM can be enabled.
9. Record BIOS version and date once the machine arrives; keep it in the row.
10. Industrial extras (multi-COM, RS485 jumper, watchdog, 8-bit GPIO):
    record presence; do not treat them as working until phase 06 and its
    hardware evidence.

## 5. Adding or changing a row

1. Capture on the exact machine: UART log for boot, storage
   (mount → write → reboot → read), network (DHCP Tx/Rx), and DMA isolation
   where VT-d exists. A failure of any mandatory requirement (R1–R7)
   disqualifies the row — record it in §6 instead; optional failures go in
   `Notes`.
2. Store logs under `.agents/<plan>/evidence/` (phase 07) and record the file
   hash.
3. Fill the row with R1–R7 (each must have passed a capture), the optional
   capabilities, and the evidence level (`S2` unless governance promotes it);
   record optional gaps in `Notes`.
4. Do not copy a row's conclusions to a different machine, revision, or BIOS
   version — that is a new row and a new capture.
5. Do not cite QEMU, compile, or host results in a row.

## 6. Rejected machines (record and why)

Empty. A machine rejected for a mandatory requirement (most likely R3, R1, R2 or
R4) is listed here with the reason, so the same mistake is not repeated.
