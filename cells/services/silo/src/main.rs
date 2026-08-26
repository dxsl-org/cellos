#![no_std]
#![no_main]

extern crate alloc;

#[cfg(all(
    feature = "development-silo-provider",
    not(all(target_arch = "aarch64", target_os = "none"))
))]
compile_error!("development-silo-provider is restricted to AArch64 bare-metal QEMU builds");

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = true
);
api::declare_syscalls![
    TrySend,
    Recv,
    Log,
    LookupService,
    RegisterService,
    GetRandom,
    CreateVm,
    CreateVcpu,
    MapGuestMemory,
    WriteGuestMemory,
    RunVcpu,
    VcpuRegs,
    ReadGuestMemory,
];

mod guest;
mod ipc;
mod run_loop;
mod vmm;

use ostd::io::println;
use service_silo::{
    artifact,
    layout::{GUEST_IPA_BASE, GUEST_RAM_BYTES, GUEST_RAM_PAGES},
};

#[no_mangle]
pub fn main() {
    println("[silo] development/reference provider starting");
    let guest_binary = match artifact::admitted_guest() {
        Ok(bytes) => bytes,
        Err(error) => {
            println(&alloc::format!(
                "[silo] guest artifact rejected: {:?}",
                error
            ));
            return;
        }
    };
    let vm_id = vmm::create_vm(GUEST_RAM_PAGES);
    if vm_id == 0 || vm_id == usize::MAX {
        println("[silo] create_vm failed");
        return;
    }
    if vmm::map_guest_memory(vm_id, GUEST_IPA_BASE, GUEST_RAM_BYTES, true) != 0 {
        println("[silo] map_guest_memory failed");
        return;
    }
    let vcpu_id = vmm::create_vcpu(vm_id, GUEST_IPA_BASE);
    if vcpu_id == 0 || vcpu_id == usize::MAX {
        println("[silo] create_vcpu failed");
        return;
    }
    // Test-hook kernels seed a smoke program during vCPU creation. Load the
    // admitted guest afterward so the bytes executed always match its digest.
    if vmm::write_guest_memory(vm_id, GUEST_IPA_BASE, guest_binary) != guest_binary.len() {
        println("[silo] guest load failed");
        return;
    }
    let session = match guest::GuestSession::initialize(vm_id, vcpu_id) {
        Ok(session) => session,
        Err(guest::GuestError::GuestFault(fault)) => {
            match (
                fault.response,
                fault.response.and_then(|response| response.mailbox_code),
            ) {
                (Some(response), Some(mailbox_code)) => println(&alloc::format!(
                    "[silo] one-time initialization guest fault: hvc_code=0x{:02x} \
                     mailbox_code=0x{:02x} request_seq={} response_seq={} status=0x{:02x}",
                    fault.hvc_code,
                    mailbox_code,
                    response.request_seq,
                    response.response_seq,
                    response.status,
                )),
                (Some(response), None) => println(&alloc::format!(
                    "[silo] one-time initialization guest fault: hvc_code=0x{:02x} \
                     mailbox_code=unavailable request_seq={} response_seq={} status=0x{:02x}",
                    fault.hvc_code,
                    response.request_seq,
                    response.response_seq,
                    response.status,
                )),
                (None, _) => println(&alloc::format!(
                    "[silo] one-time initialization guest fault: hvc_code=0x{:02x} \
                     mailbox_response=unavailable",
                    fault.hvc_code,
                )),
            }
            return;
        }
        Err(guest::GuestError::VmmFault(run_loop::SiloVmmFault::UnexpectedExit(
            run_loop::SiloUnexpectedExit::Unknown { ec, iss, pc },
        ))) => {
            match pc {
                Some(pc) => println(&alloc::format!(
                    "[silo] one-time initialization VMM fault: \
                     UnexpectedExit::Unknown ec=0x{:02x} iss=0x{:08x} pc=0x{:016x}",
                    ec,
                    iss,
                    pc,
                )),
                None => println(&alloc::format!(
                    "[silo] one-time initialization VMM fault: \
                     UnexpectedExit::Unknown ec=0x{:02x} iss=0x{:08x} pc=unavailable",
                    ec,
                    iss,
                )),
            }
            return;
        }
        Err(guest::GuestError::VmmFault(fault)) => {
            println(&alloc::format!(
                "[silo] one-time initialization VMM fault: {:?}",
                fault
            ));
            return;
        }
        Err(error) => {
            println(&alloc::format!(
                "[silo] one-time initialization failed: {:?}",
                error
            ));
            return;
        }
    };
    if ostd::service::register(api::syscall::service::SILO, 0).is_err() {
        println("[silo] readiness registration denied");
        return;
    }
    println("[silo] DEV_REFERENCE ready and registered; accepting only live KMS");
    ipc::run(session)
}
