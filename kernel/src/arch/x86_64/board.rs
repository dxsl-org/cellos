// SPDX-License-Identifier: MPL-2.0
//! `board`: the platform profile of the bound x86_64 board.

/// Returns the validated x86 platform profile paired with the bound board.
///
/// # Panics
/// Before a profile is bound (see [`crate::board::active`]).
pub(crate) fn soc() -> &'static hal_soc_x86::X86PlatformProfile {
    crate::board::x86_soc_profile(crate::board::active())
}
