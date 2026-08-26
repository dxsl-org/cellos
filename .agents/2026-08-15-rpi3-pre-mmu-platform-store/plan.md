# RPi3 pre-MMU platform store fix

## Scope

- Replace the pre-MMU `Spinlock<Option<PlatformInfo>>` with immutable write-once storage.
- Lock Raspberry Pi 3 mini-UART core frequency to 250 MHz.
- Package `kernel8.img` as raw AArch64 and reject ELF output.

## Phases

1. **Platform storage** — preserve `platform::init`/`platform::with`, publish once on the boot CPU, read immutably without LL/SC.
2. **Regression guards** — unit-test write-once behavior and inspect the RPi3 early path for exclusive atomics.
3. **Boot packaging** — add `core_freq=250`, objcopy ELF to raw image, and validate magic/hash/size.
4. **Verification** — host tests, ARM64/RPi3 build, QEMU smoke, review, then real-board UART reproduction.

## Success criteria

- `platform::{init,with}` performs no `ldxr`/`ldaxr`/`stxr`/`stlxr` before MMU activation.
- `board-rpi3` builds and packages a non-ELF `kernel8.img`.
- Real RPi3 UART passes the former `abcpxyz` stop at 115200 baud.
