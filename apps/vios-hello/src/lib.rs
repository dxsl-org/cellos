#![no_std]

extern crate alloc;


#[no_mangle]
pub fn app_main() -> ! {
    ostd::println!("ViOS Hello: Phase 16 Synchronization Test");

    use alloc::sync::Arc;
    use ostd::sync::Mutex;

    let counter = Arc::new(Mutex::new(0));
    let c1 = counter.clone();
    let c2 = counter.clone();

    // Spawn Thread 1
    ostd::task::spawn(move || {
        for _ in 0..100 {
            let mut val = c1.lock();
            *val += 1;
            // Short yield to increase contention chance
            if *val % 10 == 0 { ostd::syscall::sys_yield(); }
        }
        let _ = ostd::syscall::sys_send(1, b"T1 Done");
    });

    // Spawn Thread 2
    ostd::task::spawn(move || {
        for _ in 0..100 {
            let mut val = c2.lock();
            *val += 1;
             if *val % 10 == 0 { ostd::syscall::sys_yield(); }
        }
        let _ = ostd::syscall::sys_send(1, b"T2 Done");
    });

    // Wait for 2 completions
    let mut done_count = 0;
    let mut buf = [0u8; 16];
    while done_count < 2 {
        let res = ostd::syscall::sys_recv(0, &mut buf);
        if let ostd::syscall::SyscallResult::Ok(_) = res {
            ostd::println!("Main: Received completion signal");
            done_count += 1;
        }
    }

    let final_val = *counter.lock();
    ostd::println!("Final Counter Value: {} (Expected 200)", final_val);
    
    if final_val == 200 {
        ostd::println!("TEST PASSED: Mutex Synchronization works!");
    } else {
        ostd::println!("TEST FAILED: Race condition detected!");
    }

    loop {
        ostd::syscall::sys_yield();
    }
}
