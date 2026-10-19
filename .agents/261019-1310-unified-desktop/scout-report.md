# Unified Desktop — source recon and blast radius

Planning-only recon, 2026-10-19. Source observations are not runtime qualification. Reused earlier session context, then three read-only scout slices; no build/tests/formatter run in research. `find` failed its judge requests (HTTP 402), so discovery fell back to grep/glob/read; failure is not evidence of absence.

## Project and patterns

Rust no_std Cells with kernel/HAL authority substrate, typed fixed/postcard IPC, signed images, VFS owner handles, ostd service clients and ViUI software surfaces. Intel x86-64 is current platform scope. GUI policy belongs in services/apps; don't move it into kernel. Existing user-owned working changes must be retained and content-pinned before implementation.

## Observed reusable contracts and gaps

| Area | Evidence | Reuse / missing work |
|---|---|---|
| Surface/grant lifecycle | `libs/api/src/services/display.rs:245-259,281-345,461-577,1038-1107`; `libs/ostd/src/display/{surface,lifecycle}.rs` | Fixed-wire title/configure/ACK/close/state; staged resize Grants. No view/frame/session contract. |
| Compositor ownership | `cells/services/compositor/src/surface_table/state.rs:17-69`, `message_handler.rs:15-68`, `surface_table/storage.rs:62-70` | Owner checks currently bare TID; needs incarnation-bound authority and stale-death protection. |
| Window actions | `surface_table/{configure,lifecycle}.rs`, `pointer_router/{control,resize}.rs`, `window_decoration.rs` | Real single-window drag/resize/min/max/close refusal. No group/min-size negotiation/work-area/six-zone/fullscreen. |
| Input/event demux | compositor `input_handler.rs:113-202`; `libs/ostd/src/display/{routing,events}.rs`; input.rs:108-152 | Reuse typed bounded demux; global F11/Alt+Space must intercept before focused app receives them. |
| ViUI runner | `libs/viui/src/managed_surface.rs:38-170` | One app/one surface adapter, not existing multi-session framework. |
| Theme | `libs/viui/src/theme.rs:9-81,85-171,173-316`; app_runner.rs:173,206,277 | Semantic trait and Dark/Light/Kiosk/Robot, local set_theme. No Paper/system publication or durable preference contract. |
| App palettes | Ocel `draw.rs:29-90`; Desktop `draw.rs:31-60`; viui elm/window hard-coded DARK_THEME | Native shared theme requires caller migration, not just adding a palette. |
| Config | `libs/api/src/services/ipc.rs:575-595`; `cells/services/config/src/main.rs:26-36,62-118` | Mutable in-memory Get/Set/List; no verified auth/subscription/durability. |
| Desktop | `cells/apps/desktop/src/{main,apps,spotlight,taskbar}.rs` | Two ordinary interactive surfaces, static app catalog, direct SpawnFromPath, pinned/demo status; no session inventory or Tasks/System sections. |
| Ocel | `cells/apps/ocel/src/main.rs:77-120,281-456,938-960,1060-1123` | Real app-private Vec<Tab>, resource/browser/history state, redundant tab strip to migrate. |
| VFS enumeration/stat | `libs/api/src/services/ipc.rs:41-44,133-145,291-302`; `cells/services/vfs/src/manager.rs:118-180` | 480-byte copied directory listing; size/is_dir metadata. Full large-directory paging and richer operation contract missing. |
| VFS rights | `cells/services/vfs/src/caller.rs:21-77`, dirs.rs:1-15,33-71, dirs/lifecycle.rs:49-84 | Cell/generation-owned directory/file handles, inheritance/path sealing; reuse, no TID-only namespace widening. |
| VFS client/mutation | `libs/ostd/src/clients/vfs.rs:25-70`; VFS dispatch/backends | Bounded reads, small inline writes, narrow rename; copy/move/search/Trash/Undo policies not complete. No crash-preserving Trash claim under current G2-J gate. |
| Preview | Ocel image/mod.rs:15-80; `libs/ocel-pdf-proto/src/lib.rs:8-64`; PDF main.rs:56-82 and c/renderer.c:8-15 | Reusable codecs/rendering; PDF one active document and non-preemptible C work. Files preview API and safe cancellation/arbitration missing. |
| Terminal | Desktop apps.rs:17-23; fb-console main.rs:1-4,55-101; shell async_utils/executor/shell_state | Catalog Terminal is log-only fb-console, shell uses global input/UART and sequential Vec capture; no GUI session I/O. |
| Pipe substrate | `libs/ostd/src/syscall.rs:352-447`, shell executor.rs:899-955 | Bounded pipes/share exist; atomic spawn stdio handoff, distinct session stderr, resize/cooperative interrupt need exact contract. |
| Guest launch | `cells/apps/vm-runner/src/main.rs:25-117`; chrome main.rs:17-46 | Log/lookup/exit stubs, not actual VM launch or surface. Manifest TOML not consumed. |
| Real VMM/display | hypervisor boot_x86.rs:52-168, run_loop_x86.rs:45-284; virtio_gpu/scanout.rs:16-184 | Real x86 PVH/device/scanout code; Background noninteractive global scanout, no app view or dynamic guest resize. |
| Intel substrate | `hal/arch/x86/src/hypervisor.rs:1-39`, vmx_vcpu.rs; controlling phase-09 guest-adapter | Source facade/registry/runtime provenance must be reconciled. Presence of VMX source/SVM smoke is not Intel execution proof. |
| Boot/signing | build-cloud-images.sh, build-x86_64-cells.ps1, make-hypervisor-fs-x86.sh, generate-boot-config.py | Different profiles package different consumers; whole source/image feature pins essential. |

