# Generic x86_64 PC / server board

Compatibility contract for the x86_64 PC class: the baseline a machine must
expose to be listed — standard legacy COM1 wiring, standard firmware windows,
and the Limine ACPI boot path.

**No physical machine is qualified by this file, and it does not claim that
every PC has that wiring.** Whether a *specific* machine boots Cellos, exposes
HPET, runs its SATA controller in AHCI mode, lets Secure Boot be disabled, and
which NIC family it carries is recorded per machine in
[`docs/hardware-compatibility-list.md`](../../../docs/hardware-compatibility-list.md);
a machine that fails a mandatory requirement there is rejected, not covered by
this descriptor.

Selected at boot. There is one x86_64 kernel image; it binds this profile when
the Limine command line carries no `cellos.profile=` option (ADR-0029). This is
the strict default: DMA-capable drivers run only behind a discovered VT-d
remapper.

```bash
cargo build -p cellos-kernel --release --target x86_64-unknown-none
X86_LIMINE_CONF=scripts/x86/limine-pc.conf bash scripts/x86/make-iso-ci.sh build/x86-pc-lane/vicell-x86-pc.iso
```

The default `scripts/x86/limine.conf` names `cellos.profile=qemu-q35-x86_64`
instead. The kernel accepts that weaker profile only when CPUID reports a
hypervisor.

Driver families are added to `enabled_drivers` by the phase that ships them
(`.agents/261004-1957-x86-pc-lane/`): AHCI storage (02a/02b), xHCI USB (03),
Intel `igb` NIC (04a/04b), extra 16550 ports (06). A descriptor never lists a
driver whose cell does not exist — `has_driver` gates real kernel init.

## Machine pin and re-confirmation (ADR-0029 §3)

Before any configured service starts, init compares the kernel's machine facts
(profile and its source, ACPI root OEM, SMBIOS type 1, DMAR/VT-d, hypervisor
bit, PCI function set) with `/srv/.cellos-machine/pin` on the provisioned
CellosFS `/srv` (P5). The first boot records the pin. A later difference prints
one `Init: machine pin: changed …` or `pci +/-…` line per change and enters
**safe mode**: only `input` and the `cellbox` console shell start; `net` and
every other configured service are withheld. Under this profile a missing or
unreadable store (no `/srv`, or no VT-d so storage DMA is refused) is safe mode
too.

To accept the change, read the diff, then at the console:

```text
Cellos > machine pin --confirm
machine pin: current=<64 hex>
machine pin: recorded <64 hex>; reboot to start services
```

and reboot. Init accepts the confirm only in safe mode, only from the shell it
started, and only for the digest it currently reports. `machine pin` alone
prints the state and both digests. The pin grants nothing and changes no
profile or DMA decision; it is change detection, not a boot-integrity control
(see HCL R3).
