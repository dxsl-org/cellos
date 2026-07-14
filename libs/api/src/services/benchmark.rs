// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Benchmarking framework for performance validation.
//!
//! Provides interfaces for measuring and validating performance
//! of critical operations in ViCell.

use crate::*;
use alloc::boxed::Box;
use alloc::vec::Vec;

/// Benchmark result with timing and metadata.
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    /// Benchmark name
    pub name: &'static str,
    /// Number of iterations
    pub iterations: u64,
    /// Total cycles elapsed
    pub total_cycles: u64,
    /// Average cycles per iteration
    pub avg_cycles: u64,
    /// Minimum cycles observed
    pub min_cycles: u64,
    /// Maximum cycles observed
    pub max_cycles: u64,
    /// Standard deviation
    pub std_dev: u64,
}

impl BenchmarkResult {
    /// Check if benchmark meets performance target.
    pub fn meets_target(&self, target_cycles: u64) -> bool {
        self.avg_cycles <= target_cycles
    }
}

/// Benchmark trait for performance tests.
pub trait ViBenchmark {
    /// Get benchmark name.
    fn name(&self) -> &'static str;

    /// Setup before benchmark run.
    fn setup(&mut self) -> ViResult<()> {
        Ok(())
    }

    /// Run one iteration of the benchmark.
    ///
    /// # Returns
    /// Cycles elapsed for this iteration.
    fn run_once(&mut self) -> ViResult<u64>;

    /// Teardown after benchmark run.
    fn teardown(&mut self) -> ViResult<()> {
        Ok(())
    }

    /// Run benchmark with specified iterations.
    fn run(&mut self, iterations: u64) -> ViResult<BenchmarkResult> {
        self.setup()?;

        let mut total = 0u64;
        let mut min = u64::MAX;
        let mut max = 0u64;
        let mut samples = [0u64; 100]; // For std dev calculation

        for i in 0..iterations {
            let cycles = self.run_once()?;
            total += cycles;
            min = min.min(cycles);
            max = max.max(cycles);

            // Store sample for std dev (up to 100 samples)
            if i < 100 {
                samples[i as usize] = cycles;
            }
        }

        self.teardown()?;

        let avg = total / iterations;
        let std_dev = calculate_std_dev(&samples[..iterations.min(100) as usize], avg);

        Ok(BenchmarkResult {
            name: self.name(),
            iterations,
            total_cycles: total,
            avg_cycles: avg,
            min_cycles: min,
            max_cycles: max,
            std_dev,
        })
    }
}

/// Calculate standard deviation.
fn calculate_std_dev(samples: &[u64], mean: u64) -> u64 {
    if samples.is_empty() {
        return 0;
    }

    let variance: u64 = samples
        .iter()
        .map(|&x| {
            let diff = x.abs_diff(mean);
            diff * diff
        })
        .sum::<u64>()
        / samples.len() as u64;

    // Integer square root approximation
    integer_sqrt(variance)
}

/// Integer square root.
fn integer_sqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Benchmark suite for organizing multiple benchmarks.
pub trait ViBenchmarkSuite {
    /// Get suite name.
    fn name(&self) -> &'static str;

    /// Get all benchmarks in this suite.
    fn benchmarks(&mut self) -> &mut [Box<dyn ViBenchmark>];

    /// Run all benchmarks in suite.
    fn run_all(&mut self, iterations: u64) -> ViResult<Vec<BenchmarkResult>> {
        let mut results = Vec::new();
        // Force type hint if needed, though usually inferred
        for bench in self.benchmarks() {
            let res: BenchmarkResult = bench.run(iterations)?;
            results.push(res);
        }
        Ok(results)
    }
}

// ─── BenchReport (percentile-based) ──────────────────────────────────────────

/// Statistical report produced by the benchmark runner after a full run.
///
/// Fields carry nanosecond values computed from raw cycle counts divided by
/// the kernel-reported timer frequency.  When frequency is unavailable, the
/// fields hold raw tick counts with the same interpretation.
#[derive(Debug, Clone)]
pub struct BenchReport {
    /// Short identifier (e.g. `"context_switch"`).
    pub name: &'static str,
    /// Number of measured iterations (excluding warmup).
    pub n: u32,
    /// Minimum latency observed (ns or ticks).
    pub min: u64,
    /// Median (p50) latency.
    pub p50: u64,
    /// 99th-percentile latency.
    pub p99: u64,
    /// Maximum latency observed.
    pub max: u64,
}

