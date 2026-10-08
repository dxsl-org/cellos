# Cellos Bare-Metal Installation Guide

## 1. Prerequisites
- USB-to-TTL serial adapter cable
- MicroSD card (Class 10, 8 GB – 16 GB recommended)
- MicroSD card reader
- BalenaEtcher flashing software
- PuTTY terminal emulator

## 2. Choose the Boot Workflow

Use a full SD image only for first bootstrap or recovery. For repeated kernel
development on Raspberry Pi 3, keep firmware and U-Boot on the SD card and load
only the current Cellos kernel over a direct static TFTP link. This is the fast
iteration path: no card rewrite after each code change and no Raspberry Pi OTP
programming.

The tested development topology is:

- Windows Ethernet: `192.168.42.1/24`
- Raspberry Pi 3: `192.168.42.2/24`
- Direct LAN cable; Cat 5e or Cat 6 is sufficient
- SD card: Raspberry Pi firmware plus the Cellos U-Boot bootstrap
- TFTP payload: `cellos.uimg`
- Serial console: `COM4`, 115200 baud on the tested host

DHCP and Internet Connection Sharing are not used. WSL may remain running during
board reboots.

## 3. Flashing the OS Image

Treat this path as bootstrap or recovery only. For normal code iteration on
Raspberry Pi 3, keep the SD bootstrap fixed and use the TFTP lane in Section 7.
- Insert the MicroSD card into the card reader and connect it to your computer.
- Launch BalenaEtcher.
- Click **Flash from file** and select your Cellos OS `.img` file.
- Click **Select target** and choose the MicroSD card drive. (Ensure you select the correct target drive to avoid overwriting your system disk).
- Click **Flash!** and wait for the process to complete with a "Success" notification.

> [!CAUTION]
> Once flashing is complete, Windows may display a prompt stating the disk is unreadable and asking to format it. Click **Cancel** immediately—do not format the drive. Safely remove the MicroSD card.

## 4. Hardware Setup
- Insert the flashed MicroSD card into the slot on the underside of the Raspberry Pi 3.
- Connect the three leads (Ground/Black, RX/White, TX/Green) of the USB-to-TTL cable to the Raspberry Pi GPIO header according to the serial pinout diagram. Do not connect the external power supply to the Raspberry Pi yet.
- Plug the USB end of the TTL adapter cable into your computer.

## 5. Connecting to the Serial Console via PuTTY
Before launching PuTTY, you must identify which COM port number Windows assigned to your USB-to-TTL adapter.
- On Windows, right-click the **Start** button and select **Device Manager**.
- Expand the **Ports (COM & LPT)** section. Locate your serial adapter (e.g., "Silicon Labs CP210x..." or "USB-SERIAL CH340..."). Note the assigned port identifier in parentheses (e.g., `COM3` or `COM4`).
- Launch PuTTY.
- Under **Connection type**, select **Serial**.
- In the **Serial line** field, enter the identified COM port (e.g., `COM3`).
- In the **Speed** field, enter `115200` (the standard default baud rate for the Raspberry Pi serial console).
- Click **Open**. A blank terminal window will appear. It remains blank because the Raspberry Pi is currently powered off.

## 6. Booting a Fully Flashed Image
- Connect the power supply to the Raspberry Pi to power on the board.
- Observe the PuTTY terminal window. If the OS image is valid, the kernel boot log (`dmesg`) will begin outputting to the screen.
- Once the boot sequence completes, the terminal will display a login prompt (e.g., `Cellos>`).

## 7. Fast Raspberry Pi 3 Iteration over TFTP

Prepare the bootstrap SD once by following
[`tools/rpi3-netboot/README.md`](../../tools/rpi3-netboot/README.md). The U-Boot
build must keep `CONFIG_BOOTSTD_DEFAULTS` and `CONFIG_CMD_BOOTI` disabled because
Cellos is a raw kernel wrapped in a legacy uImage, not a Linux ARM64 Image.
Do not reflash the card for ordinary Cellos changes; keep the bootstrap fixed
and redeploy the kernel image over TFTP.

After building a new raw `kernel8.img`, wrap and publish it from PowerShell:

```powershell
.\scripts\build-aarch64-cells.ps1 -BoardRpi3
cargo build --release --features board-rpi3 `
  -p cellos-kernel --target aarch64-unknown-none-softfloat
aarch64-linux-gnu-objcopy -O binary `
  .\target\aarch64-unknown-none-softfloat\release\cellos-kernel `
  .\.agents\debug\rpi3-kernel8.img
pwsh -File .\tools\rpi3-netboot\deploy-rpi3-kernel.ps1 `
  -KernelImage .\.agents\debug\rpi3-kernel8.img
```

After each Windows reboot, restore the host's ActiveStore address from
Administrator PowerShell:

```powershell
pwsh -File .\tools\rpi3-netboot\serve-rpi3-netboot.ps1 `
  -ApplyNetworkConfig -ApplyFirewall -PreflightOnly
