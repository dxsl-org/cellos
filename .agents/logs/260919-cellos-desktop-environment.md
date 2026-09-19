# 2026-09-19 - CellOS Desktop Environment with Taskbar and Spotlight Search

## Why
CellOS needed a lightweight, responsive desktop environment cell providing a persistent bottom taskbar, quick application discovery and launching (Spotlight search modal), pinned application navigation with overflow handling, and real-time system tray status indicators. The environment operates entirely in user space under Tier 1 Pure Rust SAS (`#![forbid(unsafe_code)]`, `#![no_std]`), integrating with `service-compositor` without ambient lifecycle privilege escalation.

## What landed
1. **Desktop Cell (`cells/apps/desktop/`)**:
   - **Taskbar (`src/taskbar.rs`)**: 40px bottom bar with `CellOS` branding button, `? Search` button, pinned apps (`Terminal`, `Dashboard`, `ViUI Counter`, `Tetris`) with `<` and `>` pager navigation and overflow management for `> MAX_VISIBLE_APPS` (4), system tray daemon status (`[NET]`, `[VFS]`, `[AI]`), and monotonic clock (`HH:MM`).
   - **Spotlight Search (`src/spotlight.rs`)**: 500x280 floating modal window centered on screen with instant substring search across application catalog, keyboard (`Up`/`Down`, `Enter`, `Esc`) and mouse selection, `[Open]` app launching via `sys_spawn_from_path`, and taskbar pin/unpin toggling (`[+Pin]` / `[Unpin]`). Global hotkeys: `F1` or `Ctrl + Space`.
   - **Drawing Primitives (`src/draw.rs`)**: 2D bitmap drawing pipeline on `ViSurface` (BGRA8888) with rounded borders, accent color themes, and bitmap typography.
   - **Entry Loop (`src/main.rs`)**: connects to `service-compositor`, validates screen resolution, sets up taskbar and spotlight surfaces, requests input focus, and routes compositor pointer and keyboard events using surface-local coordinates.

2. **Kernel Security & Launch Profile Integration**:
   - Configured `desktop_profile` in `kernel/src/loader/launch_profile/profiles.rs` and `targets.rs` granting reviewed user application launch rights without ambient `SpawnCap`.
   - Configured `/bin/desktop` in `kernel/src/loader/boot_ceiling.rs` and signed operator policy in `scripts/sign-policy.py`.
   - Hardened `sys_get_resolution` in `libs/ostd/src/syscall.rs` to clamp negative error returns to `(1280, 800)`.
   - Configured `init` orchestrator in `cells/tools/init/src/boot.rs` and `gen_disk.ps1` for automatic packaging.

3. **Verification**:
   - Clean cross-compilation across all 3 architectures (`riscv64gc-unknown-none-elf`, `aarch64-unknown-none-softfloat`, `x86_64-unknown-none`).
   - Zero clippy warnings with `-D warnings` and clean `cargo fmt`.
   - `python3 scripts/cellos-sign --check` passes F1/F5 policy checks (91 crates, 604 files).
   - New integration test `tests/integration/tests/desktop-shell.rs` passes 100% in QEMU: verifies screen capture, bottom taskbar scanout, mouse pointer activation, and Spotlight modal interaction.
