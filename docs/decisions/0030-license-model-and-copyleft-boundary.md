# ADR-0030 — Keep the layered license model; copyleft only as isolated leaf Cells

**Date:** 2026-10-31
**Status:** Accepted
**Decider:** Cellos maintainer, in the planning conversation that weighed relicensing to GPL-3.0 in order to reuse Asahi Linux / Linux driver code.
**Scope:** Licensing policy and the CI gate that enforces it. This is a technical record, not legal advice. A licensing lawyer should review it before any commercial or government distribution.

## Context

The maintainer asked whether Cellos should move from "Apache 2.0" to GPL-3.0 so that GPL driver code could be copied, Asahi Linux being the main example. The alternative is to keep the current licenses and rewrite such code clean-room.

### Facts from the tree

**The project is not purely Apache-2.0.** `LICENSE` sets out a layered model:

| Component | License |
|---|---|
| `kernel/`, `hal/`, `libs/ostd` | MPL-2.0 (ostd adds the Cellos Cell Linking Exception) |
| `libs/api` | Apache-2.0 |
| `libs/types` | MPL-2.0 |
| `cells/` (default) and utility libraries | MIT OR Apache-2.0 |
| `docs/` | CC BY 4.0 |

**The copyleft crates today:**

| Crate | License | Manifest protection class |
|---|---|---|
| `cells/demos/doom` | GPL-2.0-only | `PROTECTION_CLASS_FFI` (Tier 1b, a trusted SAS profile per ADR-0003) |
| `cells/services/ocel-pdf` (MuPDF) | AGPL-3.0-or-later | `PROTECTION_CLASS_UNTRUSTED` (the most-isolated available domain) |

**Driver Cells are not "libs/api only".** Of the 28 crates in `cells/drivers/`, only `hid` depends on `libs/api` alone. Every MMIO/DMA driver (`e1000`, `igb`, `nvme`, `ahci`, `xhci`, `gpu`, `virtio-*`, `bcm-*`) links `libs/ostd` and `libs/types`, and the RPi drivers also link `hal/soc/bcm27xx`. They call `ostd::mmio`, `ostd::dma`, `ostd::sync` and `ostd::task` directly, and they run as Tier-1 SAS Cells in the kernel's address space. Tier-1 fast-path IPC uses shared-memory SPSC rings (ADR-0015).

**Linux, including the Asahi kernel drivers, is GPL-2.0-only.** Asahi's drivers also depend on Linux-internal APIs (DRM, the DMA API, the DART IOMMU, the RTKit mailbox, and for `drm/asahi` the Linux Rust `kernel` crate).

**Contributions use a DCO, not a CLA.** Relicensing therefore needs consent from every copyright holder.

## Decision

1. **Keep the layered MPL-2.0 / Apache-2.0 / MIT model. Do not relicense to GPL-3.0.**
   - GPLv3 is incompatible with GPL-2.0-only. Relicensing would not let Cellos copy Linux or Asahi kernel code, which was its only purpose.
   - It would cost the permissive guarantee that adopters rely on, and it would need consent from every contributor.

2. **No GPL/AGPL/LGPL code in `kernel/`, `hal/`, `libs/`, or any driver Cell.**
   - Hardware support is written clean-room from public specifications (datasheets, ACPI/UEFI, Intel SDM/VT-d), or derived from code that carries a compatible license.
   - Reuse order:
     1. A permissive SPDX identifier on the specific file. Examples: ACPICA (Intel/BSD), the Linux `i915`/`xe` DRM drivers (MIT), FreeBSD Intel NIC drivers (BSD), OpenBSD `apl*` Apple Silicon drivers (ISC), Asahi `m1n1` **up to v1.9.9 only** (MIT; GPL-2.0-or-later from v2.0.0, see the 2026-11-04 amendment), and dual-licensed device-tree bindings.
     2. Otherwise, clean-room: one person writes a behavioural specification and another implements it without access to the GPL source.
   - Rewriting C into Rust does not remove copyleft obligations.

3. **Copyleft is allowed only as a leaf application Cell.**
   - Such a crate is named in `deny.toml` `[[licenses.exceptions]]`.
   - No other crate depends on it.
   - It is optional in the image build.
   - It talks to the rest of the system only through `libs/api` and IPC that copies data.

4. **Copyleft driver Cells stay out of scope until a non-SAS driver path exists.** The prerequisite is a native-domain (Tier 2) driver model that has:
   - kernel-mediated MMIO/IRQ/DMA grants exposed through `libs/api`;
   - no `ostd`/`hal` linkage beyond `libs/api`;
   - no shared-memory ring with a Tier-1 Cell.

   Even with that path, a driver that shares DMA buffers is a grey zone that needs legal review. It does not become automatically eligible.