```

Before powering the Pi for each test, start the server:

```powershell
pwsh -File .\tools\rpi3-netboot\serve-rpi3-netboot.ps1
```

This host requires official Python 3.12. Its Laragon Python 3.14 installation
was verified not to receive UDP packets from the physical Ethernet adapter. A
successful transfer logs `TFTP RRQ cellos.uimg` followed by `TFTP DONE
cellos.uimg`; U-Boot then enters Cellos without another SD-card write.

Add `uart_2ndstage=1` to `config.txt` only when firmware-stage UART diagnostics
are needed. It is not required for normal netboot.

The current real-board gate reaches the Cellos scheduler and init services. The
RPi3 cell build disables the QEMU VirtIO input probe, so `/bin/input` relies on
the kernel UART push path instead of dereferencing `0x0A000000`. The verified
boot emitted `No VirtIO input device; relying on kernel push`, reached init
service supervision, and produced no `EC=0x24`, `FAR=0x0A000000`, input-service
death, or restart.

RPi3 console RX now enables AUX legacy IRQ 29 only after the kernel RX buffer
is initialized. That fixes the mini UART's 8-byte FIFO overrun under 115200-baud
bursts: the IRQ handler drains RX immediately, and direct polling stays as the
early-boot / lost-IRQ fallback. The real-board lane accepted raw
`echo 123456789\r`, returned `1 1 11` for `echo board-rpi3 \| wc\r`, and passed
100/100 unpaced burst commands in 1658 ms.

For unattended autoboot capture, connect Raspberry Pi TXD0 (physical pin 8) to
the adapter RX and connect ground, but leave the adapter TX lead disconnected
from Raspberry Pi RXD0 (physical pin 10). The tested adapter injected characters
that stopped U-Boot at its prompt when that return lead was connected. Reconnect
it only when an interactive U-Boot console is required.

This lane is an ARM64 boot/runtime regression lane, not yet proof that every G1
device feature works on Raspberry Pi 3.

RPi3 console input uses the BCM mini UART, while generic AArch64/QEMU keeps the
PL011 receiver. The real-board input gate connected adapter TX to RPi RXD0 only
after U-Boot had entered Cellos, sent `help` at 115200 baud, received the full
shell command listing, and returned to `ViCell >`.

The production RPi3 lane does not emit the old per-event `T<EC>`, timer `M`,
scheduler `N`, or context-switch `A` bring-up markers. A real-board boot and
interactive `help` gate reduced `T15` from `14,596` to `0` and `ANM` to `0`,
while retaining fault-only `FS0`-`FS3` diagnostics and bounded one-shot boot
markers.

## 8. Tier 3 Direct Firmware Boot

The existing U-Boot/TFTP production lane in Section 7 proves host shell
operation, not Tier 3 guest operation. Tier 3 needs `CurrentEL=2` on entry:
Cellos retains an EL2 monitor and runs its host at EL1. First qualify the
volatile profile on QEMU `raspi3b`, then try the *same* existing U-Boot SD
bootstrap over TFTP without flashing a different card:

```bash
bash scripts/make-hypervisor-fs-rpi3.sh --skip-fetch --volatile-disk
RPI3_GATE=machinery BOOT_WINDOW=90 bash scripts/qemu-rpi3-tier3.sh \
  target/rpi3-hv-embedded/kernel8.img
RPI3_GATE=boot BOOT_WINDOW=1200 bash scripts/qemu-rpi3-tier3.sh \
  target/rpi3-hv-embedded/kernel8.img
```

The strict `boot` gate is **load-sensitive and a hang is inconclusive, not
a result**. On 2026-10-01 the same payload that passed on 2026-09-30
(full transfer, `HVC ready=true`, Linux to `~ #`) hung three times with
the guest stopping right after `printk: legacy bootconsole [pl11]
enabled` — and so did the *previous* payload run as a control in the
same session, back to back, with identical Cellos-side logs and no
hypervisor error (`run_vcpu kernel error`, `guest timer snapshot failed`
or panic). The host's load average was ~28 on 27 CPUs at the time: this
lane needs a core per guest, so a busy workstation starves nested TCG.
Widen the window, confirm the host is idle, and re-run before reading
anything into a timeout; the physical board remains the qualifier for
Tier 3, and a QEMU hang neither passes nor disproves a board result.

### Tier 3 is "run this Linux app", not "boot a Linux machine"

Cellos is its own operating system: Tier 1 and Tier 2 cells are what it runs,
and Tier 3 exists to run a Linux application that has no Cellos build. The model
that follows from that — and the one the shell command is being shaped around —
is **one VM per app**: a launch names an app, the hypervisor cell creates the
guest for it, the app runs as the guest's init, and the VM is torn down with the
app. Nothing here is a Linux session with several apps in it, so a VM that
exists only for the length of one app costs the user nothing.

What that means for images and commands:

- **There are no tier-shaped profiles.** Cellos is embedded-first: every image
  is the base set (vfs + net + shell on a serial console) plus **orthogonal
  options** — `input`, `ui`, `ai`, `supervisor`, `tier3`, `tier3-autostart` —
  and the board descriptor supplies the drivers. A server that must be safer
  than it is capable is `--no-tier3`; a kiosk is `--ui --input --no-tier3`; a
  desktop is everything. Admission tiers are runtime policy and are the same in
  every image; an option decides only what is packaged and booted.
- **Tier 3 has three levels, and the option picks one.** Not packaged
  (`--no-tier3`: no `/bin/hypervisor`, no `vmlinuz`, no `initrd.gz`, and `hv`
  fails closed with "cell missing"), packaged but idle (the default: `hv`
  starts the guest on demand, and the EL2 smoke plus `HypervisorCap` still gate
  it at runtime), or preloaded (`--autostart`). The strongest level is the
  absence of the cell, not a flag the guest could talk its way past.
- One app per VM is what the app-launch path has to deliver: the app's name
  reaches the hypervisor cell (staged spawn argv), the cell puts it in the guest
  command line, and the guest's init execs it. The guest-side half needs an
  initramfs whose init reads that name and exits with the app — the SD profile
  already ships a custom initramfs (`tools/prepare-rpi3-guest-initramfs.py`),
  the volatile profile still boots the stock Alpine initramfs to `~ #`. Until
  that lands, `hv` boots the guest's own shell and the app launch is not
  claimed.
- **HDMI is an option, not a profile property.** `--ui` adds the compositor,
  `fb-console` and (where the board has a real display driver) KMS. The Pi
  Tier-3 image path carries no display driver, so it stays UART-console-only;
  what HDMI should show in Tier 3 is the **guest's** framebuffer (the VMM
  already emulates virtio-gpu), and presenting that surface is part of the
  one-app-per-VM work.

**Which guest runs is a build-time choice too** (`--guest`), mirroring the x86
lane instead of inventing a second mechanism: `alpine` is the proven 128 MiB
lane (static or musl binaries, shell tools); `alpine-wide` is 256 MiB for a musl
userspace with real packages — Python, Node, and a **headless** browser, which
needs no display device at all, only RAM (Chromium wants ~300–500 MiB, so a
browser base will likely be its own 512 MiB class); `alpine-gui` is 512 MiB for a
guest that draws a window, and its presentation path (virtio-gpu surface →
Cellos compositor → panel) is not built yet, so that profile is a build for the
day it lands, not a claim that a window appears today. The cell logs which one it
booted (`[hv] guest profile: alpine-wide (256 MiB)`), the carve follows the
profile (`boot_arm_profile.rs`), and two wide profiles at once is a compile
error.

