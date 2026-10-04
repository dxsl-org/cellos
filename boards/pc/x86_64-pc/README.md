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

Selected at build time:

```bash
cargo build -p cellos-kernel --release --target x86_64-unknown-none --features board-x86-pc
```

Driver families are added to `enabled_drivers` by the phase that ships them
(`.agents/261004-1957-x86-pc-lane/`): AHCI storage (02a/02b), xHCI USB (03),
Intel `igb` NIC (04a/04b), extra 16550 ports (06). A descriptor never lists a
driver whose cell does not exist — `has_driver` gates real kernel init.