5. **CI enforces the boundary.**
   - `cargo deny check licenses` rejects any license outside the allow-list. Copyleft is admitted only by a named exception.
   - `scripts/check-copyleft-leaves.sh` (Security Scan job) fails if any crate depends on a copyleft leaf.

## Why the "SAS" exception does not help

The ostd Cell Linking Exception states that SAS co-residence does not make a Cell a derivative of Cellos. That is a permission granted by the Cellos copyright holders over Cellos code only.

It binds no Linux or Asahi copyright holder. Linux's own syscall note does not apply either, because there is no syscall boundary between a Tier-1 driver and the kernel. A GPL driver in the SAS therefore risks making the whole image — kernel, the other Tier-1 Cells and proprietary Cells — one combined work.

## Rejected alternatives

| Option | Why rejected |
|---|---|
| Relicense to GPL-3.0 | Incompatible with GPL-2.0-only Linux code. Loses the permissive core. Needs every contributor's consent. |
| Relicense to GPL-2.0-only | Would allow copying Linux code, but makes every image GPL. Proprietary Cells end up in the same SAS combined work, which breaks the adopter promise. Same consent problem. |
| Allow GPL driver Cells now | Drivers are Tier-1 SAS Cells linking `ostd`/`types`/`hal`. No arm's-length boundary exists. |
| Linux-API shim (DDE / LinuxKPI style) in the SAS | Same combined-work problem, plus a large compatibility layer to maintain. |

## Consequences

- Hardware enablement is slower where only GPL references exist, notably Apple Silicon (ADR-0029 group 3). The Intel C2C track (ADR-0022) has enough permissive or public sources.
- **`doom` placement (owner decision, 2026-10-31):** `doom` (GPL-2.0) currently declares the Tier-1b FFI protection class, so it runs in the SAS. It will:
  - move to its own repository, as a separately distributed GPL-2.0 work that builds against the published `libs/api` ABI only;
  - run as a Tier 2 app (`PROTECTION_CLASS_UNTRUSTED`, private MMU domain).

  This keeps Doom under its own license without combining it with the Cellos image. When the move lands, remove `cells/demos/doom` from the workspace, the `doom` exception from `deny.toml`, the `doom` entry from `LEAVES`, the DOOM section of `LICENSE`, and the `doom` rows in the image scripts. Until then it stays a developer demo, and images distributed to third parties must leave it out.
- `ocel-pdf` (AGPL) must stay optional (`CELLOS_NO_OCEL_PDF=1`). AGPL source-offer obligations apply to any image that ships it. ADR-0017 already flags a commercial MuPDF license as an option.
- The license gate became meaningful only once `deny.toml` explicitly accepted the existing `ocel-pdf` (AGPL-3.0) and `webpki-roots` (CDLA-Permissive-2.0, CA data) licenses. Before that it failed on both.
- Adding a copyleft exception now requires an ADR-0030 review entry in the PR, plus a matching edit to `LEAVES` in `scripts/check-copyleft-leaves.sh`.

### Amendment 2026-11-04 — m1n1 licence correction (ADR-0032)

The example "Asahi `m1n1` (MIT)" was incomplete. The m1n1 README states that code up to v1.9.9 stays MIT, and that from v2.0.0 m1n1 is GPL-2.0-or-later, with code checked in after that GPL-only. Current source files carry `SPDX-License-Identifier: GPL-2.0-or-later` (verified 2026-11-04). Only m1n1 tag `v1.9.9` and earlier is a permissive source. Newer m1n1 may be used only as a separate, unmodified boot or development program. OpenBSD's `sys/arch/arm64/dev/apl*` drivers (ISC) are the primary permissive Apple Silicon reference. Rules and the provenance ledger: [ADR-0032](0032-apple-silicon-secondary-lane.md) §7.

A second correction of the same date: the Context sentence "Linux, including the Asahi kernel drivers, is GPL-2.0-only" holds per file only. `irq-apple-aic.c` (GPL-2.0-or-later), `apple-dart.c` (GPL-2.0-only), `pcie-apple.c` and `nvme/host/apple.c` (GPL-2.0) are GPL. `drivers/gpu/drm/asahi/*.rs` (top-level files checked), `drivers/soc/apple/{rtkit,mailbox,sart}.c` (`GPL-2.0-only OR MIT`) and `include/uapi/drm/asahi_drm.h` (MIT) offer MIT. Mesa's Asahi driver is MIT. Those files fall under reuse order 1. The Linux `kernel::` abstractions they call remain GPL and are not copied.
