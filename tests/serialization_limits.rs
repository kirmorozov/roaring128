//! Deserialization of inputs announcing far more containers than they contain.
//!
//! `FORMAT.md`: `deserialize_from` "never allocates more than the input actually provides". A
//! counting global allocator records the largest single allocation and the peak of live heap
//! memory while deserializing; both must stay proportional to the input length (plus a constant
//! for one container payload). Counts are tried in increasing order so that an implementation
//! that pre-allocates from the count fails with an assertion before it reaches counts whose
//! allocation would abort the process. This binary is separate from `serialization` for that
//! reason.

#[macro_use]
mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;
use roaring128::RoaringBitmap128;

use common::*;

struct Counting;

static CUR: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static LARGEST: AtomicUsize = AtomicUsize::new(0);

fn grow(size: usize) {
    LARGEST.fetch_max(size, Ordering::Relaxed);
    let cur = CUR.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(cur, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        CUR.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        CUR.fetch_sub(layout.size(), Ordering::Relaxed);
        grow(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Measurements are not reliable with concurrent tests: serialize them.
static SERIAL: Mutex<()> = Mutex::new(());

/// Constant slack: one container payload (at most 65536 runs of 4 bytes) and bookkeeping.
const SLACK: usize = 1 << 20;

/// Runs `f`, returning its result, the largest single allocation and the peak growth of live
/// heap memory during the call.
fn measure<T>(f: impl FnOnce() -> T) -> (T, usize, usize) {
    LARGEST.store(0, Ordering::Relaxed);
    let base = CUR.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let out = f();
    let largest = LARGEST.load(Ordering::Relaxed);
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(base);
    (out, largest, peak)
}

fn header(count: u64, flags: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(0x3831_5252u32.to_le_bytes());
    out.extend(flags.to_le_bytes());
    out.extend(count.to_le_bytes());
    out
}

#[track_caller]
fn check_rejected_cheaply(bytes: &[u8], what: &str) {
    for unchecked in [false, true] {
        let ((ok, desc), largest, peak) = measure(|| {
            let res = if unchecked {
                RoaringBitmap128::deserialize_unchecked_from(bytes)
            } else {
                RoaringBitmap128::deserialize_from(bytes)
            };
            match res {
                Ok(rb) => (true, format!("{rb:?}")),
                Err(e) => (false, e.to_string()),
            }
        });
        let bound = 8 * bytes.len() + SLACK;
        assert!(!ok, "{what} (unchecked: {unchecked}): accepted as {desc}");
        assert!(
            largest <= bound,
            "{what} (unchecked: {unchecked}): allocated {largest} bytes at once for a {} byte input",
            bytes.len()
        );
        assert!(
            peak <= bound,
            "{what} (unchecked: {unchecked}): peak heap growth {peak} bytes for a {} byte input",
            bytes.len()
        );
    }
}

/// Container counts larger than what the inputs below contain (at most 3 containers).
const COUNTS: [u64; 12] = [
    4,
    1000,
    1 << 16,
    1 << 20,
    1 << 24,
    1 << 28,
    1 << 32,
    1 << 40,
    1 << 48,
    (1 << 63) - 1,
    1 << 63, // "negative" as a signed count
    u64::MAX,
];

#[test]
fn deserialize_huge_container_count() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for count in COUNTS {
        for flags in [0, 1] {
            let bytes = header(count, flags);
            check_rejected_cheaply(
                &bytes,
                &format!("header only, count {count}, flags {flags}"),
            );
            // a few valid-looking descriptors and payloads after the header
            let mut bytes = header(count, flags);
            if flags == 1 {
                bytes.push(0);
            }
            for k in 0..3u128 {
                bytes.extend((k << 16).to_le_bytes());
            }
            bytes.extend([0u8; 6]);
            check_rejected_cheaply(
                &bytes,
                &format!("3 containers, count {count}, flags {flags}"),
            );
        }
    }
}

#[test]
fn deserialize_huge_count_after_valid_content() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rb: RoaringBitmap128 = (0..1000u128).map(|i| i << 20).collect();
    rb.insert_range(u128::MAX - 100_000..);
    for optimize in [false, true] {
        if optimize {
            rb.optimize();
        }
        let valid = serialize(&rb);
        let n = u64::from_le_bytes(valid[8..16].try_into().unwrap());
        for count in COUNTS.into_iter().filter(|&c| c > n) {
            let mut bytes = valid.clone();
            bytes[8..16].copy_from_slice(&count.to_le_bytes());
            check_rejected_cheaply(
                &bytes,
                &format!("valid content, count {count} instead of {n}"),
            );
        }
    }
}

/// The measurement itself: deserializing a valid input stays within the bound too.
#[test]
fn valid_input_allocations_are_proportional() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rb: RoaringBitmap128 = (0..50_000u128)
        .map(|i| i.wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835))
        .collect();
    rb.insert_range(1 << 64..(1 << 64) + 300_000);
    rb.extend((0..10_000u128).map(|i| (1 << 100) + 3 * i));
    let bytes = serialize(&rb);
    let (res, largest, peak) = measure(|| RoaringBitmap128::deserialize_from(&bytes[..]));
    let back = res.unwrap();
    assert_eq!(back, rb);
    let bound = 8 * bytes.len() + SLACK;
    assert!(
        largest <= bound,
        "largest allocation {largest} for {} bytes",
        bytes.len()
    );
    assert!(
        peak <= bound,
        "peak heap growth {peak} for {} bytes",
        bytes.len()
    );
}

proptest! {
    #![proptest_config(config(64))]

    /// Any container count written over a valid encoding is either the right one or rejected
    /// without large allocations.
    #[test]
    fn random_count_corruption(spec in set_spec_with(2), count in prop_oneof![any::<u64>(), 0..100_000u64], flags in 0..2u32) {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (rb, _) = spec.build();
        let mut bytes = serialize(&rb);
        let n = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        prop_assume!(count != n);
        bytes[8..16].copy_from_slice(&count.to_le_bytes());
        bytes[4..8].copy_from_slice(&flags.to_le_bytes());
        let (_, largest, peak) = measure(|| {
            let _ = RoaringBitmap128::deserialize_from(&bytes[..]);
            let _ = RoaringBitmap128::deserialize_unchecked_from(&bytes[..]);
        });
        let bound = 8 * bytes.len() + SLACK;
        prop_assert!(largest <= bound, "largest allocation {} for {} bytes (count {})", largest, bytes.len(), count);
        prop_assert!(peak <= bound, "peak heap growth {} for {} bytes (count {})", peak, bytes.len(), count);
    }
}
