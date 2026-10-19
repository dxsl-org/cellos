---
phase: 11
title: "Integrated desktop image and user-path qualification"
status: unblocked
priority: P1
tier: thinking
dependencies: [2, 3, 4, 5, 6, 7, 8, 9, 10]
---
# Phase 11 — Integrated qualification

## Overview
Qualify the complete approved desktop on a pinned image. Earlier phases already package and smoke their changed surfaces; this is cross-feature acceptance, not the first integration attempt.

## Requirements
AC-01..AC-20. No missing app, fake provider, skipped prerequisites or source-text assertion counts as end-to-end completion. Physical/production/remote promotion remains separate.

## Related Files
Existing image builders, boot-config sources, signing policy/allowlist, `run-desktop.sh`, x86 ISO runners and `tests/integration` infrastructure. New exact desktop-x86 scenario runner/fixtures only where current lanes cannot exercise the real surface; record guest/VMware hardware/profile pins. Update relevant docs/display/input/VFS/app-development/desktop guides and CHANGELOG.


## Architecture
Use the same signed source/image contract across consumers, actual x86 QMP input/screendump scenarios and state assertions. See [verification commands](verification.md); it distinguishes existing boot-only commands from the planned desktop-x86 target and preserves exact VM/hardware evidence ceilings.
## Implementation Steps
1. Assemble signed deterministic image with Desktop, shared theme/session contracts, Ocel, Files, preview services, Terminal and real Tier3 app adapter/payload plus CLI recovery. Verify embedded content matches actual signed binaries/config; don't replace /bin/ocel alone while leaving incompatible SDK services stale. Validate default memory/VA profiles and resource budgets from real image.
2. Exercise shell CLI, shell GUI foreground/background, shell->desktop and desktop->Terminal; input/focus never depends on whichever UART/global consumer wins. Test desktop autoload and recovery CLI selection. Verify persisted preferences/background only to approved storage guarantees.
3. Run actual native scenarios with List/Grid/Columns, large directory, file operations, four preview formats, Ocel reader modes, two Terminal sessions, mixed-owner frame groups, close refusal and app death. Capture meaningful visible content/state, not nonblack pixels alone.
4. Run all six placement cells and edge/corner targets with fractional/odd dimensions and larger text; F11 keyboard/pointer escape, repeated theme switches, session switching, bar restore, full queues, lost updates and stale owners.
5. Run real Tier3 Chrome/selected app with guest fullscreen at every host size, popup/file-open and F11 filtering. Intel VMX execution is its own prerequisite; retain exact limits of TCG/SVM evidence. QEMU TCG/KVM and VMware desktop matrix per available hardware; missing environment is blocked evidence, not pass/skip-success.
6. Measure cold startup, theme repaint, idle CPU, resident/buffer memory, navigation/first-preview/completion and interaction latency using pinned workloads/hardware. Do not invent <16MiB/<50ms/<10ms guarantees. Test increasing tabs/directories/guest sizes to actual capacity limits; denial must preserve existing work.
7. Mouse-only and keyboard-only walkthrough with Vietnamese names, high/normal DPI, 3 palettes, focus and text contrast, reduced motion and no automatic theme recolor of artwork. Record user-task success/errors separately from automated screenshots; no claim of superiority to Windows/macOS without comparative study.
8. Review actual protocol/caller/security deltas, obtain required CP2, remove obsolete tab/theme/launcher paths made redundant, update docs/changelog and evidence ledger. User-approved deferrals remain explicit; no new placeholders.

## Assumptions
Executable tools and hardware availability determined in phase 01. Existing RV64 test definitions are pattern references only, not proof of Intel GUI coverage. Third-party guest licence/artifact reproduction must be recorded before distribution.

## Success Criteria
- [ ] Every acceptance ID links to exercised command/profile/source/image + capture/state result and applicable failure boundaries.
- [ ] Complete desktop walkthrough works with no missing consumers, UI-only stubs or silent capability fallbacks; relevant regressions pass.
- [ ] KVM/TCG/VMware and physical/production evidence ceilings separated honestly; any unavailable required platform remains a named blocker.
- [ ] ABI CP2 and migration census complete; no permanent duplicated palette/native-tab implementations or old fake Terminal catalog path remain.

## Security Considerations
Audit launch/file bridges/session ownership, untrusted preview inputs, unauthorized power/kill/settings, clipboard/terminal framing and VM isolation boundaries. QA uses disposable volumes and never disables admission to pass.

## Risk Assessment
Integrated test can reveal shared quota or guest-resource contention. Roll back to the pinned prior image and source patch set, not user source/volume resets. File operations and guest/app writes already performed are not undone by image rollback; retain test volumes/evidence and describe irreversible effects.

## Deviation Log
Append during execution.
