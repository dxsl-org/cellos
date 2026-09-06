use api::ring_channel::{BiRingChannel, RingError};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

#[test]
fn test_fastpath_spsc_throughput_and_latency() {
    const ITERS: usize = 100_000;
    let channel = Arc::new(BiRingChannel::new());

    let ch_worker = Arc::clone(&channel);
    let worker = thread::spawn(move || {
        let mut req_buf = [0u8; 64];
        let resp = [0u8; 1];

        for _ in 0..ITERS {
            // Receive request
            let meta = loop {
                match ch_worker.a_to_b.try_pop(&mut req_buf) {
                    Ok(Some(m)) => break m,
                    Ok(None) => std::hint::spin_loop(),
                    Err(e) => panic!("worker pop error: {:?}", e),
                }
            };
            assert_eq!(meta.len, 64);
            assert_eq!(req_buf[0], 0x42);

            // Send reply
            loop {
                match ch_worker.b_to_a.try_push(&resp, meta.seq, 0) {
                    Ok(()) => break,
                    Err(RingError::Full) => std::hint::spin_loop(),
                    Err(e) => panic!("worker push error: {:?}", e),
                }
            }
        }
    });

    let mut req = [0u8; 64];
    req[0] = 0x42;
    let mut resp_buf = [0u8; 1];

    let mut latencies_ns = Vec::with_capacity(ITERS);
    let t0_total = Instant::now();

    for seq in 0..ITERS {
        let t_start = Instant::now();

        // Send request
        loop {
            match channel.a_to_b.try_push(&req, seq as u32, 0) {
                Ok(()) => break,
                Err(RingError::Full) => std::hint::spin_loop(),
                Err(e) => panic!("client push error: {:?}", e),
            }
        }

        // Receive reply
        let meta = loop {
            match channel.b_to_a.try_pop(&mut resp_buf) {
                Ok(Some(m)) => break m,
                Ok(None) => std::hint::spin_loop(),
                Err(e) => panic!("client pop error: {:?}", e),
            }
        };

        let elapsed = t_start.elapsed().as_nanos() as u64;
        latencies_ns.push(elapsed);

        assert_eq!(meta.seq, seq as u32);
        assert_eq!(resp_buf[0], 0);
    }

    let total_elapsed = t0_total.elapsed();
    worker.join().expect("worker should join cleanly");

    let throughput_msg_sec = (ITERS as f64) / total_elapsed.as_secs_f64();

    latencies_ns.sort_unstable();
    let p50 = latencies_ns[ITERS * 50 / 100];
    let p90 = latencies_ns[ITERS * 90 / 100];
    let p99 = latencies_ns[ITERS * 99 / 100];
    let min = latencies_ns[0];
    let max = latencies_ns[ITERS - 1];

    println!("\n=== FASTPATH SPSC RING IPC BENCHMARK (100,000 ROUND-TRIPS) ===");
    println!("Total Time:   {:?}", total_elapsed);
    println!("Throughput:   {:.2} msg/s (Target: >= 500,000 msg/s)", throughput_msg_sec);
    println!("Min Latency:  {} ns ({:.3} µs)", min, min as f64 / 1000.0);
    println!("P50 Latency:  {} ns ({:.3} µs)", p50, p50 as f64 / 1000.0);
    println!("P90 Latency:  {} ns ({:.3} µs)", p90, p90 as f64 / 1000.0);
    println!("P99 Latency:  {} ns ({:.3} µs) (Target: <= 10.0 µs)", p99, p99 as f64 / 1000.0);
    println!("Max Latency:  {} ns ({:.3} µs)", max, max as f64 / 1000.0);
    println!("===============================================================\n");

    assert!(
        throughput_msg_sec >= 500_000.0,
        "Throughput {:.2} msg/s did not meet 500,000 msg/s target",
        throughput_msg_sec
    );
    assert!(
        p99 <= 10_000,
        "P99 latency {} ns exceeded 10,000 ns (10 µs) target",
        p99
    );
}
