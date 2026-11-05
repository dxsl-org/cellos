# Mesa `anv` on Cellos: build configuration, OS surface and port gaps

**Date:** 2026-11-04. Input to [phase G3](../phase-03-mesa-anv-tier2.md) step 1 ("ioctl/POSIX inventory") and to assumption A3 in [plan.md](../plan.md).
**Pins read:**
- Mesa `main` @ `e5d557857074767553ccadcc5e80014f86a69d52` (2026-11-04), sparse clone of `meson.build`, `meson.options`, `src/{intel,util,c11,vulkan,compiler,drm-shim}`.
- llama.cpp `master` (fetched 2026-11-04): `ggml/src/ggml-vulkan/{CMakeLists.txt,ggml-vulkan.cpp,ggml-vulkan-types.h}`.
- Cellos working tree on this date.

**Licences:** all Mesa files cited are MIT (SPDX or the MIT header text). llama.cpp is MIT. No GPL file was read.
**Tags:** `[inference]` marks a claim that was not verified in source.
**Scope:** this note covers the libc/OS surface. The `xe` request subset belongs to `xe-uapi-subset.md` (peer XeUapiSubset). Kernel ABI belongs to `cp1-gpu-kernel-abi.md` (peer GpuKernelAbiInventory).

---

## 0. Headline findings

1. **A3 ("anv builds without LLVM") is true for the target build only.**
   - With `-Dvulkan-drivers=intel`, Mesa turns on CLC, and CLC force-enables LLVM (`meson.build:1019-1042`).
   - Using `-Dmesa-clc=system` removes LLVM from the target. The build host then needs a native `mesa_clc` + `vtn_bindgen2` (both need LLVM, clang, SPIRV-LLVM-Translator and SPIRV-Tools) and `glslangValidator`.
   - `intel_clc` and `-Dintel-clc` no longer exist. `meson.options` has no such option, and `src/intel/shaders/meson.build:44-70` uses `prog_mesa_clc` and `prog_vtn_bindgen2`.
2. **Mesa will not configure or compile for an unknown OS.**
   - Meson stops on `auto` drivers or platforms (`meson.build:290-293`, `:492-495`).
   - It disables the DRM path for any system outside its list, `system_has_kms_drm` (`meson.build:161`, `:805-820`).
   - `util/os_misc.c:87` and `:422` stop with `#error unexpected platform`.
   - Mesa already has a microkernel precedent, **Managarm**: it is listed in `meson.build:161` and `:1337`, has `DETECT_OS_MANAGARM` at `util/detect_os.h:89-92`, and a case at `util/u_thread.c:211`. Managarm runs `mlibc` with `posix` + `linux` (`third_party/mlibc-src/sysdeps/managarm/meson.build:1-3`).
   - A Cellos port needs the same few-line identity patch. This is a decision for the owner (§7, Q1).
3. **The Tier A POSIX shim cannot host Mesa.** It has no `pthread_*`, `ioctl`, `munmap`, `clock_gettime`, `getenv`, `qsort`, `strto*`, `dl*`, `errno` or working `mmap` (`docs/guides/posix-shim-contract.generated.md:3-4`, `:75`, `:141`; `docs/posix-ledger.md:26-29`).
   - The realistic base is **Tier B mlibc**, with mlibc's `posix` option on and new x86_64 sysdeps.
   - Today the vicell sysdeps turn `posix`/`linux` off (`third_party/mlibc/sysdeps/vicell/meson.build:10-15`), ship CRTs for RV64/AArch64 only (`:52-59`), and do not build on x86_64 (`docs/guides/tier1b-c-zig.md:483-485`, `scripts/build-mlibc.sh:2`).
4. **Hard prerequisites outside G3** (each has to land first):
   - x86_64 Tier-2 admission is closed (`docs/specs/22-native-domain-cell-implementation-gate.md:7-11`).
   - Tier-2 has no general anonymous or shared VM mapping: `ShmAlloc` returns one frame (`kernel/src/task/syscall.rs:6379-6397`), and domain grants are RV64-only (`kernel/src/task/syscall.rs:340-348`).
   - The x86 Cell target uses the soft-float ABI (`targets/x86_64-unknown-cellos.json`: `+soft-float`, `rustc-abi: softfloat`), while Mesa needs SSE2 hard-float C.
   - The kernel saves user state with FXSAVE only; XSAVE/AVX is not enabled (`hal/arch/x86/src/x86_64.rs:554-571`, `hal/arch/x86/src/x86_64/syscall.rs:344`).
5. **Mesa needs a C++17 standard library.** `brw` and `gen` use `<vector>`, `<optional>`, `<variant>`, `<map>`, `<string>` and `<charconv>`, without exceptions or RTTI. llama.cpp's Vulkan backend additionally needs C++ exceptions and `<thread>`/`<future>`. Cellos has no C++ standard library, and its C++ profile excludes x86 (`docs/guides/tier1b-c-zig.md:278-290`).
6. **`anv` is linked in-process.** A Vulkan ICD is a library inside the application. The Tier-2 Cell is therefore "llama.cpp (or `ai` engine) + anv + mlibc + transport shim", not a standalone anv Cell (§7, Q3).

---

## 1. Minimal Mesa meson configuration (Intel Vulkan, compute, headless)

### 1.1 Target (cross) build

