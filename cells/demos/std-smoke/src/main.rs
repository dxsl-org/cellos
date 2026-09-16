#![no_main]

extern crate ostd;

api::declare_syscalls![Log, Yield, GetTime, GetRandom];

ostd::cell_main!(extern "C" cell_main);

fn cell_main() {
    println!("[std-smoke] Starting Rust std Cell execution in CellOS SAS!");

    // 1. Memory allocation test
    let mut vec = Vec::new();
    for i in 0..10 {
        vec.push(i * 10);
    }
    let boxed = Box::new(vec);
    println!(
        "[std-smoke] Allocated vector with {} elements, sum = {}",
        boxed.len(),
        boxed.iter().sum::<i32>()
    );

    // 2. Monotonic time test
    let t0 = std::time::Instant::now();

    // 3. Scheduler yield test
    println!("[std-smoke] Performing scheduler yield...");
    std::thread::yield_now();

    let elapsed = t0.elapsed();
    println!("[std-smoke] Yield completed, elapsed = {:?}", elapsed);

    // 4. Thread parallelism query
    let parallelism = std::thread::available_parallelism().unwrap().get();
    println!(
        "[std-smoke] Available parallelism = {} (expected 1)",
        parallelism
    );
    assert_eq!(parallelism, 1);

    // 5. Environment constants
    println!(
        "[std-smoke] Target OS const = {} (expected cellos)",
        std::env::consts::OS
    );
    assert_eq!(std::env::consts::OS, "cellos");

    // 6. Fail-closed unsupported APIs
    assert!(std::fs::read("/test").is_err());
    assert!(std::net::TcpStream::connect("127.0.0.1:80").is_err());
    assert!(std::process::Command::new("sh").spawn().is_err());
    println!("[std-smoke] All unsupported APIs correctly failed closed!");

    println!("[std-smoke] PASS: All Rust std PAL invariants verified successfully!");
    ostd::syscall::sys_exit(0);
}
