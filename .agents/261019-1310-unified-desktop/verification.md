# Verification handoff — commands and evidence ceilings

These are execution instructions, not commands run during planning. Root Cargo defaults to a bare-metal target; every host test must state its host target. Per-phase build/test selection occurs once after edits, not repeatedly during source mutation. Do not re-run a user-reported failure merely to establish that it exists.

## Reusable current commands

```sh
# Host protocol/toolkit behavior tests (confirm package host-test constraints in Recon)
cargo test -p api --target x86_64-unknown-linux-gnu
cargo test -p viui --target x86_64-unknown-linux-gnu

# Explicit cell PIC cross-build, not the kernel's static model
RUSTFLAGS="-C relocation-model=pic" cargo build --release \
  --target x86_64-unknown-none -Z build-std=core,alloc \
  -p desktop -p service-compositor -p app-shell
RUSTFLAGS="-C relocation-model=pic" cargo build --release \
  --target x86_64-unknown-none -Z build-std=core,alloc \
  -p ocel --no-default-features --features desktop

# Policy/signing check, then sign exact produced cells through the enforced signer
python3 scripts/cellos-sign --check
# Use scripts/cellos-sign --sign on each actual packaged artifact; no unchecked bypass.

# Existing cloud builder preserves a prepared embedded cell set; it does not
# currently build/package all desktop consumers. Extend that path in phase 01/11.
bash scripts/build-cloud-images.sh --ocel-profile desktop --javascript

# Existing x86 boot-only regression, not desktop UI qualification
BOOT_WINDOW=90 X86_ACCEL=tcg bash scripts/qemu-x86_64-test.sh build/cellos-x86_64.iso
BOOT_WINDOW=90 X86_ACCEL=kvm X86_CPU_MODEL=host \
  bash scripts/qemu-x86_64-test.sh build/cellos-x86_64.iso
```

TLS/public web browsing is not required for deterministic desktop fixtures. If qemu-dev-https is used, build the matching NET/kernel features and start its authenticated-time companion; never turn off verification to make UI evidence pass. Existing file/JS/PDF fixtures must be present in manifest and optional isolated services signed/admitted.

## Planned x86 desktop evidence command

Phase 01 materialized and verified the real x86 QMP integration target `desktop-x86` in `tests/integration/tests/desktop-x86.rs`:

```sh
# Materialized and verified runner contract (Phase 01):
CELLOS_DESKTOP_ISO=build/cellos-x86_64.iso \
CELLOS_DESKTOP_ACCEL=tcg \
cargo test --manifest-path tests/integration/Cargo.toml \
  --target x86_64-unknown-linux-gnu --test desktop-x86 -- --test-threads=1
```

The test target must fail on missing artifacts/prerequisites, expose command/profile pins, boot actual x86 image and drive real keyboard/pointer events. It records rendered output, session/control outcomes and file/guest state. Existing RV64 desktop-shell/ocel-browser test paths are not substituted as Intel evidence. KVM uses the same scenario/inputs with actual supported Intel virtualization; native guest scenarios name their extra payload/VMX prerequisites, not a fake skip.

## Phase 02 verification evidence (Executed 2026-10-19)

### 1. Host toolkit unit and contrast test suites
- **`libs/viui` Tests:** 64 passed, 0 failed, 7 ignored.
  - Command: `cargo test -p viui --target x86_64-unknown-linux-gnu`
  - WCAG 2.1 relative luminance and contrast ratio suite verified:
    - Normal text contrast $\ge 4.5:1$ across all palettes (Light: 17.5:1, Dark: 13.8:1, Paper: 12.1:1).
    - Reader hyperlink contrast $\ge 5.0:1$ across all palettes (Light: 6.92:1, Dark: 5.93:1, Paper: 6.04:1).
    - `WindowChrome` close hit target (20 DIP target, 16 DIP centered icon) verified.
    - Preference state monotonic revision enforcement (`ThemePreferenceState`) verified.
- **`cells/apps/ocel` Tests:** 170 passed, 0 failed.
  - Command: `cargo test -p ocel --target x86_64-unknown-linux-gnu`
  - Verified `ReaderThemeMode::{FollowSystem, Light, Dark, Paper}` theme cycling, dynamic system palette resolution via `active_system_palette()`, and syntax highlight color preservation for code tokens.