```sh
meson setup build-cellos \
  --cross-file meson/cellos-x86_64.ini \
  -Ddefault_library=static -Dbuildtype=release -Db_ndebug=true \
  -Dvulkan-drivers=intel -Dgallium-drivers= -Dplatforms= \
  -Dopengl=false -Dgles1=disabled -Dgles2=disabled \
  -Dglx=disabled -Degl=disabled -Dgbm=disabled -Dglvnd=disabled \
  -Dllvm=disabled -Dmesa-clc=system -Dprecomp-compiler=system \
  -Dintel-elk=false -Dintel-rt=disabled -Dintel-virtio-experimental=false \
  -Dvulkan-layers= -Dtools= -Dbuild-tests=false -Dhtml-docs=disabled \
  -Dshader-cache=disabled -Dzlib=disabled -Dzstd=disabled \
  -Dxmlconfig=disabled -Dexpat=disabled \
  -Dvalgrind=disabled -Dlibunwind=disabled -Dperfetto=false \
  -Dvideo-codecs= -Dspirv-tools=disabled -Dsse2=true
```

| Option | Value | Why | `meson.options` line / effect in `meson.build` |
|---|---|---|---|
| `vulkan-drivers` | `intel` | `auto` stops with "Unknown OS" on a non-KMS system | `meson.options:191-202`; `meson.build:270-293`, `:303` |
| `gallium-drivers` | (empty) | `auto` stops on unknown OS | `meson.options:82-95`; `meson.build:163-203` |
| `platforms` | (empty) | no X11/Wayland; `auto` stops on unknown OS | `meson.options:10-19`; `meson.build:482-495` |
| `opengl` / `gles1` / `gles2` | `false` / `disabled` | no GL | `meson.options:340-359` |
| `glx` / `egl` / `gbm` / `glvnd` | `disabled` | no window system | `meson.options:361-403`; `meson.build:564-574` |
| `shared-glapi` | (do not set) | deprecated no-op | `meson.options:333-338` |
| `llvm` | `disabled` | anv does not need LLVM on the target, provided CLC is external | `meson.options:434-439`; `meson.build:1041-1042` |
| `mesa-clc` | `system` | otherwise `with_clc` becomes true for intel-vk and requires LLVM | `meson.options:852-860`; `meson.build:1019-1042` |
| `precomp-compiler` | `system` | keeps `with_drivers_clc` false; Intel has no precomp executable | `meson.options:879-887`; `meson.build:1015-1019` |
| `intel-elk` | `false` | Gfx8-and-older compiler (57 `.cpp` files); not needed for Xe3 | `meson.options:736-743`; `src/intel/meson.build:9-16` |
| `intel-rt` | `disabled` | no ray tracing / BVH sub-build | `meson.options:729-734`; `src/intel/vulkan/meson.build:46-47` |
| `shader-cache` | `disabled` | no on-disk cache (`flock`, `mkdir`, `posix_fallocate`); also lifts the compression requirement | `meson.options:255-260`; `meson.build:1258-1262`, `:1905-1910` |
| `zlib` / `zstd` | `disabled` | `zlib` defaults to **enabled**; only the cache, the decoder and debug use it | `meson.options:624-638`; `meson.build:1883-1903` |
| `xmlconfig` / `expat` | `disabled` | driconf is compiled in ("the default driconf file is hardcoded") | `meson.options:74-80`, `:822-830`; `meson.build:1930-1949` |
| `vulkan-layers` / `tools` / `build-tests` | empty / empty / `false` | no extra binaries | `meson.options:322-331`, `:563-571`, `:497-503` |
| `video-codecs` | (empty) | default `all_free`; `anv_video.c` still compiles | `meson.options:745-758` |
| `perfetto` / `valgrind` / `libunwind` | off | tracing and debug only | `meson.options:659-664`, `:476-488` |
| `spirv-tools` | `disabled` | only needed by CLC | `meson.options:913-917`; `meson.build:2112-2121` |
| `intel-virtio-experimental` | `false` | virtio native-context transport; a design precedent only (§4) | `meson.options:926-931`; `meson.build:350-352` |
| `sse2` | `true` (default) | `-msse2 -mfpmath=sse`: hard-float SSE2 C | `meson.options:646-650`; `meson.build:1593-1599` |
| `default_library` | `static` | Cellos Cells are static, and "Rust owns final cell linking" | `cmake/cellos-riscv64.cmake:1`; `tools/cellos-cc:3-4` |

**Still required on the target after these options:**
- A threads dependency: `dep_thread = dependency('threads')` (`meson.build:1912-1928`) feeds `src/c11/impl/threads_posix.c`, which uses `pthread_create`, `pthread_join`, `pthread_detach`, `pthread_exit`, `pthread_once`, `pthread_key_*`, `pthread_mutex*`, `pthread_mutex_timedlock` and `pthread_cond_*`.
- `dl` (`meson.build:1854-1868`).
- `libm` (`dep_m`, `meson.build:1956`).
- `regex` is optional: without it Mesa defines `-DNO_REGEX` (`meson.build:1959-1961`; `util/xmlconfig.c:52-53`).
- The ICD is a `shared_library` (`src/intel/vulkan/meson.build:308-320`, linking `dep_thread, dep_dl, dep_m`). A static Cellos link either consumes the intermediate static archives (`libanv_common`, the per-generation libraries, `libintel_*`, `libvulkan_runtime`, `libmesa_util`, …) or needs a small meson patch to make the ICD a `library()` `[inference: no option exists for this]`.

### 1.2 Build-host tools (native, Linux)

