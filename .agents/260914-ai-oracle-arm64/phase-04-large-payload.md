# Phase 04 — A large payload panics the board (reported, worked around)

**Status**: defect reproduced on the RPi3, handed to that lane; the AI lane takes the workaround
**Ceiling**: `physical` development hardware (Raspberry Pi 3 Model B+)

## The observation

Two netboot payloads, same cells, same kernel features, same boot chain:

| Payload | Size | Board result |
|---|---|---|
| AI cells + deterministic fixture checkpoint in VIFS1 | 10 027 072 B | boots to the shell; `ai-test` PASS |
| AI cells + `stories15M-q8_0` (25.6 MiB) in VIFS1 | 43 581 504 B | kernel panics at compositor setup |

The failing boot is normal until the compositor is spawned, then repeats forever:

```
22:30:44 [boot] kernel_phys_base=0x0000000000080000
22:30:45 [ INFO] RAM Disk: read-only, 40960 KB (81920 sectors)
22:30:48 USER: [bcm-display] validated framebuffer registered
22:30:48 [ INFO] [loader] SpawnFromElf: /bin/compositor (147168 bytes from grant)
22:30:48 [ INFO] ELF LOAD: 0x10E000000-0x10E00A42C flags=R-X
22:30:48 [ INFO] ELF LOAD: 0x10E00A430-0x10E00C860 flags=R--
22:30:48 [ INFO] ELF LOAD: 0x10E00D000-0x10E00D570 flags=RW-
22:30:48 [KERNEL PANIC] Critical failure.      (then once per iteration, indefinitely)
```

## What was ruled out

- **Not the AI lane.** The panic is in the display path, before the service is ever asked to infer;
  the same cells with a small payload boot and serve.
- **Not simply "a big VIFS1".** Reproduced the same 40 MB VIFS1 *including* `service-compositor` and
  `fb-console` on QEMU aarch64 virt: boots, `[ai] model ready: 32000 vocab, context 128, resident
  bytes 28087038`, then `[ai-test] PASS` (24 tokens, 3468 ms). So the limit is board-specific.
- **Not the device tree being clobbered.** Both boots report the same U-Boot placement
  (`Working FDT set to 3b3d7210`, `Loading Device Tree to 0x1fff7000`), far outside either payload's
  destination range (`0x80000` + payload size).
- **Not the kernel's own image reservation.** `kernel/src/boot.rs` derives `kernel_end` from linker
  symbols (`__kernel_end` / `__stack_top`), so a 43 MB image is accounted for in the reserved range,
  and the ramdisk is a `include_bytes!` static in `.rodata` with no heap copy
  (`kernel/src/task/drivers/ramdisk.rs` says so explicitly).

## What is left for the owning lane

The failure is at compositor cell setup on the board only. The most likely remaining shapes are the
board's own memory layout around the framebuffer (registered at `0x3E8E0000`, 3.6 MB) or a
fixed-size assumption in the board's frame-allocator/heap setup that a 43 MB image violates. Useful
next probes, none of which need the AI lane:

1. Print the allocator range and the heap's start on the board with both payload sizes (the `[boot]`
   line exists on other architectures behind a cfg; enabling it for `board-rpi3` is one line).
2. Same payload but with `driver-bcm-display`/`service-compositor` removed, to separate "large image"
   from "large image + framebuffer mapping".
3. A payload padded to intermediate sizes (fixture checkpoint + N MiB of padding) to find the
   threshold, which is a two-line change to the image inputs and one power cycle each.

## The AI lane's response

The checkpoint moved out of the image, which is where data belongs anyway: `service-ai` looks for
`/bin/ai-model.gguf` and then `/mnt/sd/ai-model.gguf` (`/mnt/sd` is the card's own FAT volume, P1) and
logs which path it used. `scripts/build-aarch64-cells.ps1 -AiCells` builds the AI cells with no
checkpoint, so the board payload stays in the size class that boots. Verified on QEMU aarch64 with the
model packed at `/mnt/sd`: `[ai] model path: /mnt/sd/ai-model.gguf`, `resident bytes 28087038`, PASS.
