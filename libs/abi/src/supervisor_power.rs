// SPDX-License-Identifier: Apache-2.0
//! The supervisor power request the kernel emits itself (the ACPI power
//! button), carried inside the App SDK envelope ([`crate::app_envelope`]).
//! `wire::proto::supervisor` re-exports these at their service-contract path.

use crate::app_envelope::{KIND_MESSAGE, MAGIC};

pub const OP_POWER: u8 = 0x04;

pub const POWER_ACTION_REBOOT: u8 = 1;
pub const POWER_ACTION_POWEROFF: u8 = 2;

/// The only power request the kernel sends: the ACPI power button.
pub const POWER_BUTTON_REQUEST: [u8; 4] = [MAGIC, KIND_MESSAGE, OP_POWER, POWER_ACTION_POWEROFF];
/// Sender id of kernel-originated requests (the power button).
pub const KERNEL_SENDER: usize = 0;