| Tool | Needed for | Source |
|---|---|---|
| `mesa_clc` (LLVM + clang + LLVMSPIRVLib + SPIRV-Tools) | compiles `src/intel/shaders/*.cl` (DGC, query copy, generated draws) to SPIR-V, once per generation 8…35 | `src/intel/shaders/meson.build:21-63`; built only when `mesa-clc != system` (`src/compiler/clc/meson.build:127-141`); `meson.build:2095-2121` |
| `vtn_bindgen2` | turns that SPIR-V into C/C++ bindings linked into `libintel_shaders` | `src/intel/shaders/meson.build:65-80`; `src/compiler/spirv/meson.build:82-95` |
| `glslangValidator` ≥ 12.2 | `float64_spv.h`, which is always built for intel-vk; `with_bvh = … or with_intel_vk` | `meson.build:735-760`; `src/intel/shaders/meson.build:4-18` |
| Python 3 + `mako` + `yaml` (+ `packaging`) | entrypoint, genxml and opcode generators | `meson.build:1160-1167` |
| flex/bison | **not** needed: `needs_flex_bison` is false without GL, gallium or intel tools | `meson.build:2212` |

Host stage, recommended by the comments in Mesa's own meson files:

```sh
meson setup build-host -Dvulkan-drivers= -Dgallium-drivers= -Dplatforms= -Dopengl=false \
  -Dglx=disabled -Degl=disabled -Dgbm=disabled -Dmesa-clc=enabled -Dinstall-mesa-clc=true \
  -Dllvm=enabled -Dspirv-tools=enabled
```

Put the resulting `mesa_clc` and `vtn_bindgen2` on `PATH` for the cross build. `meson.build:1031-1032` locates them with `find_program(..., native: true)`. Pin the same Mesa commit for both stages `[inference: the SPIR-V/bindgen ABI is not versioned across commits]`.

### 1.3 Compiler selection on Xe3

On Xe2/Xe3, Mesa uses the new C compiler **Jay** by default for compute: `bool by_default = devinfo->ver == 20 || devinfo->ver == 30` (`src/intel/dev/intel_debug.c:304-324`).
- Jay links `libintel_compiler_gen` (`src/intel/compiler/jay/meson.build:89`).
- `gen` is **C++**: `gen_parse.cpp` uses `<map>`, `<string>`, `<string_view>` and `<charconv>` (`src/intel/compiler/gen/meson.build:20-36`).
- `brw` (71 `.cpp` files: `<vector>`, `<memory>`, `<algorithm>`, `<optional>`, `<variant>`, `<list>`) is still linked (`src/intel/compiler/meson.build:57-62`).
- Neither uses `throw`, `try`, `dynamic_cast` or `typeid` outside tests (grep over `brw/`, `gen/` and `util/*.cpp`). Both can therefore build with `-fno-exceptions -fno-rtti` against a libc++ built the same way `[inference: not compiled]`.

Approximate source closure (lines, including headers): anv 114k, NIR 139k, `util` 88k, brw 73k, vk runtime 58k, SPIR-V 28k, WSI 22k, Jay 20k, ISL 19k, gen 19k, BLORP 13k.

---

## 2. Runtime OS surface used by anv + `src/util` + `src/vulkan/runtime`

Method: regex scan of call sites in `src/intel/{vulkan,common,dev,isl,compiler,blorp,perf,ds,decoder,mda}`, `src/util`, `src/c11`, `src/vulkan/{runtime,util,wsi}` and `src/compiler/{nir,spirv}`, excluding tests and tools, with each hit read at the cited line.

"Needed" means the compute-only build with §1.1 options calls it at runtime. "Link" means it must resolve at link time but stays off the compute path.