## Precedents

- `3c3b38b7e` memory-profile desktop Ocel: touched kernel ELF/VA/admission/launch profiles, boot-config, init image, package script and cloud images. Blind-spot checklist: VA/heap/signing/config consistency, not only app source.
- `8c2c0d0ff` GUI input fix: touched compositor, Ocel, shell, PS/2/console kernel and embedded binaries. Blind-spot checklist: end-to-end focus across shell and GUI; don't independently fix only one caller.
- `3765ae897` x86 framebuffer/SSH: includes display resource registration/syscall, SSH session, image/script/policy/guest docs. Retain serial/SSH recovery and avoid claiming user-facing terminal from framebuffer logs.

## Prior failures / retained evidence

- `.agents/review-history.jsonl:29-30`: VFS poisoned pending cleanup and directory reads must not become successful empty data; listing/preview must propagate real errors.
- `:38-39`: timeout/late marker and boot lookalikes cannot establish pass; require bounded actual interaction evidence.
- `:40`: detached models/source-echo tests are not actual consumer behavior proof.
- `:41`: directory ID exhaustion must fail before mutation and preserve owner state.
- Earlier session observed signing refusal for missing QuickJS native_layout unsafe allowlist, old image disabled Tier2, later real QuickJS self-check and foreground Ocel exit. These are PRIOR observations of those exact artifacts, not current desktop/guest qualification. Earlier broad final claimed more than exercised (e.g. no proven compositor deadlock repro or full CSS resource success); do not inherit those claims.

## Verification entry points

- Existing integration `desktop-shell.rs` and `ocel-browser.rs` hardcode RV64 kernel at lines 29-43, not ready-made x86 evidence.
- `tests/integration/tests/{window-policy,viui-managed-surface,compositor-cursor,srv-cellosfs}.rs` and actual-module compositor/VFS/shell/Ocel tests are reusable patterns.
- `scripts/qemu-hypervisor-smoke-x86.sh` machinery only checks vCPU readiness; virtio-e2e/hostile tests do not prove GUI application operation.
- Plan adds a pinned x86 QMP desktop scenario path, plus actual Intel KVM and VMware procedure; unavailable prerequisite is explicit blocker, never successful skip.

## Blast radius and owner constraints

Compositor/session -> API display/services -> ostd event/Grant/input -> ViUI runners -> Ocel/Files/Terminal/Desktop; launch -> init/shell/catalog/policy/config; Files -> VFS/backends/client and PDF/codec leases; guest -> VMM/device/display/VMX and app payload packaging. Shared API/SDK and kernel files have one integrating owner; no new ambient authority to shortcut missing handoff.

## Inconsistencies to resolve, not silently repair

`docs/roadmap/current-focus.md:18-43` retains documentation-only freeze; controlling plan headline/table says complete while :64 and scoped authorization retain G2-J/frozen-wire gates and phase-09 acceptance is not qualified by header alone. New user request admits desktop planning; exact per-slice implementation/ABI ownership still needs explicit determination. Do not rewrite old evidence, resume locked legacy programs or claim global substrate completion. Normal shared consumer work is distinct from protected/physical/remote promotion.
