//! Benchmark runner: warmup loop + measurement loop + percentile computation.

extern crate alloc;
use super::{report, timer};
use alloc::vec::Vec;
use api::{
    benchmark::{BenchReport, ViBenchmark},
    ViError,
};

/// Default warmup iterations (discarded; exist to heat up caches and QEMU JIT).
pub const DEFAULT_WARMUP: u32 = 100;
/// Default measurement iterations per scenario.
pub const DEFAULT_ITERS: u32 = 1_000;

/// Private execution stage used to classify an invalid experiment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStage {
    Setup,
    Warmup,
    Measure,
    Teardown,
}

impl RunStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Warmup => "warmup",
            Self::Measure => "measure",
            Self::Teardown => "teardown",
        }
    }
}

/// Failure from a private benchmark run.
///
/// When both the experiment and teardown fail, `stage`/`error` retain the
/// original experiment failure and `teardown_error` records cleanup failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunFailure {
    pub stage: RunStage,
    pub error: ViError,
    pub teardown_error: Option<ViError>,
}

impl RunFailure {
    const fn operation(stage: RunStage, error: ViError) -> Self {
        Self {
            stage,
            error,
            teardown_error: None,
        }
    }

    const fn teardown(error: ViError) -> Self {
        Self::operation(RunStage::Teardown, error)
    }
}

/// Run a benchmark through setup, warmup, measurement, and teardown.
///
/// A failed operation terminates the experiment without producing a report.
/// Teardown is attempted after every setup attempt. If teardown also fails, the
/// original operation failure remains primary and the cleanup error is retained
/// in [`RunFailure::teardown_error`].
pub fn run<B: ViBenchmark>(
    bench: &mut B,
    warmup: u32,
    iters: u32,
) -> Result<BenchReport, RunFailure> {
    let operation = (|| {
        bench
            .setup()
            .map_err(|error| RunFailure::operation(RunStage::Setup, error))?;

        for _ in 0..warmup {
            bench
                .run_once()
                .map_err(|error| RunFailure::operation(RunStage::Warmup, error))?;
        }

        let mut samples: Vec<u64> = Vec::with_capacity(iters as usize);
        for _ in 0..iters {
            let t0 = timer::read_ticks();
            bench
                .run_once()
                .map_err(|error| RunFailure::operation(RunStage::Measure, error))?;
            let t1 = timer::read_ticks();
            samples.push(t1.saturating_sub(t0));
        }

        Ok(report::build_report(bench.name(), &mut samples))
    })();

    match (operation, bench.teardown()) {
        (Ok(report), Ok(())) => Ok(report),
        (Ok(_), Err(error)) => Err(RunFailure::teardown(error)),
        (Err(failure), Ok(())) => Err(failure),
        (Err(mut failure), Err(error)) => {
            failure.teardown_error = Some(error);
            Err(failure)
        }
    }
}
