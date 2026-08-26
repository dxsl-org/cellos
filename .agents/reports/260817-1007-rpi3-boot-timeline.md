# RPi3 boot timeline QA

## Verdict

PASS. The board fetched and started the published Cellos image, reached the
service registry, and announced the ViCell shell without panic or service
restart output.

## Observed UART timeline

Baseline is the first UART byte observed by the Windows logger, not power-on.

- First UART byte: 0.000 s
- U-Boot banner: 2.110 s
- TFTP request: 6.704 s
- Image transfer complete: 14.367 s
- Kernel entry: 14.461 s
- Scheduler start: 16.811 s
- Input service selected kernel push: 18.026 s
- Service registry verified: 18.342 s
- ViCell shell ready: 33.173 s

The 9,564,224-byte image transferred at the U-Boot-reported 1.2 MiB/s.

## Diagnostics

- `panic`: 0
- `EC=0x24`: 0
- `service restarted`: 0
- Removed hot-path probes `T15` and `ANM`: 0
- `FS0` through `FS3`: 0 exact fault markers; the substring in `VIFS1` is not
  an `FS1` marker.

## Evidence

- `C:\Users\Admin\AppData\Local\Temp\cellos-rpi3-boot-timeline.log`
- `C:\Users\Admin\AppData\Local\Temp\cellos-rpi3-boot-timeline-raw.log`

## Limits

This run measures host-observed UART milestones. Exact power-on-to-first-byte
latency was not instrumented, and no interactive shell command was sent during
this run because the adapter TX line remained disconnected.
