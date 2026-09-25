# Policy and packaging cleanup

## Requirements

- Remove `/bin/dwc2-hid` from Cargo binary targets, signing policy, RPi3 packaging, boot ceilings, and DWC2 launch edges.
- Keep `/bin/lan9514` as the only capability-free child allowed from `/bin/dwc2-usb`.
- Keep the DWC2 host's USB and spawn ceilings no broader than required.
- Update architecture documentation and changelog without claiming physical verification.

## Files

- `cells/drivers/dwc2-usb/Cargo.toml`
- `scripts/build-aarch64-cells.ps1`
- `scripts/sign-policy.py`
- `kernel/src/loader/boot_ceiling.rs`
- `kernel/src/loader/launch_profile/profiles.rs`
- relevant launch-profile tests
- `docs/input-api.md`
- `docs/project-changelog.md`

## Success criteria

- RPi3 package contains `/bin/dwc2-usb` and `/bin/lan9514`, not `/bin/dwc2-hid`.
- The DWC2 launch profile accepts only `/bin/lan9514`.
- Signing and boot-ceiling checks pass.

## Risk assessment

Rollback restores the removed binary and policy rows. No persistent state is changed. A stale launch-policy reference would either broaden authority or break packaging, so exact-reference searches are required before verification.
