# QEMU q35 x86_64

This board contract covers the `qemu-system-x86_64 -machine q35` target booted
by Limine via BIOS or UEFI. Firmware supplies the memory map and ACPI tables;
Cellos does not invent fallback LAPIC, IOAPIC, HPET, or PCIe ECAM addresses.

Static q35 platform facts such as COM1 wiring and the bounded legacy firmware
windows live in `hal/soc/x86`. CPU, paging, APIC, and port-I/O mechanisms remain
in `hal/arch/x86`; PCIe, NVMe, and e1000 drivers remain shared.

## Production image

Prerequisites are Rust nightly, `qemu-system-x86_64`, GNU `objdump`, `xorriso`
(or `XORRISO=/path/to/a-compatible-tool`), and the checked-in Limine boot
files.

```sh
cargo build -p cellos-kernel --release --target x86_64-unknown-none
bash scripts/x86/make-iso-ci.sh
BOOT_WINDOW=90 bash scripts/qemu-x86_64-test.sh
```

## Per-vector IDT actual-entry lane

The dedicated lane enables `x86-idt-cpl3-test`, which depends on the generic
`test-hooks` infrastructure but alone selects the terminal CPL0/CPL3 fixture
and HAL `qemu-exit` dependency. It builds in isolated `target/x86-idt-test/`
and `build/x86-idt-test/` paths, leaving generic test and production outputs
separate:

```sh
bash scripts/build-x86_64-idt-test-ci.sh
objdump -d --disassemble=x86_64_idt_common \
  target/x86-idt-test/x86_64-unknown-none/release/cellos-kernel
BOOT_WINDOW=90 bash scripts/qemu-x86_64-idt-test.sh
```

The runner forces QEMU `+pku`; missing CPUID.PKU or CR4.PKE activation is a
failure, not a skip. It passes only on `isa-debug-exit` status 33 with exactly
one
`X86-IDT-SELFTEST: PASS bp=3 gp=13/ec=fffc gprs=15 df=ok align=ok timer=32`
marker, exactly one
`X86-IDT-CPL3: PASS fresh=ok int80=ok timer=32 switch=syscall-resume gs=kernel/user pkru=0/55555550/55555544`
marker, two pre-scheduler timer wakeups, one scheduler initialization, and no
FAIL, PANIC, FAULT, SKIP, reset, or triple-fault output.
The Ring-3 phase covers fresh IRET, INT80, timer preemption, suspended-SYSCALL
resume, GS/KERNEL_GS_BASE balance, and per-task PKRU restoration.

Generic `test-hooks` keeps its existing non-terminal harnesses and does not
compile or invoke this fixture. The production ELF/ISO also excludes its
symbols, module namespaces, markers, dispatch shim, and terminal task hook.

QEMU is an integration witness only. Physical PC support remains hardware-gated.
