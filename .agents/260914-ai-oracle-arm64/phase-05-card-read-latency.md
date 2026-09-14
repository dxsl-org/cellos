# Phase 05 — Reading a real checkpoint from the board's card does not complete

**Status**: measured, reported; the board payload now embeds a small real checkpoint instead
**Ceiling**: `physical` development hardware (Raspberry Pi 3 Model B+)

## What was tried and what happened

Following phase 04's workaround, the checkpoint was placed on the card's own FAT volume and the board
payload built without any model (`-AiCells`, 10 027 072 B — the size class that boots). The board
came up correctly and the storage path was healthy:

```
05:52:05 [ INFO] [sd] SD card probed: 30318592 sectors (~14804 MiB), block_addr=true
05:52:06 USER: [vfs] FAT32 /mnt/sd volume mounted
05:52:06 USER: [vfs] FAT32 /bin volume mounted
05:52:09 USER: [ai] inference service starting
```

Then the service went quiet. Over the next ~20 minutes it never printed
`[ai] model path:` / `model bytes:` / `model ready:`, and the oracle cell that had been started from
the console never received its `describe` reply (no `FAIL`, no `PASS` — it is blocked in its first
IPC call). The board itself stayed alive: a CR after nine minutes produced fresh console output
(`[service-registry] 10 -> tid 6`, `[dwc2-usb] Successfully registered as system NIC Driver Cell!`)
and the USB driver's retry loop resumed.

## Why, in numbers

`libs/ostd/src/clients/vfs/read_file/wire.rs` sets `MAX_READ_CHUNK = 4000`, and
`session.rs::read_chunks` requests at most that per IPC round trip. 26 671 328 B / 4000 B =
**6 668 round trips**, each one a request, a VFS-side FAT read from the SD card, and a response.
At 100-200 ms per trip — plausible on this board with a 4000-byte payload per trip — the read takes
11-22 minutes, which is what was observed. Nothing reported progress, so from the console this is
indistinguishable from a hang.

The same code path is why the *fixture* and *260K* payloads are fast: their checkpoint sits in VIFS1,
which the service reads through the kernel capability path (`read_model_cap`), not through VFS IPC.
The service's own comment records the same effect under QEMU: "146 s in QEMU TCG against a few
seconds for the cap path".

## Reported, with the two candidate fixes

This is the VFS/IPC lane's call, not the AI lane's:

1. **Raise the chunk.** A 4000-byte message is sized for a 4 KiB IPC page. If the transport can carry
   a larger payload (grant-backed scatter), 64 KiB chunks cut 6 668 trips to 407 — a 16× reduction,
   which would make a card-resident checkpoint usable.
2. **Teach the kernel FS the card volume.** `read_model_cap` works on the kernel's own FS
   (the VIFS1 ramdisk). The board's early boot already reads cells from a block-device volume
   (`[early] VIFS1-first "/bin/block" missing — falling back to block table`), so the same mechanism
   could serve `/mnt/sd` files and make the read a memcpy instead of thousands of messages.

## The AI lane's fallback for this measurement

Payload `cellos-ai-260k.uimg` embeds `stories260K.gguf` (1 185 376 B, a real llama2.c checkpoint with
a 512-token vocabulary) in VIFS1, keeping the payload at 10 027 072 B — the size class that boots —
and the service reads it through the cap path. `serve-ai-oracle.ps1 -Variant 260k` stages and runs it.
This yields a real `[ai] model ready: … resident bytes …` on the board. What it does not yield is the
25.6 MiB-checkpoint number: that size is blocked by this phase and by phase 04 (the 43.6 MB embedded
payload panics the kernel at compositor setup). Both are board-path limits with reproductions
attached, and neither is a property of the engine.
