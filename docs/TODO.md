# TODO - Quick references for dev

## Tasks

### G1 Active
- Hypha AI agent P3 boot verify → P4 tool-peripheral (robot NL control = G1 showcase)

### G2 / Deferred
- TLS server-side accept `[G2-parked]` — plan at `.agents/260623-1500-tls-server-accept/`; GATE: only for edge nodes without LB/VM
- PKU PTE key tagging `[G2]` — PTE-level key assignment; ARM64 MTE/x86 MPK base done
- DICE/RIoT attestation `[G3/hardware-gated]` — OpenTitan-backed Silo; needs real hardware
- Compositor full desktop + mouse `[G2]` — mouse path done; full desktop shell (Terminal Cell VT100) is G2
- App Platform J: L2/L3/L4 `[G2]` — Middleware, tooling, observability (see roadmap §J)
- Cell-to-Cell Anywhere G2 `[G2]` — HyParView gossip, Pkarr DNS discovery, K2 per-node key; G1 P00-P09 complete
- VirtIO-GPU PCI transport for x86 `[G2]` — P03 of GPU backend plan; requires PciRoot/ECAM adapter

## Bugs

- ~~**GPU flush spam** `[gpu_flush] GPU Driver Cell not registered` — warn-once fix in kernel/src/task/syscall.rs (AtomicBool swap)~~ ✅ fixed
- ~~**Platform double-spawn** `PlatformCap already granted` — init was re-spawning /bin/platform; kernel already spawns it before init; removed from init~~ ✅ fixed

