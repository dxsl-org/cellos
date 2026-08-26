# Phase 06 — VirtIO Net Driver Cell

## Context Links
- Plan: [plan.md](plan.md) · Prereqs: [phase-00](phase-00-prerequisites.md), [phase-05](phase-05-virtio-blk-cell.md) (pattern), [phase-01](phase-01-platform-cell-ecam.md) (x86_64 PCI)
- Source: `kernel/src/task/drivers/virtio_net.rs` (154) + `virtio_pci.rs` (shared PCI transport with blk)
- Reference Cell: `cells/drivers/e1000/` (NIC pattern: `sys_register_nic_driver=417`, OP_TX/OP_RX/OP_GETMAC)
- Kernel router: `kernel/src/task/drivers/nic.rs`
- Net consumer: `cells/services/net/src/interface.rs` (probes `service::NIC_DRIVER=10`, IPC `[op]++frame`)

## Overview
- **Priority:** P2 (after Phase 05 establishes the pattern).
- **Status:** complete (2026-06-24)
- **Risk:** MED — VirtIO net is the *fallback* NIC when e1000 absent; net service already routes to a registered NIC Cell.
- **Description:** Migrate VirtIO net to `cells/drivers/virtio-net/`. Serves the same NIC IPC as e1000 (`OP_TX`/`OP_RX`/`OP_GETMAC`). net service finds it via `service::NIC_DRIVER`.

## Key Insights (verified)
- e1000 Cell proved the NIC pattern: `sys_register_nic_driver()` (417) → `service::NIC_DRIVER=10`; net service (`interface.rs`) probes it and uses `[op=0]++frame` (tx), `[op=1]` (rx), `[op=2]` (getmac).
- VirtIO net is the QEMU default NIC; e1000 is the PCI NIC. Both can register `NIC_DRIVER` — net service uses whichever registered. (Only one NIC Cell at a time per the AtomicUsize single-slot; if both present, last-writer wins — confirm intended; likely only one NIC device exists per VM.)
- IRQ: RISC-V/ARM64 MMIO → `sys_wait_irq(net_irq)` (replaces kernel `virtio_net::ack_irq` + `waker::signal_net_rx`). x86_64 PCI → polled (like e1000). The net service's existing `WaitForEvent(NET_RX)` event must now be signalled by the **Cell** (the Cell receives the frame and can notify net service via IPC reply to its blocking rx request — net already does `nic_rx_from_cell`).
- Shares `virtio_pci.rs` transport with blk — that logic is ported into a shared cell-side helper (consider a small `libs/virtio-cell` crate OR duplicate the ~50 lines; YAGNI suggests duplicate unless blk+net+gpu all need it → then extract).

## Requirements
### Functional
1. `cells/drivers/virtio-net/`: claim VirtIO net MMIO/BAR, drive rx/tx queues, serve NIC IPC, `sys_register_nic_driver()`.
2. RISC-V/ARM64 `sys_wait_irq(net_irq)`; x86_64 poll.
3. `nic.rs` routes to the Cell when registered; kernel `virtio_net` fallback until Phase 08.

### Non-Functional
- `#![forbid(unsafe_code)]` except MMIO/DMA; Law 2 owned frame buffers.

## Architecture
```
net service           virtio-net Cell                 Hardware
-----------           ---------------                 --------
tx frame:
 sys_send([0]++frame) ► AppEvent::Message op=0 → dev.send(frame) → tx virtqueue
rx (blocking):
 sys_send([1]) ──────► block on rx; sys_wait_irq(net_irq) → drain rx queue
 ◄── reply [len][frame]   reply with frame (or empty on timeout)
getmac:
 sys_send([2]) ──────► reply 6 MAC bytes
```

## Related Code Files
**Create:** `cells/drivers/virtio-net/` (Cargo.toml, build.rs, src/main.rs, src/dispatch.rs (copy e1000 protocol), src/device.rs (queue), src/transport.rs (MMIO + PCI; share/duplicate from blk)).
**Modify:**
- `kernel/src/task/drivers/nic.rs` — route to NIC Cell when `NIC_DRIVER_CELL != 0`; kernel `virtio_net` fallback.
- `kernel/src/loader.rs` — `/bin/virtio-net` PcieDriverCap grant.
- `kernel/src/task/drivers/virtio_blk.rs:94` — remove `virtio_net::ack_irq` branch (Phase 08; gate inert on registration).
- `cells/tools/init/src/main.rs` — spawn virtio-net (optional) before/with net service.
- `gen_disk.ps1` + root Cargo.toml.

## Implementation Steps
1. Scaffold from e1000 + blk transport.
2. Port `virtio_net.rs` rx/tx queue handling into `src/device.rs` (ostd::dma).
3. `src/dispatch.rs`: copy e1000 OP_TX/OP_RX/OP_GETMAC protocol exactly (net service speaks it).
4. transport.rs: MMIO probe + x86_64 PCI (reuse/duplicate blk's).
5. Init: claim → init queues → `sys_register_nic_driver()`.
6. rx blocking via `sys_wait_irq(net_irq)` (RISC-V/ARM64) / poll (x86_64).
7. `nic.rs` route-to-Cell + fallback.
8. init spawn + gen_disk + member.
9. Boot net test: DHCP, ping, TLS handshake (http-smoke) through the Cell.

## Todo List
- [ ] Scaffold from e1000 + blk transport
- [ ] Port rx/tx queues → device.rs
- [ ] dispatch.rs (copy e1000 protocol)
- [ ] transport MMIO + PCI
- [ ] Init claim + register_nic_driver
- [ ] rx wait_irq / poll
- [ ] nic.rs route + fallback
- [ ] init spawn + gen_disk + member
- [ ] DHCP/ping/TLS boot test

## Success Criteria
- [ ] DHCP lease + ping + HTTPS (http-smoke) work through the VirtIO net Cell (kernel `virtio_net` static unused).
- [ ] `service::NIC_DRIVER` resolves to the Cell; net service uses IPC path.
- [ ] x86_64 PCI net path works (or e1000 Cell covers x86 and VirtIO-net covers RISC-V/ARM).
- [ ] Disabling the Cell → kernel fallback nets (rollback proof).

## Risk Assessment
| Risk | L | I | Mitigation |
|------|---|---|-----------|
| rx latency via IPC + wait_irq | Med | Med | Same model net already uses via `nic_rx_from_cell`; batch-drain on wake |
| `waker::signal_net_rx` removal breaks net service WaitForEvent | Med | High | Net service rx now driven by Cell reply, not kernel event; verify interface.rs path; keep WaitForEvent for transition |
| Two NIC Cells (e1000 + virtio) both register | Low | Med | Single AtomicUsize slot; one NIC per VM in practice; document last-writer-wins |
| Duplicated PCI transport drifts from blk | Low | Low | Extract `libs/virtio-cell` only if 3rd consumer (gpu) needs it |

## Security Considerations
- Net MMIO/DMA scoped to NIC BDF; frames are owned buffers, not borrowed.
- A compromised net Cell sees only network traffic (already untrusted data); cannot reach other Cells (LBI + IOMMU).

## Next Steps
- Phase 08 deletes `virtio_net.rs`, the `virtio_net::ack_irq` IRQ branch, and (if unused after blk+net migrate) `virtio_pci.rs` + `waker::signal_net_rx`.
