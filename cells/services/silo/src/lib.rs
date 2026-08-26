#![no_std]

//! Host-testable implementation shared with the Silo service binary.

pub mod artifact;
pub mod mailbox;
pub mod protocol;
pub mod vm_exit;



#[path = "../../../guests/silo-guest/src/layout.rs"]
pub mod layout;
