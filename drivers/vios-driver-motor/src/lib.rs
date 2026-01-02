#![no_std]

extern crate alloc;
use ostd::prelude::*;

pub struct MotorDriver {
    pub speed: u32,
}

impl SafeDriver for MotorDriver {
    fn name(&self) -> &str {
        "MotorDriver v1.0"
    }

    fn init(&mut self) -> Result<(), &'static str> {
        ostd::println!("MotorDriver: Initializing...");
        
        // 1. Create a buffer to share with Kernel (Simulated DMA buffer)
        // 1. Create a buffer to share with Kernel (Simulated DMA buffer)
        let dma_buffer: [u8; 64] = [0xAA; 64]; 
        
        // 2. Share it! (Syscall: Lend)
        // Lend to Task 1 (Init Process) with READ permission (1)
        ostd::println!("MotorDriver: Lending DMA buffer to Task 1...");
        match ostd::syscall::sys_lend(1, &dma_buffer, 1) {
            ostd::syscall::SyscallResult::Ok(lid) => {
                 ostd::println!("MotorDriver: Success! Lease ID = {}", lid);
            },
            ostd::syscall::SyscallResult::Err(e) => {
                 ostd::println!("MotorDriver: Lend Failed: {:?}", e);
            }
        }

        // 3. Main Loop (Async)
        ostd::println!("MotorDriver: Entering Async Control Loop...");
        
        ostd::executor::block_on(async {
            ostd::println!("MotorDriver: Async Listening (TryRecv)...");
            let mut buf = [0u8; 64];
            
            // Wait for a message asynchronously
            match ostd::ipc::recv_async(0, &mut buf).await {
                ostd::syscall::SyscallResult::Ok(sender) => {
                     ostd::println!("MotorDriver: Async Message Received from ID {}!", sender);
                },
                val => {
                     ostd::println!("MotorDriver: Async Recv Unexpected: {:?}", val);
                }
            }
        });
        
        ostd::println!("MotorDriver: Halted.");
        Ok(())
    }
}

pub fn create_driver() -> Box<dyn SafeDriver> {
    Box::new(MotorDriver { speed: 0 })
}

#[no_mangle]
pub fn driver_main() -> ! {
    ostd::println!("MotorDriver: Starting...");
    loop {
        ostd::syscall::sys_yield();
    }
}