| Surface | Where (Mesa path:line) | Compute-only? | Notes |
|---|---|---|---|
| `pthread_*` (via C11 `thrd_`/`mtx_`/`cnd_`/`call_once`/`tss`) | `src/c11/impl/threads_posix.c`; `mtx_lock` in `vk_queue.c:554`, `anv_device.c:987`; `call_once` in `anv_instance.c:130`, `intel_debug.c:340`, `nir.c:134` | **Needed** | mutexes, condition variables and `call_once` on every path |
| Thread creation | `vk_queue.c:809` (`thrd_create` submit thread) | Link; runtime only on demand | drm-syncobj types lack `WAIT_BEFORE_SIGNAL` (`vk_drm_syncobj.c:658-700`), so the timeline mode is ASSISTED and submit is `THREADED_ON_DEMAND` (`vk_device.c:224-245`); the thread starts only for wait-before-signal `[inference]`. `util_queue` threads only for the disk cache (`disk_cache.c:90`), which is off |
| `pthread_setaffinity_np`, `setname_np`, `sigmask`, `getcpuclockid` | `util/u_thread.c:88-215` | Link only | guarded by `HAVE_PTHREAD_SETAFFINITY` and others; Managarm excluded at `u_thread.c:211` |
| Futex | `util/futex.c:37`, `:109`; `util/simple_mtx.h:60-203` | Optional | `UTIL_FUTEX_SUPPORTED` is 0 unless Linux, FreeBSD, OpenBSD or Windows (`util/futex.h:27-40`); `simple_mtx` then falls back to `mtx_t` |
| TLS (`thread_local`) | `util/u_call_once.c:14`, `util/os_misc.c:231`, `util/u_debug.c:411-412` | **Needed** | static `_Thread_local` in C, so the static TLS block must be set up per thread |
| `mmap`/`munmap` of the DRM fd at the fake offset, `MAP_SHARED`, optionally `MAP_FIXED` | `xe/anv_kmd_backend.c:148-186` (`DRM_IOCTL_XE_GEM_MMAP_OFFSET` then `mmap(fd, args.offset)`, then `munmap` of the head) | **Needed** | every host-visible BO map |
| `mmap` of a dma-buf/prime fd | `anv_allocator.c:1797-1811` (slab BOs via `anv_gem_handle_to_fd`) | **Needed** if slab BOs are mapped | `DRM_IOCTL_PRIME_HANDLE_TO_FD` plus a mappable fd |
| `memfd`/anonymous file + `MAP_SHARED`, re-mapped as it grows (aliasing) | `anv_allocator.c:135` (1 GiB `BLOCK_POOL_MEMFD_SIZE`, `anv_private.h:705`), `:189-190` (`MAP_SHARED\|MAP_POPULATE`); `util/anon_file.c:187-200` | **Needed** | the state table; several live maps of the same file must alias; sparse 1 GiB backing |
| `mmap(PROT_NONE, MAP_FIXED\|MAP_ANONYMOUS)` | `anv_allocator.c:1899` | Placed-map extension only | `VK_EXT_map_memory_placed` |
| `ioctl` | `intel/common/intel_gem.h:82-93` (`intel_ioctl`, retries on `EINTR`/`EAGAIN`); libdrm wrappers | **Needed** | the single choke point; under `HAVE_INTEL_VIRTIO` it already reroutes to `intel_virtio_ioctl` |
| `open(render node, O_RDWR\|O_CLOEXEC)`, `close` | `anv_physical_device.c:3023`, `:3283` (primary node only for `KHR_display`) | **Needed** | path comes from `drmDevice.nodes[DRM_NODE_RENDER]` |
| `stat` → `major()/minor()` | `anv_physical_device.c:3299-3320` | Optional | failure leaves `has_local = false` (`VK_EXT_physical_device_drm` only) |
| libdrm `drmGetDevices2`, `drmGetDevice2`, `drmFreeDevice(s)` | `vk_instance.c:415-417`; `intel_device_info.c:2066-2081` (PCI ids and BDF) | **Needed** | on Linux libdrm reads sysfs; on Cellos it must be synthesised |
| libdrm `drmGetVersion`/`drmFreeVersion` | `intel_kmd.c:31-52` (`name == "xe"` selects the backend) | **Needed** | `DRM_IOCTL_VERSION` |
| libdrm `drmGetCap(DRM_CAP_SYNCOBJ_TIMELINE)` | `util/u_sync_provider.c:142` | **Needed** | turns on timeline syncobjs |
| libdrm `drmSyncobj*` (create, destroy, wait, reset, signal, timeline wait/signal, query, transfer, fd↔handle, sync file) | `util/u_sync_provider.c:23-103`; probed at `vk_drm_syncobj.c:658-700`; anv asserts `TIMELINE` and `CPU_WAIT` (`anv_physical_device.c:3158-3163`) | **Needed** | `util_sync_provider` is an existing Mesa vtable (virtio uses its own) |
| Without libdrm | `util/libdrm.h:33-162` | — | if `HAVE_LIBDRM` is unset, these become stubs that return errors, so no device is enumerated; `anv_physical_device.c:28` includes `<xf86drm.h>` directly |
| `dladdr` + `dl_iterate_phdr` + build-id note | `anv_physical_device.c:2757-2761` → `util/disk_cache.h:107-121` → `util/build_id.c:150-169` | **Needed (fatal)** | without them `vkEnumeratePhysicalDevices` fails with "Failed to find build-id"; `HAVE_BUILD_ID` needs `HAVE_DL_ITERATE_PHDR` (`util/build_id.h:29-33`); link with `--build-id` |
| `dlopen`/`dlsym` | `intel_perf_metrics_library.cpp:57-62`, `util/u_dl.c:48`, `vk_instance.c:657` | Link only; returns `NULL` | Intel metrics library and layers |
| `getenv`/`secure_getenv` | `util/os_misc.c:244-246` (`os_get_option`, used 50+ times: `INTEL_DEBUG`, `MESA_VK_*`) | **Needed** | may return `NULL` |
| `clock_gettime(MONOTONIC/REALTIME)`, `clock_nanosleep` | `util/os_time.c:57-71`; `c11/impl/time.c:97-127`; `vk_drm_syncobj.c:255` | **Needed** | wait deadlines are absolute monotonic ns |
| `sched_yield` | `vk_drm_syncobj.c:258`, `util/os_time.c:135` | **Needed** | |
| `sysconf(_SC_PAGESIZE, _SC_PHYS_PAGES, _SC_NPROCESSORS_*)` | `util/os_misc.c:371-424`, `:499-524`; `util/u_cpu_detect.c:819-843` | **Needed** | Mesa will not compile unless `HAVE_SYSCONF && HAVE_SC_PHYS_PAGES` or a known OS (`os_misc.c:422` `#error`) |
| `sched_getaffinity` | `util/u_cpu_detect.c:801` | Optional | under `DETECT_OS_POSIX`; falls back to sysconf |
| `/proc`, `/sys` reads | `util/os_misc.c:431` (`/proc/meminfo`, Linux only); `intel/perf/*` (`/proc/sys/dev/xe/observation_paranoid`, `/sys/...`); `util/u_process.c:242` (`readlink /proc/self/exe`) | Optional | all fail soft; perf queries are not used for compute |
| `getauxval`, `cpuid` | `util/u_cpu_detect.c:183`, `:298` | `cpuid` needed on x86 | SSE4.1 path in `anv_image_host_copy.c:185`; AVX is reported only when OSXSAVE is set |
| `qsort`, `bsearch` | NIR (15 sites), `vk_shader.c:691`, `brw`/`jay`, `vtn_variables.c:2834` | **Needed** | compiler passes |
| `strtod`/`strtod_l`+`newlocale` | `util/strtod.c:52-76` | Needed by parsers | falls back to `strtod` without `HAVE_STRTOD_L` |
| `asprintf`/`vasprintf`, `snprintf` with `%*s` and `PRIx64` | `util/u_string.h:110`, `util/u_debug_stack.c:79`, NIR print | **Needed** | full printf semantics |
| `open_memstream` | `util/memstream.c:67` (used by `anv_shader.c`, `nir_print.c`) | Link; runtime for executable properties | |
| `strtok_r`, `strcasecmp`, `strerror`, `sscanf`, `getline` | `anv_physical_device.c:2832`, `util/u_debug.c:116`, `xmlconfig.c:834` | **Needed** (debug-option parsing) | |
| `getpid`, `getuid`, `time`, `localtime_r` | `anv_util.c:386`, `util/os_misc.h:47` | Link; minor runtime | |
| `atexit` | `util/u_queue.c:85` | Link | |
| `errno` (thread-local) | `intel_gem.h:91`, `anv_physical_device.c:3025` | **Needed** | ioctl retry and error mapping |
| `posix_memalign`/`aligned_alloc` | `util/os_memory_aligned.h:47` | **Needed** | |
| expat, zlib, zstd | `util/xmlconfig.c:1276`, `util/compress.c:80-149`, `intel_decoder.c:748` | Not needed | removed by §1.1 options |
| `poll`, `eventfd`, `fcntl`, `flock`, `mkdir`, `opendir` | `util/os_file_notify.c`, `util/disk_cache_os.c`, WSI display | Not needed | cache/WSI/notify are off or unused |
| GCC `__atomic` builtins | `util/u_atomic.h:25-31` | **Needed** | compiler feature, not OS |

