# R6 → G3: registering the IPC transport as the `/dev/dri/` fd provider

Written 2026-11-05 at R6 close, for the iGPU G3 owner's interface review (phase R6 success criterion 3). Every `file:line` below is in R6's commits.

## 1. What G3 implements

One `struct cellos_fd_provider_ops` table and one registration call, before Mesa's first `drmGetDevices2` (for example in the inference Cell's start-up, before `vkCreateInstance`):

```c
#include <vicell/fd_provider.h>   // third_party/mlibc/sysdeps/vicell/include
#include "drm-cellos.h"           // libs/drm-cellos/include

static const struct cellos_fd_provider_ops xe_transport_ops = {
    .abi = CELLOS_FD_PROVIDER_ABI,
    .open = xe_open,     // one IPC session = one drm_file; ENOENT for any name but DRM_CELLOS_RENDER_NODE
    .close = xe_close,   // session teardown
    .ioctl = xe_ioctl,   // marshal (request, *arg) to the driver Cell
    .fstat = xe_fstat,   // S_IFCHR, st_rdev = DRM_CELLOS_RENDER_RDEV
    .mmap = xe_mmap,     // routed after R3b (see §4)
    .munmap = xe_munmap,
};

drm_cellos_set_pci_bus(domain, bus, dev, func);              // launch-config BDF (D-R9)
int err = cellos_fd_provider_register(DRM_CELLOS_NODE_PREFIX, &xe_transport_ops, &session_factory);
```

No R6 file changes for this: the fake provider (`libs/drm-cellos/c/fake_provider.c`) is registered the same way and is simply not linked (it sits behind the `fake-provider` feature of the `drm-cellos` crate).

## 2. Contract the transport can rely on (`fd_provider.h`)

- **Routing.** `open` of any path starting with the prefix goes to `ops->open` with the full path; `close`, `fstat`, `ioctl` on the returned fd go to the same provider with the provider's `handle`. Every other path and fd keeps the kernel route. `ioctl` on a kernel fd is `ENOTTY`.
- **fd numbers.** `0x100000`…`0x10003f` (64 per process, all providers together). A 65th open gives `EMFILE`.
- **Errors.** Every op returns 0 or a positive errno; libc turns it into `-1`/`errno`. Upstream libdrm's `drmIoctl` retries `EINTR`/`EAGAIN`, so the transport must not use them for "busy".
- **ioctl.** `request` arrives as the 32-bit Linux number (the vicell sysdep clears `posix_devctl`'s sign extension). `*result` starts at 0 and becomes `ioctl()`'s return value on success. Struct layouts are the Linux uAPI's (`third_party/libdrm/include/drm/{drm,xe_drm}.h`), x86_64 sizes.
- **Concurrency.** Ops run without registry locks and may block on IPC; several threads may be inside ops on the same fd (Mesa's submit thread + waits). The transport serialises its own session state.
- **Close.** `ops->close` runs exactly once per successful `open`, after the last in-flight op on that fd returns, even if the application closed the fd mid-op (`fd_provider_core.h`, slot life cycle; host test `test_close_during_ioctl`). That is the point for session teardown (`xe-uapi-subset.md` §4 "File descriptor").
- **Lifetime.** `ops` and `ctx` must live for the process; registration is permanent. One provider per prefix; nested prefixes are refused (`EEXIST`).

## 3. What libdrm-lite asks of the transport

`drmGetDevices2`/`drmGetDevice2` (`libs/drm-cellos/c/device.c`) need, on the render node:

| Request | Answer | Used for |
|---|---|---|
| `fstat` | `S_IFCHR`, `st_rdev` = makedev(226, 128) (Linux encoding, `DRM_CELLOS_RENDER_RDEV`) | `drmGetDevice2` only; anything else gives `-EINVAL`/`-ENODEV` |
| `DRM_IOCTL_VERSION` | `name` = `"xe"`, two-call length protocol (`drm_copy_field`) | any other name: no device |
| `DRM_IOCTL_XE_DEVICE_QUERY` `CONFIG` | size protocol; `info[REV_AND_DEVICE_ID]` = device id (bits 0-15) and revision (bits 16-23) from PCI config | PCI ids; vendor fixed at `0x8086` |

The bus address is the launch configuration's (`drm_cellos_set_pci_bus`, default `0000:00:02.0` [inference]). Subsystem ids read 0.

## 4. Open items for G3

1. **`mmap`/`munmap`.** In the v1 ops table but not routed: `VmMap` still refuses non-anonymous mappings with `EINVAL` until R3b. Routing them needs a vicell change (fd-aware `VmMap`, and a provider-mapping table for `VmUnmap`, which gets no fd). The table shape is fixed now, so G3's provider does not change when that lands.
2. **`ioctl` and `<sys/ioctl.h>` come from libdrm-lite**, not libc: mlibc builds `ioctl` only with its `glibc` option, which stays off (D-R8). Mesa must link `drm-cellos` and put `libs/drm-cellos/compat` on its include path (also for `<linux/types.h>` and `<asm/ioctl.h>`).
3. **`__linux__`.** The pinned `drm.h` and Mesa's `drm-uapi/drm.h` take their Linux branch only when `__linux__` is defined. The host gcc (R6's builds) defines it; `tools/cellos-cc` uses clang `--target=x86_64-unknown-none-elf`, which does not. A Mesa build through `cellos-cc` must add `-D__linux__` for those headers or use gcc.
4. **`major()`/`minor()`.** mlibc's `posix` option has no `<sys/sysmacros.h>`; Mesa's `drmGetNodeTypeFromFd`-style checks need it or a substitute. `drm-cellos.h` spells out the Linux encoding (`DRM_CELLOS_MAJOR_OF`/`DRM_CELLOS_MINOR_OF`).
5. **libdrm surface.** Present: `drmIoctl`, `drmGetVersion`/`drmFreeVersion`, `drmGetCap`, `drmPrime{HandleToFD,FDToHandle}`, all `drmSyncobj*`, `drmMsg`, `drmMalloc`/`drmFree`, and the five enumeration entry points. Everything else is undefined at link; G3 step 1's `nm -u` inventory decides additions (`third_party/libdrm/update.sh` `RANGES`).
6. **Header installation.** `vicell/fd_provider.h` is not installed into the mlibc sysroot (`meson.build` installs only `abi-bits`; it was outside R6's file set). Consumers add `third_party/mlibc/sysdeps/vicell/include` after the sysroot (`-idirafter`), as `libs/drm-cellos/build.rs` does; `DEP_DRM_LITE_INCLUDE` lists it last.
