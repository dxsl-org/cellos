# TetrisOS

TetrisOS is a small 32-bit IA-32 freestanding kernel that boots through GRUB 2 from an ISO image and runs a playable Tetris game in VGA mode 13h.

This project is an independent educational falling-blocks OS demo and is not affiliated with or endorsed by The Tetris Company.

## Dependencies

Ubuntu/Debian:

```sh
sudo apt install build-essential nasm grub-pc-bin grub-common xorriso qemu-system-x86
```

Arch Linux:

```sh
sudo pacman -S base-devel nasm grub libisoburn qemu-system-x86
```

Required tools: `gcc`, `nasm`, `ld`, `grub-mkrescue`, `xorriso`, and `qemu-system-x86_64`.

## Build

```sh
make
```

This produces `kernel.elf`, prepares the `iso/boot/grub/` tree, and emits a bootable `tetris.iso`.

## Run

```sh
make run
```

The run target launches:

```sh
qemu-system-x86_64 -cdrom tetris.iso -m 32M
```

The ISO can also be attached as an optical disk in VirtualBox.

## Compatibility

TetrisOS is built for 32-bit i486-compatible CPUs and has been tested with QEMU's 486 CPU model. It can run with roughly 3 MB of RAM, though 8 MB or more is recommended for emulator and bootloader variance.

## Controls

- `G`: choose German/Austrian QWERTZ layout on the startup layout screen
- `U`: choose US QWERTY layout on the startup layout screen
- `Enter`: start or restart
- `A` or Left arrow: move left
- `D` or Right arrow: move right
- `S` or Down arrow: soft drop
- `W` or Up arrow: rotate clockwise
- `Space`: hard drop
- `P`: pause or unpause
- `Escape`: return to the keyboard layout screen / reset

Keyboard input is handled through IRQ1 PS/2 scancode set 1. The startup screen lets you select German/Austrian QWERTZ or US QWERTY mappings.

## Sound

TetrisOS uses the PC speaker through PIT channel 2. Music is encoded as a small table of note frequencies and durations, so no MP3, WAV, filesystem, decoder, or external library is needed.

## 486 Check

```sh
make check-486
```

This disassembles `kernel.elf` and fails if Pentium Pro-era `cmov`/`fcmov` instructions are present.
