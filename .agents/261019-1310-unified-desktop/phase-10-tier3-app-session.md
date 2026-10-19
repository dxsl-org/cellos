---
phase: 10
title: "Real Tier 3 single-app guest session integration"
status: pending
priority: P1
tier: thinking
dependencies: [1, 3, 4, 5, 7]
---
# Phase 10 — Tier 3 application sessions

## Overview
Replace current launcher print-only behavior with an actual admitted guest application and an interactive host view. This phase cannot pass on hypervisor lookup, `vCPU ready` or a guest boot log alone.

## Requirements
AC-08/13/18/19. One app fullscreen inside each application VM, one outer system tab, no Linux desktop/navigation, actual guest display resize and dialog/input operation. Guest app internal tabs remain allowed.

## Architecture
Current vm-runner/chrome only discover service 14 and log/exit. Existing hypervisor can boot x86 PVH guest and export a VMM scanout grant as a noninteractive Background surface; use actual device/Grant machinery but cut it over to an app-session Interactive view. Privileged VMM owns VM lifecycle, guest execution and constrained display/file bridges; ordinary desktop/runner holds only per-session control. Host/guest clipboard integration is out of this plan; guest code has no ambient host clipboard access. Kernel VMX/EPT substrate is a named prerequisite with an exact owner, not reimplemented inside Desktop.

## Related Files
Existing `cells/apps/{vm-runner,chrome}/src/main.rs`, `apps/chrome/chrome.toml`, guest build scripts; hypervisor `main`, `boot_x86`, `run_loop_x86`, `virtio_gpu/scanout`, `virtio_gpu/resource/control`, `virtio_input` and profile/device handlers; init supervisor and manifest/launch policy. Frozen `libs/api/src/abi/hypervisor.rs` changes require exact approval, never reinterpret version-2 exits. Existing x86 hypervisor/virtio/hostile lanes are regression inputs, not app-session proof.

## Implementation Steps
1. Verify actual call path into Intel VMX/EPT, not just presence of vmx_vcpu.rs or legacy SVM smoke. Inspect controlling P09 handoff's unchecked runtime criteria and current HAL facade/registry wiring. Adopt missing Intel execution/isolation/device/teardown prerequisites through the controlling owner and exact approvals; do not mark guest phase ready based on complete plan header.
2. Parse and validate app manifest, real kernel/initrd/base/app volumes and runtime entrypoint. Use two independently owned block devices: `/dev/vda` for the base root image, `/dev/vdb` for the read-only application payload expected by the current guest init. Implement both BlkDisk/device registrations with nonoverlapping MMIO/IRQ/virtqueue IDs, size validation, ownership and teardown; update package/boot mappings together. Source-build/pin/licence-check artifacts and launch the app-only guest compositor. Remove `/bin/sh` fallback from the selected application profile: missing payload/entrypoint fails to host, never exposes Linux. No unbounded command expansion.
3. Define owner-bound per-app guest launch/ready/failed/close/death operations and their actual consuming VMM endpoint/dispatcher. Bind to verified launcher incarnation; service 14 registration alone does not create a receiver. Integrate bounded request handling and correlated terminal replies with the VM run loop, including launch, resize, input, close, cancel and death branches. VM execution must yield to this control path within a bounded timeslice even for a guest with no voluntary exits; missing Intel exit/preemption support is an explicit substrate prerequisite. Preserve app identity across host frame changes, enforce per-VM budgets/startup deadlines and unwind partial creation. Multiple sessions cannot share an autostart guest invisibly.
4. Replace Background scanout topology with Interactive native view and generation-safe configure/damage/grant lifecycle. Negotiate guest display dimensions on host resize/snap/F11; no permanent framebuffer stretching substituted for guest relayout. Old scanout resources remain quarantined until their actual retirement is proved.
5. Route host-local pointer coordinates to guest display with correct scale/origin, pressed-key/button state and focus loss. Reserve host F11 once; guest remains fullscreen at all host sizes. Popup/file chooser/modal windows stay within the single-app guest presentation; no Linux launcher/terminal becomes visible on error or app exit.
6. Open With uses a separate bounded framed file-transfer channel over the guest console/agent boundary, not either whole base/app block device as a host-file export. Bind selected-resource identity, recipient VM generation and launch intent atomically; stage only that resource in a private guest path, default read-only. Guest writes require an explicit approved save/export operation, never ambient host writeback. Validate transfer lengths/path components and revoke on cancellation/death. Do not share host root or raw host block storage. Guest VM close follows app unsaved-data/termination policy; dirty/unknown guest shutdown is not successful save.
   Implement guest PrepareClose/CancelClose/CommitClose bound to host view/VM generation, prepared inventory and actual app content/dirty revision. Prepare must keep VM/display/input alive; only complete group/system commit may retire them. A per-app guest adapter must expose honest consent/refusal; unsupported preparation returns NeedsUserDecision/Unknown and blocks normal group/power close rather than inventing success. Explicit user save/close or separately confirmed force handles that case. Exercise a dirty guest app with a refusing native view in both preparation orderings, edits invalidating consent and app death.
7. Build real app payload and run UI scenario: launch from Spotlight/shell/Files, type/click/open a local fixture, internal app tab, popup/dialog, multiple host sizes, snap, F11 round trip, focus loss, app/VM crash and host session cleanup. Observe actual guest pixels and resulting content, not just scanout dimensions.

## Assumptions
Intel nested virtualization, host tooling and guest graphics/runtime payloads may be unavailable. Such absence blocks its exact runtime evidence and is recorded as prerequisite; software-hosted/SVM results cannot be relabeled Intel VMX. Guest kiosk policy must allow necessary dialogs; unsupported multiwindow guest apps require an explicit limitation, not silently narrowed acceptance.

## Success Criteria
- [ ] Real app in real VM renders/responds through one host system tab; underlying Linux UI never becomes normal interaction path.
- [ ] Host changes guest dimensions and pointer hit geometry correctly; F11 exit restores host frame while guest stays fullscreen.
- [ ] Admitted VM/device/frame resources are reclaimed on failure/death; no print-only launcher, background scanout workaround or host-root bridge remains on selected app path.
- [ ] Exact Intel VMX development execution and GUI evidence recorded separately from TCG/SVM regressions and physical qualification.

## Security Considerations
VMM authority never moves into general Desktop app. Guest commands/payload identities and disk/window sizes validated; per-application exports least-authority. No weakened signing/Tier2 admission, passthrough or device policy to reach a UI screenshot.

## Risk Assessment
Highest platform dependency. Revert app-session adapter and paired guest payload/image, retaining previously pinned no-guest recovery image. VMX/device/security changes revert only through integrating owner, never by rolling back unrelated user edits. Guest writes and terminated app sessions cannot be undone by code rollback; qualify on disposable guest volumes, never production data.

## Deviation Log
Append during execution.
