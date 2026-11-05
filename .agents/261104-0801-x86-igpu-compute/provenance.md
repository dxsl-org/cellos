# x86 iGPU provenance ledger

**Rule:** [ADR-0030](../../docs/decisions/0030-license-model-and-copyleft-boundary.md) reuse order 1 (a permissive SPDX line on the specific file) and [ADR-0033](../../docs/decisions/0033-x86-intel-igpu-native-compute.md) §5. Add a row before review for every Cellos file adapted from or modelled on an external source.

| Class | Sources | Allowed use |
|---|---|---|
| P — permissive code | Linux `drivers/gpu/drm/xe/**`, `drivers/gpu/drm/i915/**`, `include/uapi/drm/xe_drm.h` (check each file's SPDX; MIT expected); Mesa `src/intel/**` (MIT) | Adapt with attribution in `NOTICE` |
| D — documentation | Intel Graphics for Linux PRMs (published generations) | Facts, register definitions |
| F — firmware | linux-firmware `xe/*_guc_70.bin` (and HuC/GSC only if G2 proves them necessary), `LICENSES/LICENSE.xe` | Ship unmodified with the notice; **no disassembly** |
| X — forbidden | GPL-only Linux files (shared DRM core such as `drm_gem.c`/`drm_sched` where not MIT, `drivers/gpu/drm/*` files without an MIT option); any disassembly of firmware or closed drivers | Not read by the writer of the matching component |

| Cellos file | Component | Sources (class, exact path + revision) | Notes |
|---|---|---|---|
| `libs/intel-gpu-core/src/guc_fw.rs` | GuC CSS header parse, pin check | P: Linux master `drivers/gpu/drm/xe/abi/uc_fw_abi.h` (`struct uc_css_header`), `xe_uc_fw.c` `parse_css_header`, `guc_read_css_info` (fetched 2026-11-04) | Clean Rust rewrite of the size/version rules; stricter exact-pin check per ADR-0033 §5. Verified against the real `xe/ptl_guc_70.bin` (70.72.1; header+uCode+RSA = blob size 365,376 B) |
| `libs/intel-gpu-core/src/pte.rs` | PPGTT entry encoding, PAT indices | P: `drivers/gpu/drm/xe/regs/xe_gtt_defs.h`; `xe_vm.c` `pde_encode_pat_index`, `pte_encode_pat_index`, `pte_encode_ps`, `xelp_pte_encode_addr`; `xe_pat.c` (graphics 20/30: WB=2, UC=3, WT=15) | Bit constants and PAT spreading |
| `libs/intel-gpu-core/src/ppgtt.rs` | 4 KiB PPGTT builder | P: `xe_pt.c` level shift (`XE_PTE_SHIFT + XE_PDE_SHIFT * level`), `xe_bo.h` shifts, `xe_pci.c` `ptl_desc` (va_bits 48, vm_max_level 4, dma_mask_size 46) | Own walker; huge pages not created |
| `libs/intel-gpu-core/src/ct.rs` | GuC CT headers and ring arithmetic | P: `abi/guc_communication_ctb_abi.h`, `abi/guc_messages_abi.h`, `xe_guc_ct.c` `h2g_write`, `MAKE_ACTION`, CTB sizes, `CIRC_SPACE` usage | Own ring code; wrap zero-fill and retry mirror upstream `-EAGAIN` |
| `libs/intel-gpu-core/src/syncobj.rs` | DRM syncobj semantics | P: `include/uapi/drm/drm.h` (SPDX MIT) flag values; `drivers/gpu/drm/drm_syncobj.c` (MIT permission notice) `drm_syncobj_array_wait_timeout`, `drm_syncobj_add_point`, `drm_syncobj_query_ioctl`, wait-ioctl flag checks | Own model; chain-order completion approximates `dma_fence_chain`; non-increasing CPU timeline points are accepted as upstream does (debug warning only) |
