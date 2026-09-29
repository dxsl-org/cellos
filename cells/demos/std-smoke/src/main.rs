#![no_main]
#![forbid(unsafe_code)]

extern crate ostd;

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false,
    i2c = false,
    spi = false
);

api::declare_syscalls![Log, Yield, GetTime, GetRandom, StateRestore];

ostd::cell_main!(extern "C" cell_main);
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct SensorTelemetry {
    device_id: String,
    seq: u64,
    readings: Vec<f32>,
    status: String,
}

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

    // 1b. HashMap & BTreeMap collections
    let mut map = HashMap::new();
    map.insert("alpha", 100);
    map.insert("beta", 200);
    map.insert("gamma", 300);
    assert_eq!(map.get("beta"), Some(&200));
    println!("[std-smoke] HashMap len = {}, lookup PASS", map.len());

    let mut btree = BTreeMap::new();
    btree.insert(3, "three");
    btree.insert(1, "one");
    btree.insert(2, "two");
    let keys: Vec<_> = btree.keys().copied().collect();
    assert_eq!(keys, vec![1, 2, 3]);
    println!("[std-smoke] BTreeMap sorted keys = {:?}, PASS", keys);

    // 1c. JSON Serialization / Deserialization (serde_json)
    let tele = SensorTelemetry {
        device_id: "cellos-node-01".to_string(),
        seq: 42,
        readings: vec![23.5, 45.2, 1013.25],
        status: "OK".to_string(),
    };
    let json_str = serde_json::to_string(&tele).expect("serialize json");
    println!("[std-smoke] Serialized JSON: {}", json_str);
    let parsed: SensorTelemetry = serde_json::from_str(&json_str).expect("deserialize json");
    assert_eq!(parsed, tele);
    println!(
        "[std-smoke] serde_json round-trip PASS (device={})",
        parsed.device_id
    );
    // 1d. Freeing allocator test: repeated allocate & free cycles
    for cycle in 0..1000 {
        let mut temp = Vec::with_capacity(512);
        for j in 0..512 {
            temp.push((cycle + j) as u32);
        }
        assert_eq!(temp.len(), 512);
        // temp drops here, deallocating back to free list!
    }
    println!("[std-smoke] Freeing allocator cycle test (1000 iter x 2KB) PASS");
    // 1e. Over-aligned allocation: `Layout::align` > 16 must be honoured.
    // Rust's GlobalAlloc contract requires the returned pointer to satisfy
    // `layout.align()`; a cell using a `#[repr(align(N))]` type, SIMD or an
    // over-aligned slice would otherwise get a misaligned (UB) pointer.
    #[repr(align(64))]
    struct Aligned64([u8; 64]);
    #[repr(align(256))]
    struct Aligned256([u8; 256]);
    let mut over_aligned_64 = Vec::new();
    println!(
        "[std-smoke] align_of(Aligned64) = {}, align_of(Aligned256) = {}",
        core::mem::align_of::<Aligned64>(),
        core::mem::align_of::<Aligned256>()
    );
    for i in 0..4u8 {
        let boxed = Box::new(Aligned64([i; 64]));
        let addr = core::hint::black_box(&*boxed as *const Aligned64 as usize);
        println!(
            "[std-smoke]   Aligned64 #{i} at {addr:#x} (mod 64 = {})",
            addr % 64
        );
        assert_eq!(addr % 64, 0, "64-byte alignment violated at {:#x}", addr);
        assert_eq!(boxed.0, [i; 64], "over-aligned 64 B payload is not intact");
        over_aligned_64.push(boxed);
    }
    let mut over_aligned_256 = Vec::new();
    for i in 0..2u8 {
        let boxed = Box::new(Aligned256([i; 256]));
        let addr = core::hint::black_box(&*boxed as *const Aligned256 as usize);
        println!(
            "[std-smoke]   Aligned256 #{i} at {addr:#x} (mod 256 = {})",
            addr % 256
        );
        assert_eq!(addr % 256, 0, "256-byte alignment violated at {:#x}", addr);
        assert_eq!(
            boxed.0, [i; 256],
            "over-aligned 256 B payload is not intact"
        );
        over_aligned_256.push(boxed);
    }
    println!(
        "[std-smoke] Over-aligned allocations PASS ({} x 64 B, {} x 256 B)",
        over_aligned_64.len(),
        over_aligned_256.len()
    );
    drop(over_aligned_64);
    drop(over_aligned_256);
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
    // 7. Command line arguments test
    let args: Vec<String> = std::env::args().collect();
    println!(
        "[std-smoke] Command line args count = {}, argv[0] = {}",
        args.len(),
        args[0]
    );
    assert!(!args.is_empty());

    println!("[std-smoke] PASS: All Rust std PAL invariants verified successfully!");
    ostd::syscall::sys_exit(0);
}