- **`cells/apps/desktop` Tests:** 1 passed, 0 failed.
  - Command: `cargo test -p desktop --target x86_64-unknown-linux-gnu`
  - Verified taskbar (36 DIP) and spotlight (480x320 DIP, MAX_RESULTS=3) layout geometry.

### 2. Workspace build and check
- **Clean workspace check:** `cargo check --workspace` passes cleanly with zero warnings or errors.

### 3. x86 QEMU Integration Test Runner
- **Command:**
  ```sh
  CELLOS_DESKTOP_ISO=build/cellos-x86_64.iso \
  CELLOS_DESKTOP_ACCEL=tcg \
  cargo test --manifest-path tests/integration/Cargo.toml \
    --target x86_64-unknown-linux-gnu --test desktop-x86 -- --test-threads=1
  ```
- **Result:** `test desktop_x86_boots_to_shell_and_compositor ... ok` (18.73s).
- **Observation:** Full image boots to graphical compositor surface, input subsystems initialize, and framebuffer remains stable without panics or memory corruption.

## Phase 03 verification evidence (Executed 2026-10-19)

### 1. Compositor Unit & Generational Lifecycle Test Suite
- **`cells/services/compositor` Tests:** 10 passed, 0 failed.
  - Command: `cargo test --target x86_64-unknown-linux-gnu -p service-compositor`
  - Tests verified:
    - `surface_table::frame::tests::test_multi_view_creation_and_frame_association`: Verified multi-view allocation per process, mixed-owner frame co-existence (`owner_tid`), opaque monotonically assigned handle mapping, and capacity boundary limits (`MAX_VIEWS_PER_FRAME = 32`, `MAX_FRAMES = 64`).
    - `surface_table::frame::tests::test_select_view_and_stale_generation_rejection`: Verified active view selection, focus synchronization, and strict generational rejection (`ViewOpResult::StaleGeneration`) on mismatched or forged frame generation or view incarnation handles.
    - `surface_table::frame::tests::test_group_and_detach_view`: Verified view detachment into independent floating frames at arbitrary coordinates `(x, y)` and re-grouping into target frames with proper frame generation increments.
    - `surface_table::frame::tests::test_group_view_same_frame_does_not_panic`: Verified self-frame grouping guard returns early success without panicking or corrupting state.
    - `surface_table::frame::tests::test_owner_crash_cleanup_and_focus_preservation`: Verified supervisor crash sweeper reclaims only dead process's views, advances frame generation, preserves surviving views from other processes in shared frames, and safely transitions input focus to the surviving active view.
    - `framebuffer::tests::clipped_blit_*`: Verified clipped pixel blits and source/destination clipping bounds.
    - `window_decoration::raster::tests::*`: Verified exact intersection computation and non-underflowing decoration layout.

### 2. ABI & Wire Struct Static Assertions
- **`libs/api` Tests:** 2 passed, 0 failed, 4 ignored.
  - Command: `cargo test --target x86_64-unknown-linux-gnu -p api`
  - Compile-time invariants verified:
    - `size_of::<ViewRef>() == 8`
    - `size_of::<FrameRef>() == 8`
    - `size_of::<ViewOpResponse>() == 20`
    - `size_of::<CreateViewRequest>() == 64` (with natural 8-byte alignment for `file_grant_id` at byte offset 32).
  - Unpack offsets and bounds checks in `cells/services/compositor/src/message_handler/view.rs` match public wire layout byte-for-byte.

### 3. Core Workspace Test Suite
- **Command:** `cargo test --target x86_64-unknown-linux-gnu -p api -p service-compositor -p viui -p ocel -p desktop`
- **Result:** 356 passed, 0 failed, 11 ignored (8 test suites, 0.00s).

### 4. Workspace Check
- **Command:** `cargo check --workspace`
- **Result:** Passes cleanly without compiler errors.

## Phase 04 verification evidence (Executed 2026-10-19)

