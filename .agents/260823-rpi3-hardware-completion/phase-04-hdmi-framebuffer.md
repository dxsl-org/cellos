---
phase: 4
title: "Secure RPi3 HDMI Mailbox and Scanout"
status: in-progress
priority: P1
effort: "5d"
dependencies: []
tier: thinking
---

# Phase 04: Secure RPi3 HDMI Mailbox and Scanout

> **Required — deviation-log:** Record each Decision / Deviation / Surprise when it occurs. Choose the smallest reversible response; escalate irreversible or contract-breaking changes.

## Context Links

- **VERIFIED:** RPi3 init now selects `/bin/bcm-display` while generic AArch64 retains `/bin/virtio-gpu` (`cells/tools/init/src/boot.rs:4`, `cells/tools/init/src/boot.rs:35`).
- **VERIFIED:** RPi3 packaging builds and installs BCM display, compositor, and fb-console artifacts (`scripts/build-aarch64-cells.ps1:132`, `scripts/build-aarch64-cells.ps1:181`); netboot guards pin this behavior (`tools/rpi3-netboot/test-netboot-scripts.ps1:95`).
- **VERIFIED:** The current mailbox transport allocates and frees one `DmaBuf` per call, including after a post-submit timeout (`cells/drivers/bcm-display/src/mailbox.rs:32`). This is unsafe because a late VideoCore response can target recycled frames.
- **VERIFIED:** The driver converts the VideoCore alias to a raw physical pointer and later dereferences it from EL0 without kernel validation (`cells/drivers/bcm-display/src/lib.rs:84`, `cells/drivers/bcm-display/src/lib.rs:135`).
- **VERIFIED:** Physical boot reaches mailbox MMIO but fails framebuffer allocation; HDMI remains black. The connected-board framebuffer range and geometry are therefore still **[UNVERIFIED]**.

## Overview

Implement the chosen **A+ architecture**: two grant-owned cache-sync syscalls with exact pin lifecycle, one persistent mailbox DMA page, a dedicated display-framebuffer registration syscall, and a narrowly privileged BCM driver. The kernel validates authority and metadata; the trusted Tier-1 display cell performs scanout writes under the existing SAS/LBI model. No generic cache-maintenance or physical-map interface is introduced.

## Key Insights

- **VERIFIED:** `DmaBuf` is a contiguous identity-mapped `GrantAlloc` allocation and requires explicit free (`libs/ostd/src/dma.rs:16`, `libs/ostd/src/dma.rs:62`). `GrantFree` refuses pinned ranges (`kernel/src/task/syscall.rs:4865`), and task death already moves pinned grant frames to quarantine (`kernel/src/task/syscall.rs:233`, `kernel/src/memory/pin.rs:386`).
- **VERIFIED:** AArch64 already exposes PoC clean, invalidate, and clean-invalidate primitives (`hal/arch/arm/src/aarch64/cache.rs:56`, `hal/arch/arm/src/aarch64/cache.rs:77`, `hal/arch/arm/src/aarch64/cache.rs:98`). Their fixed 64-byte line assumption must be reviewed against the existing CTR-derived instruction-cache implementation (`hal/arch/arm/src/aarch64/cache.rs:18`) before reuse.
- **VERIFIED:** The pin registry is a bounded leaf lock and tracks owner, exact page span, hold count, and quarantine (`kernel/src/memory/pin.rs:28`, `kernel/src/memory/pin.rs:50`, `kernel/src/memory/pin.rs:113`). Its current `acknowledge(tid)` clears every pin for an owner (`kernel/src/memory/pin.rs:503`), so cache-sync completion needs an exact operation identity rather than owner-wide acknowledgement.
- **VERIFIED:** Cellos Tier 1 is a shared SAS with LBI, not per-cell MMU isolation (`docs/system-architecture.md:142`, `kernel/src/memory/paging.rs:208`). A USER framebuffer PTE cannot honestly be claimed as owner-only; registry/capability checks and the trusted unsafe allowlist are the boundary.
- **VERIFIED:** Existing GPU registration has exactly two callers: VirtIO (`cells/drivers/virtio-gpu/src/main.rs:69`) and BCM (`cells/drivers/bcm-display/src/main.rs:49`). Existing `GpuFlush` forwards kernel-origin IPC with sender TID 0 (`kernel/src/task/syscall.rs:4110`, `kernel/src/task/syscall.rs:4133`), and VirtIO already rejects non-kernel senders (`cells/drivers/virtio-gpu/src/main.rs:78`).

