//! Memory footprint table (not a criterion benchmark): heap bytes per value after building each
//! structure, measured with a counting global allocator, plus allocation counts and serialized
//! sizes.
//!
//! Columns:
//! * `heap B/v`: live heap bytes requested from the allocator (`Layout::size`) per value, after a
//!   random-order `insert` build, a sorted bulk build and `insert` + `optimize()`.
//! * `malloc B/v`: same as the insert column but with glibc's real chunk sizes
//!   (`malloc_usable_size + 8` bytes of chunk header per block), i.e. what the process pays.
//! * `blocks/v`: live heap blocks per value (insert build).
//! * `allocs/v`: `alloc` + `realloc` calls per value during the build (insert / sorted).
//! * `ser B/v`: serialized bytes per value as built and after `optimize()` (roaring types).
//!
//! Run: `cargo bench --bench memory [-- <workload substring>]`.
//!
//! `cargo bench --bench memory -- --probe` instead prints the SIMD backend roaring128 selects on
//! this CPU (like `.github/ci/backend-probe`), for the run metadata.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering::Relaxed};

use common::*;

struct Counting;

static LIVE_BYTES: AtomicI64 = AtomicI64::new(0);
static LIVE_CHUNK: AtomicI64 = AtomicI64::new(0);
static LIVE_BLOCKS: AtomicI64 = AtomicI64::new(0);
static ALLOCS: AtomicU64 = AtomicU64::new(0);

#[cfg(all(target_os = "linux", target_env = "gnu"))]
unsafe extern "C" {
    fn malloc_usable_size(ptr: *mut core::ffi::c_void) -> usize;
}

/// Bytes glibc really uses for the block: usable size plus the 8-byte chunk header.
#[inline]
fn chunk_size(ptr: *mut u8, layout_size: usize) -> i64 {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        let _ = layout_size;
        // SAFETY: `ptr` is a live block returned by the system allocator (malloc family).
        (unsafe { malloc_usable_size(ptr.cast()) } + 8) as i64
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        let _ = ptr;
        layout_size as i64
    }
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded contract.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            LIVE_BYTES.fetch_add(layout.size() as i64, Relaxed);
            LIVE_CHUNK.fetch_add(chunk_size(p, layout.size()), Relaxed);
            LIVE_BLOCKS.fetch_add(1, Relaxed);
            ALLOCS.fetch_add(1, Relaxed);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded contract.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            LIVE_BYTES.fetch_add(layout.size() as i64, Relaxed);
            LIVE_CHUNK.fetch_add(chunk_size(p, layout.size()), Relaxed);
            LIVE_BLOCKS.fetch_add(1, Relaxed);
            ALLOCS.fetch_add(1, Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size() as i64, Relaxed);
        LIVE_CHUNK.fetch_sub(chunk_size(ptr, layout.size()), Relaxed);
        LIVE_BLOCKS.fetch_sub(1, Relaxed);
        // SAFETY: forwarded contract.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old_chunk = chunk_size(ptr, layout.size());
        // SAFETY: forwarded contract.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            LIVE_BYTES.fetch_add(new_size as i64 - layout.size() as i64, Relaxed);
            LIVE_CHUNK.fetch_add(chunk_size(p, new_size) - old_chunk, Relaxed);
            ALLOCS.fetch_add(1, Relaxed);
        }
        p
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[derive(Clone, Copy)]
struct Snap {
    bytes: i64,
    chunk: i64,
    blocks: i64,
    allocs: u64,
}

fn snap() -> Snap {
    Snap {
        bytes: LIVE_BYTES.load(Relaxed),
        chunk: LIVE_CHUNK.load(Relaxed),
        blocks: LIVE_BLOCKS.load(Relaxed),
        allocs: ALLOCS.load(Relaxed),
    }
}

