//! Shared benchmark infrastructure: workloads, the baselines, a common set interface, lazily
//! built structures, timing helpers, the environment banner and the self-check.
//!
//! Each benchmark binary includes this module with `mod common;`, so not every item is used by
//! every binary.
#![allow(dead_code)]

pub mod check;
pub mod sets;
pub mod workloads;
pub mod wrapped64;

use std::cell::OnceCell;
use std::hint::black_box;
use std::time::{Duration, Instant};

use criterion::Bencher;

pub use self::sets::*;
pub use self::workloads::*;
pub use self::wrapped64::Wrapped64;

/// A structure built lazily (on first use inside a benchmark closure) by inserting the
/// workload's values in random order, optionally followed by `optimize()`.
///
/// Lazy building means a criterion filter that excludes an implementation never builds (or
/// panics in) it. Benchmarks run implementation by implementation and drop the structure
/// afterwards, so only one implementation's (possibly large) structures are alive at a time.
pub struct Built<'a, S> {
    pub w: &'a Workload,
    optimized: bool,
    cell: OnceCell<S>,
}

impl<'a, S: Set128> Built<'a, S> {
    pub fn new(w: &'a Workload) -> Self {
        Built {
            w,
            optimized: false,
            cell: OnceCell::new(),
        }
    }

    /// Insert build followed by `optimize()` (run containers where smaller, roaring types).
    pub fn optimized(w: &'a Workload) -> Self {
        Built {
            optimized: true,
            ..Built::new(w)
        }
    }

    pub fn get(&self) -> &S {
        self.cell.get_or_init(|| {
            let mut s = S::build_insert(&self.w.shuffled);
            if self.optimized {
                s.optimize();
            }
            s
        })
    }
}

/// Times `f` and drops its result outside the timed region (one result alive at a time, unlike
/// `iter_with_large_drop`, which keeps whole batches of large structures alive).
pub fn time_build<O>(b: &mut Bencher<'_>, mut f: impl FnMut() -> O) {
    b.iter_custom(|iters| {
        let mut total = Duration::ZERO;
        for _ in 0..iters {
            let start = Instant::now();
            let out = black_box(f());
            total += start.elapsed();
            drop(out);
        }
        total
    });
}

/// Like [`time_build`] with an untimed `setup` producing the routine's input.
pub fn time_with_setup<I, O>(
    b: &mut Bencher<'_>,
    mut setup: impl FnMut() -> I,
    mut f: impl FnMut(I) -> O,
) {
    b.iter_custom(|iters| {
        let mut total = Duration::ZERO;
        for _ in 0..iters {
            let input = black_box(setup());
            let start = Instant::now();
            let out = black_box(f(input));
            total += start.elapsed();
            drop(out);
        }
        total
    });
}

fn read_trimmed(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
}

/// One line describing the machine state that matters for reading the numbers.
pub fn environment() -> String {
    let cpu = read_trimmed("/proc/cpuinfo")
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|m| m.trim().to_owned())
        })
        .unwrap_or_else(|| std::env::consts::ARCH.to_owned());
    let governor = read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
        .unwrap_or_else(|| "?".into());
    let epp = read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference")
        .unwrap_or_else(|| "?".into());
    let boost = read_trimmed("/sys/devices/system/cpu/cpufreq/boost").map_or("?".to_owned(), |b| {
        if b == "1" { "on".into() } else { "off".into() }
    });
    let load = read_trimmed("/proc/loadavg")
        .map(|l| l.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|| "?".into());
    format!(
        "backend={} | cpu={cpu} | governor={governor} epp={epp} boost={boost} | loadavg={load}",
        roaring128::simd_backend().name()
    )
}

/// Prints the banner (backend, CPU, frequency policy) and runs the self-check, unless
/// `BENCH_SKIP_CHECK=1`. Panics if an implementation computes a wrong result.
pub fn prelude(binary: &str) {
    println!("== roaring128 benches: {binary} ==");
    println!("{}", environment());
    if std::env::var_os("BENCH_SKIP_CHECK").is_some_and(|v| v == "1") {
        println!("self-check: skipped (BENCH_SKIP_CHECK=1)");
    } else {
        let start = Instant::now();
        let report = check::run_all();
        println!("self-check ({:.1?}): {report}", start.elapsed());
    }
    println!();
}
