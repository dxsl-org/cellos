# Phase 06 — QEMU q35 Run Script + Boot Verify

**Status:** TODO  
**Priority:** High — final validation  
**Estimated effort:** Medium (build tooling + QEMU flags + debug iteration)

---

## Context Links

- `run.ps1` — current RISC-V QEMU invocation
- `kernel/linker-x86-64.ld` — higher-half 0xFFFFFFFF80000000
- Research: QEMU report §A1–A6 (q35 QEMU flags, VirtIO PCIe vs MMIO)

---

## Overview

The x86_64 kernel cannot boot via QEMU `-kernel <elf>` directly because:
1. QEMU's `-kernel` on x86_64 uses a simplified Linux boot protocol, not Limine
2. The kernel requires Limine to set up the higher-half virtual address (paging)

Solution: **Limine BIOS ISO boot** using xorriso + the `limine` binary.

Build pipeline:
1. Compile kernel ELF for x86_64
2. Create an ISO filesystem with: `limine.cfg`, `limine-bios-cd.bin`, `limine-bios.sys`, kernel ELF
3. Run xorriso to generate ISO
4. Run `limine bios-install` to install the BIOS boot sector
5. Boot: `qemu-system-x86_64 -machine q35 -cpu qemu64 -m 256M -cdrom vicell-x86.iso -serial stdio -no-reboot`

---

## Requirements

- `run-x86.ps1` builds the kernel, creates an ISO, and launches QEMU
- Boot produces serial output: `[ViCell] kernel boot v...` on COM1
- Kernel reaches the `ViCell>` shell prompt OR produces enough output to confirm
  progress (memory init, HAL init, scheduler start) before any cell-loading issues
- Script exits cleanly with QEMU exit code 0 or Ctrl-a x

---

## Architecture

### Limine tool availability

The `limine` binary (for `bios-install`) is available from the Limine v8.x GitHub releases
as a static binary. For Windows: `limine.exe` from the release, or compile from source.

Check: does the dev machine have `limine.exe`? If not, the build step can be skipped and
the ISO built manually. Add a check in `run-x86.ps1`.

Alternative: use pre-built ISO (commit it to `.gitignore`d `build/` directory) for fast iteration.

### `limine.cfg` content

```
TIMEOUT=0

/ViCell x86_64
    PROTOCOL=limine
    KERNEL_PATH=boot:///kernel.elf
```

### Build directory layout for ISO

```
build/x86-iso-root/
  EFI/BOOT/              (optional, for UEFI boot)
  boot/
    limine/
      limine-bios-cd.bin
      limine-bios.sys
    limine.cfg
    kernel.elf
```

### `run-x86.ps1` outline

```powershell
# 1. Build kernel
Write-Host "Building x86_64 kernel..."
cargo build --release -p vicell-kernel --target x86_64-unknown-none `
    -Z build-std=core,alloc `
    --config "target.x86_64-unknown-none.rustflags=['-C', 'code-model=kernel']"
$kernel = "target/x86_64-unknown-none/release/vicell-kernel"
if (-not (Test-Path $kernel)) { Write-Host "Build failed"; exit 1 }

# 2. Setup ISO root
$iso_root = "build/x86-iso-root"
New-Item -ItemType Directory -Force "$iso_root/boot/limine" | Out-Null
Copy-Item $kernel "$iso_root/boot/kernel.elf"
Copy-Item "limine/limine-bios-cd.bin" "$iso_root/boot/limine/"
Copy-Item "limine/limine-bios.sys"    "$iso_root/boot/limine/"
Copy-Item ".agents/260607-1543-x86-hal-bringup/limine.cfg" "$iso_root/boot/limine.cfg"

# 3. Build ISO with xorriso
xorriso -as mkisofs `
    -b boot/limine/limine-bios-cd.bin `
    -no-emul-boot -boot-load-size 4 -boot-info-table `
    -o build/vicell-x86.iso `
    $iso_root

# 4. Install Limine BIOS sector
& ".\limine\limine.exe" bios-install build/vicell-x86.iso

# 5. Boot in QEMU
$qemu = "qemu-system-x86_64"
& $qemu -machine q35 -cpu qemu64 -m 256M `
        -cdrom build/vicell-x86.iso -boot d `
        -serial stdio `
        -no-reboot -no-shutdown