#[derive(Default)]
struct Row {
    insert_bytes: Option<f64>,
    sorted_bytes: Option<f64>,
    optimized_bytes: Option<f64>,
    insert_chunk: Option<f64>,
    blocks: Option<f64>,
    insert_allocs: Option<f64>,
    sorted_allocs: Option<f64>,
    ser: Option<f64>,
    ser_optimized: Option<f64>,
    note: Option<String>,
}

/// Sorted bulk build (if supported), then random-order insert build; returns the latter.
fn measure<S: Set128>(w: &Workload, row: &mut Row) -> S {
    let n = w.len() as f64;
    let before = snap();
    let sorted = S::build_sorted(&w.sorted);
    let after = snap();
    if sorted.is_some() {
        row.sorted_bytes = Some((after.bytes - before.bytes) as f64 / n);
        row.sorted_allocs = Some((after.allocs - before.allocs) as f64 / n);
    }
    drop(sorted);

    let before = snap();
    let s = S::build_insert(&w.shuffled);
    let after = snap();
    row.insert_bytes = Some((after.bytes - before.bytes) as f64 / n);
    row.insert_chunk = Some((after.chunk - before.chunk) as f64 / n);
    row.blocks = Some((after.blocks - before.blocks) as f64 / n);
    row.insert_allocs = Some((after.allocs - before.allocs) as f64 / n);
    s
}

fn row_for<S: Set128>(w: &Workload) -> Row {
    let mut row = Row::default();
    measure::<S>(w, &mut row);
    row
}

/// The roaring types: also `optimize()` and serialized sizes.
fn row_for_serial<S: Serial>(w: &Workload) -> Row {
    let mut row = Row::default();
    let n = w.len() as f64;
    let mut s = measure::<S>(w, &mut row);
    row.ser = Some(s.serialized_size() as f64 / n);
    let before = snap();
    s.optimize();
    let after = snap();
    row.optimized_bytes = Some(row.insert_bytes.unwrap() + (after.bytes - before.bytes) as f64 / n);
    row.ser_optimized = Some(s.serialized_size() as f64 / n);
    row
}

fn fmt(v: Option<f64>) -> String {
    match v {
        Some(v) if v >= 100.0 => format!("{v:.0}"),
        Some(v) if v >= 10.0 => format!("{v:.1}"),
        Some(v) => format!("{v:.2}"),
        None => "-".to_owned(),
    }
}

fn print_row(w: &Workload, name: &str, row: &Row) {
    if let Some(note) = &row.note {
        println!("| {} | {name} | {note} |||||||||", w.name);
        return;
    }
    println!(
        "| {} | {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
        w.name,
        fmt(row.insert_bytes),
        fmt(row.sorted_bytes),
        fmt(row.optimized_bytes),
        fmt(row.insert_chunk),
        fmt(row.blocks),
        fmt(row.insert_allocs),
        fmt(row.sorted_allocs),
        fmt(row.ser),
        fmt(row.ser_optimized),
    );
}

/// Reports a panic in the roaring128 measurement in the table instead of aborting the whole
/// table.
fn guarded(f: impl FnOnce() -> Row) -> Row {
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let r = panic::catch_unwind(AssertUnwindSafe(f));
    panic::set_hook(hook);
    r.unwrap_or_else(|payload| {
        let msg = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_default();
        Row {
            note: Some(format!("panicked: {}", msg.lines().next().unwrap_or(""))),
            ..Row::default()
        }
    })
}

fn composition(w: &Workload) -> String {
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let r = panic::catch_unwind(AssertUnwindSafe(|| {
        let mut s = R128::build_insert(&w.shuffled);
        let before = s.statistics();
        s.optimize();
        let after = s.statistics();
        format!(
            "| {} | {} | {} / {} / {} | {} / {} / {} | {:.1} |",
            w.name,
            before.n_containers,
            before.n_array_containers,
            before.n_bitset_containers,
            before.n_run_containers,
            after.n_array_containers,
            after.n_bitset_containers,
            after.n_run_containers,
            w.len() as f64 / before.n_containers.max(1) as f64,
        )
    }));
    panic::set_hook(hook);
    r.unwrap_or_else(|_| format!("| {} | (roaring128 unimplemented) |||| ", w.name))
}

