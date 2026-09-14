# Handoff — closing the RPi3 memory-budget leg

**State**: everything except two manual steps is done; a 12-hour capture window was open from
2026-09-15T04:28 local time.

## What is staged and ready

| Piece | State |
|---|---|
| TFTP server | already running on the host (UDP 69, pid 8520), serving `tools/rpi3-netboot/root` |
| Payload | `cellos.uimg` = `cellos-ai-nomodel.uimg`, 10 027 072 B, sha256 `7EF469F9…` (AI cells, no checkpoint in the image) |
| Host NIC | `Ethernet` ifIndex 26 with `192.168.42.1/24` (applied in an earlier session) |
| UART | COM4 at 115200; the driver holds it while its window is open |
| Model (to place) | `.ai-models/stories15M-q8_0.gguf` → the card's boot volume as `ai-model.gguf` |

## The two manual steps

1. Copy the checkpoint onto the SD card's **boot volume** (the FAT partition with `config.txt` and
   `kernel8.img`) as `ai-model.gguf`. `/mnt/sd` in Cellos is exactly that partition (P1), so no
   imaging tool is involved — one file copy, and the boot files are untouched.
2. Insert the card, power the Pi. The rest is automatic if a capture window is open.

## If a window is not open (run it yourself)

```powershell
# Administrator PowerShell on the Windows host
pwsh -File \\wsl.localhost\Ubuntu\home\dmin\cellos\tools\rpi3-netboot\serve-ai-oracle.ps1 `
  -Variant nomodel -SkipServer -ComPort COM4 -DriveShell -TimeoutSec 2400
```

`-SkipServer` uses the TFTP server that is already up (only one process can hold UDP 69).
The transcript streams to `.agents/260914-ai-oracle-arm64/evidence/rpi3-nomodel-<stamp>.log`, and the
driver types `ai-test` at the prompt itself — the bare name, because the shell resolves bare names
under `/bin` and mangles an explicit path into `/bin//bin/<name>`.

## What a pass looks like

```
[ai] model path: /mnt/sd/ai-model.gguf
[ai] model bytes: 26671328 read in … ms
[ai] model ready: 32000 vocab, context 128, resident bytes 28087038
USER: [ai-test] real model continuation: 24 tokens
USER: [ai-test] text: … (prose)
USER: [ai-test] PASS
```

`resident bytes 28087038` with a real checkpoint served from the card is CP-3's third leg at the
`physical` development-hardware ceiling: the number the QEMU legs predicted, observed on the board.
The fixture leg at that ceiling is already recorded (`phase-03-physical-fixture-pass.md`).

## If it does not pass

- No console output at all: the board is not powered, or the adapter's RX is disconnected from the
  Pi's TXD0 (pin 8). A bare CR after 45 s is the driver's own wake-up probe.
- A TFTP request that never completes: the running server pins the client to `192.168.42.2`; check
  the direct cable and that no other TFTP process took UDP 69.
- `[ai] no model at any of 2 candidate paths`: the copy did not land in the boot volume, or its name
  differs — the service looks for exactly `ai-model.gguf` at the volume root.
- A kernel panic at compositor setup: the payload staged is not the nomodel one (the `15m` variant
  embeds the checkpoint and is the variant that panics on this board; see
  `phase-04-large-payload.md`).