```

### Rust build flags for x86_64-unknown-none

x86_64 kernel code MUST avoid red-zone (the ABI uses a 128-byte red-zone that interrupt
handlers would corrupt without this flag):

```
-C target-feature=+mmx,+sse,-red-zone
-C code-model=kernel
```

Add to `.cargo/config.toml` under:
```toml
[target.x86_64-unknown-none]
rustflags = ["-C", "code-model=kernel", "-C", "target-feature=-red-zone"]
```

**Note:** do NOT use `-C relocation-model=pic` for x86_64 (unlike RISC-V where PIE
self-relocation is needed). x86_64 with Limine uses a static higher-half mapping with
PC-relative addressing that the linker resolves — no runtime relocation.

### VirtIO note for Phase 06

The existing VirtIO drivers (block, net, keyboard, GPU) use VirtIO-MMIO transport
(RISC-V `virt` machine style). On x86_64 q35, VirtIO devices use PCIe transport
(`virtio-blk-pci`). For Phase 06, **skip VirtIO** entirely:
- Boot with just `-cdrom` (no disk)
- Verify serial output up to "Kernel initialization complete"
- The init cell will fail to load cells from disk — acceptable for bring-up

Full VirtIO PCIe support (PCIe config space enumeration, BAR mapping) is a separate
Phase that depends on PCIe ECAM driver — deferred to G2 roadmap.

---

## Related Code Files

| Action | File |
|--------|------|
| Create | `run-x86.ps1` |
| Create | `.cargo/config-x86.toml` (or add to `.cargo/config.toml`) |
| Create | `limine.cfg` (committed to repo under `scripts/x86/` or `build/`) |
| Verify | xorriso and limine.exe available on dev machine |

---

## Implementation Steps

1. Download `limine.exe` + `limine-bios-cd.bin` + `limine-bios.sys` from Limine v8.x releases
   into `limine/` directory (add `limine/` to `.gitignore`)

2. Add `[target.x86_64-unknown-none]` to `.cargo/config.toml` with red-zone disable

3. Create `scripts/x86/limine.cfg` with TIMEOUT=0, Limine protocol, kernel path

4. Create `run-x86.ps1` (as outlined above, with error checks at each step)

5. Run `run-x86.ps1`; iterate until serial shows `[ViCell] kernel boot v...`

6. Verify the boot sequence reaches at minimum:
   - `[ViCell] kernel boot v...` banner
   - `Frame allocator initialized`
   - `Paging initialized`
   - `HAL initialized`
   - `Scheduler initialized`
   - (init cell will fail — expected, disk is not present)

---

## Success Criteria

- `run-x86.ps1` runs without script errors
- QEMU boots to at least `[ViCell] kernel boot v...` on COM1
- No triple fault (QEMU reset loop) — indicates paging or boot entry is working
- `Paging activated` line appears (confirms HPET + x86_64 paging path works)
- Document exact serial output in phase report

---

## Risk Assessment

- **HIGH** — this is the integration phase; all prior phase bugs surface here
  - QEMU `-cpu qemu64` LAPIC frequency: measured by HPET calibration, should give ~1000–3000 ticks/ms
  - `__stack_top` alignment: confirm 16-byte alignment at boot (Phase 02)
  - Higher-half addressing: Limine maps kernel at `0xFFFFFFFF80000000`; the linker script
    starts `.text` there; all `static` data references must use `rip`-relative addressing
    (default for `x86_64-unknown-none` with `code-model=kernel`)
- **MED** — xorriso availability on Windows. If not available via winget, note manual ISO
  creation steps using WSL or the Limine Windows binary's built-in ISO mode
- **MED** — `.cargo/config.toml` red-zone disable: if this flag is missing, interrupt handlers
  will corrupt cell stacks. Verify by checking interrupt behavior under any x86 test

---

## Security Considerations

- QEMU `-cpu qemu64` does not expose SMEP/SMAP — consistent with Phase 05 decision to defer these
- `-no-reboot` prevents silent triple-fault restart loops from masking crashes
- Limine BIOS ISO: do NOT commit the ISO (binary) — only the `limine.cfg` + build script