/// Prints the architecture, the backends this CPU supports and the one roaring128 selects.
fn probe() {
    use roaring128::{SimdBackend, simd_backend};
    let supported: Vec<String> = [SimdBackend::Avx2, SimdBackend::Avx512, SimdBackend::Neon]
        .iter()
        .map(|b| {
            format!(
                "{}={}",
                b.name(),
                if b.is_supported() { "yes" } else { "no" }
            )
        })
        .collect();
    let forced = std::env::var("ROARING128_FORCE_BACKEND").unwrap_or_default();
    println!(
        "{} (arch {}, supported: {}{})",
        simd_backend().name(),
        std::env::consts::ARCH,
        supported.join(" "),
        if forced.is_empty() {
            String::new()
        } else {
            format!(", ROARING128_FORCE_BACKEND={forced}")
        }
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--probe") {
        probe();
        return;
    }
    let filter: Vec<String> = args.into_iter().filter(|a| !a.starts_with("--")).collect();
    println!("== roaring128 benches: memory ==");
    println!("{}", environment());
    println!(
        "allocator: system (glibc malloc); `heap B/v` counts requested bytes, `malloc B/v` real chunk sizes"
    );
    println!();

    let specs: &[(Kind, usize)] = &[
        (Kind::UuidV4, 10_000),
        (Kind::UuidV4, 100_000),
        (Kind::UuidV4, 1_000_000),
        (Kind::UuidV7, 1_000_000),
        (Kind::DenseRuns, 1_000_000),
        (Kind::DenseGaps50, 1_000_000),
        (Kind::DenseSmall, 1_000_000),
        (Kind::Mixed, 1_000_000),
        (Kind::RandomU64, 1_000_000),
        (Kind::DenseRunsU64, 1_000_000),
        (Kind::DenseGaps50U64, 1_000_000),
        (Kind::DenseSmallU64, 1_000_000),
    ];
    let workloads: Vec<Workload> = specs
        .iter()
        .enumerate()
        .map(|(i, &(k, n))| (i, k, n))
        .filter(|&(_, k, n)| {
            let name = format!("{}_{}", k.name(), short_n(n));
            filter.is_empty() || filter.iter().any(|f| name.contains(f.as_str()))
        })
        .map(|(i, k, n)| Workload::new(k, n, 3000 + i as u64))
        .collect();

    println!(
        "| workload | impl | heap B/v insert | heap B/v sorted | heap B/v optimized | malloc B/v insert | blocks/v | allocs/v insert | allocs/v sorted | ser B/v | ser B/v optimized |"
    );
    println!("|---|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|");
    for w in &workloads {
        print_row(w, "roaring128", &guarded(|| row_for_serial::<R128>(w)));
        if w.kind.fits_u64() {
            print_row(w, "treemap", &row_for_serial::<Tm>(w));
        } else {
            print_row(w, "wrapped64", &row_for_serial::<Wrapped64>(w));
            print_row(w, "btreeset", &row_for::<Bts>(w));
            print_row(w, "sortedvec", &row_for_serial::<Sv>(w));
            print_row(w, "hashset", &row_for::<Hs>(w));
            print_row(w, "hashset_fx", &row_for::<HsFx>(w));
        }
    }
    println!();
    println!("roaring128 container composition (insert build, then after optimize):");
    println!();
    println!(
        "| workload | containers | array / bitmap / run | after optimize | values/container |"
    );
    println!("|---|--:|--:|--:|--:|");
    for w in &workloads {
        println!("{}", composition(w));
    }
    println!();
    println!(
        "`sortedvec` is built by sorting (its insert column is a sort build); its serialized form is a raw little-endian dump."
    );
}