## Requirements

- **Functional:** mailbox negotiation returns a validated framebuffer; `GpuGetResolution` reports the negotiated width/height; compositor flushes reach HDMI; fb-console and software cursor are visible.
- **DMA lifetime:** allocate one mailbox page once, reuse it only after an exact successful completion, and never free/complete it after a post-submit timeout. A poisoned request forces cell exit; task reaping quarantines the still-pinned page until reboot.
- **Cache coherency:** begin is owner-checked, bounds-checked, pins before making the request device-visible, and performs bidirectional clean/invalidate. Complete is token- and owner-checked, invalidates before CPU parsing, then releases only that exact operation pin.
- **Framebuffer authority:** only a `DEV_DISPLAY` holder that owns the BCM mailbox window may register one framebuffer. Validate address conversion, page alignment, overflow, allocator/peripheral bounds, width, height, pitch, and byte coverage before exposing the range as USER|DEVICE.
- **IPC/allowlist:** BCM accepts flush messages only from kernel sender TID 0 and declares the minimal syscall set it actually invokes. New display/cache syscalls receive explicit allowlist bits; absent `__ViCell_syscalls` remains backward-compatible permit-all as currently documented (`libs/api/src/abi/syscall.rs:577`).
- **Compatibility:** changes are additive. VirtIO keeps its existing registration, flush/cursor IPC, build selection, and syscall behavior; no ViSurface, Grant sharing, compositor message, or generic `RequestMmio` contract changes.

## Architecture

```text
bcm-display startup
  -> claim exact mailbox MMIO with DEV_DISPLAY
  -> allocate one persistent Grant/DmaBuf page
  -> encode property request into owned page
  -> wait mailbox-not-full (nothing submitted yet)
  -> GrantCacheSyncBegin(grant, offset, length)
       grant table: prove caller owns exact live range
       pin registry: reserve exact operation token
       AArch64: clean+invalidate range to PoC
  -> submit VC bus alias
  -> matching response
  -> GrantCacheSyncComplete(token)
       prove same caller/token; invalidate stored range; release exact pin
  -> parse response
  -> RegisterDisplayFramebuffer(base, size, packed_width_height, pitch)
       require DEV_DISPLAY + caller-owned mailbox window
       validate exact geometry/range; register one active display; USER|DEVICE map
  -> existing sys_register_gpu_driver()

post-submit timeout/MMIO uncertainty
  -> do not complete, reuse, or free mailbox page
  -> mark transport poisoned and exit
  -> grant reaper transfers pinned frames to boot-lifetime quarantine

kernel GpuFlush -> sender_tid=0 IPC -> bcm-display -> validated framebuffer
```

### Cache-sync state and lifetime

- Add planned ABI operations `GrantCacheSyncBegin(grant_id, offset, len) -> token` and `GrantCacheSyncComplete(token)`. Names/opcodes are **[PLANNED/UNVERIFIED]** until assigned in `ViSyscall`; choose unused opcodes and unused allowlist bits after a full enum scan.
- Store an exact active operation entry inside the existing pin registry transaction: token/generation, grant base, byte offset/length, page span, and owner TID. The bounded table and per-owner ceiling remain load-bearing (`kernel/src/memory/pin.rs:50`).
- Begin lock order is `PAGE_GRANT_TABLE -> pin REGISTRY`; all locks are released before cache maintenance. If cache maintenance cannot complete, roll back the exact newly-created pin before returning because no device submission has occurred.
- Complete looks up the immutable stored range by token and owner, invalidates it, and atomically removes only that operation. Duplicate, foreign, stale, zero, overflow, or mismatched completion fails closed.
- Once mailbox data is submitted, timeout is an indeterminate device-ownership state. The driver intentionally leaks the active pin and exits; neither timeout nor a timer may release it. Reboot is the only recovery boundary.

### Display registration and SAS boundary

