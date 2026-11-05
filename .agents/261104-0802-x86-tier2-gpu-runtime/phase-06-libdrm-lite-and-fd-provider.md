---
phase: 6
title: "libdrm-lite and the Cellos fd provider (transport-shim side)"
status: pending
priority: P1
effort: "5-8d"
dependencies: ["R3 (R3a for fd/ioctl; R3b for provider-backed mmap)"]
tier: thinking
checkpoint: false
---

# Phase R6: libdrm-lite and fd provider

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- [plan.md](plan.md) D-R9; [port gaps](../261104-0801-x86-igpu-compute/research/mesa-anv-port-gaps.md) §2 rows "open/close", "libdrm …", "Without libdrm", §3 rows "fd table", "libdrm", §4, §6 G-e, G-f
- [xe-uapi-subset.md](../261104-0801-x86-igpu-compute/xe-uapi-subset.md) §4 rows "Device identity" (`:130`), libdrm enumeration (`:73-76`), open question Q7 (`:158`)
- [phase G3](../261104-0801-x86-igpu-compute/phase-03-mesa-anv-tier2.md) (owns request marshalling to the driver Cell)

## Overview

`anv` finds its device and drives it through two surfaces:
- **libdrm.** `drmGetDevices2`/`drmGetDevice2` read sysfs on Linux, and the rest are `ioctl` wrappers.
- **libc.** `open`, `ioctl`, `mmap`, `munmap`, `close` and `fstat` on the render node.

R6 delivers the user-space side of both. G3 then plugs one transport into it:
- **libdrm-lite:** pinned upstream libdrm source with only device enumeration replaced;
- **an mlibc fd provider hook:** a path prefix plus an fd range routed to a registered provider;
- **an `Ioctl` sysdep;**
- **a fake provider** for tests.

Nothing here is a new GPU API. Every call is an existing libdrm or libc function, and identity comes from an existing `xe` query.

## Key Insights

**What `anv` needs:**
- enumeration requires `available_nodes & RENDER`, `bustype == PCI` and `vendor_id == 0x8086`;
- `drmGetDevice2(fd, DRM_DEVICE_GET_PCI_REVISION)` gives the PCI device id used for the `intel_device_info` table lookup, plus the BDF;
- `drmGetVersion` must return `name == "xe"`;
- `drmGetCap(SYNCOBJ_TIMELINE)` must return 1.
(`xe-uapi-subset.md:49-50`, `:73-74`)

**Without libdrm** Mesa's stubs enumerate no device (port gaps §2 row "Without libdrm").

**Licences.** Upstream `xf86drm.h` carries `SPDX-License-Identifier: MIT` (libdrm `main`, fetched 2026-11-04). `xf86drm.c` is MIT at pinned `33fab8cb` (`xe-uapi-subset.md:13`). No libdrm code exists in the Cellos tree (grep for `xf86drm`/`libdrm` in `.rs`/`.c`/`.h`/`.toml` outside the mlibc clone, this session).

**Identity without a new API:**
- The device id and revision come from `DRM_XE_DEVICE_QUERY` `CONFIG` → `REV_AND_DEVICE_ID`, an existing `xe` query the driver Cell answers from PCI config (`xe-uapi-subset.md:87`, `:130`).
- The vendor is fixed at `0x8086`, because the G2 driver is matched by exact Intel PCI ID.
- The BDF has no `xe` query. Options:
  - the Cell's launch configuration (static), with default `0000:00:02.0` `[inference: the conventional Intel iGPU address; confirm on the G0 machine]`;
  - `QueryPcieFunction = 434`, which is gated by `PcieDriverCap` and BDF ownership (`.agents/261103-2218-x86-platform-breadth/cp1-pcie-irq-power-abi.md:36-41`) and is therefore unavailable to an untrusted client.

  D-R9 picks launch configuration.

**fd routing today.** Tier-B `sys_open` goes straight to the kernel VFS (`third_party/mlibc/sysdeps/vicell/generic/generic.cpp:88-97`; `docs/posix-ledger.md:278`). Tier A already keeps a user-space fd range for sockets starting at `SOCK_BASE_FD = 10` (`libs/api/src/services/posix/net.rs:19`). That is the precedent for shim-owned fds.

**Mesa's single ioctl choke point.** `intel_ioctl` covers Mesa's own requests, but libdrm calls and `mmap` bypass it (`xe-uapi-subset.md:123`). Routing at the libc fd level catches all of them with zero Mesa changes (port gaps §4, "Preferred").

## Requirements

**mlibc fd provider (vicell sysdeps):**
- `cellos_fd_provider_register(prefix, ops)`, where `ops` = `open`, `ioctl`, `mmap`, `munmap`, `close`, `fstat`.
- Provider fds are allocated from a reserved range above any kernel fd `[inference: choose the bound after reading the kernel fd limit]`.
- `open` on a registered prefix (`/dev/dri/`) goes to the provider. Every other path keeps today's VFS route.
- The `Ioctl` sysdep (`third_party/mlibc-src/options/internal/include/mlibc/sysdep-tags.hpp:398`): provider fd → provider; kernel fd → `ENOTTY`.
- `VmMap` with a provider fd → provider `mmap`; `munmap` of a provider mapping → provider.
- `fstat` on a provider fd returns `S_IFCHR` with `st_rdev` = makedev(226, 128).

