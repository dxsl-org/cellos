# Research: Cellos SoC and board layering

**Mode:** arch · **Depth:** standard · **Date:** 2026-08-17

## Verdict

Cellos should add explicit `hal/soc/` and `boards/` layers, but generic controller drivers
must live in a separate shared `drivers/` layer. Board packages should be declarative build
targets; they must not own controller implementations or introduce `board-*` branches into
architecture and driver code.

## What other systems converge on

### Zephyr: the closest structural model

Zephyr hardware model v2 defines a hierarchy of board → SoC → SoC series/family → CPU
cluster/core → architecture. A board inherits the supported SoC and its features, while board
targets can encode revision, SoC, CPU cluster, and build variant. Its SoC package contains
metadata, build integration, SoC selection, defaults, and available peripheral support.

Zephyr also makes a useful hard distinction:

- Devicetree describes hardware and boot-time configuration.
- Kconfig selects software compiled into the image.

This maps directly to Cellos: DTS/DTB should carry register ranges, interrupts, clocks,
resets, DMA relationships, pinctrl and PHY topology; a board build manifest should select the
closed driver set and image profile.

Sources: [Zephyr board porting guide](https://docs.zephyrproject.org/latest/hardware/porting/board_porting.html),
[Zephyr SoC porting guide](https://docs.zephyrproject.org/latest/hardware/porting/soc_porting.html),
[Devicetree versus Kconfig](https://docs.zephyrproject.org/latest/build/dts/dt-vs-kconfig.html).

### Linux: data-driven binding, shared drivers, rare fixups

Linux uses DT for platform identity, runtime configuration, and device population. The root
`compatible` list starts with the exact board and falls back toward the SoC family. Drivers
bind to device compatibles rather than board names. Linux organizes reusable hardware as an
SoC `.dtsi`, optional SoM/common `.dtsi`, and a board `.dts` containing integration details.

The important boundary is that DT describes hardware; it is not a place to encode driver
policy. Bindings are treated as an ABI, require documented compatible strings, and reuse
common properties. When two controller implementations differ in programming model or errata,
they get a more specific compatible or a driver quirk selected by compatible, not a board
fork.

Sources: [Linux and the Devicetree](https://docs.kernel.org/6.1/devicetree/usage-model.html),
[Linux SoC subsystem](https://docs.kernel.org/6.6/process/maintainer-soc.html),
[DTS coding style](https://docs.kernel.org/6.8/devicetree/bindings/dts-coding-style.html),
[binding guidance](https://docs.kernel.org/next/devicetree/bindings/writing-bindings.html).

### U-Boot: DT parameters plus build-time driver availability

U-Boot's driver model mirrors the devicetree: a generic driver can operate on any board with
the supported controller, while Kconfig decides whether that driver exists in the image and
DT supplies its addresses, clocks and wiring. This is also the clearest warning against
overpromising: DT can only select drivers that were compiled in, and early boot may still
need narrowly scoped platform hooks or DT fixups.

Sources: [U-Boot driver model design](https://docs.u-boot.org/en/v2025.04/develop/driver-model/design.html),
[U-Boot devicetree control](https://docs.u-boot.org/en/stable/develop/devicetree/control.html),
[pre-relocation DT fixups](https://docs.u-boot.org/en/stable/develop/driver-model/fdt-fixup.html).

### seL4/Microkit: reproducible board products, but more static

Microkit exposes a named board as an SDK product with a fixed kernel configuration, loader,
image format, load address, CPU IDs and boot instructions. This is a strong precedent for
making every Cellos board independently rebuildable. It also shows the cost of static board
facts: Raspberry Pi 4 RAM sizes become distinct board targets because memory must be known at
build time. Cellos should copy the reproducible packaging, while preferring validated runtime
DTB discovery so RAM variants do not multiply unnecessarily.

Source: [Microkit user manual](https://docs.sel4.systems/projects/microkit/manual/latest/).

## Current Cellos pressure points

This direction is already the documented intent, not a competing redesign:

- `docs/TODO.md:5-25` names the target `hal/arch`, `hal/soc`, and `boards` tree and explicitly
  forbids per-board copies of UART, SDHCI, DesignWare I²C/SPI, GIC/PLIC, and PCIe drivers.
- `docs/specs/04-hardware.md:29-32,63` makes DTB the MMIO registry source and requires HAL
  traits to remain board-neutral with zero kernel changes for a new board implementation.
- `docs/specs/13-peripherals.md:128-133,160-176` shows shared generic bit-bang I²C/SPI today
  and defers hardware controller support plus DTB discovery to the real-board phase.

The proposed separation addresses current, concrete coupling rather than a hypothetical future
problem:

- `hal/core/Cargo.toml:40-41` propagates `board-rpi3` into the ARM architecture crate.
- `hal/arch/arm/src/aarch64.rs:9-14,42-45,94-102` places BCM283x interrupt, timer and UART
  selection inside the AArch64 facade.
- `kernel/Cargo.toml:83-100` mixes emulator memory variants, real boards, fallback maps,
  console restrictions and storage selection in flat Cargo features.
- `kernel/src/platform.rs:14-32,118-175,221-282` parses DT only on RV64, hardcodes ARM board
  layouts, and embeds a Pioneer board override in generic platform discovery.
- `kernel/src/task/drivers/mmc.rs:16-33,108-124` chooses SDHCI base and pinmux by board feature.
- `kernel/src/task/drivers/mmc/sdhci.rs:18-21,49-156` embeds BCM2835 register access and timing
  quirks directly behind `board-rpi3` conditionals in the generic SDHCI controller.
- `kernel/src/boot.rs:280-307,336-359` stores VisionFive2 and RPi3 fallback maps in generic
  kernel boot code rather than exact board packages.

The current direction is mixed: DT-driven discovery already exists, but only on part of the
platform surface. Board conditionals still cross architecture, SoC integration, generic driver,
and kernel orchestration boundaries.

## Recommended dependency model

```text
boards/<vendor>/<board>  -> hal/soc/<vendor>/<soc> -> hal/arch/<isa>
          |                         |
          +-----------> drivers/ <-+
                              |
                         hal/traits/
```

Allowed dependencies:

- `hal/arch` knows only ISA/ABI mechanisms: entry, trap frame, context switch, page-table
  operations, cache/TLB instructions, architectural timer/counter and CPU feature probes.
- `hal/soc` owns on-chip integration: interrupt topology, clock/reset/power controllers,
  SoC-local buses, boot CPU release, SoC errata and controller glue.
- `drivers` owns reusable controller implementations: PL011/NS16550/BCM mini UART, SDHCI,
  DesignWare I²C/SPI, GIC/PLIC versions, PCIe ECAM/DW core and reusable PHY classes.
- `boards` supplies identity, firmware contract, topology and build selection. It may invoke a
  tiny typed early-boot hook only when the behavior cannot be expressed as data.
- The kernel consumes a board-neutral `PlatformDescriptor` or generated device registry. It
  must not test `cfg(feature = "board-...")`.

Forbidden dependencies:

- `hal/arch` importing a board or SoC module.
- A generic driver testing a board identity.
- Board packages copying driver source.
- A SoC module embedding carrier-board pin assignments or RAM size.
- Generic kernel orchestration switching on a board feature.

## Exact ownership rules

| Fact or behavior | Owner |
|---|---|
| ISA entry ABI, exception level transition, TLB/cache primitives | `hal/arch` |
| SoC interrupt/clock/reset/power topology and silicon errata | `hal/soc` |
| Controller programming model and compatible-specific quirks | `drivers` |
| Board/SOM identity and compatible strings | board manifest + DTS |
| External PHY, connector, pinmux state, regulators and wiring | board DTS |
| Boot firmware, entry register contract, image/load format | board manifest |
| DRAM and reserved-memory discovered at boot | runtime DTB |
| Exact-board fallback RAM/MMIO map | board fallback DTS/descriptor |
| Software compiled into a board image | board configuration fragment |
| Fully resolved, reproducible configuration | generated lock artifact |

Pinmux needs one nuance: the board owns the selected pin groups and their wiring, but the code
that programs a pin controller belongs to the pinctrl driver or SoC layer. Likewise, a board
selects and connects a PHY; the PHY protocol implementation stays shared.

## Proposed board package contract

```text
boards/<vendor>/<board>/
├── board.toml                 # schema version, identity, SoC, revisions, profiles
├── <board>.dts                # board/SOM wiring; includes SoC .dtsi
├── fallback.dts               # optional exact-target fallback, never generic guessing
├── defconfig.toml             # minimal software selection fragment
├── boot.toml                  # firmware ABI, privilege, load address, image recipe
├── firmware.lock              # optional firmware URLs/versions/hashes, no binary copying
└── tests/
    ├── manifest-validation.*
    └── dt-binding-validation.*
```

`board.toml` should name `board@revision/soc/cluster/variant`, following the useful part of
Zephyr's target model. The build resolves:

```text
architecture defaults + SoC defaults + board fragment + profile
    -> resolved-board-config.json + driver-set.lock + DTB + image recipe
```

The resolved files should be deterministic CI artifacts and hash inputs to the final image.
Do not hand-maintain a complete copied config for every board: keep composable sources and
materialize the complete resolved config. That preserves independent rebuildability without
creating configuration drift.

## Driver binding strategy for Cellos

1. Treat compatible strings and their schemas as stable hardware contracts.
2. Parse the runtime DTB when the firmware contract guarantees one.
3. Validate the root compatible against the selected board target before accepting an exact
   fallback map.
4. Generate a static, no-allocator device registry and driver match table at build time from
   the board DT and compiled driver set.
5. At boot, bind each DT device to the most specific compatible supported by the table.
6. Represent controller differences as typed quirk/capability data selected by compatible.
7. Fail closed for a required boot device with no matching driver; log and skip optional
   devices.

This avoids both a heavyweight dynamic driver framework and board-specific driver forks.

Examples:

- SDHCI: `SdhciController<Q: SdhciQuirks>` or a compact quirk descriptor owns 32-bit-only
  accesses, transfer-mode shadowing and write spacing. BCM2835 selects that implementation by
  controller compatible, not `board-rpi3`.
- UART: one PL011 driver and one 16550/DW-APB driver; SoC/DT supplies clock, register shift,
  width and access type. BCM mini UART remains its own reusable controller driver.
- GIC/PLIC: architecture supplies IRQ masking instructions; controller drivers implement GIC
  or PLIC; SoC describes topology and errata.
- PCIe: share ECAM enumeration and DesignWare core; SoC glue owns outbound windows, resets,
  clocks and PHY sequencing; board DTS wires the PHY, slots and regulators.

## Exceptions and anti-overengineering guard

The claim "a board contains no executable code" is too strong. Linux, Zephyr and U-Boot all
retain early hooks or fixups for hardware that cannot be described reliably as static data.
The enforceable rule should be:

> Board executable code is an audited exception for pre-driver boot sequencing or genuine
> board errata; it cannot implement a reusable controller and cannot be imported by generic
> architecture or driver code.

This claim is **CONTESTED** only in its absolute form. The main recommendation—data-first board
support with shared drivers—is **VERIFIED** across Zephyr, Linux and U-Boot.

Avoid initially building a universal multi-board kernel. A per-board closed driver set gives
smaller images and simpler validation. The same architecture can later support a family image
by compiling a superset driver registry and selecting from the runtime DTB.

## Suggested migration order

1. Define `BoardManifest`, `BootContract`, `PlatformDescriptor`, compatible schema rules and
   the dependency-direction test before moving files.
2. Create a first board package for QEMU RV64 using the existing DT-driven path; preserve its
   current boot behavior as the reference.
3. Move ARM QEMU virt and RPi3 hardcoded platform data into board DTS/manifests; make DT parsing
   architecture-neutral.
4. Move BCM283x IRQ/timer/UART integration out of `hal/arch/arm` into the BCM2837 SoC layer and
   shared controller drivers.
5. Refactor SDHCI board branches into compatible-selected host quirks plus board-owned pinctrl.
6. Replace flat board Cargo features with generated board-target configuration and a static
   driver registry.
7. Add CI matrix entries that rebuild every board target, validate bindings/config closure,
   reject board imports from `hal/arch` and `drivers`, and boot the emulated targets.

## Acceptance gates

- Adding a board using an already-supported SoC changes only `boards/` and documentation.
- Adding a board never copies UART, SDHCI, I²C/SPI, interrupt-controller or PCIe code.
- Every enabled DT compatible has exactly one compiled matching driver or an explicit optional
  waiver.
- No `cfg(feature = "board-...")` appears under `hal/arch`, `drivers`, or generic kernel code.
- Runtime DTB identity and fallback identity mismatch fails before MMIO use.
- Rebuilding the same board target from the locked toolchain/config yields identical resolved
  config, DTB and image inputs.
- QEMU RV64, QEMU AArch64 and RPi3 retain their existing boot/peripheral evidence during the
  migration.

## Unresolved questions

- Whether Cellos wants DT binding compatibility with upstream Linux/Zephyr schemas or a
  deliberately smaller compatible subset. Reusing upstream bindings is preferable where the
  hardware matches.
- Which layer owns firmware-mediated services such as PSCI, SBI DBCN and Raspberry Pi mailbox:
  recommended split is a shared firmware driver with a SoC-selected transport.
- Whether the final board manifest belongs at repository root `boards/` or under `hal/boards/`.
  Root `boards/` communicates that it is a product/build package rather than HAL code.
- Which existing real-board targets have current runtime evidence beyond RPi3; this affects the
  migration validation matrix, not the architecture.