- Add planned ABI operation `RegisterDisplayFramebuffer(base, size, packed_width_height, pitch)` **[PLANNED/UNVERIFIED]**. It is separate from `RequestMmio`: framebuffer RAM is not a peripheral allowlist entry, and overloading MMIO would widen that public contract.
- Require `DEV_DISPLAY` and exact ownership of the RPi3 mailbox window in `resource_registry`. Add a read-only exact-owner query; do not add an unchecked or generic physical-range mapper.
- Strip/validate the VideoCore alias in the BCM cell, then kernel-check: nonzero fields; `base` page alignment; checked `end = base + size`; `width,height > 0`; bounded dimensions; `pitch >= width * 4`; checked `pitch * height <= size`; `base >= FRAME_ALLOCATOR.memory_end()` (`kernel/src/memory/frame.rs:235`); and `end <= BCM2837.mmio.peripheral_base` (`hal/soc/bcm27xx/src/profile.rs:50`). Reject instead of widening if hardware contradicts the assumed GPU-reserved interval.
- Map the page-rounded exact range USER|DEVICE in the shared AArch64 root and record one registered display owner/geometry for cleanup and `GpuGetResolution`. This prevents accidental arbitrary mapping through the syscall but is **not owner-only hardware isolation**: all trusted Tier-1 cells share the USER mapping. Document this limitation next to the mapper and keep framebuffer dereference confined to the reviewed BCM unsafe allowlist.
- Registration is single-owner/single-range per boot. Same-owner identical replay may be idempotent; mismatched replay or another owner fails. Cell exit clears logical registration and driver role, but the firmware framebuffer allocation remains until reboot and must not be returned to the frame allocator.

## Related Code Files / Ownership and Dependencies

- **Lane A — public ABI and wrappers (exclusive ownership):** `libs/api/src/abi/syscall.rs`, `libs/api/src/abi/syscall_tests.rs`, `libs/ostd/src/syscall.rs`, `libs/ostd/src/dma.rs`. Defines only the two cache-sync operations and one display-registration operation.
- **Lane B — cache/pin kernel path (exclusive ownership, parallel with Lane C after Lane A contract):** `kernel/src/memory/pin.rs`, `kernel/src/memory/pin_tests.rs`, `kernel/src/task/syscall.rs`, `hal/arch/arm/src/aarch64/cache.rs` and colocated tests. Must not edit display resource policy.
- **Lane C — framebuffer authority/mapping (exclusive ownership, parallel with Lane B after Lane A contract):** `kernel/src/resource_registry.rs`, `kernel/src/memory/paging.rs`, `kernel/src/task/syscall.rs` only in separately assigned display-registration sections, plus colocated tests. Coordinate non-overlapping hunks in `syscall.rs` before parallel execution.
- **Lane D — BCM cell (after B and C):** `cells/drivers/bcm-display/src/mailbox.rs`, `cells/drivers/bcm-display/src/lib.rs`, `cells/drivers/bcm-display/src/main.rs`, `cells/drivers/bcm-display/Cargo.toml` if imports require it.
- **Lane E — integration only (after D):** `tools/rpi3-netboot/test-netboot-scripts.ps1` for static guards if needed; no production ownership. Existing boot/package files are verification-only because Lane A packaging is already present (`cells/tools/init/src/boot.rs:4`, `scripts/build-aarch64-cells.ps1:132`).

## Implementation Steps

1. **Freeze additive ABI (Lane A):** allocate unused raw opcodes and allowlist bits; update enum conversion, dispatcher mapping, syscall-set tests, rustdoc, and ostd wrappers. Enumerated new callers: BCM mailbox alone calls Begin/Complete; BCM startup alone calls RegisterDisplayFramebuffer. Existing VirtIO callers remain unchanged.
2. **Implement exact cache-sync lifecycle (Lane B):** extend pin-registry state with exact tokens; validate PageGrant ownership/range while following grant-table-to-leaf lock order; wire begin/complete to AArch64 PoC operations; provide a safe unsupported-target result rather than a silent no-op. Never expose arbitrary virtual addresses or user-selected cache instructions.
3. **Implement dedicated display registration (Lane C):** add exact mailbox-owner lookup, DEV_DISPLAY gate, checked geometry/reserved-range validation, one-display state, resolution update, shared-root USER|DEVICE mapping, and teardown semantics. Explicitly document trusted-SAS visibility.
4. **Make mailbox ownership persistent (Lane D):** construct `BcmMailbox` with one page; encode/wait/begin/submit/wait/complete/parse in that order; poison and exit on any uncertainty after submit; complete before returning malformed-response errors only when the matching response proves device completion.
5. **Harden BCM boundary (Lane D):** call the dedicated framebuffer syscall before storing/dereferencing the framebuffer; add `api::declare_syscalls!` with only Recv (required by `run_app!`, `libs/ostd/src/app.rs:160`), Log, RequestMmio, GrantAlloc, cache Begin/Complete, and RegisterDisplayFramebuffer. `RegisterService`/exit/yield remain always permitted and dispatch-gated (`libs/api/src/abi/syscall.rs:759`); match `AppEvent::Message { sender_tid: 0, .. }` like VirtIO.
6. **Verify and review (Lane E):** run unit/build/compatibility matrices; assign `haily-tester` final-code verification and `haily-reviewer` production/security review. Resolve all High findings and rerun affected gates before creating a physical image.
7. **Physical gate:** deploy only a reviewed image; keep USB-TTL TX disconnected from Pi pin 10 during boot; capture mailbox token lifecycle, returned geometry/range, registration, resolution, and compositor markers; visually verify stable fb-console and moving software cursor.