**libdrm-lite:**
- Pinned upstream `xf86drm.h`, `xf86drm.c` subset, `include/drm/drm.h`, plus `libdrm_macros.h` as needed.
- Upstream bodies are kept for `drmIoctl`, `drmGetVersion`/`drmFreeVersion`, `drmGetCap`, `drmSyncobj*` and `drmPrime*`.
- `drmGetDevices2`, `drmGetDevice2`, `drmGetDevice`, `drmFreeDevice` and `drmFreeDevices` are replaced by a Cellos backend that:
  - asks the registered provider for identity (`DRM_IOCTL_VERSION` + `CONFIG` query);
  - builds one `drmDevice` with `bustype = DRM_BUS_PCI`, PCI ids, the launch-config BDF, `available_nodes = 1 << DRM_NODE_RENDER` and `nodes[RENDER] = "/dev/dri/renderD128"`.
- Every other libdrm entry point is absent at link time, so a new dependency fails loudly.

**Fake provider (test-only):** answers `VERSION`, `GET_CAP`, `CONFIG` and syncobj create/destroy, and maps anonymous memory for `mmap`. G3 replaces it with the IPC transport.

**The exact function set** is confirmed by G3 step 1's `nm -u` inventory.

## Architecture

```text
anv / vk runtime
  ├─ drmGetDevices2 ─> libdrm-lite Cellos backend ─> provider identity (VERSION, XE CONFIG) + launch BDF
  ├─ drmSyncobj*/drmGetCap/drmGetVersion ─> upstream wrapper ─> drmIoctl ─> ioctl(fd)
  └─ open("/dev/dri/renderD128") / ioctl / mmap / munmap / close / fstat
        ─> mlibc vicell sysdeps ─> fd provider registry ─> provider ops
                                                     ├─ R6: fake provider (tests)
                                                     └─ G3: IPC transport to the G2 driver Cell
```

## Related Code Files

- Create: `third_party/libdrm/` (pinned MIT subset + `README.md` with commit and licence), `libs/drm-cellos/` (C: enumeration backend + fake provider; build glue), `cells/tests/drm-lite-x86/**` + runner
- Modify: `third_party/mlibc/sysdeps/vicell/{generic/*.cpp,include/**}` (provider registry, `Ioctl`, fd-aware `VmMap`), `docs/posix-ledger.md` (Tier-B rows), [provenance.md](../261104-0801-x86-igpu-compute/provenance.md) row for libdrm (by the integrating session)
- Not touched: Mesa sources, `libs/api/**`, `kernel/**`

## Implementation Steps

1. Pin libdrm and check SPDX per file. Copy the subset and record provenance.
2. Provider registry + `Ioctl` + fd-aware `open`/`close`/`fstat` in vicell (R3a base).
3. libdrm-lite enumeration backend + fake provider. Host unit tests on Linux build the same C against a host stub provider.
4. QEMU x86 Tier-2 witness:
   - enumerate one device 8086:devid with a render node;
   - `drmGetVersion` = `"xe"`;
   - `drmGetCap(SYNCOBJ_TIMELINE)` = 1;
   - syncobj create/destroy;
   - `mmap`/`munmap` through the provider (after R3b);
   - `ioctl` on a VFS fd → `ENOTTY`; `open("/dev/dri/card0")` → `ENOENT`.

## Todo List

- [ ] Pin libdrm + provenance
- [ ] fd provider registry + `Ioctl`
- [ ] Enumeration backend + fake provider
- [ ] Host tests
- [ ] QEMU x86 witness
- [ ] Ledger rows

## Success Criteria

- [ ] The witness passes under QEMU TCG and KVM. The host tests pass.
- [ ] A link of the witness against the libdrm-lite archive has no unresolved `drm*` symbol.
- [ ] G3 can register its transport with no change to R6 code (interface review with the G3 owner).

## Risk Assessment

- **Mesa may call libdrm functions outside the subset** (for example `drmGetNodeTypeFromFd`, `drmPrimeFDToHandle`). Mitigation: the G3 `nm -u` inventory decides, and absent symbols fail at link.
- **A static BDF can be wrong on the real machine.** It only feeds `VK_EXT_pci_bus_info` and logging `[inference]`. Mitigation: the G0 machine record supplies it.
- **Provider `mmap` needs fixed-address placement** inside the client's own reservation. Mitigation: it uses the large-memory CP1 `FIXED` op; BO frames themselves come through the GPU CP1 export path (G3).

## Security Considerations

The provider runs inside the untrusted inference Cell. The authority boundary is the driver Cell's service and the kernel's DMA export (GPU CP1), not libdrm-lite. A forged identity only misconfigures the client's own `anv`.

## Next Steps

G3 implements the IPC transport provider and the Mesa build against R3, R4 and R6.

## Deviation Log

None.
