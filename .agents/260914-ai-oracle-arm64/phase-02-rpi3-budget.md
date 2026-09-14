# Phase 02 — RPi3 memory-budget leg: prepared, awaiting the board

**Status**: prepared and locally verified; the physical run needs the board
**Ceiling**: `physical` development hardware (exact device) — a QEMU result is not a substitute

## Why this is the remaining piece

CP-3's gate reads "QEMU RV64/ARM64 and RPi3 memory budget validation". Both QEMU legs run
(phase 01; the CI job is a two-arch matrix). The third leg is the one the QEMU numbers cannot stand
in for: whether a Cell that serves a real checkpoint fits and behaves on the actual board, whose
budget is 1 GB of RAM, a 32 MiB Cell VA slot, and the service's 29 MiB `large-arena` heap.

The numbers already say it *should* fit — the engine reports `resident bytes 28 087 038` for a
25.6 MiB checkpoint, and `cells/services/ai/src/main.rs` refuses anything above its `ENGINE_LIMIT` —
but "should fit" computed from a QEMU log is exactly the kind of claim this repository does not
accept for physical hardware.

## What was prepared

- `scripts/build-aarch64-cells.ps1` gained an opt-in AI lane: `-AiModel <path to a GGUF>` builds
  `service-ai` (with `large-arena`) and `ai-test`, packages `/bin/ai`, `/bin/ai-test` and
  `/bin/ai-model.gguf` into the aarch64 `kernel_fs.img`, and refuses a checkpoint above
  `ENGINE_LIMIT` (29 MiB) instead of shipping an image whose AI service truthfully refuses every
  request. Off by default: a 26 MB checkpoint does not belong in every aarch64 image.
- With `-BoardRpi3` the packaging lands in `target/rpi3-embedded/`, which `kernel/build.rs` picks up
  automatically for a `board-rpi3` build — the same convention the rest of the aarch64 lane uses, so
  there is still one source of truth for what an aarch64 image carries.
- The boot path is already wired for this: `cells/tools/init`'s service table starts `/bin/ai` with a
  fail-soft policy on every board ("when the image has no `/bin/ai` (or no model), init skips it with
  a log line"), and `kernel/src/loader/launch_profile/targets.rs` admits `/bin/ai-test` on the
  *shell* launch edge. Init deliberately does **not** auto-spawn the test cells on `board-rpi3`, so
  the oracle is run by the operator from the console — which is also the more honest demonstration.

Produced and verified locally (contents, not behaviour):

```
pwsh scripts/build-aarch64-cells.ps1 -BoardRpi3 -AiModel .ai-models/stories15M-q8_0.gguf
  -> target/rpi3-embedded/kernel_fs.img   24 files, 40960 KB
     /bin/ai 291448 B, /bin/ai-test 165928 B, /bin/ai-model.gguf 26671328 B
bash scripts/flash-sd-physical.sh --board rpi3 --output rpi3-ai-cellos.img
  -> P1 FAT32 with the firmware and kernel8.img = 43 581 440 B (the embedded VIFS1 above)
```

The smaller variant, useful as a wiring check before the real measurement, swaps the checkpoint for
the deterministic fixture:

```
pwsh scripts/build-aarch64-cells.ps1 -BoardRpi3 -AiModel models/tiny-llama-64.gguf
  -> kernel_fs.img 8192 KB, kernel8.img ≈ 11 MB
```

## The physical procedure (needs the board)

1. Flash: `sudo dd if=rpi3-ai-cellos.img of=/dev/sdX bs=4M status=progress conv=fsync` (or
   `--device /dev/sdX` in the same script).
2. Boot with the UART console attached (or read the HDMI console) and capture the log.
3. Expect, from the service that init started:
   `[ai] model bytes: 26671328 read in … ms` / `[ai] engine load: … ms` /
   `[ai] model ready: 32000 vocab, context 128, resident bytes 28087038`.
4. At the `Cellos >` prompt run `/bin/ai-test`. Against a real checkpoint the oracle prints the
   continuation, the embeddings line, and `[ai-test] PASS` with its own token timing.
5. Record: the two AI lines, the oracle's `generate: N tokens in X ms`, and the board's
   `cat /proc/meminfo`-equivalent if the shell offers one.

## What the result would and would not prove

It would give the AI lane its first **exact-device** evidence: the same fixture ids and the same
resident-bytes accounting on real Cortex-A53 silicon, with the real 1 GB budget and the real SD-card
read path — the number a server/office cohort would actually live with. It would not qualify the
board for production, make any throughput claim beyond that device, or substitute for the RPi3 lane's
own security posture: RPi3 is a development board, never a production-security qualification target.