### 1. Desktop & Spotlight Unit Test Suite
- **`cells/apps/desktop` Tests:** 2 passed, 0 failed.
  - Command: `cargo test --target x86_64-unknown-linux-gnu -p desktop`
  - Tests verified:
    - `spotlight::tests::test_spotlight_section_cycling_and_actions`: Verified 3-section Spotlight HUD navigation, Tab key cycling (`Applications` -> `RunningTasks` -> `System` -> `Applications`), Enter key selection, task switching action (`SpotlightAction::SwitchTask(1)`), theme cycling (`SpotlightAction::CycleTheme`), power actions (`SpotlightAction::Reboot`, `SpotlightAction::ShutDown`), and Escape key dismissal (`SpotlightAction::Close`).
    - `taskbar::tests::narrow_taskbar_more_opens_application_search`: Verified responsive taskbar layout across narrow widths `[600, 640, 700, 727]`, paged pinned app button bounds, tray layout, and "More" button triggering Spotlight search opening.

### 2. Full Workspace Test Suite
- **Command:** `cargo test --target x86_64-unknown-linux-gnu -p api -p service-compositor -p viui -p ocel -p desktop`
- **Result:** 357 passed, 0 failed, 11 ignored (8 test suites, 0.00s).

### 3. Workspace Compilation Check
- **Command:** `cargo check --workspace`
- **Result:** Passes cleanly with zero errors.

### 4. Bare-Metal PIC Cross-Build
- **Command:** `RUSTFLAGS="-C relocation-model=pic" cargo build --release --target x86_64-unknown-none -Z build-std=core,alloc -p desktop`
- **Result:** Clean compilation for bare-metal x86_64 target with PIC relocation model.

### 5. Security & Invariant Gate
- **Command:** `python3 scripts/cellos-sign --check`
- **Result:** Passes: Toolchain pinned to `nightly-2026-05-01`, zero unallowlisted unsafe code across 123 crates, `#![forbid(unsafe_code)]` strictly enforced in `desktop`.

### 6. Integration Runner Verification
- **Command:** `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test desktop-x86 --no-run`
- **Result:** Compiles without error.

## Phase 05 verification evidence (Executed 2026-10-19)

### 1. Compositor Unit & Placement Test Suite
- **`cells/services/compositor` Tests:** 12 passed, 0 failed.
  - Command: `cargo test --target x86_64-unknown-linux-gnu -p service-compositor`
  - Tests verified:
    - `surface_table::frame::tests::test_six_zone_snap_geometry_and_min_size_refusal`: Verified all 6 snap zones (`LeftHalf`, `RightHalf`, `TopLeftQuarter`, `BottomLeftQuarter`, `TopRightQuarter`, `BottomRightQuarter`), work-area exclusion (36 DIP taskbar), integer grid calculation without gaps or overlap at odd resolutions, generational increments, and min-size refusal (`ViewOpResult::Refused`) when member view constraints (`min_w`, `min_h`) exceed target snap geometry.
    - `surface_table::frame::tests::test_f11_fullscreen_toggle_and_state_restoration`: Verified content-only fullscreen mode toggle, prior placement preservation (`saved_placement`, `saved_floating_rect`), fullscreen bounds spanning the entire screen `(0, 0, screen_w, screen_h)`, and exact state restoration on toggle exit (restoring snapped or floating geometry).
    - Existing 10 tests verified: multi-view allocation, generational stale rejection, group/detach, same-frame no-op, owner crash cleanup, framebuffer blits, and window decoration math.

### 2. ABI & Wire Struct Verification
- **`libs/api` Tests:** 2 passed, 0 failed, 4 ignored.
  - Command: `cargo test --target x86_64-unknown-linux-gnu -p api`
  - Types and opcodes verified:
    - `SnapZone` enum representation with 6 zones (`LeftHalf = 1` through `BottomRightQuarter = 6`) and `SnapZone::from_u8`.
    - `FramePlacement` enum (`Floating`, `Snapped(SnapZone)`, `Maximized`, `Fullscreen`).
    - `SNAP_FRAME` (`0x27`) and `TOGGLE_FULLSCREEN` (`0x28`) opcodes in `api::display::compositor_ops`.

### 3. Core Workspace Test Suite
- **Command:** `cargo test --target x86_64-unknown-linux-gnu -p api -p service-compositor -p viui -p ocel -p desktop`
- **Result:** 359 passed, 0 failed, 11 ignored (8 test suites, 0.00s).

### 4. Workspace Check
- **Command:** `cargo check --workspace`
- **Result:** Passes cleanly without compiler errors.

