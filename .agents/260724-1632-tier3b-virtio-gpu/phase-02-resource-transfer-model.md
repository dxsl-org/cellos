# Phase 02 — 2D Resource + Transfer Model

## Context Links
- Plan: [plan.md](plan.md) · Prev: [phase-01](phase-01-wiring-and-capabilities.md)
- Virtqueue chain processor: `cells/services/hypervisor/src/virtqueue.rs:29-101` (process_notify, DescBuf).
- Mirror device: `cells/services/hypervisor/src/virtio_blk.rs:59-99` (notify → process_notify → handle → write response into last desc).
- Guest mem I/O: `cells/services/hypervisor/src/vmm.rs:68-92`.

## Overview
- **Priority:** P1
- **Status:** complete (implementation; Linux lifecycle proof remains in phase 05)
- **Description:** Extend phase-01's device model with the full controlq command set and a host-side
  resource table (BTreeMap). After this phase a guest can create a 2D resource, attach guest-page
  backing, transfer pixels, and set a scanout (the single scanout copy lands directly in the Grant) —
  validated in isolation via serial (no display output yet; that is phase 03's DamageNotify path).
  **This is the only Track A milestone demonstrable before phase 00's display sink lands.**

## Key Insights (from research — cite in code comments)
- **Every controlq command needs a device-writable response** of at least `sizeof(ctrl_hdr)` (24 B),
  even "fire-and-forget" ones; the guest's ring free-count never recovers otherwise and the ring
  stalls. `process_notify`'s `handle` closure returns bytes written and writes into the last
  (writable) descriptor — the exact shape virtio-blk uses (virtio_blk.rs:97-98). Confirmed the
  model fits: a controlq chain is `[req readable desc][resp writable desc]`.
- `struct virtio_gpu_ctrl_hdr { u32 type; u32 flags; u64 fence_id; u32 ctx_id; u8 ring_idx; u8 pad[3]; }`
  = **24 bytes**, prefixes every request AND response.
- Command codes (req 0x01xx / resp 0x11xx):
  - `GET_DISPLAY_INFO=0x0100` → resp `OK_DISPLAY_INFO=0x1101` with the FULL
    `virtio_gpu_resp_display_info` (280 B, `pmodes[0].enabled=1`, 1024×768). Per red-team M6 the real
    payload is required from phase 01 (the driver's init-time probe consumes it) — phase 02 reuses
    that shared encoder; it is NOT deferred to phase 04.
  - `RESOURCE_CREATE_2D=0x0101` `{hdr; u32 resource_id; u32 format; u32 width; u32 height}` → `OK_NODATA=0x1100`.
  - `RESOURCE_UNREF=0x0102` `{hdr; u32 resource_id; u32 pad}` → OK_NODATA.
  - `SET_SCANOUT=0x0103` `{hdr; rect r; u32 scanout_id; u32 resource_id}` → OK_NODATA.
  - `RESOURCE_FLUSH=0x0104` `{hdr; rect r; u32 resource_id; u32 pad}` → OK_NODATA (handler lives in phase 03).
  - `TRANSFER_TO_HOST_2D=0x0105` `{hdr; rect r; u64 offset; u32 resource_id; u32 pad}` → OK_NODATA.
  - `RESOURCE_ATTACH_BACKING=0x0106` `{hdr; u32 resource_id; u32 nr_entries}` + `nr_entries × {u64 addr; u32 length; u32 pad}` → OK_NODATA.
  - `rect = {u32 x; u32 y; u32 width; u32 height}`.
- **Format (red-team C1 — corrected):** accept **format 1 (`B8G8R8A8_UNORM`) AND format 2
  (`B8G8R8X8_UNORM`)**. The Linux virtio-gpu DRM driver maps DRM's default `XRGB8888` to **format 2**
  (`virtio_gpu_translate_format`), so a format-1-only VMM returns `ERR_INVALID_PARAMETER` on the very
  first `RESOURCE_CREATE_2D` and nothing ever renders. Both formats have host byte order B,G,R,{A|X}
  == compositor `PixelFormat::Bgra8888` — treat the X byte as opaque alpha, **no per-pixel swap**.
  Formats 3 (`X8R8G8B8`) / 4 (`A8R8G8B8`) are rejected-by-design (would need a swap) unless research
  shows the guest actually negotiates them. Reject any other format with `ERR_INVALID_PARAMETER=0x1205`.
- **Resource table keyed by id (red-team M3):** `BTreeMap<u32, Resource>` — NEVER a Vec indexed by
  resource_id, or a hostile `CREATE_2D(id=0xFFFFFFFF)` forces a ~4-billion-entry allocation → OOM.
  Cap the number of distinct live resources independently of the host-pixel byte budget (allow ≥2
  scanout resources so guest double-buffering/page-flip is not rejected — Xorg modesetting needs ≥2).
- **ATTACH_BACKING replaces (red-team M2):** per spec, RESOURCE_ATTACH_BACKING REPLACES any prior
  backing for the resource (drop the old sg-list), it does not append. Cap cumulative entries.
- **One host copy of scanout pixels lives in the Grant (red-team C5):** there is NO separate
  `host_pixels` buffer for the scanout resource. The resource table stores only the backing sg-list
  (small pointers into guest RAM). The single guest→host copy of the scanout happens directly into
  the owned Grant (at SET_SCANOUT for the initial full copy, and at TRANSFER_TO_HOST_2D for rect
  updates — see Architecture). Only the small cursor resource (≤64×64×4) keeps a bounded host buffer.
- **Fence:** synchronous model — copy `flags` + `fence_id` straight from request hdr into response
  hdr; no async fence engine needed.
- Error responses (`ERR_INVALID_RESOURCE_ID=0x1203`, `ERR_INVALID_PARAMETER=0x1205`,
  `ERR_OUT_OF_MEMORY=0x1201`) are header-only and handled by the driver as command failure — the
  correct hostile-guest reply, never a VMM panic.

## Requirements
**Functional**
1. Host-side resource table: `BTreeMap<u32, Resource>` (M3), `Resource = { width, height, format,
   backing: Vec<MemEntry> }` — **no `host_pixels`** for the scanout (C5); the sole scanout pixel copy
   lives in the phase-03 Grant. Cap distinct live resources (count cap, allow ≥2). The small cursor
   resource may keep a bounded (≤64×64×4) host buffer.
2. Parse + service CREATE_2D, UNREF, ATTACH_BACKING, TRANSFER_TO_HOST_2D, SET_SCANOUT on controlq;
   write correct response hdr (type + echoed flags/fence_id) into the response descriptor.
3. TRANSFER_TO_HOST_2D of the **bound scanout resource** copies the requested rect from guest backing
   pages (walk the sg-list, honor `offset`) **directly into the scanout Grant** (checked arithmetic,
   M4). SET_SCANOUT does the initial full guest-backing→Grant copy. There is exactly ONE host copy of
   the scanout pixels, in the Grant (reconciled with phase 03: FLUSH is DamageNotify-only, no copy).
4. Track the current scanout→resource binding; invalidate it (M1) on RESOURCE_UNREF of the bound
   resource, on virtio device reset (status write→0 surfaced from `VirtioMmio` to the GPU model), and
   on SET_SCANOUT rebind. Refuse UNREF of the live scanout resource. SET_SCANOUT `resource_id=0` =
   disable scanout → return `OK_NODATA`, stop flushing. FLUSH/transfer with no valid binding →
   `ERR_INVALID_RESOURCE_ID`. Re-derive dims from the currently-bound resource (no cached geometry).
5. ATTACH_BACKING REPLACES any prior backing for the resource (M2); cap cumulative entries.
6. `inject_irq(vm_id, vcpu_id, 19)` after draining the queue (mirror virtio_blk.rs:72),
   **independent of any compositor send** (M5).

**Non-functional**
- Split across `virtio_gpu.rs` (device trait + dispatch) + `virtio_gpu/command.rs` (parse/encode
  structs) + `virtio_gpu/resource.rs` (table + transfer copy). Each < 200 LOC.
- **Host-pixel byte budget (C5):** define an explicit total budget of ≤~5 MiB across all host pixel
  copies (the ~3 MiB scanout Grant + the ~16 KiB cursor buffer), enforced before any allocation —
  replaces the vague "1 scanout + 1 cursor" count. Verify the real cell heap size against blk's
  4 MiB scratch baseline (virtio_blk.rs:15-19); 4 MiB (blk) + ~3 MiB (Grant) ≈ 7 MiB < ~8 MiB heap.
  If tight, shrink the blk scratch. Allocate the Grant with `try_reserve`; on failure return
  `ERR_OUT_OF_MEMORY` — **never an infallible `vec!`** (which aborts the cell → triggers C6).

## Architecture
Data flow: `notify(0, qcfg)` → `process_notify` walks each avail chain → `handle(bufs)` reads the
24-B hdr from `bufs[0]`, dispatches on `type`, reads the command body, mutates the resource table,
writes the response hdr into the last writable desc, returns bytes written. The single guest→host
copy of the scanout goes straight into the Grant (created in phase 03, ensured at SET_SCANOUT).

```
controlq chain: [ req desc (readable) | resp desc (writable, >=24B) ]
  handle(): parse hdr.type → body → mutate ResourceTable (BTreeMap) → write resp hdr → return n
SET_SCANOUT(res): validate res dims == Grant dims (or realloc+re-ATTACH, C7a); ensure Grant;
  full copy guest backing → Grant (checked); bind scanout→res.
TRANSFER_TO_HOST_2D(bound res): for each backing entry covering the rect (checked_add/checked_mul):
  read_guest_memory(vm_id, entry.addr + off, dst) with INDEPENDENT src/dst strides, each row
  bounded by grant_len AND grant width; check read_guest_memory != usize::MAX (M4).
FLUSH(bound res): DamageNotify only — pixels already in the Grant (phase 03); no second copy.
```
Ordering note: the guest sequence is CREATE_2D → ATTACH_BACKING → SET_SCANOUT → {TRANSFER → FLUSH}*,
so SET_SCANOUT (which ensures the Grant + initial copy) precedes any scanout TRANSFER. A TRANSFER to a
resource that is not the bound scanout (and is not the cursor) is a no-op on the Grant until it is set
as scanout.

## Related Code Files
- **Create:** `cells/services/hypervisor/src/virtio_gpu/command.rs` (req/resp struct codecs),
  `cells/services/hypervisor/src/virtio_gpu/resource.rs` (ResourceTable + transfer copy).
- **Modify:** `cells/services/hypervisor/src/virtio_gpu.rs` (real controlq dispatch replacing stub).

## Implementation Steps
1. `command.rs`: constants (types, resp codes, **formats {1,2}**) + `parse_hdr`, per-command body
   parsers, `write_resp_hdr(bufs_last, type, flags, fence_id)`, and the shared 280 B
   `resp_display_info` encoder (also used by phase 01).
2. `resource.rs`: `ResourceTable` as `BTreeMap<u32, Resource>` with
   create/unref/attach_backing(replace)/set_scanout/transfer_to_host; distinct-resource count cap;
   `set_scanout` ensures the Grant + full initial copy; `transfer_to_host` does the checked,
   independent-stride, rect-bounded guest→Grant copy for the bound resource; scanout-binding
   invalidation on UNREF/reset/rebind; `resource_id=0` disable.
3. `virtio_gpu.rs`: in `notify(0,...)`, `process_notify(|bufs| dispatch(&mut table, bufs, vm_id))`;
   dispatch returns response byte count; then `inject_irq(...,19)` unconditionally (M5).
4. Surface a device-reset signal (status write→0) from `VirtioMmio` to the GPU model so M1 can
   invalidate the scanout binding + release the Grant.
5. Guard: `notify(1,...)` (cursorq) drained but no-op (phase 04); still push used buffers (handled
   by process_notify) so the ring frees.
6. Boot Alpine; run `modetest` (or a tiny DRM dumb-buffer test); confirm CREATE/ATTACH/TRANSFER/
   SET_SCANOUT all return success in guest, no VMM error logs, cell not OOM-killed.

## Todo List
- [x] command.rs: bounded wire codecs + response writer + full display-info encoder (formats {1,2})
- [x] resource.rs: BTreeMap ResourceTable + checked guest→Grant transfer + binding invalidation
- [x] virtio_gpu.rs: controlq dispatch + inject_irq(19) independent of send
- [x] device-reset (status→0) surfaced from VirtioMmio → invalidate binding + release Grant
- [x] cursorq dispatch drains and publishes used entries
- [ ] Alpine: modetest CREATE/ATTACH/TRANSFER/SET_SCANOUT succeed, no OOM

## Success Criteria
- Guest issues the full CREATE_2D → ATTACH_BACKING → TRANSFER_TO_HOST_2D → SET_SCANOUT sequence
  (with format 1 OR 2) and each returns OK; `dmesg` shows no virtio-gpu ring timeout; the VMM cell
  stays alive. This is the serial-observable milestone (no display needed → validatable before P00).
- A host-side unit-style trace (debug log of resource table after SET_SCANOUT) shows correct
  width/height/format, the bound scanout resource id, and a populated scanout Grant.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Host pixel copies exceed ~8 MiB cell heap → OOM-kill / infallible-`vec!` abort (cf. virtio_blk.rs:15-19) | H×H | Explicit ≤~5 MiB host-pixel byte budget (1 scanout Grant ~3 MiB + cursor ~16 KiB); `try_reserve` → `ERR_OUT_OF_MEMORY`, never infallible `vec!`. 2+ scanout *resources* are allowed (backing sg-lists only, cheap) — only ONE host Grant. Shrink blk scratch if the heap is tight. |
| Vec-indexed table + hostile `CREATE_2D(id=0xFFFFFFFF)` → ~4 B-entry alloc | H×H | `BTreeMap<u32,Resource>` keyed by id (M3); never index by resource_id. Independent distinct-resource count cap. |
| Malformed nr_entries / huge length overflows on copy | M×H | `nr_entries` capped; each length validated; `checked_add`/`checked_mul` for every offset; `offset + rect_bytes ≤ Σ backing lengths` and `dst_end ≤ grant bytes` (M4); overflow → `ERR_INVALID_PARAMETER`. |
| `read_guest_memory` returns `usize::MAX` (bad GPA) but caller treats it as a length | M×H | Check `got == usize::MAX` (and `got == 0`) and abort the copy (mirror virtio_blk.rs:127). |
| Response descriptor smaller than 24 B (hostile/buggy guest) | L×M | If `bufs.last().len < 24`, write nothing and return 0; ring still frees the slot. |
| process_notify writes one used entry per chain but GET_DISPLAY_INFO resp is larger | L×M | Return the real byte count (24 for NODATA, 280 for display_info); truncate to `bufs.last().len` defensively. |

## Security Considerations (hostile-guest boundary #1 — parse/transfer)
Every guest-controlled field is validated before use:
- `resource_id` looked up in the `BTreeMap`; miss → `ERR_INVALID_RESOURCE_ID`. Never a Vec index (M3).
- `format` must equal 1 (BGRA) or 2 (BGRX, X = opaque alpha); else `ERR_INVALID_PARAMETER` (C1).
- `width*height*4` computed with `checked_mul` and checked against the host-pixel byte budget before
  ensuring the Grant.
- ATTACH_BACKING: REPLACES prior backing (M2); `nr_entries` capped; cumulative entries capped; each
  `{addr,length}` used only through `vmm::read_guest_memory` (kernel does GPA→HVA + bounds, returns
  `usize::MAX` on bad GPA — checked, never a raw deref, Law 4).
- TRANSFER rect `{x,y,w,h}` + `offset`: every src/dst offset via `checked_add`/`checked_mul`;
  `offset + rect_bytes ≤ Σ backing lengths` and `dst_end ≤ grant bytes` before copying; INDEPENDENT
  src/dst strides, each row bounded by BOTH grant_len and grant width; dst stride never derived from
  the guest resource width (C7a); overflow → `ERR_INVALID_PARAMETER`.
- Scanout binding invalidated on UNREF/reset/rebind (M1); UNREF of the live scanout refused; FLUSH
  with no valid binding → `ERR_INVALID_RESOURCE_ID`.

## Next Steps
Phase 03 adds the Grant + surface handshake and the RESOURCE_FLUSH handler. Because the single scanout
copy already lands in the Grant here (at SET_SCANOUT / TRANSFER), phase-03 FLUSH is DamageNotify-only
(no second copy). Phase 04 adds the cursor (GET_DISPLAY_INFO's real payload already landed in phase
01/02).
