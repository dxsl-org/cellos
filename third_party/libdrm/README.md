# libdrm (pinned subset) — libdrm-lite

**Upstream:** https://gitlab.freedesktop.org/mesa/libdrm
**Pin:** `33fab8cbca7f7581e955a675597d14412702edc4` (2026-10-27, "amdgpu: fix memory leak in ASIC ID table lookup")
**Licence:** MIT (`LICENSES/MIT.txt`; SPDX lines in `xf86drm.c`, `xf86drm.h`, `libdrm_macros.h`; MIT permission text in `include/drm/drm.h` and `include/drm/drm_mode.h`)
**Purpose:** the libdrm calls Mesa `anv` makes on x86_64 Tier-2 Cells (runtime plan R6, owner decision D-R9). Built by `libs/drm-cellos`.

Vendored; regenerate with `bash third_party/libdrm/update.sh`, which fetches the pins, checks every SHA-256 below and rewrites the files. Moving a pin is a deliberate edit of that script.

## Files

| File | Source | SHA-256 of the upstream file |
|---|---|---|
| `xf86drm.h` | libdrm, verbatim | `898987f2…f1b069` |
| `libdrm_macros.h` | libdrm, verbatim | `e46bbea5…694125` |
| `include/drm/drm.h` | libdrm, verbatim | `f9eadbfa…cdf08d` |
| `include/drm/drm_mode.h` | libdrm, verbatim (included by `drm.h`) | `ee4068c0…f3a3bf` |
| `include/drm/xe_drm.h` | Linux `include/uapi/drm/xe_drm.h` @ `05441d697f7f9a927264f4058cbbc6a03dfcac9d`, verbatim; SPDX MIT. Not in libdrm at the pin | `a8aab5a3…52ac70` |
| `LICENSES/MIT.txt` | libdrm, verbatim | `4af446c9…827155` |
| `xf86drm.c` | libdrm `xf86drm.c` (`ad1ba6cf…d7e706`): licence block plus the ranges below, unmodified, behind a fixed include preamble | — |

Full hashes are in `update.sh`.

## `xf86drm.c` subset

| Upstream lines | Content |
|---|---|
| 1-13 | licence block |
| 103, 105 | `memclear`, `drm_server_info` (never set: `drmSetServerInfo` is not kept) |
| 627-649 | `drmDebugPrint`, `drmMsg` |
| 658-680 | `drmMalloc`, `drmFree`, `drmIoctl` |
| 1280-1391 | `drmFreeVersion`, `drmFreeKernelVersion`, `drmCopyVersion`, `drmGetVersion` |
| 1428-1442 | `drmGetCap` |
| 3351-3382 | `drmPrimeHandleToFD`, `drmPrimeFDToHandle` |
| 4957-5194 | `drmSyncobjCreate` … `drmSyncobjEventfd` (all `drmSyncobj*`) |

Replaced, not copied: `drmGetDevices2`, `drmGetDevice2`, `drmGetDevice`, `drmFreeDevice`, `drmFreeDevices` (upstream reads sysfs). The Cellos backend is `libs/drm-cellos/c/device.c`: identity from `DRM_IOCTL_VERSION` (`"xe"`) and the `xe` `DEVICE_QUERY` `CONFIG` item, bus address from the Cell's launch configuration.

Every other libdrm entry point is absent, so a new caller fails at link time. The set Mesa needs is fixed by iGPU phase G3's `nm -u` inventory.

## Building

`drm.h` and `xf86drm.h` take their `__linux__` branch (the host gcc predefines it): Cellos serves the Linux DRM uAPI unchanged. On Cellos targets `libs/drm-cellos/compat/` supplies `<linux/types.h>`, `<asm/ioctl.h>` and `<sys/ioctl.h>`; host tests use the Linux headers.
