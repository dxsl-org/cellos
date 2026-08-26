# Phase 02: Two-surface RV64 QEMU policy evidence

## Context Links
- `tests/integration/tests/compositor-cursor.rs:1-194`
- `tests/integration/src/lib.rs:1320-1500,1635-1706`
- `libs/ostd/src/display.rs:59-184`; `libs/ostd/src/args.rs:48-72`
- `gen_disk.ps1:123-126,550-576`

## Overview
Add one tiny, packaged graphical probe executable and extend the existing RV64
pointer integration test. Two separately spawned probe cells create real,
overlapping grant-backed surfaces; QMP pointer/keyboard events and PPM screen
crops prove policy behavior without a desktop shell feature.

## Key Insights
- `QemuRunner::boot_with_pointer` already has a VirtIO GPU, tablet, keyboard,
  QMP monitor, absolute pointer injection, screen dump, and key injection.
- `compositor_cursor_moves_on_mouse_event` already boots that runner, launches a
  real dashboard, waits serial markers, and compares PPM pixel regions.
- `ViSurface::create` and `move_to` supply real compositor surfaces. `ostd::args`
  permits one binary to choose `back` or `front` geometry in separate cells.

## Requirements
- The test must use two distinct cell owners and two opaque, real ViSurface
  grant buffers. It must not fake z-order with a unit double or direct IPC.
- Back and front must overlap, use stable distinct colors, and expose one back-
  only click point plus one front-only point for capture verification.
- After clicking back, prove visible raise from an overlap PPM sample and prove
  back-only press, captured release, and key serial markers; explicitly reject a
  front key marker after the key oracle.
- Use one QEMU integration test only; preserve the current cursor-motion check.

## Architecture
`window-policy-probe back &` creates a back surface at a deterministic offset;
`window-policy-probe front &` creates the later top surface at another offset.
Each polls its ordinary input queue and logs role-qualified pointer/key markers.
The host test clicks exposed back, waits its press (and focus reassertion path),
compares pre/post overlap pixels, sends a press/move/release across a front-only
point, then injects one QMP key.

## Related Code Files
- New: `cells/tests/window-policy-probe/{Cargo.toml,build.rs,src/main.rs}`.
- Modify: workspace `Cargo.toml`, `gen_disk.ps1`, launch-profile targets,
  `tests/integration/src/lib.rs`, and new `tests/integration/tests/window-policy.rs`.
- Reuse: `libs/ostd/src/{display.rs,input.rs}`, integration PPM helpers.

## Implementation Steps
1. Add `window-policy-probe` as a no_std packaged test cell. Parse its first
   argument as exactly `back` or `front`; create a fixed-size BGRA `ViSurface`,
   move it to fixed coordinates, paint its opaque role color/identifier, damage,
   then poll input and log one role-qualified ready/press/release/key marker.
2. Package the new binary using the existing `gen_disk.ps1` build, signing, and
   `/bin/...` table patterns; add it to the workspace. Do not auto-start it.
3. Add a narrow `QemuRunner` button-down/button-up helper (or equivalent reuse
   of its QMP event encoding) so the test can press back, move to a point inside
   front but outside back, then release. Keep `send_qemu_mouse_click` working.
4. Extend the existing test: launch back then front through the shell, wait both
   ready markers, sample an overlap PPM region (front color), click back-only,
   wait back press, re-capture and assert back color in that same region.
5. Send explicit left down, move to the front-only point, then left up; require
   back release and no front press/release marker. After that wait, inject `a`,
   require back key marker, sleep a bounded settling interval, and assert the
   accumulated serial output contains no front key marker.
6. Rebuild the disk by its established workflow; run `window-policy` and the
   cursor regression locally; wire the named policy test into CI `boot-suite`.

## Todo List
- [x] Add/package deterministic interactive and background graphical probes.
- [x] Add granular QMP button sequencing helper.
- [x] Assert raise, captured move/release, background exclusion, and keyboard routing.
- [x] Rebuild disk and run the named RV64 QEMU tests.
- [x] Wire the named QEMU policy test into CI `boot-suite`.

## Success Criteria
The test visibly observes a top-front overlap before click and top-back overlap
after clicking the exposed back surface. It logs back-only press and captured
release, then back-only key receipt after the post-click wait. Failure of any
z-order, pointer capture, focus reassertion, or keyboard-routing link fails the
same test with serial/PPM evidence.

## Risk Assessment
QMP coordinates and asynchronous input delivery can make weak assertions flaky.
Use fixed small surfaces and points with margins, wait each serial transition,
and compare the same overlap crop. QEMU prerequisites retain the existing test's
explicit skip guard; absence is not a policy pass.

## Security Considerations
The probe uses ordinary unprivileged surface/input APIs and does not expose test
focus control. The test verifies compositor-mediated routing; it does not weaken
sender ownership checks or bypass input service authentication.

## Next Steps
After this bounded milestone, consider later policy work only under a separately
approved plan. Decorations, drag/resize, close lifecycle, and general window
management remain explicitly out of scope.
