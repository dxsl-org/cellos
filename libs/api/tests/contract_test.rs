// SPDX-License-Identifier: MPL-2.0
// Architecture Validation Test: Step 1 - Contract Testing

//! Mock implementations to verify trait contracts are sufficient.

// #![no_std] // Commented out for integration test on Host which has std
// extern crate alloc; // Available via std prelude or we can just use std

use api::vm::*;
use api::*;

/// Mock VMM for ViVmRuntime testing.
struct MockVMM {
    vms: Vec<MockVM>,
}

#[allow(dead_code)] // reason: mirrors the real VMM's per-VM record shape; id/state verify construction even though only `running` is read back in these tests
struct MockVM {
    id: usize,
    state: VmState,
    running: bool,
}

impl MockVMM {
    fn new() -> Self {
        Self { vms: Vec::new() }
    }
}

impl ViVmRuntime for MockVMM {
    fn create_vm(&mut self, state: VmState) -> Result<usize> {
        // Test: Can we create VM with provided state?
        let id = self.vms.len();
        self.vms.push(MockVM {
            id,
            state,
            running: false,
        });
        Ok(id)
    }

    fn run_vcpu(&mut self, vm_id: usize, _vcpu_id: usize) -> Result<VmTrap> {
        // Test: Can we simulate VM execution?
        if vm_id >= self.vms.len() {
            return Err(ViError::NotFound);
        }

        self.vms[vm_id].running = true;

        // Simulate a syscall trap
        Ok(VmTrap::Syscall {
            nr: 1, // write
            args: [1, 0x1000, 13, 0, 0, 0],
        })
    }

    fn handle_trap(&mut self, vm_id: usize, _vcpu_id: usize, trap: VmTrap) -> Result<()> {
        // Test: Can we handle different trap types?
        if vm_id >= self.vms.len() {
            return Err(ViError::NotFound);
        }

        match trap {
            VmTrap::Syscall { nr, .. } => {
                // Simulate syscall handling
                if nr == 1 {
                    // write syscall - would translate to ViCell API
                    Ok(())
                } else {
                    Err(ViError::InvalidArgument)
                }
            }
            VmTrap::PageFault { .. } => {
                // Would allocate page and map
                Ok(())
            }
            VmTrap::Halt => {
                self.vms[vm_id].running = false;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn map_memory(
        &mut self,
        vm_id: usize,
        _gpa: PhysAddr,
        _hpa: PhysAddr,
        _size: usize,
        _writable: bool,
    ) -> Result<()> {
        // Test: Can we map memory?
        if vm_id >= self.vms.len() {
            return Err(ViError::NotFound);
        }
        Ok(())
    }

    fn destroy_vm(&mut self, vm_id: usize) -> Result<()> {
        // Test: Can we clean up VM?
        if vm_id >= self.vms.len() {
            return Err(ViError::NotFound);
        }

        // In real implementation, would free resources
        self.vms[vm_id].running = false;
        Ok(())
    }
}

#[test]
fn test_vi_vm_runtime_lifecycle() {
    let mut vmm = MockVMM::new();

    // Create VM
    let vm_state = VmState {
        gpa_base: 0x8000_0000,
        gpa_size: 128 * 1024 * 1024, // 128MB
        entry: 0x8000_0000,
        vcpu_count: 2,
    };
    let vm_id = vmm.create_vm(vm_state).unwrap();
    assert_eq!(vm_id, 0);

    // Map memory
    vmm.map_memory(vm_id, 0x8000_0000, 0x4000_0000, 4096, true)
        .unwrap();

    // Run VCPU
    let trap = vmm.run_vcpu(vm_id, 0).unwrap();
    match trap {
        VmTrap::Syscall { nr, .. } => assert_eq!(nr, 1),
        _ => panic!("Expected syscall trap"),
    }

    // Handle trap
    vmm.handle_trap(vm_id, 0, trap).unwrap();

    // Destroy VM
    vmm.destroy_vm(vm_id).unwrap();
}