---

## 3. Cellos gap table

**Profiles:**
- **A** = Tier A Rust POSIX shim (`libs/api/src/services/posix/`, exports listed in `docs/guides/posix-shim-contract.generated.md`).
- **B** = Tier B mlibc (`third_party/mlibc/sysdeps/vicell/`; mlibc source is a gitignored clone at `third_party/mlibc-src/`, `.gitignore:112`).
- **PP** = `libs/port-platform`.

| Need | Cellos today (file:line) | Status | Gap for anv compute |
|---|---|---|---|
| x86_64 Tier-2 Cell admission | closed on AArch64/x86_64 (`docs/specs/22-native-domain-cell-implementation-gate.md:7-11`; `docs/decisions/0022-intel-x86-64-c2c-only-direction.md:19-21`) | **missing** | kernel prerequisite, outside G3 |
| C toolchain for x86_64 Cells | `tools/cellos-cc:7-18` (RV64 and AArch64 only); `tools/cellos-zig-cc:4-8` (has x86_64); `cells/tests/cpp-smoke/build.rs:72-73` (`-mno-red-zone -mcmodel=small`) | partial | needs `meson/cellos-x86_64.ini` (only `meson/cellos-riscv64.ini:1-16` exists) and `cmake/cellos-x86_64.cmake` (only RV64/AArch64 in `cmake/`) |
| Hard-float SSE2 C ABI | Cell target `targets/x86_64-unknown-cellos.json` (`features: -sse…,+soft-float`, `rustc-abi: softfloat`); kernel enables OSFXSR and FXSAVEs on entry (`hal/arch/x86/src/x86_64.rs:554-571`; `hal/arch/x86/src/x86_64/syscall.rs:344`) | **partial / conflict** | SSE2 user code survives traps `[inference: no saved-state test seen]`. Rust soft-float FFI (Tier A `math.rs` `double` args) mismatches hard-float C `[inference]`. Use mlibc libm compiled hard-float, with no float args across the Rust↔C boundary |
| AVX/YMM user state | no XSAVE or OSXSAVE in `hal/arch/x86` (grep) | **missing** | not needed by Mesa (SSE2/SSE4.1); llama.cpp CPU code must build `GGML_AVX*=OFF` until XSAVE exists |
| libc base (ansi + posix) | A: about 150 symbols; refusals `dlopen` → NULL, `mmap` → NULL (`posix-shim-contract.generated.md:75`, `:141`); errno never set (`docs/posix-ledger.md:26-29`). B: `posix`/`linux`/`glibc`/`bsd` off (`third_party/mlibc/sysdeps/vicell/meson.build:10-15`) | **missing (A) / partial (B)** | turn mlibc `posix` on for vicell; A cannot be extended to this scale |
| mlibc on x86_64 | CRT only for riscv64/aarch64 (`vicell/meson.build:52-59`); `sys_tcb_set` is a no-op on other targets (`docs/posix-ledger.md:275`; `generic/generic.cpp:61`); build script and shim are RV64/AArch64 (`scripts/build-mlibc.sh:2`, `libs/mlibc-shim/build.rs:24-28`); "Level B not yet available on x86_64" (`docs/guides/tier1b-c-zig.md:483-485`) | **missing** | add `crt-x86_64/crt1.S`, `sys_tcb_set` → `SetTlsBase` (`libs/api/src/abi/syscall.rs:172`, `FS_BASE` per `tier1b-c-zig.md:325`), and an x86 syscall thunk |
| Threads | PP subset: 16 threads, no TLS keys, detach or `__thread` (`libs/port-platform/include/cellos_pthread.h:5-8`, `:19-20`); syscall thunk is RISC-V only (`libs/port-platform/include/cellos_syscall.h:144-166`); B: pthreads disabled (`docs/posix-ledger.md:276`); kernel has `Spawn`/`Wait`/`Exit` (`syscall.rs:38`, `:160`; `libs/port-platform/cellos_pthread.c:9-14`, `:113-115`) | **partial** | mlibc threads need the `Clone`, `PrepareStack` and `ThreadExit` sysdeps (`third_party/mlibc-src/options/internal/generic/threads.cpp:123-132`; `.../sysdep-tags.hpp:27-35`) on `Spawn`, plus per-thread TLS |
| Futex | kernel `FutexWait`/`FutexWake` (`libs/api/src/abi/syscall.rs:186`, `:191`); B `sys_futex_wait` is a yield loop with timeout ignored, and wake is a no-op (`generic/generic.cpp:74-86`; ledger `:276-277`) | **partial** | wire both to syscalls 17/18; timeout in 10 ms ticks (`syscall.rs:173-185`) is coarse for `pthread_cond_timedwait` `[inference: acceptable]` |
| Thread-safe, large heap | `ostd` heap 1 MiB static and unlocked ("single-task cells never enter the allocator concurrently", `libs/ostd/src/heap.rs:13-19`, `:28`, `:77-79`); B arena 4 MiB bump, never freed (`generic/generic.cpp:32`, `:158-187`) | **missing** | mlibc slab over a real `sys_vm_map`; depends on a Tier-2 page-allocation primitive (peer GpuKernelAbiInventory: CP1 item D-G0) |
| Anonymous `mmap`/`munmap`, `MAP_FIXED`, `PROT_NONE` reserve | A `mmap` → NULL (`libs/api/src/services/posix/sysio.rs:210-223`); B `MAP_ANONYMOUS` only, unmap no-op (`generic/generic.cpp:178-187`); `ShmAlloc` = one frame at its identity VA (`kernel/src/task/syscall.rs:6379-6397`) | **missing** | same kernel prerequisite |
| `memfd` + aliasing `MAP_SHARED` (1 GiB sparse) | nothing | **missing** | shim design: one reserved range per memfd with commit on `ftruncate`/growth, returning the same base on each re-map `[inference]`; needs lazy commit or a smaller `BLOCK_POOL_MEMFD_SIZE` patch |
| BO `mmap(drm_fd, fake_offset)` | nothing; x86 domain Cells cannot use grants (`kernel/src/task/syscall.rs:340-348`) | **missing** | the transport shim maps the BO pages it owns; kernel side is CP1 (peer) |
| File-descriptor table with a DRM pseudo-fd (`open("/dev/dri/renderD128")`, `close`, `ioctl`, `fstat`) | A/B `open` go straight to the kernel VFS (`sysio.rs:140-150`; ledger `:278`) | **missing** | shim-owned fd range routed to the transport (like A's socket fds 10-17, `net.rs:19-43`); add mlibc `Ioctl` sysdep (`sysdep-tags.hpp:398`) |
| libdrm (about 26 functions in §2) | none in repo | **missing** | "libdrm-lite" (MIT `xf86drm.h` API subset): ioctl wrappers plus a synthetic `drmDevice` (PCI 8086:devid from the driver Cell); `drmMode*` absent |
| `dladdr` + `dl_iterate_phdr` (+ build-id) | A `dlopen` → NULL only (`sysio.rs:195-201`); mlibc has `dl_iterate_phdr` via rtld (`third_party/mlibc-src/options/elf/generic/phdr.cpp`) | **missing** for static Cells | ~60 LOC single-image implementation over `__ehdr_start`, plus `-Wl,--build-id=sha1` in the Rust host link `[inference]` |
| `dlopen`/`dlsym` | refused (`sysio.rs:195-201`; `docs/guides/porting-c-apps.md:10`) | refused (OK) | Mesa tolerates `NULL` |
| `clock_gettime`, `nanosleep` | A has none (`_time`/`_gettimeofday` in seconds only, ledger `:247-248`); B `sys_clock_get` REALTIME/MONOTONIC (`generic/generic.cpp:137-153`); `GetTime` (`syscall.rs:254`; x86 op 0 = ns, `tier1b-c-zig.md:457`) | **partial (B)** | add `Sleep`/`SetTimer`-based `nanosleep` |
| `sysconf`, `getenv`, `getpid`, `sched_yield` | none in A; B none for sysconf (sysdep `Sysconf`, `sysdep-tags.hpp:512`); `Yield` exists (`syscall.rs:220`); `MemInfo` (`syscall.rs:148`) | **missing / easy** | `_SC_PHYS_PAGES` from `MemInfo`; `environ` from the manifest or empty |
| `qsort`, `bsearch`, `strto*`, `asprintf`, `strtok_r`, `strcasecmp`, `open_memstream`, `sscanf` | absent from A (`posix-shim-contract.generated.md`); present in mlibc ansi/posix sources (`third_party/mlibc-src/options/posix/generic/*`) | **missing (A) / present (B, once posix is on)** | none beyond enabling B |
| `errno` (thread-local) | A: none (`docs/posix-ledger.md:26-29`); B sets errno (ledger `:266-268`) | **present (B)** | needs TLS |
| C++17 standard library | none; C++ profile is a language subset, RV64/AArch64 only, with `_ZSt*` asserted absent (`docs/guides/tier1b-c-zig.md:278-290`); A gives `operator new`/`delete` and single-threaded `__cxa_guard` (`docs/posix-ledger.md:101-112`) | **missing** | LLVM libc++ (+ libc++abi), static over mlibc: `-fno-exceptions -fno-rtti` suffices for Mesa; llama.cpp needs exceptions (libc++abi + libunwind, which itself uses `dl_iterate_phdr` for `.eh_frame_hdr`) |
| Final link (static archives into a Rust host) | `cmake/cellos-riscv64.cmake:1`; `tools/cellos-cc:3-4`, `:23-26` (compile-only) | present (pattern) | an x86 launch row in `kernel/src/loader/launch_profile` (`docs/guides/tier1b-c-zig.md:292-294`) |
| Completion waits for syncobjs | `WaitCompletion` (`syscall.rs:783`) | present (primitive) | mapping owned by the transport shim and CP1 (peers) |

---

## 4. Transport-shim seams Mesa already has

These are the "only a transport shim" insertion points. Each is an existing Mesa interface; none is a new GPU API.

| Seam | Location | Use |
|---|---|---|
| libc `open`/`ioctl`/`mmap`/`close` on the DRM fd | `intel_gem.h:82-93`, `xe/anv_kmd_backend.c:174` | **Preferred:** zero Mesa source changes. The fd range is owned by the shim, which forwards `xe_drm.h` and `drm.h` requests over IPC |
| `intel_ioctl` → `intel_virtio_ioctl` under `HAVE_INTEL_VIRTIO` | `intel_gem.h:86-89`; `anv_physical_device.c:3034-3047` | proves Intel already carries the same ioctls over a non-ioctl transport (virtio-gpu native context); reference design for the request marshalling |
| `util_sync_provider` vtable | `util/u_sync_provider.c:120-147`; `anv_physical_device.c:3155-3160` (`intel_virtio_sync_provider`) | syncobj ops without libdrm if libdrm-lite is not wanted (needs a small anv patch) |
| Mesa `drm-shim` (MIT) | `src/drm-shim/drm_shim.c:364` (`open`), `:785-799` (`ioctl`), `:853-866` (`mmap`), `:768-775` (`realpath` for libdrm), plus `stat`/`opendir`/`readlink` of sysfs | host-side harness: LD_PRELOAD on Linux, anv unmodified, forwarding to the G1 fake `xe` backend. `intel_noop_drm_shim.c` is i915-only, so an `xe` backend is new test code |

Core `drm.h` requests the subset must include beyond `xe_drm.h` (sent to XeUapiSubset): `VERSION`, `GET_CAP`, `GEM_CLOSE`, `SYNCOBJ_*`, `PRIME_HANDLE_TO_FD`/`FD_TO_HANDLE`.

---

## 5. Vulkan loader and llama.cpp

| Question | Finding | Source |
|---|---|---|
| Loader or direct link? | **Link anv directly.** The Khronos loader needs `dlopen` and JSON manifest discovery, both refused in SAS (`docs/guides/porting-c-apps.md:10`, `sysio.rs:195-201`) | — |
| ICD entry without a loader | anv exports `vk_icdGetInstanceProcAddr` (`src/intel/vulkan/anv_instance.c:168-178`); negotiation is optional, since `vk_icd_version` defaults to 7 (`src/vulkan/runtime/vk_instance.c:572`, `:581-646`); dispatchable handles carry `ICD_LOADER_MAGIC` (`vk_object.c:39`) but anv entrypoints are real implementations, not trampolines | glue: `vkGetInstanceProcAddr(i, n) { return vk_icdGetInstanceProcAddr(i, n); }` (one function, Cellos-side) `[inference: no layer or loader behaviour needed for compute]` |
| What llama.cpp needs from Vulkan | `VULKAN_HPP_DISPATCH_LOADER_DYNAMIC 1` (`ggml-vulkan-types.h:12`); dispatcher seeded from `vkGetInstanceProcAddr` (`ggml-vulkan.cpp:5227`); links `Vulkan::Vulkan` (`CMakeLists.txt:123`) | provide `Vulkan::Vulkan` as an imported target over the static anv archive plus the glue; Vulkan-Headers (`vulkan.hpp`) from Mesa's `include/vulkan` or Khronos |
| Build-time shader compile | `find_package(Vulkan COMPONENTS glslc REQUIRED)` and `SPIRV-Headers` (`CMakeLists.txt:9`, `:14`); a host `vulkan-shaders-gen` ExternalProject runs `glslc` (`CMakeLists.txt:164-210`, `:247`); `GGML_VULKAN_SHADERS_GEN_TOOLCHAIN` selects the host toolchain when cross-compiling | host-only: shaderc `glslc` plus a host C++ compiler; output is embedded SPIR-V (`ggml-vulkan-shaders.hpp`), so no runtime shader compiler is needed beyond anv's SPIR-V → NIR → Jay |
| C++ runtime | Vulkan-Hpp with exceptions: `throw vk::SystemError` (`ggml-vulkan.cpp:5232`), 20 try/catch sites; `<thread>`, `<future>`, `<shared_mutex>`, `<mutex>`, `<condition_variable>`, `<iostream>`, `<sstream>`, `<map>` (`ggml-vulkan-types.h:50-69`); 47 `getenv` sites | full hosted libc++ with exceptions on mlibc + pthreads |
| Pipeline-compile cost | with `shader-cache=disabled`, every process start recompiles all ggml pipelines on the CPU `[inference: measure in G4]` | an in-memory `VkPipelineCache` still works |

---

## 6. Work-size estimates per gap `[inference]`

Engineer-days for one person who knows the codebase. "Prereq" items are kernel work outside G3's 40 days.

| # | Gap | Estimate | Depends on |
|---|---|---|---|
| P1 | x86_64 Tier-2 admission (spec 22 gate) | prereq; not sized here | kernel program |
| P2 | Tier-2 page allocation: anonymous VM map/unmap/reserve, lazy commit | prereq (CP1 D-G0, peer) | P1 |
| P3 | BO page mapping / IOMMU plumbing for the transport | prereq (CP1, peer) | P1, P2 |
| G-a | `meson/cellos-x86_64.ini` + `cmake/cellos-x86_64.cmake` + hard-float C ABI decision (C with SSE2; no float FFI into Rust) | 2-4 | — |
| G-b | mlibc vicell x86_64: CRT, syscall thunk, `sys_tcb_set` → `SetTlsBase`, static TLS init | 3-5 | G-a |
| G-c | mlibc `posix` on: futex wired to 17/18, `Clone`/`PrepareStack`/`ThreadExit` on `Spawn`/`Exit`, `clock_get`, `sleep`, `sysconf`, `getpid`, `environ` | 6-10 | G-b |
| G-d | mlibc `vm_map`/`vm_unmap` on P2; thread-safe slab; large heap | 2-4 | P2, G-c |
| G-e | fd table: DRM pseudo-fd, `ioctl` sysdep, `mmap(drm_fd)`/`munmap`, memfd emulation with aliasing | 6-10 | G-d, P3 |
| G-f | libdrm-lite (about 26 functions, synthetic `drmDevice`, `xf86drm.h` from libdrm MIT) | 3-4 | G-e |
| G-g | `dladdr` + `dl_iterate_phdr` for a static image; build-id in the Rust host link | 1-2 | G-b |
| G-h | Mesa port patch: `DETECT_OS_CELLOS`, `meson.build` system lists (`:161`, `:1337`), static ICD target | 1-2 | owner decision Q1 |
| G-i | libc++ / libc++abi (no exceptions) over mlibc for Mesa | 4-6 | G-c |
| G-j | exceptions + libunwind for llama.cpp | 4-8 | G-i, G-g |
| G-k | host stage (`mesa_clc`, `vtn_bindgen2`, glslang, glslc) as pinned build-host tools | 1-2 | — |
| G-l | Vulkan glue (`vkGetInstanceProcAddr` → ICD) + a `Vulkan::Vulkan` CMake target | 0.5-1 | — |
| | **G3 user-space subtotal** (G-a … G-l, excluding P1-P3 and the transport shim's request marshalling) | **~34-58** | |

The upper half of that range exceeds G3's 40-day budget before the transport shim and the compute smokes are counted `[inference]`. The C++ runtime (G-i, G-j) and the mlibc posix enablement (G-c) dominate.

---

## 7. Recommended first build step and open questions

**First build step (host only; no Cellos dependency).** On a Linux x86_64 build host:
1. Build the host stage from §1.2 at the pinned Mesa commit. Install `mesa_clc` and `vtn_bindgen2`.
2. Configure Mesa natively for Linux with the exact §1.1 option set, but with no cross file. Then:
   - Confirm it builds without LLVM in the target tree (`ldd`/`nm` of `libvulkan_intel.so` shows no `libLLVM`).
   - Run `nm -u` over the static archives behind `libvulkan_intel.so` to produce the authoritative undefined libc/libdrm/libstdc++ symbol list.
   - Diff that list against mlibc's exports with `posix` enabled. The result replaces the grep-based §2 with a linker-exact inventory.
3. Then, still on the host, run anv on an `xe` drm-shim backend (Mesa `src/drm-shim`) wired to the G1 fake `xe` backend. This proves the transport seam before any Cellos code exists.

**Open questions for the owner / Main.**
- **Q1.** May the Mesa port carry a small, upstreamable OS-identity patch (`DETECT_OS_CELLOS` in `util/detect_os.h`, `cellos` added to `meson.build:161` and `:1337`, a static ICD target), on the Managarm precedent? Without it, Mesa does not configure or compile (§0.2). This is porting glue, not a GPU API.
- **Q2.** Should A3 be restated as "no LLVM on the target; LLVM/clang on the build host" (§0.1)?
- **Q3.** Should the G3 deliverable be "a Tier-2 inference Cell that links anv", since the ICD is in-process (§0.6)? This affects whether the `ai` service (Rust) hosts anv or a separate C++ llama.cpp Cell does.
- **Q4.** Should hosted C++ (libc++ with exceptions) be admitted as a new Cellos runtime profile? This is required by llama.cpp and by Mesa's compiler (`tier1b-c-zig.md:278-290` currently forbids `_ZSt*`).
- **Q5.** For the x86 Cell ABI, should C Tier-2 code be compiled hard-float SSE2 while the Rust target stays soft-float, or should the target change? Also, who owns XSAVE/AVX enablement for llama.cpp CPU paths?