Build one with the front end, which resolves the board, validates the option
combination and prints the gate:

```bash
bash scripts/build-image.sh --list
bash scripts/build-image.sh --board raspberry-pi/3-model-b --no-tier3
bash scripts/build-image.sh --board raspberry-pi/3-model-b --autostart
bash scripts/build-image.sh --board raspberry-pi/3-model-b --guest alpine-wide --autostart
```

### Tier 3 TFTP trial on the existing U-Boot card

The TFTP root's `cellos.uimg` is the **volatile** ARM64 diagnostic build
(60,538,944 bytes; SHA-256
`bf10cced6dc655b52602ddabc3ed7a093ef3a7d506e09877f61ec3c867827d1c`),
built 2026-10-03 as the **prompt-first** Tier-3 image: VFS + Input + Net
+ shell, no VM running at boot (`Init: tier-3 VM idle — run 'hv' in the
shell to start a guest`), guest started on demand. Its gate was
`RPI3_GATE=host` on this exact payload
(`build/rpi3-gate/ondemand-host2/raspi3b.log`): `SpawnFromElf:
/bin/shell` then `=== Cellos shell ready — type 'help' for commands ===`.
The other flavour — `make-hypervisor-fs-rpi3.sh … --autostart`, which
preloads the VM for servers — was verified the same day with
`RPI3_GATE=machinery` and `RPI3_GATE=boot` (`build/rpi3-gate/autostart-{machinery,boot}/`):
`[hv] vCPU ready — entering run loop` with the shell banner behind it,
and the Alpine guest reaching `~ #`. A rebuild is not byte-identical
even with the same flags — the FAT image stores each cell's file
timestamp, so a recompiled cell changes the payload's bytes without
changing its behaviour. What the record means is that the deployed file
is the one the named gate ran on, and the SHA above is the file being
served.
U-Boot's existing `boot.scr` uses `bootm` to load its raw payload at
`0x80000`. Keep the SD bootstrap unchanged; it has no
`/mnt/sd/guest_disk.img`, so this profile cannot test persistence.
Rollback copies in the TFTP root: the safe-USB build
`cellos.uimg.before-lanscratch-bf10cced`, the on-demand build
`cellos.uimg.before-ondemand2-96f9a2af`, the enumeration-retry build
`cellos.uimg.before-ondemand-deab6f59`, the carve build
`cellos.uimg.before-enumretry-8b4fa080`, the paced build
`cellos.uimg.before-carve-256f33df`, the coherent-EL2 build
`cellos.uimg.before-split-pacing-aaa48727`, the HVC-ready but
memory-incoherent trial `cellos.uimg.before-coherent-mmu-cc2baf12`,
first Tier 3 trial `cellos.uimg.before-el2-probe-6ab762d0`, the previous
HVC-ready trial `cellos.uimg.before-s2-probe-0a8813a2`, and production
payload `cellos.uimg.before-tier3-20260930-dec6d941`. The latest rollback copies
are `cellos.uimg.before-lanwakeup-412fdb41` and
`cellos.uimg.before-lanreply-745928f5` (the two builds before the LAN9514 reply
and wakeup fixes), `cellos.uimg.before-lanuboot-155c95c2` (before the driver was
aligned with U-Boot's register map and sequence), and
`cellos.uimg.before-lanorder2-7a521062`. Copy the desired
one to `cellos.uimg` to revert without reflashing the card.

### Guest RAM carve (board failure of 2026-10-02)

A board boot on 2026-10-02 reached the hypervisor cell and then printed
`[hv] create_vm failed — EL2 unavailable or host OOM allocating 128 MiB
guest RAM` followed by `[hv] service quiesced`: **no guest**, so nothing
could type into it. The same image had created its VM on 2026-10-01, and
the carve is the suspect because its scan released `FRAME_ALLOCATOR`
every 256 frames (a lock-hold mitigation for the RT watchdog) — any
allocation by another cell during the search could split the run being
counted, which makes the same layout succeed or fail depending on
timing. `allocate_guest_ram` now takes one lock and makes one linear
pass (`find_free_run`, run-length counting, frames marked in the same
hold), so no window is left for a candidate run to be taken.

Two things to read in the next kernel log, because the cell prints the
same sentence for every `create_vm` failure:

- `[hv] create_vm refused: EL2 monitor not verified (ready=…)` — the
  fail-closed EL2 gate, not memory.
- `[hv] create_vm: no contiguous guest run (… MiB total, … MiB used,
  largest free run … MiB)` — a genuine carve failure, now with the size
  of the largest free run so a fragmented map can be told from a small
  one.

The UART capture that reported this was cell-side output only; the
kernel's `[ ERROR]`/`[ WARN]` lines carry the reason and must be included
next time.

From Windows PowerShell in the repo root, start the established server:

```powershell
pwsh -File .\tools\rpi3-netboot\serve-rpi3-netboot.ps1
```

The NIC must already have `192.168.42.1/24`; if server preflight refuses,
follow [the static-network setup](../../tools/rpi3-netboot/README.md#deploy-and-serve)
with its identified adapter MAC, rather than opening a different interface.
Power on the Pi after the server says `Preflight PASS`; look for `TFTP RRQ
cellos.uimg` and `TFTP DONE cellos.uimg` before interpreting UART output.
The first, 56,340,544-byte uImage passed physical TFTP transfer and CRC
verification but booted with `HVC ready=false`. Moving the HVC handshake
before EL1 caching let the second physical run reach `HVC ready=true`,
but the first guest smoke failed. A register-returned EL2 probe in the
third run identified the cause: EL1 held vCPU entry `0x40000000`,
instruction `0xd2800540`, and written Stage-2 descriptors; MMU-off EL2
saw entry `0`, all descriptors `0`, and faulted at PC `0` with
ESR `0x82000005`. Conversely, EL2 returned those exit registers through
HVC, while EL1 still read the original `exit_esr`/`exit_elr` markers.
EL2's uncached physical RAM accesses and EL1's Normal-WB cached accesses
did not share a coherent view. The fail-closed smoke kept `HypervisorCap`
closed through that run.

The coherent-EL2 build ran on the board on 2026-10-01 and settled it.
Both `first-run` lines agreed field for field —
`EL1 entry=0x40000000 insn=0xd2800540` against
`EL2 entry=0x40000000 s2=[0x406e003,0x406f003,0x406b7ff] insn=0xd2800540`,
`exit=[0x5a000000,0x40000008,irq=0]` returned to EL2 and read back
`EL1 exit=[0x5a000000,0x40000008,irq=0]` — followed by
`HVC/MMIO/VI/PREEMPT smoke PASS; HypervisorCap open`,
`[hv] vCPU ready — entering run loop` and Linux starting on the
Cortex-A53 (`Booting Linux on physical CPU 0x0000000000 [0x410fd034]`,
`Linux version 6.12.13-0-virt`, `earlycon: pl11 at MMIO 0x0000000009000000`).
That is the EL2 row closed on hardware. The captured trace ended at the
guest's early console, so that run recorded no hardware `~ #`; the board
reached the guest's own `~ #` prompt on 2026-10-06 with the volatile profile
(`[hv] volatile disk selected by build policy`, then the guest's `ifconfig` and
`ping` output), so the `~ #` leg of the physical gate is now observed. The
fail-closed rule stands: only board
`HVC/MMIO/VI/PREEMPT smoke PASS; HypervisorCap open` followed by
`[hv] vCPU ready — entering run loop` **and** Linux `~ #` qualifies the
physical guest. QEMU or `HVC ready=true` alone does not.

The Pi image packages signed `/bin/input`, `/bin/dwc2-usb` and
`/bin/lan9514` and routes focused USB HID keystrokes into the guest
PL011 **only after the guest runs**.

**Complete-split pacing is verified on the board (2026-10-01).** The
earlier trace had carried the whole failure: the start-split was
accepted (`hcint=0x00000022`) and **both** complete-split retries came
back in that same microframe —

```
split ss  hfnum=0000184A->0000184A->0000184A hcint=0x00000022 hcsplt=0x8000C083
split cs1 hfnum=0000184A->0000184A->0000184A hcint=0x00000042 hcsplt=0x8001C083
split cs2 hfnum=0000184A->0000184A->0000184A hcint=0x00000042 hcsplt=0x8001C083
err=IO - complete split NYET exhausted (00000002 attempts)
```

`0x42` is NYET plus the channel halt. A hub runs the low-speed half in
the microframe *after* the one that carried the start-split, so a
complete-split issued in the same microframe can only ever be told
NYET: the budget of two was spent before the hub could have an answer.
The driver now paces each retry (`wait_microframe`), waits without
yielding so the pair stays inside the window, and bounds the loop with
the rule U-Boot's working driver uses — abandon a split whose
complete-split is still NYET more than four raw `HFNUM` ticks after its
start-split (`split_window_open`; the counter wraps, so the comparison
wraps with it). The deployed build shows the difference on the
low-speed device at hub port 3:

```
split ss  hfnum=00001930->00001930->00001930 hcint=0x00000022 hcchar=0x20D28008 hcsplt=0x8000C083
split cs1 hfnum=00001931->00001931->00001AC0 hcint=0x0000000A hcchar=0x00D28008 hcsplt=0x8001C083
[usb-hid] vendor:product 10c4:0005
[usb-hid] configured interfaces=1
[usb-hid] interface 0 alt=0 class=3 subclass=1 protocol=2 interrupt-in=1
[usb-hid] HID interface 0 class=3 boot=1
[usb-hid] iface 0 addr=3 port=3 hub=1
[lan9514] Hardware MAC: B8:27:EB:12:34:56 (Ready)
[usb-hid] driving 1 HID interface(s)
```

The two halves are now one microframe apart (the `cs1` arm at
`00001931` against the start-split's `00001930`), the device answers,
and a **HID boot keyboard** (class 3, subclass 1, protocol 2) is
claimed and driven while the LAN9514 Ethernet path stays up. What this
run does **not** prove: **keystrokes reaching the guest** (no key was
pressed, and the captured trace stops before `~ #`), and hub port 5's
device, which answers every complete-split with **NAK**
(`hcint=0x00000012`, then `GET_DESCRIPTOR(device) exhausted 3 attempts`)
— that device is not responding; it is not a scheduling failure. The
keyboard itself refuses two optional control requests: the HID
report-descriptor read (`wValue=0x2200`) and the LED output report
(`SET_REPORT wValue=0x0200`) both STALL, and the driver records the
latter once (`iface 0 rejected the LED output report; waiting for
lock-state change`), so lock LEDs do not light on this keyboard. A
`cs` line ending in STALL or NAK is a different answer from NYET and
must not be read as the same bug.

**Why the guest shell answered "not found" for `ls`.** The volatile profile
boots the stock Alpine `initramfs-virt`, and that image is a bootstrap: it
carries `busybox`, `kmod`, `modprobe` and `sh` and **no applet symlinks** — no
`bin/ls`, `bin/cat`, `bin/ps`, no `/etc/profile`. The applets are compiled into
the busybox binary that is sitting right there (all 66 names the shell needs
were found in its applet table), so `busybox ls /` worked while `ls` did not.
The Pi builder now repacks the guest initramfs (`tools/prepare-rpi3-shell-initramfs.py`):
73 applet links, each name checked against that table so a link cannot dangle,
and an `/init` that mounts `/proc`, `/sys` and `/dev`, loads the virtio
transport and NIC the guest DTB declares, and hands over to the shell — the
volatile profile's `rdinit` is `/init` now, which is also where app launch will
exec its app instead. (Bypassing Alpine's init meant nothing modprobed those
devices, so the guest came up with only `lo` even though the DTB carries a
virtio-net node and the cell implements the device.) The kernel already hands `rdinit` the default
`PATH=/sbin:/usr/sbin:/bin:/usr/bin`, so no profile script is involved.

Two things are worth keeping from how this was verified. The builder grew the
x86 lane's `INITRD_OVERRIDE` hook, and the check ran an initramfs whose init
executed `ls /`, `uname -a`, `ps`, `mount` and `ifconfig -a` before
`exec /bin/sh`: the QEMU boot gate then showed the commands resolving *inside
the guest* (`LS_ROOT_OK`, `PS_OK`, `MOUNT_TABLE_OK`), not merely that the links
existed in the archive. That check caught a real bug the archive listing could
not: the first cut linked `sbin/mount -> busybox`, a same-directory target, so
it resolved to a `/sbin/busybox` that does not exist and the guest answered
`mount: not found` while `/bin` links worked. Targets are relative to the
link's own directory now. The second is the window: QEMU runs this guest about
four times slower than the board (70 s of guest time at 300 s of wall clock), so
the `boot` gate needs `BOOT_WINDOW=900`; the earlier `machinery` runs had never
required the guest shell, which is why a 300 s window looked like a failure.

**Both directions are up.** `[net-bridge] first e1000 RX len=64` and
`[net-bridge] first e1000 TX accepted len=304` on the same boot: the chip
receives, transmits, and the bridge now sees the reply. The failure detail added
for the previous fault paid for itself immediately — `reply from tid 0, expected
6 (status 0x01)` named the next one, a kernel exit-watch wakeup (sender 0, no
payload) landing between a request and its reply and being read as a refusal,
which invalidated the NIC connection in the middle of DHCP. Wakeups are skipped
now and the wait continues; only a real sender mismatch or a timeout ends it.
What is left on the board is the lease itself: `[net] DHCP acquired — IP
configured`.

**The frame left, but the bridge was told it had not.** The driver cell printed
`TX packet transmitted OK` while the net bridge still logged `accepted=false`,
because replies routed back through the `/bin/lan9514` front-end arrive from that
cell's tid and the bridge waits on the NIC cell's tid — a timeout, not a
refusal. The host cell replies to the client directly now; requests still only
reach the USB dispatcher after the capability-free front-end decodes them, and
the response branch (with its `encode_response`/`decode_response` envelope) went
with the indirection. The bridge also names the reason the first time a reply is
not accepted, so a tid mismatch cannot hide behind a single `accepted=false`.

**The chip is configured and frames leave the board.** The read-backs say so
directly: `MAC_CR=0x0008000C` (`MCPAS|TXEN|RXEN`), `TX_CFG=0x00000004`
(`TX_CFG_ON`), `HW_CFG=0x00001022` (`BIR|MEF|BCE`), alongside `PHY link up
(BMSR=0x0000782D), auto-negotiation complete` and `[dwc2-usb] TX packet
transmitted OK`. The `accepted=false` in the net bridge's `first e1000 TX` line
is that service's first attempt, which races the NIC cell's bring-up and is
logged only once; the bridge now reports the first success too. `ADDRL` and
`ADDRH` read all-ones, so the chip really has no address and the driver programs
a locally administered `02:00:00:00:00:01` — a placeholder that has to become
unique per board before two Cellos Pis share a LAN. What is still unproven on
the board is the receive side and the lease: `[net] DHCP acquired — IP
configured` and `[net-bridge] first e1000 RX len=…`.

**Then the chip answered, and the driver's own gate was in the way.** With the
U-Boot map in place the board wrote `0x5A5AA5A5` to a spare register and read
`0x0000A5A5` back: the write reached the chip and the chip answered. The gate
rejected it because it compared 32 bits and `VLAN1` keeps a 16-bit tag. The ID
read was also in the wrong place — before the lite reset it returned
`0xEC000002`, the USB function's `idProduct`/`bcdDevice` pair, so the resets now
come first (U-Boot's order) and the ID is logged rather than enforced, since
U-Boot does not check it either. What follows the gate — MAC address, buffers,
`HW_CFG`, LEDs, flow control, `MAC_CR`, PHY auto-negotiation and the TX/RX
enable — had been skipped every time on a chip that was answering.

**The reference was in the tree the whole time.** The Pi's Ethernet works when the
firmware netboots the Cellos image, so the hardware and the DWC2 host path are
fine — and `.agents/debug/u-boot-v2026.07/drivers/usb/eth/smsc95xx.c` is the
driver that does it. Against that file the faults were plain: the vendor requests
are read `0xA1` / write `0xA0` with the offset in `wIndex` (this driver had the
requests right but the offset in `wValue`, and the attempt to fix it swapped the
requests instead — which is why the probe returned one value for every offset),
the register map is `ID_REV = 0x00`, `HW_CFG = 0x14`, `MAC_CR = 0x100`,
`ADDRL = 0x108`, `MII_ADDR = 0x114`, `MII_DATA = 0x118` (this driver used
`0x50`/`0x74` and an indirect `MAC_CSR` window the chip does not have), and the
init sequence is lite reset, PHY reset, MAC address, buffers, `HW_CFG` flags,
LEDs, flow control, `MAC_CR`, `VLAN1`, checksum offload off, PHY
auto-negotiation, then TX and RX enable — which the driver now follows line for
line, with a scratch write/read-back and the PHY's own `BMSR` as the gate.

**What the probe said, and what it rules out.** With the vendor requests fixed
and the NIC no longer holding up the port scan, the board reported
`probe direct[0x00]=0x98021E11 [0x64]=0x98021E11 [0x6C]=0x98021E11` and
`csr[ADDRL]=0x00000000 csr[ADDRH]=0x00000000`. Three different register offsets
returning the *same* value rules out "the offsets are wrong": the chip is not
selecting a register by `wIndex` at all, even though the setup packet places
`wValue` and `wIndex` correctly and the hub answers per-port class requests
through the same path. The value is also not a chip ID — it changed between
boots (`0xB8021E11` then `0x98021E11`, low three bytes identical) — so it is
uninitialized chip state, which fits the Pi: the VideoCore firmware normally
programs the LAN9514's MAC from OTP, and a bare-metal Cellos boot does not. Two
consequences are recorded rather than guessed at: the driver leaves an
unrecognized chip unconfigured (it cannot program what it cannot address), and
the next measurement asks the same registers with the index in `wValue` instead
of `wIndex`, which is the one placement left that would make a chip answer.

**Fixing the register path broke the keyboard — and that is a lesson about
ordering.** With the vendor requests right, the LAN9514's register reads returned
real data, which turned its waits into real USB transfers; that init ran inside
the hub port scan on port 1, ahead of the keyboard's port 4, so the keyboard
never enumerated at all (no LEDs, no keys) while the rest of the boot continued
as if nothing had happened. The NIC candidate is now remembered during the scan
and initialized after it, the waits are tightened to tens of iterations, and a
chip whose ID does not match is reported and skipped rather than configured
through addressing that just failed. One more thing the fix revealed: the ID
read returned `0xB8021E11` — not zero any more, but not a LAN9514 ID either —
so the driver prints an addressing probe (direct `0x00`/`0x64`/`0x6C` against the
indirect MAC-CSR `ADDRL`/`ADDRH`) to settle which path returns the chip's MAC
before anyone configures it again.

**The Pi's Ethernet never worked, and the reason was two swapped numbers.** The
board log said `[lan9514] WARN: unexpected chip ID: 0x00000000` and then
`Hardware MAC: B8:27:EB:12:34:56 (Ready)` — the placeholder in the driver, which
means `ADDRL`/`ADDRH` never read either. `read_reg` was issuing vendor request
`0xA1` (the *write* request) with an IN direction and the register index in
`wValue`; the LAN95xx protocol is `0xA0` read / `0xA1` write with the index in
`wIndex` and `wValue` zero. Every read therefore failed and the buffer stayed
zero, so the MAC soft reset, the PHY auto-negotiation and `MAC_CR`'s TX/RX
enable all wrote into a chip that was not listening. Bulk-OUT still completed,
which is why TX reported OK while frames died in the chip and the net bridge's
`first e1000 RX` line never appeared — the guest's `eth0` RX stayed at 0 for the
same reason. Both requests are issued the documented way now, and the driver
reports the chip MAC and `PHY link up/down (BMSR=0x…)` so a cable problem and a
configuration problem stop looking alike; register waits are bounded so a chip
that never clears BUSY fails the access instead of hanging the cell.

**Why a keystroke felt slower once the NIC was up.** Input is only forwarded from
the guest's idle exits (`WFI` and every preemption), and each of those ran, in
order, a compositor service lookup, a timed IPC round trip to the Net Cell, and
only then the input poll — so every echoed character queued behind a network
call. With `eth0` up the guest left idle far more often, which made the cost
visible. Input is forwarded first now, and the RX poll is guarded: it reads the
guest's available ring and only asks the Net Cell for a frame when the guest has
an RX buffer posted. That also fixes a silent drop in the old order, which
dequeued a frame and then threw it away when no buffer was available. What is
left is the guest's own cost of a live network stack — its threads and timers —
which is the minimal-guest and SMP work, not the input path.

**The guest's own devices are there now too.** Bypassing Alpine's init meant
nothing modprobed the virtio devices the guest DTB declares, so `ifconfig -a` on
the board showed only `lo` while the DTB carried a virtio-net node and the cell
implemented the device. The volatile `/init` loads `virtio_mmio` and
`virtio_net`, and the QEMU boot gate now shows five bound devices
(`virtio0`–`virtio4`) and `eth0 Link encap:Ethernet HWaddr 52:00:00:00:BB:00`
with `ls /sys/class/net` = `eth0 lo`. Nothing starts a DHCP client, so the
interface comes up unconfigured — the traffic path to the host net service is
part of the app-launch milestone, not this one.

**2026-10-04, the last hop: a keystroke reaches the guest.** With the console
filter in place the board's guest shell took a typed command byte-for-byte —
`~ # uname -a` arrived clean (no `[71;11R` prefix) and the guest's own shell
answered `/bin/sh: uname: not found`, which is the guest *executing* what was
typed; the applet is simply absent from the stock Alpine initramfs. The filter
reported itself once (`[hv] suppressed 1 cursor-position quer(ies): this console
does not answer them`), and the VM was created normally, so the smoke and the
cache maintenance were both fine on that boot. What remains is not the input
path: it is what the guest *contains* (a minimal busybox) and how fast it runs.

**2026-10-04, the EL2 smoke failed intermittently on the board — and it is a
cache-alias bug, not a regression.** A later boot of the *same* payload that had
already passed (and booted a guest) reported
`[pi-monitor] first-run … exit=[0x82000005,0x200,irq=0] EL1 exit=[0x82000005,0x200,irq=0]`
and `HVC/MMIO/VI/PREEMPT smoke FAILED; HypervisorCap closed`, so `create_vm` was
refused and the guest could not start. Every *input* to that line was identical
to the passing run — same `root`, `page`, `s2=[0x406e003,0x406f003,0x406b7ff]`,
same `insn=0xd2800540` — and QEMU passes the same kernel, which is the signature
of a real-cache effect (TCG has no caches): the smoke writes the guest blob
through EL1's mapping and the guest fetches it through Stage-2, a different VA
and ASID, so `dc cvau` + the per-VA `ic ivau` in `sync_instruction_cache` do not
cover the alias and the guest's first fetch can read a stale line. Two fixes,
one per path: the smoke now cleans the blob page to the point of coherency and
invalidates the whole I-cache before entry (`invalidate_instruction_cache_all`),
and `run_vcpu` does the same once per VM over the whole guest-RAM window on its
first entry — which also covers the real guest image, since
`write_guest_memory` copies without any cache maintenance. The smoke's failure
path additionally logs EL1's own read of the root leaf and of the blob
(`EL1 leaf0=… EL1 insn=…`), so the next board run distinguishes "the two regimes
disagree about memory" from "software agrees and the hardware walk/fetch did
not".

**The 2026-10-04 fourth run: `ls` is clean and `hv` boots the guest from the
prompt.** The `ls` dedup is confirmed on hardware — `ls /bin` printed every name
once (`ai … vfs`, no repeated `config`/`input`/`shell`/`vfs`) — and typing `hv`
at `Cellos >` ran the whole on-demand path: `[hv] hypervisor service cell
starting`, `[hv] guest profile: alpine (128 MiB)`, `hv: hypervisor cell started,
tid=13`, `[hv] VM created vm_id=1`, `kernel=34603008 B initrd=8743468 B
(streamed)`, `[hv] vCPU ready — entering run loop`, then Linux 6.12.13 on the
Cortex-A53 (`Booting Linux on physical CPU 0x410fd034`, `earlycon: pl11`). The
lock LEDs kept following the keys (`iface 0 LEDs 0x02 / 0x00`), and `LS` in
capitals was refused (`DENY launch edge … /bin/LS`) exactly as the case-sensitive
spawn path intends. What the trace stops before is the guest's own `~ #`: the
last hop to witness is a keystroke landing inside the guest.

**The 2026-10-04 third run: keyboard, LEDs and the HDMI console all work on the
board.** With the known-good receiver (`2a7a:8a53`) on hub port 4 the full-option
image enumerated it (`vendor:product 2a7a:8a53`, two interfaces, `HID interface 0
class=3 boot=1`, `driving 2 HID interface(s)`), the shell received typed commands
(`ls` printed the root, `ls /bin` the cells) and the lock LEDs moved with the
keys (`[usb-hid] iface 0 LEDs 0x02 / 0x00 / 0x01`). The display path ran end to
end as well: `[bcm-display] validated framebuffer registered … 1280x720`, then
`[fb-console] background surface created` → `[compositor] first scanout flush
submitted` → `[bcm-display] first scanout flush completed`. Tier 3 stayed idle
(`Init: tier-3 VM idle`), so the guest is still started by hand with `hv`.

Two small findings from the same trace, neither blocking: interface 1 of the
receiver declares no LED output report and STALLs `SET_IDLE`, which the driver
records once and tolerates; and `ls` prints some entries twice (`config config`,
`input input`, `shell shell`, `vfs vfs`), which looks like the listing merging
the kernel's VIFS1 view of `/bin` with the VFS cell's.

**The 2026-10-04 second run: the probe fix worked, the device is the silent
one.** Ports 2 and 3 reported empty, **port 4 was probed**, and its device
(low-speed, `status=0x0303`) answered **NAK** to all three `GET_DESCRIPTOR`
attempts with the paced halves correct (`ss hfnum=0000253E` → `cs1 0000253F
hcint=0x00000012`) — so the driver ended it exactly as designed:
`device never answered at address 0; leaving the port alone`. The same device has
now failed this way on two different ports (5 on 2026-10-02, 4 on 2026-10-03 and
today), which is what "the device is silent" means. After that the hub itself
started failing transactions — the *valid* standard `GET_STATUS` probe came back
`XACTERR`, so `hub stopped answering — ending port enumeration` fired for a real
reason this time and port 5 was not probed. A keyboard that never answers
address 0 cannot be enumerated by any host; use the device that enumerated
(`10c4:0005`) or the receiver `2a7a:8a53` before reading anything into a silent
port.

**The 2026-10-04 run: the "hub stopped answering" guard was asking the wrong
question, and hid the keyboard's port.** The board reached its prompt and
enumerated port 1 (the LAN9514 Ethernet), then port 2 reported empty — and the
next line was `[usb-hid] hub stopped answering — ending port enumeration`, so
ports 3–5 were never probed and the keyboard on port 4 was never tried. The
probe behind that line reused `get_port_status(0)`: a *hub-class* request whose
`wIndex` is a port, so asking it with `wIndex = 0` is an invalid question and
this hub answers STALL. `Hub::is_responsive()` now sends the **standard device**
`GET_STATUS` (`0x80`, 2 bytes) — a request every device must answer — and
`attach_port` reports `Empty` vs `Failed` so the probe only runs after a real
failure, never after an empty port. Until the next board run confirms it, the
lesson stands on its own: a guard that aborts a whole enumeration must be built
on a request the peer is required to answer.

**The 2026-10-03 evening run: the port-reset retry was the wrong shape,
and is now bounded.** The prompt-first image reached the board's own
prompt (`Init: tier-3 VM idle — run 'hv' in the shell to start a guest`,
`SpawnFromElf: /bin/shell`, `=== Cellos shell ready — type 'help' for
commands ===`, `Cellos >`), and the keyboard on port 4 answered **NAK**
to every `GET_DESCRIPTOR(device)` exactly as it had the day before. The
driver's new retry then made things worse, and the trace says why:

```
control status failed: bmRequestType=0x23 bRequest=0x01 wValue=0x14 wIndex=4 … STALL   ← CLEAR_FEATURE(C_PORT_RESET)
control status failed: bmRequestType=0x23 bRequest=0x03 wValue=0x04 wIndex=4 … STALL   ← SET_FEATURE(PORT_RESET)
control data failed:   bmRequestType=0xA3 bRequest=0x00 wIndex=4 … STALL               ← GET_STATUS(port 4)
control setup failed:  … wIndex=5 … XACTERR                                            ← and port 5's status became unreadable
```

The hub stalled the *pre-emptive* `CLEAR_FEATURE(C_PORT_RESET)` — a
change bit the earlier reset had already cleared — and then stalled
every request after it, so one silent device turned into a hub that
stopped answering. The rule is now: **retry a port reset only when the
device answered address 0** (a device that never answers is not fixed by
another reset), never pre-clear the reset-change bit (`reset_port`
already clears it and ignores that clear's error), and **stop
enumerating ports** as soon as the hub fails its own status read
(`hub stopped answering — ending port enumeration`). A silent device now
ends with `device never answered at address 0; leaving the port alone`
and the port's `final status`, which is all the evidence that device can
give.

Two more things this profile does not do, so they are not bugs to chase:

- **No HDMI output.** The Tier-3 image packages no display path at all —
  no compositor, no `fb-console`, no display driver — so the screen keeps
  whatever the firmware left in the framebuffer. UART is this profile's
  console. A display console would mean adding those cells to the Tier-3
  boot table and image (a profile decision, not a fix).
- **Typing at the prompt needs the UART adapter's TX line.** Reading the
  console only needs Pi TXD0 → adapter RX; *typing* needs adapter TX →
  Pi RXD0 (pin 10) and a common ground. With an RX-only hookup the prompt
  appears and nothing can be typed into it, which is exactly what the
  board showed before the keyboard question even arises.

**The 2026-10-02 run had a different hub, not a different driver.** Hub
ports 2, 3 and 4 reported `status=0x0100` (power only, nothing
connected) — the port the keyboard enumerated on the day before was
empty — and the single low-speed device on port 5 answered every
complete-split with **NAK** (`hcint=0x00000012`), so
`GET_DESCRIPTOR(device)` was exhausted and the driver ended with
`[usb-hid] no HID device attached`. Nothing in that trace exercises the
paced path's success branch. Note what *was* verified earlier, because
it is the standard to restore: an earlier generation of this driver was
physically verified on a Raspberry Pi 3 with receiver `2a7a:8a53`
(`a` reaches the shell, keypad Enter submits it, `ls` completes) and
with working lock LEDs, on the **host-shell** profile. The Tier-3
profile has no host shell, so a key can only land in the guest — which
makes a running guest a prerequisite for any keystroke claim.

**2026-10-03: the guest starts, the keyboard still does not.** The
atomic carve fixed the VM: `[hv] VM created vm_id=1`, `[hv] vCPU ready —
entering run loop`, Linux booting on the Cortex-A53. The keyboard was
moved to hub port 4, where it reported as low-speed
(`after reset status=0x0303 speed=1`) and answered **NAK** to every
`GET_DESCRIPTOR(device)`: the paced halves are visible
(`ss hfnum=00001BB7->…` → `cs1 00001BB8 … hcint=0x00000042` (NYET) →
`cs2 00001BB9 … hcint=0x00000012` (NAK)), so the schedule is right and
the device is silent — the same behaviour the device showed on port 5
the day before, and different from the device that enumerated on port 3
(`10c4:0005`). Two things follow. First, the driver now re-resets a port
whose enumeration failed and retries, up to three rounds, because a
low-speed device that answers NAK after a hot-plug often comes up only
after a second reset — the log shows `enumeration failed; re-resetting
hub port N and retrying` and then either the device or
`hub port N final status=0x…`, which distinguishes "vanished" from
"present but silent". Second, the device that *did* enumerate
(`10c4:0005`) and the historically verified receiver (`2a7a:8a53`) are
the known-good references: use one of them on ports 1–4 before reading
anything into a silent port.

There is no Cellos host shell in this profile, so keys cannot be typed
at `create_vm failed`. UART terminal input is independent of USB:
once a guest runs, Cellos forwards UART RX into its PL011. For UART
typing, connect the USB-to-TTL adapter's TX to Pi RXD0 (pin 10)
**after** U-Boot's countdown; Pi TXD0 and ground alone display logs but
cannot send keystrokes. An interactive QEMU UART trial with the
previous image ran `echo PI_UART_INPUT_OK` at the Alpine prompt;
QEMU had no attached USB HID device, so that test did not validate
the physical USB keyboard.

### SD-backed direct firmware profile (separate card)

The SD-backed profile is a different build and needs its own card image:

```bash
bash scripts/make-hypervisor-fs-rpi3.sh --skip-fetch
RPI3_GATE=boot BOOT_WINDOW=2400 bash scripts/qemu-rpi3-tier3.sh \
  target/rpi3-hv-embedded/kernel8.img disk_rpi3_hv.img
```

The builder emits raw `target/rpi3-hv-embedded/kernel8.img`, an embedded
signed VIFS1 with the Alpine kernel/initrd, and (by default) a 1 GiB
`disk_rpi3_hv.img`. The SD image's P1 has `bootcode.bin`, `start.elf`,
`fixup.dat`, `config.txt` (`arm_64bit=1`, `kernel=kernel8.img`) and
`guest_disk.img`; it is a *direct firmware* image, not a U-Boot image.
P2 has a valid empty cell bootstrap table: the signed boot Cells come from
VIFS1. P3/P4 use the kernel's canonical non-overlapping partition map, and
P1 mounts at `/mnt/sd` for the guest's ext4 image. The persistent profile
adds Alpine's pinned ext4 modules to the initramfs and loads virtio-mmio,
virtio-blk and ext4 before mounting `/dev/vda` at `/mnt/disk`. The boot gate
checks the mount marker, but durability additionally needs a write, sync,
reboot and read-back with the **same** SD image. Re-running the builder
reformats `disk_rpi3_hv.img` and erases that proof.

For an interactive durability check (the gate itself redirects stdin), run:

```bash
qemu-system-aarch64 -machine raspi3b -cpu cortex-a53 -m 1G \
  -display none -serial null -serial stdio \
  -kernel target/rpi3-hv-embedded/kernel8.img -no-reboot \
  -drive if=sd,file=disk_rpi3_hv.img,format=raw
```

At the guest `~ #` prompt, run
`echo PI_DATA_123456 > /mnt/disk/pi-proof && /bin/busybox sync &&
/bin/busybox cat /mnt/disk/pi-proof`. Stop QEMU after `sync` returns;
launch the **same command above without rebuilding**, then run
`/bin/busybox cat /mnt/disk/pi-proof` and compare the bytes.

The Pi's mini UART is 115200 baud. Verify the startup log says
`[pi-monitor] entry EL=2 host EL=1 HVC ready=true` and the monitor smoke
opens `HypervisorCap`. The U-Boot TFTP runs have confirmed EL2 entry and
HVC op0 but not the Stage-2 guest smoke; neither alone establishes a
usable guest monitor.
Use the strict `RPI3_GATE=boot` gate for a guest shell and SD mount; neither
the monitor smoke nor guest Linux boot messages establish that result.

QEMU `raspi3b` returned an `echo PI_TIER3_EXEC_OK` result from the volatile
guest. The SD-backed guest mounted ext4, wrote and synced `PI_DATA_123456`
to `/mnt/disk/pi-proof`, and returned the same contents after QEMU restarted
with the unchanged SD image. The host-extracted backing ext4 file also held
the marker. Guest boot under QEMU-TCG printed a soft-lockup warning before
reaching the shell; physical monitor readiness, real-board VM execution and
real-board storage durability remain unverified.
Keep the existing Section 7 TFTP card for recovery; flashing the new SD
image replaces that bootstrap and requires explicit selection of the
target card.
