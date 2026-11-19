---
phase: 5
title: "Guest networking beside the host stack"
status: completed
priority: P1
effort: "3-4d"
dependencies: [4]
tier: thinking
checkpoint: true
---

# Phase 5: Guest networking beside the host stack

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

Let the guest reach the network while the CellOS host stack (Ocel, DHCP) keeps working.
Today `service-net/hypervisor-bridge` compiles the host's DHCP/ARP/link-local out, so the
bridge and the host stack cannot coexist.

## Requirements

- Functional: one net-service build serves both: host stack on the NIC MAC, guest L2 frames
  bridged by destination MAC (guest MAC `52:54:00:AA:BB:CC`, unique per session) plus
  broadcast to both.
- Functional: the guest gets its own DHCP lease from the upstream network (QEMU user-mode
  NIC gives one per MAC) and resolves DNS.
- Functional: Chromium in the window loads a real HTTPS page.
- Non-functional: host-only traffic never reaches the guest except broadcasts; the guest
  cannot send with the host's MAC (source-MAC check).

## Architecture

`cells/services/net`: replace the compile-time `hypervisor-bridge` switch with a runtime
bridge port registered by the hypervisor Cell (existing `L2Send`/`L2Recv` path). Frames:
dst == guest MAC -> guest queue; dst == host MAC -> host stack; broadcast/multicast -> both.
QEMU in lanes and the play VM: `-nic user,model=virtio-net-pci` (or e1000) instead of `-nic none`.

## Assumptions

- **Claim:** the net service's MAC learning (`interface.rs:440-461`, `handlers.rs:240-264`)
  can run with the host stack enabled. **Confidence:** medium. **Verify:** read + host test.
- **Claim:** the host NIC driver accepts frames with a foreign source MAC (no filtering).
  **Confidence:** medium for virtio-net/e1000 in QEMU. **Verify:** guest DHCP in the lane.
- **Claim:** QEMU slirp hands distinct leases to two MACs on one NIC. **Confidence:** high.

## Related Files

- Modify: `cells/services/net/src/main.rs`, `interface.rs`, `handlers.rs`, `Cargo.toml`
- Modify: `cells/services/hypervisor/src/net_backend.rs`, `virtio_net.rs` (per-session MAC)
- Modify: `scripts/make-hypervisor-fs-x86.sh` (drop the feature where no longer needed),
  lanes that used `hypervisor-bridge`

## Implementation Steps

1. Host tests: frame demux (unicast host, unicast guest, broadcast, spoofed source MAC dropped).
2. Runtime bridge port; remove the compile-time feature and migrate its lanes.
3. Lane step: guest `udhcpc` lease + DNS marker; Chromium opens a fixed HTTPS URL and the
   window shows it (pixel check on a known region, or the page title in the tab strip).
4. Rerun `qemu-x86-python-gate.sh` and `qemu-x86-virtio-e2e.sh` (bridge users) and Ocel's
   HTTP fetch on the desktop.
5. Changelog entry.

## Success Criteria

- [x] Net host tests for the demux pass (`bridge-ports-tests.rs`: host unicast, guest unicast,
      broadcast/multicast, spoofed source refused, taken address, learning, release; service-net 105).
- [x] Lane PASS: `qemu-tier3-display-x86.sh` 41/41 (guest lease 10.0.2.16 vs host 10.0.2.15, DNS,
      HTTP); `qemu-tier3-chrome-x86.sh` 8/8, Chromium renders https://example.com/ typed into its
      omnibox (`evidence/phase05-chromium-page.png`), UI ~132 s after the taskbar click.
- [x] Ocel fetches on the same boot while the guest runs (plain HTTP, see deviation);
      python gate and virtio-e2e PASS on QEMU 11.1.3 TCG (unqualified: lanes pin 10.2.0, which is
      not installed here; Windows 10.2.0 reports a suffixed version string).

## Security Considerations

Bridge port authority: only the hypervisor Cell may register one (service capability
check). Source-MAC anti-spoofing on guest TX. No guest access to the host stack's sockets.

## Risk Notes

Promiscuous/multicast handling in the NIC drivers may need enabling for the guest MAC.

## Deviation Log

- Decision (ports): `cells/services/net/src/bridge-ports.rs` keeps one port per L2 client Cell
  owner generation (max 8, 64 queued frames each, tail drop), released with the owner's
  death watch. Unicast to a port's MAC -> that port only; broadcast/multicast -> host and every
  port; anything else -> host. A port MAC may never be the host's or another port's, checked
  at open (`L2Recv`) and on every guest frame (`L2Send`); refusal is the new
  `NET_ERR_ADDR_IN_USE` (0xFB) and the frame is not sent. The existing `L2` right (catalog:
  hypervisor only) stays the authority check. `hypervisor-bridge` feature removed everywhere
  (x86 `HV_INIT_MIN` had no other effect and was removed with it).
- Decision (per-session MAC): the hypervisor opens its port before the guest reads the virtio
  config, trying `52:54:00:AA:BB:CC` + n (n < 16) until the net service accepts one, so a new
  session reuses an ended session's address and upstream lease (QEMU slirp has 16 leases).
- Deviation: empty `L2Recv` now answers `Ok` (API doc) instead of `Err(0)`, which the
  hypervisor treated as a failure and re-looked-up the service on every idle poll.
- Surprise (CONFIRMED by reading): the guest RTC returned a zero clock (year 2000), so
  Chromium could not validate any certificate. `cmos_rtc.rs` now serves the host's UTC wall
  time (BCD, century register 0x32).
- Deviation: Ocel's check uses plain HTTP: desktop `service-net` builds with the private-CA
  TLS flavor (`tls-ca-private`), so public HTTPS from the host stack fails independently of
  this phase.
- Deviation: `scripts/qemu-tier3-chrome-x86.sh` (+ `tier3_chrome_check.py`) is created here for
  the Chromium page check; phase 6 extends it with resize/Close and registers it.
- Surprise: the working tree carries another session's unfinished multi-sector block edits
  (`service-vfs` does not compile); lanes run from a detached worktree of HEAD plus this phase.
- Surprise (CONFIRMED, fixed in the HAL): the guest got ~1.5 PIT ticks/s (IRQ0=93 at 62 s
  uptime, `sleep 1` = 5.5 s, soft lockups), so DHCP stalled after `select`. VMX exits on
  `sti; hlt` kept "blocking by STI" in the VMCS interruptibility field after RIP was advanced,
  and `inject_ext_irq_gated` refused every IRQ injected at a HLT exit. `complete_instruction`
  (`vmx_vcpu.rs`, also used by the MMIO path) clears bits 0-1 when the VMM completes an
  instruction, as KVM does. After: ~95 ticks/s (HZ 100), `sleep 1` = 1.01 s, guest userspace at
  4.8 s instead of 62 s, lease 0.4 s after link-up. Reviewed (haily-reviewer: ship). The SVM
  backend clears its shadow on HLT only (reviewer LOW, untouched: SVM is reference-only).
- Surprise: in the Chrome window the guest's hypervisor reads the CellOS serial input (`sys_read(0)`
  in `drain_host_input`), so the serial shell cannot start Ocel; the lane opens Ocel from the
  taskbar and types the URL.
- Blocked (not this phase): `qemu-tier3-hostile-runner-x86.sh`, the other former
  `hypervisor-bridge` user, does not build at HEAD: `supervisor` with `hostile-backend-recovery`
  calls a missing `sender_has_exact_name` (`hostile_backend_recovery.rs:17-18`).