## Test Matrix

- **ABI:** unique opcode/from mapping; unique/stable allowlist bits; `declare_syscalls!` permits exactly the BCM list; older cells without the new calls still build and run.
- **Grant sync begin:** valid owned subrange; non-owner; unknown grant; zero length; offset/end overflow; beyond logical/allocated size; table/per-owner exhaustion; duplicate begin; unsupported architecture; cache-maintenance failure rollback.
- **Grant sync complete:** matching token invalidates then releases exact pin; foreign/stale/zero/duplicate token rejected; completion does not clear another active pin; GrantFree refused while active; task death quarantines a submitted page; timeout path has no completion/free/reuse.
- **Display registration:** valid reserved range; no DEV_DISPLAY; no mailbox ownership; zero/unaligned base; zero/overflowing size; allocator overlap; peripheral overlap; width/height zero or above cap; `width*4` overflow; short pitch; `pitch*height` overflow/over-size; duplicate/mismatched owner; partial mapping rollback; resolution state only changes on success.
- **BCM unit:** one allocation per mailbox lifetime; wait-full timeout before begin is reclaimable; post-submit timeout poisons transport; matching response completes before parse; wrong channel/address does not complete; malformed completed response is rejected; non-kernel IPC ignored; flush remains pitch- and bounds-safe.
- **Build/static:** `cargo fmt --all --check`; host tests for `api` and `driver-bcm-display`; RPi3 kernel and driver checks for `aarch64-unknown-none-softfloat`; policy signer/self-tests; `pwsh -NoProfile -File scripts/build-aarch64-cells.ps1 -BoardRpi3`; `pwsh -NoProfile -File tools/rpi3-netboot/test-netboot-scripts.ps1`.
- **Compatibility:** generic AArch64 init still selects VirtIO; VirtIO registration, kernel-sender-only flush/cursor, QEMU boot, compositor software fallback, and existing GrantDma/VFS pin lifecycle tests pass unchanged.
- **Physical:** serial reports one mailbox page, successful begin/complete, validated base/size/pitch/resolution, GPU registration, compositor fallback, and no cell fault; HDMI stays lit for 10 minutes and visibly shows fb-console plus cursor movement.

## Success Criteria / Todo List

- [x] RPi3 packaging contains BCM display, compositor, and fb-console; generic selection remains VirtIO.
- [ ] One persistent mailbox page is allocated; success reuses it only after exact completion, while post-submit timeout causes exit and boot-lifetime quarantine.
- [ ] Cache Begin/Complete prove grant ownership and exact bounds, order PoC maintenance correctly, and cannot release unrelated pins.
- [ ] RegisterDisplayFramebuffer rejects every malformed/unauthorized case in the matrix before changing mapping, owner, or resolution state.
- [ ] Documentation and tests state that framebuffer USER mapping is shared-SAS/trusted-LBI, not owner-only PTE isolation.
- [ ] BCM embeds a minimal syscall allowlist and ignores every non-kernel GPU IPC sender.
- [ ] Additive VirtIO and generic AArch64 regression gates pass with no ABI/protocol changes.
- [ ] Reviewer reports no unresolved High/Critical finding; physical serial gate passes and HDMI remains visibly stable for 10 minutes.

## Assumptions / Blockers

