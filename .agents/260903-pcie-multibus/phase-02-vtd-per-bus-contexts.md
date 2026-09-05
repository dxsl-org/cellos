# Phase 02 — VT-d Per-Bus Contexts

Status: completed

## Change

- Replace the shared VT-d context table with one lazily allocated table per bus.
- Publish each bus root entry only after its zeroed context table is initialized.
- Correct register-based context and IOTLB invalidation encodings.
- After publishing a new bus root/context entry while translation is active, perform global context-cache invalidation followed by global IOTLB invalidation with read/write drains before GrantDma succeeds.
- Index context entries by devfn inside the selected bus table and track exact BDFs in each domain.
- Teardown clears every exact BDF context first, then invalidates the context cache, then invalidates and drains the IOTLB before pinned memory may be released.
- This phase landed atomically with nonzero-bus discovery and bus-master enable;
  the former `bus != 0` gate is closed for the verified q35 software lane.

## Acceptance

- Equal devfn values on two buses resolve to distinct context pages and cannot overwrite each other.
- Clearing one BDF cannot clear the same devfn on another bus.
- Unit tests pin context/IOTLB command encodings and teardown ordering helpers.
- q35 NVMe at bus 1 completes queue/Identify DMA under VT-d.

## Evidence

- Kernel host tests passed 100/100 and the `driver-nvme` cross-check passed.
- Strict q35 multibus passed 2/2 and strict `nvme-x86` passed 3/3.
- Runtime ordered `VT-d ACTIVE` before exact BDF `01:00.0` DMA authorization
  before block-driver registration.
- Unacknowledged invalidation keeps DMA-pinned frames quarantined.
