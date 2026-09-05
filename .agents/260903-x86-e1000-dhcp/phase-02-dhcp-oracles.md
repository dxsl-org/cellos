# Phase 02 — q35 DHCP Oracles

Status: completed

## Change

- Extend `nic-x86` ordinary q35 coverage through e1000 registration, first accepted bridge Tx, first bridge Rx, and DHCP acquisition.
- Extend the VT-d case with the same data-plane evidence after the isolation marker.
- Add a shared deadline-bounded serial accept for the two x86 NIC runners. It
  must poll `child.try_wait()` and always kill/wait the child on timeout or early
  exit before a `QemuRunner` exists.
- Assert marker ordering and reject kernel panic or Cell fault.
- Reuse existing q35/SLIRP `restrict=on` topology and bounded marker waits.

## Acceptance

- Ordinary q35 proves e1000 Tx/Rx and DHCP ACK without VirtIO fallback.
- VT-d q35 proves `ACTIVE < accepted e1000 Tx < e1000 Rx < DHCP acquired`.
- Tests use the existing standalone integration manifest and strict prerequisite guard.
- QEMU exit or failure to connect serial cannot hang the test or leak a child.

## Evidence

- Strict ordinary q35 passed registration → accepted e1000 Tx → e1000 Rx →
  DHCP/IP-address ordering.
- Strict VT-d q35 passed `ACTIVE` before the same e1000 data plane, while NVMe
  independently completed translated DMA.
- Serial connection setup is deadline-bounded and reaps QEMU on every failure.
- The final fault scan follows the reachable ASCII IP marker and a 500 ms
  post-success observation window.