- **[UNVERIFIED physical blocker]:** RPi3 firmware returns a 4-KiB-aligned ARM framebuffer range wholly in `[FRAME_ALLOCATOR.memory_end(), BCM2837.mmio.peripheral_base)`. Verify by logging rejected raw values on the connected board; if false, stop and derive a DTB/firmware-reserved aperture rather than weakening bounds.
- **[UNVERIFIED hardware blocker]:** PoC cache maintenance plus the VC bus alias is sufficient for BCM2837 mailbox coherency. Verify with a canary-filled page and matching response on hardware; stale words block framebuffer registration and require architecture evidence, not longer polling.
- **[UNVERIFIED ABI assignment]:** exact opcodes and allowlist bits for the three operations remain to be selected after re-grepping the complete current enum. No existing opcode/bit may be repurposed.
- Existing `GpuFlush` forwards a raw SAS pointer (`kernel/src/task/syscall.rs:4116`); this systemic trusted-SAS limitation is compatibility-preserved, not solved by this phase.
- HDMI must be powered and connected before boot. SD repartitioning and the independent SD write-transfer timeout are outside this phase.

## Risk Assessment, Security, and Rollback

- **Top risk — late firmware DMA:** rollback is to stop launching BCM and restore the last UART/SD-safe image. A page already submitted then timed out cannot be safely reclaimed; it remains quarantined until reboot. This intentional boot-lifetime memory loss is the non-reversible part of a running boot.
- **Cache ordering risk:** revert Lane B ABI/implementation together before shipping; no device request may be submitted without a successful Begin. Incorrect cache maintenance can yield stale/corrupt responses but does not justify a generic cache syscall.
- **Shared-SAS exposure:** capability checks prevent accidental registration but cannot create per-owner PTE visibility. Rollback removes the USER framebuffer mapping from future images; exposure during the current boot ends only at reboot. Only signed/reviewed trusted Tier-1 cells may coexist with this mapping.
- **ABI compatibility:** all three syscalls and allowlist bits are additive. Rollback removes BCM’s use and then the new ABI in reverse order. Never reinterpret an existing opcode/bit; co-package kernel, API, ostd, driver, and signed policy in one image.
- **Framebuffer allocation:** VideoCore allocation cannot be reclaimed by cell teardown with the current property protocol. Reboot is the recovery boundary; no persistent SD data is modified.

## Validation Log

- **Tier:** Light for this single phase; Fact Checker and Contract Verifier applied using `hc-plan/references/verification-roles.md`.
- **Claims checked:** 23; **Verified:** 20; **Failed:** 0; **Unverified:** 3 (physical range, hardware coherency, ABI number assignment).
- Re-grep on 2026-08-26 confirmed every cited existing path/symbol, both `sys_register_gpu_driver` callers, current broad pin acknowledgement, current stack-to-DmaBuf transport, RPi3 packaging, and VirtIO kernel-sender guard.
- Behavioral trace verified: compositor `GpuFlush` -> kernel `ipc_post_nonblock(0, gpu_cell, ...)` (`kernel/src/task/syscall.rs:4110`) -> driver `AppEvent::Message`; BCM currently discards sender identity (`cells/drivers/bcm-display/src/main.rs:58`) while VirtIO matches sender 0 (`cells/drivers/virtio-gpu/src/main.rs:78`).

## Deviation Log

- **Decision:** replace short-lived/free-on-timeout DMA with one persistent page plus exact pin token. **Why:** VideoCore may respond after the CPU timeout. **Impact:** ABI, pin registry, ostd, BCM mailbox. **Revert:** disable BCM image and reboot; a timed-out page remains quarantined for that boot.
- **Decision:** add three narrow syscalls instead of reusing `RequestMmio` or exposing generic cache/map operations. **Why:** grant ownership and framebuffer authority are distinct contracts. **Impact:** additive ABI only. **Revert:** remove BCM callers, then ABI additions.
- **Decision:** state trusted SAS explicitly; do not claim owner-only framebuffer PTEs. **Why:** the active architecture uses one shared address space (`docs/system-architecture.md:142`). **Impact:** security claim and review gate. **Revert:** none; hardware owner isolation requires a future private-domain architecture.
- **Decision:** preserve existing VirtIO behavior and make BCM sender filtering additive. **Why:** VirtIO is the generic target path and already enforces sender 0. **Impact:** BCM event match only. **Revert:** restore BCM handler, though doing so reopens forged scanout IPC.

## Next Steps

Run `$hc-cook .agents/260823-rpi3-hardware-completion/phase-04-hdmi-framebuffer.md`: freeze Lane A first; execute Lane B and Lane C in parallel with non-overlapping `syscall.rs` hunks; then Lane D, specialist test/review, and finally the connected-board physical gate.
