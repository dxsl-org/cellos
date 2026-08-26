# Phase 08 — virtio-net → Net Cell (M4, apk works)

## Context Links
- Plan: [plan.md](plan.md) · Prev: [phase-06](phase-06-virtio-mmio-x86.md)
- Sibling ARM: `.agents/260613-2134-tier3b-vmm-arm64-el2/phase-08-virtio-net.md`
- Verified: `cells/services/hypervisor/src/virtio_net.rs:1-...` + `net_backend.rs:1-...` (arch-generic —
  reused), `run_loop.rs:84` (net slot 2 dispatch), `:124-135` (WFI/Preempted RX-poll — x86 analog is Hlt/
  Preempted), `run_loop.rs:27` (`sys_lookup_service(service::NET)`).

## Overview
- **Priority:** P1 · **Status:** pending · **Depends on:** 06
- Attach a **virtio-net** device (slot 2) bridged to the ViCell **Net Cell** for L2 frame forwarding, so
  the guest gets DHCP + internet and `apk add` works. Reuses arch-generic `virtio_net.rs` +
  `net_backend.rs` — x86-new work is slot wiring, IRQ, and mapping the ARM `Wfi`/`Preempted` RX-poll to
  the x86 `Hlt`/`Preempted` arms. Success = **M4**: `apk add <pkg>` completes in the guest.

## Key Insights
- **Fully reused device + backend:** `virtio_net.rs` + `net_backend.rs` are arch-generic;
  `run_loop.rs:84` dispatches net on slot 2. The x86 run loop routes the virtio-window MMIO to the same
  code. `net_backend::try_receive(net_tid)` + `push_rx_frame` are reused verbatim.
- **RX-poll point mapping:** ARM polls RX on `Wfi` (`run_loop.rs:124`) and `Preempted` (`:131`). x86 has
  no WFI — poll RX on **`Hlt`** and **`Preempted`** instead. Same cadence: inject timer IRQ0 (P05) +
  push any pending RX frame on each idle/budget exit.
- **Net Cell bridge (existing):** resolve `sys_lookup_service(service::NET)` (`run_loop.rs:27`); forward
  guest TX frames to the Net Cell and inject RX frames into the guest RX virtqueue — the same SLIRP/
  DHCP path the ARM track used (`10.0.2.15` via user-mode networking). Owned buffers (Law 2).
- **cmdline:** add `virtio_mmio.device=0x1000@0xd0002000:7` (slot 2, IRQ7). Guest configures the NIC via
  standard `ip`/udhcpc; `apk` uses the default Alpine mirror over the Net Cell bridge.

## Requirements
**Functional**
- virtio-net slot 2 wired into the x86 virtio-mmio window with an 8259 IRQ.
- TX frames → Net Cell; RX frames from Net Cell → guest RX virtqueue (poll on Hlt/Preempted).
- Guest gets DHCP lease; `apk add` fetches + installs a package.

**Non-functional**
- Law 2 owned buffers; Law 4 cell `#![forbid(unsafe_code)]`.

## Architecture
```
guest TX → virtio-net slot 2 QueueNotify → run_loop MmioWrite @0xd0002000 → virtio_net.process_tx
  → net_backend → Net Cell (L2 frame, Box<[u8]>)
Net Cell RX → net_backend::try_receive(net_tid) [polled on Hlt/Preempted]
  → virtio_net.push_rx_frame → pic.deliver(IRQ7) → sys_inject_irq
```

## Related Code Files
**Modify**
- `cells/services/hypervisor/src/run_loop.rs` — net slot 2 routed in x86 arms; RX poll on `Hlt`/`Preempted` (mirror ARM `:124-135`); IRQ7 delivery
- `cells/services/hypervisor/src/boot_info.rs` — net `virtio_mmio.device=` cmdline entry
**Reuse unchanged**
- `virtio_net.rs`, `net_backend.rs`, `virtqueue.rs`
**Verify**
- P02 virtio window slot-2 sub-range unmapped; Net Cell reachable via `service::NET`

## Implementation Steps
1. Wire slot 2 (net) in the x86 run-loop virtio dispatch + assign IRQ7.
2. Add RX-poll calls on the x86 `Hlt` and `Preempted` arms (mirror ARM Wfi/Preempted).
3. Add net cmdline entry in `boot_info`.
4. Confirm `net_backend` Net-Cell bridge is arch-neutral.
5. Boot → udhcpc gets `10.0.2.15` → `apk update` + `apk add <pkg>` completes.

## Todo List
- [ ] virtio-net slot 2 wired in x86 run loop + IRQ7
- [ ] RX poll on Hlt/Preempted arms (x86 analog of ARM Wfi/Preempted)
- [ ] net cmdline entry in boot_info
- [ ] net_backend Net-Cell bridge confirmed arch-neutral
- [ ] **Guest DHCP + `apk add` works (M4)**

## Success Criteria
- Guest obtains a DHCP lease (`10.0.2.15`); `ping`/`apk update` succeed; `apk add <small-pkg>` installs.
- RX frames are not dropped under a sustained download (poll cadence sufficient; Preempted keeps Net
  Cell live).
- No regression to blk (P07) or console (P06).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| RX frames dropped (poll too coarse) | Med×Med | Poll on both Hlt + Preempted; budget short enough (SCHED_TICK) |
| Net Cell starvation during download | Med×Med | Preempted yield (C-x2) |
| DHCP fails (frame format / checksum) | Med×High | Reuse proven ARM net_backend path; pcap via QEMU filter-dump |
| Flaky mirror in CI | Med×Med | Pin/cache a local mirror; mark net-matrix non-blocking |

## Security Considerations
- Guest L2 frames are mediated by the Net Cell — no raw host NIC access. virtqueue descriptor GPAs
  bounds-checked against the carve before forwarding.
- SLIRP user-mode networking isolates the guest from the host L2 (same posture as ARM track).

## Next Steps
- P10 CI matrix asserts DHCP + apk; P09 optionally upgrades interrupt delivery (APIC) for throughput.
