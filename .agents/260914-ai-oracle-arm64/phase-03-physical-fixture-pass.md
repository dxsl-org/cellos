# Phase 03 — The AI oracle on the physical Raspberry Pi 3

**Status**: fixture PASS on the board; the 25.6 MiB checkpoint run is lined up
**Ceiling**: `physical` development hardware (exact device: Raspberry Pi 3 Model B+, boardrev a22082)

## What ran

U-Boot (already on the SD card from the previous netboot work) pulled the AI payload over the static
TFTP lane — `TFTP RRQ cellos.uimg -> 10027072 bytes` / `TFTP DONE`, 3.4 s — and booted Cellos on the
board. Init started the inference service fail-soft and the console reached its shell:

```
22:02:33 SER: [ai] inference service starting
22:02:33 USER: [ai] engine load: 0 ms
22:02:35 USER: [ai] model ready: 270 vocab, context 128, …
22:02:39 USER: === Cellos shell ready — type 'help' for commands ===
22:02:39 USER: Cellos >
```

The oracle was then run from the board's own console (`ai-test`) and passed on real Cortex-A53:

```
[ai-test] AI inference oracle starting
[ai-test] model=tiny-llama-64
[ai-test] vocab=270 context=128 sessions=4
[ai-test] generate: 8 tokens in 59 ms (135593 tokens/s x1000, 6 polls)
[ai-test] greedy ids matched: 8 tokens over 6 polls
[ai-test] embedding matched: 64 dims
[ai-test] abandoned session released; service still serving
[ai-test] prompt stream matched: 8 tokens
[ai-test] PASS
```

This is the lane's first exact-device evidence: the same golden ids and embedding that the QEMU legs
reproduce, produced by the real service on real silicon, with the real console path. Artifacts:
`evidence/board-fixture-boot.log` (boot + service) and `evidence/board-fixture-run.log` (the oracle).

## Three defects the board found that QEMU could not

1. **The shell never accepts a path argument.** Typing `/bin/ai-test` produced
   `DENY launch edge: … target=/bin//bin/ai-test` and `command not found`:
   `cells/tools/shell/src/executor.rs::spawn_external` unconditionally prefixes `/bin/`, so an
   operator who types the path they see in every document gets a doubled path and a message that
   reads as if the cell were missing. The working spelling is the bare name (`ai-test`), which is what
   the driver and the lane README now use; the pass-through fix (`if prog starts with '/', use it
   as-is`) is three lines in that function and lands in the same reviewed launch edge, so it changes
   no capability — reported rather than applied, because it is the shell lane's call.
2. **A driver bug of my own that cost a boot**: the capture required `Cellos >` to be the last thing on
   a line. The board's USB driver retries every ~5 s (`[dwc2] TIMEOUT ch=…`), so the prompt was
   permanently glued to an error line and the driver typed nothing for twelve minutes. Now matched
   anywhere on the line.
3. **`U-Boot>` is the prompt, not `=>`** — the driver only knew the latter, so it could not have
   started a boot from an idle board.

## Board-side conditions observed (reported, not this lane's)

- `[dwc2] TIMEOUT ch=00000002 hcint=0x00000000 hcchar=0xC0981200` / `[dwc2-usb] TX packet transmission
  failed` repeats every ~5 s for as long as the board is up. The Pi 3's Ethernet sits behind the
  SMSC LAN9514 USB hub, so this is the network path, not the AI lane — but it floods the console and
  is worth the USB lane's attention.
- One cell terminated during boot: `[fault-probe] a64 vector=0 ec=0x24 iss=0xf … far=0xa000000` then
  `[fault] Cell 5 … terminated: cause=0x9200000f`. The boot continued to a working shell and the
  service answered, so this looks like the dev-policy fault probe; recorded because "looks like" is
  all a log line can support.

## Next step in this phase

The memory-budget number needs the real checkpoint: payload `cellos-ai-15m.uimg` (26,671,328 B model
inside a 43.6 MB kernel) is staged as `cellos.uimg`, and one power cycle produces
`[ai] model ready: 32000 vocab, context 128, resident bytes 28087038` plus a real-model continuation —
the observation CP-3's third leg asks for.
