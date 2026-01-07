// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Virtualization runtime interface for Tier 3 applications.
//!
//! This module defines the `ViVmRuntime` trait for running legacy applications
//! (unmodified Linux/Windows/Android binaries) via hardware virtualization.
//!
//! See: `docs/architecture/05-application-tiers.md` § Tier 3

use crate::*;


/// Guest virtual machine state.
#[derive(Debug, Clone, Copy)]
pub struct VmState {
    /// Guest physical memory base address
    pub gpa_base: PhysAddr,
    /// Guest physical memory size in bytes
    pub gpa_size: usize,
    /// Entry point (guest virtual address)
    pub entry: VAddr,
    /// VCPU count
    pub vcpu_count: usize,
}

/// Trap reason from guest VM.
#[derive(Debug, Clone, Copy)]
pub enum VmTrap {
    /// Guest attempted a syscall
    Syscall { nr: usize, args: [usize; 6] },
    
    /// Guest triggered a page fault
    PageFault { addr: VAddr, write: bool },
    
    /// Guest executed an I/O instruction
    IoAccess { port: u16, write: bool, size: u8 },
    
    /// Guest executed HLT or WFI
    Halt,
    
    /// Guest received an interrupt
    Interrupt { 
        /// Interrupt vector number
        vector: u8,
        /// Is external interrupt (vs timer/software)
        external: bool,
    },
    
    /// Guest triggered an exception
    Exception { 
        /// Exception code (e.g., illegal instruction, breakpoint)
        code: u8,
        /// Faulting address (if applicable)
        addr: VAddr,
    },
    
    /// Guest made a hypercall to VMM
    Hypercall { 
        /// Hypercall number
        nr: usize,
        /// Hypercall arguments
        args: [usize; 4],
    },
    
    /// Debug trap (breakpoint, watchpoint)
    Debug {
        /// Debug event type
        event: DebugEvent,
        /// Associated address
        addr: VAddr,
    },
    
    /// Other trap not covered above
    Other(usize),
}

/// Debug event types for VM debugging.
#[derive(Debug, Clone, Copy)]
pub enum DebugEvent {
    /// Breakpoint hit
    Breakpoint,
    /// Watchpoint (data access)
    Watchpoint { write: bool },
    /// Single-step
    SingleStep,
}


/// Trait for Virtual Machine Monitor (VMM) Cells.
///
/// # Architecture
/// - **Hypervisor Cell**: Acts as VMM implementing this trait
/// - **Traps**: Guest syscalls trap to VMM, which translates them to ViOS API
/// - **Isolation**: Hardware Stage-2 paging (Guest Physical → Host Physical)
///
/// # Performance
/// ~85-90% native performance due to hardware virtualization support.
pub trait ViVmRuntime {
    /// Create a new virtual machine.
    ///
    /// # Arguments
    /// * `state` - Initial VM configuration
    ///
    /// # Returns
    /// A unique VM identifier, or an error if creation fails.
    /// Create a new virtual machine.
    ///
    /// # Arguments
    /// * `state` - Initial VM configuration
    ///
    /// # Returns
    /// A unique VM identifier, or an error if creation fails.
    fn create_vm(&mut self, state: VmState) -> ViResult<usize>;

    /// Run a virtual CPU until it traps.
    ///
    /// # Arguments
    /// * `vm_id` - VM identifier from `create_vm`
    /// * `vcpu_id` - Virtual CPU index (0..vcpu_count)
    ///
    /// # Returns
    /// The trap reason, or an error if execution fails.
    fn run_vcpu(&mut self, vm_id: usize, vcpu_id: usize) -> ViResult<VmTrap>;

    /// Handle a trap and resume execution.
    ///
    /// # Arguments
    /// * `vm_id` - VM identifier
    /// * `vcpu_id` - Virtual CPU index
    /// * `trap` - The trap to handle
    ///
    /// # Returns
    /// `Ok(())` if trap was handled successfully, or an error otherwise.
    fn handle_trap(&mut self, vm_id: usize, vcpu_id: usize, trap: VmTrap) -> ViResult<()>;

    /// Map guest physical memory to host physical memory.
    ///
    /// # Arguments
    /// * `vm_id` - VM identifier
    /// * `gpa` - Guest physical address
    /// * `hpa` - Host physical address
    /// * `size` - Mapping size in bytes
    /// * `writable` - Whether the mapping is writable
    ///
    /// # Returns
    /// `Ok(())` if mapping succeeded, or an error otherwise.
    fn map_memory(
        &mut self,
        vm_id: usize,
        gpa: PhysAddr,
        hpa: PhysAddr,
        size: usize,
        writable: bool,
    ) -> ViResult<()>;

    /// Destroy a virtual machine and free its resources.
    ///
    /// # Arguments
    /// * `vm_id` - VM identifier to destroy
    ///
    /// # Returns
    /// `Ok(())` if VM was destroyed successfully, or an error otherwise.
    fn destroy_vm(&mut self, vm_id: usize) -> ViResult<()>;
}
