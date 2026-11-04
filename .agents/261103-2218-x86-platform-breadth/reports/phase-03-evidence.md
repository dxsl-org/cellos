# Phase 03 evidence — x2APIC and multi-IOAPIC (2026-11-04)

Base HEAD `64ad34027`; working tree uncommitted. Evidence ceiling: host + QEMU 8.2.2 TCG + QEMU/KVM (local).

## Boot lanes

| Lane | Result | Kernel lines |
|---|---|---|
| TCG `qemu64` | PASS | `ioapics registered=1 declared=1`; `timer: clock=hpet … apic=xapic`; UART RX IRQ initialised |
| KVM `-cpu host` | PASS | `timer: clock=kvmclock clockevent=TscDeadline … apic=x2apic`; UART RX IRQ initialised |
| KVM `-cpu host`, `hpet=off` | PASS | same, `apic=x2apic` |
| TCG `-smp 2` (manual QEMU) | `Cellos >` | `apic=xapic`; `[x86-smp] transport IPI-invalidate bidirectional=pass`; `topology online=2` |
| KVM `-smp 2` (manual QEMU) | `Cellos >` | `apic=x2apic`; IPI bidirectional pass over the x2APIC ICR MSR; `topology online=2` |
| `scripts/x86/qemu-domain-test.sh`, TCG, `X86_CPUS=2` | PASS | every marker present |
| `scripts/x86/qemu-domain-test.sh`, KVM `-cpu host` | FAIL, **pre-existing** | Stalls after `PKU-SELFTEST`, at the VMM smoke step. A clean worktree of HEAD `64ad34027` stalls at the same line (`FAIL: expected exactly 1 contained cell fault, saw 0`). This tree's log shows `apic=x2apic` and `topology online=2` before the stall. |

## Integration suites (`CELLOS_X86_ACCEL` tcg / kvm)

| Suite | tcg | kvm |
|---|---|---|
| `serial-x86` (RX-IRQ input) | 1/1 | 1/1 |
| `local-service-lifecycle-x86` (incl. `-smp 2`) | 3/3 | 3/3 |
| `iommu-dmar-x86` | 3/3 | 3/3 (own constructors; TCG) |
| `x86_64-boot` | 7/9 | 7/9 |

The two `x86_64-boot` failures are the prod-refusal witness image's Cellbox authentication drift, described in the phase 02 evidence.

## Host

- `cargo test -p hal-x86 --target x86_64-unknown-linux-gnu`: 23 passed. New tests:
  - ICR encoding for xAPIC and x2APIC;
  - GSI → (unit, pin) lookup across two IOAPICs;
  - MPS INTI flags → RTE polarity and trigger.
- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`: 408 passed.
- `cargo fmt -p cellos-kernel -p hal-x86 -- --check`: clean. Kernel clippy (riscv64, `-D warnings`): clean.