impl BenchReport {
    /// Build a `BenchReport` from a sorted sample slice.
    ///
    /// `samples` must be sorted ascending before calling.  Panics in debug
    /// builds if the slice is empty.
    pub fn from_sorted(name: &'static str, samples: &[u64]) -> Self {
        debug_assert!(!samples.is_empty(), "BenchReport: sample slice is empty");
        let n = samples.len();
        let p50 = samples[n / 2];
        let p99 = samples[(n * 99) / 100];
        Self {
            name,
            n: n as u32,
            min: samples[0],
            p50,
            p99,
            max: samples[n - 1],
        }
    }

    /// Emit a compact JSON object (single line, no trailing newline).
    ///
    /// Format: `{"name":"ctx","n":1000,"min":42,"p50":55,"p99":90,"max":120}`
    ///
    /// Uses a small fixed buffer; truncates silently if `out` is too short.
    pub fn write_json(&self, out: &mut [u8]) -> usize {
        let mut pos = 0;
        let parts: [(&str, u64); 5] = [
            ("min", self.min),
            ("p50", self.p50),
            ("p99", self.p99),
            ("max", self.max),
            ("n", self.n as u64),
        ];
        let prefix = b"{\"name\":\"";
        if pos + prefix.len() > out.len() {
            return 0;
        }
        out[pos..pos + prefix.len()].copy_from_slice(prefix);
        pos += prefix.len();
        for b in self.name.bytes() {
            if pos >= out.len() {
                return pos;
            }
            out[pos] = b;
            pos += 1;
        }
        for (key, val) in &parts {
            let sep = if pos < out.len() {
                out[pos] = b'"';
                pos += 1;
                b",\""
            } else {
                return pos;
            };
            for b in sep {
                if pos < out.len() {
                    out[pos] = *b;
                    pos += 1;
                }
            }
            for b in key.bytes() {
                if pos < out.len() {
                    out[pos] = b;
                    pos += 1;
                }
            }
            if pos + 2 <= out.len() {
                out[pos] = b'"';
                out[pos + 1] = b':';
                pos += 2;
            }
            pos += write_u64(&mut out[pos..], *val);
        }
        if pos < out.len() {
            out[pos] = b'}';
            pos += 1;
        }
        pos
    }

    /// Return true if the p99 latency is within `target` (inclusive).
    pub fn meets_target(&self, target: u64) -> bool {
        self.p99 <= target
    }
}

/// Write a `u64` in decimal ASCII into `buf`; returns bytes written.
fn write_u64(buf: &mut [u8], mut n: u64) -> usize {
    if buf.is_empty() {
        return 0;
    }
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 20];
    let mut len = 0;
    while n > 0 {
        tmp[len] = b'0' + (n % 10) as u8;
        n /= 10;
        len += 1;
    }
    let written = len.min(buf.len());
    for i in 0..written {
        buf[i] = tmp[len - 1 - i];
    }
    written
}

// ─── Legacy ──────────────────────────────────────────────────────────────────

/// Performance targets for critical operations.
pub struct PerformanceTargets {
    /// File read (4KB) - target cycles
    pub file_read_4kb: u64,
    /// Network send (1KB) - target cycles
    pub net_send_1kb: u64,
    /// Hot-swap (1KB state) - target cycles
    pub hotswap_1kb: u64,
    /// VM-exit handling - target cycles
    pub vm_exit: u64,
    /// IPC roundtrip - target cycles
    pub ipc_roundtrip: u64,
}

impl Default for PerformanceTargets {
    fn default() -> Self {
        Self {
            file_read_4kb: 10_000, // 10K cycles
            net_send_1kb: 5_000,   // 5K cycles
            hotswap_1kb: 50_000,   // 50K cycles
            vm_exit: 1_000,        // 1K cycles
            ipc_roundtrip: 2_000,  // 2K cycles
        }
    }
}