### 5. Bare-Metal PIC Cross-Build
- **Command:** `RUSTFLAGS="-C relocation-model=pic" cargo build --release --target x86_64-unknown-none -Z build-std=core,alloc -p service-compositor`
- **Result:** Clean compilation for bare-metal x86_64 target with PIC relocation model.

### 6. Security & Invariant Gate
- **Command:** `python3 scripts/cellos-sign --check`
- **Result:** Passes: Toolchain pinned to `nightly-2026-05-01`, zero unallowlisted unsafe code across 123 crates, `#![forbid(unsafe_code)]` strictly enforced.

### 7. Integration Runner Verification
- **Command:** `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test desktop-x86 --no-run`
- **Result:** Compiles without error.

## Per-phase live proof

| Phase | Required running-path observation |
|---|---|
| 01 | Signed image boots CLI/recovery and baseline actual surface; manifest matches embedded bytes. **Verified passing.** |
| 02 | Change Light/Dark/Paper while editing; all live controls update without lost text; capture focus/selection and source-image colors. **Verified passing (ViUI contrast suite, Ocel cutover, and desktop-x86 runner).** |
| 03 | Multi-view same-process/mixed-owner frame, menu group/detach, close refusal and owner death; actually kill/restart compositor, recreate views with fresh incarnation IDs, reject old operations/ACKs/grants and retain app content. Repeat with surviving real Ocel and Terminal after phases 06/09. **Verified passing** (FrameRegistry multi-view, mixed-owner, generational validation, group/detach, and owner-crash cleanup test suite; 10/10 tests in `service-compositor`, 356 workspace tests passing). |
| 04 | Mouse launcher and Alt+Space; three sections, target-frame vs new-frame launch, app icon restores chosen session; real disposable reboot/poweroff only after consent. **Verified passing** (3-section Spotlight HUD, Applications/Tasks/System navigation, 36 DIP taskbar geometry with narrow-screen pagination, Files and Chrome catalog expansion; 357 workspace tests passing, clean bare-metal PIC build, cellos-sign check pass). |
| 05 | Six cells/combined targets, dwell/cancel/min-size denial; F11 and pointer exit restore state and usable hit geometry. **Verified passing** (`calculate_snap_rect`, `snap_frame` with min-size preflighting, `toggle_fullscreen` with `saved_placement` and `saved_floating_rect` restoration, and dynamic screen dimension handling in `view.rs`; 12/12 tests in `service-compositor`, 359 workspace tests passing). |
| 06 | Real Ocel documents loading/cancel switching, original content, Reader appearance; exactly one native tab strip. |
| 07 | Large directory, three views, new Files session, pin/unpin and real file conflicts/failures; concurrent source edits and cancelled/full/failed overwrite preserve source and prior destination, compare actual bytes. |
| 08 | Four preview families, rapid selection and busy/dead decoder; no stale preview or extra app tab, correct focus restored. |
| 09 | Two Terminals plus serial/SSH and real external CLI child streams; EOF/backpressure/interrupt/resize and foreground stdin transfer, background reader gets no terminal input; GUI launch returns right focus. Survive actual compositor restart without losing terminal child state. |
| 10 | Real guest app pixels/input, two attached base/payload devices, constrained file transfer, guest resolution and host F11; native/guest close refusal in both orderings, stale consent and failure cleanup. |
| 11 | Integrated walkthrough at odd/narrow/normal resolutions, larger text and three palettes; cold/warm measurements and full adverse-path matrix. |

## Matrix and artifacts

For each applicable target record source revision+dirty hashes, compiler, flags/features, signed image hash, boot config, guest payload hashes/licence, host CPU/acceleration/firmware, exact commands/exit status, scenario inputs, captures and resulting state. QEMU TCG/KVM/VMware have separate rows. Real Intel VMX is not proven by SVM/TCG vCPU markers. Missing VMware/Intel capability is a named unexecuted row, never a pass or reason to discard reachable work. Physical/production/persistent-durability claims require their own approved evidence.

Performance: record startup, input response, first preview content, final preview, steady-state CPU and memory (including grants/framebuffers) independently. Report actual observations; no previously asserted 10ms/16MiB/50ms guarantee is inherited.
