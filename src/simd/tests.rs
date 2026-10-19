//! Differential tests: every supported backend must agree with the scalar reference.
//!
//! Output buffers are allocated with exactly the documented bound followed by a guard zone; a
//! kernel that writes past its bound corrupts the guard and fails the test.

use alloc::vec::Vec;
use alloc::{format, vec};
use core::mem::MaybeUninit;

use super::*;

const ALL_BACKENDS: [SimdBackend; 3] = [SimdBackend::Avx2, SimdBackend::Avx512, SimdBackend::Neon];

/// An implementation under test: the scalar reference itself or a SIMD backend.
#[derive(Clone, Copy, Debug)]
enum Impl {
    Reference,
    Simd(SimdBackend),
}

impl Impl {
    fn name(self) -> &'static str {
        match self {
            Impl::Reference => "reference",
            Impl::Simd(b) => b.name(),
        }
    }
}

/// The scalar reference plus every backend supported by this CPU.
fn backends() -> impl Iterator<Item = Impl> {
    core::iter::once(Impl::Reference).chain(
        ALL_BACKENDS
            .into_iter()
            .filter(|b| b.is_supported())
            .map(Impl::Simd),
    )
}

/// Calls kernel `$name` on the given implementation.
macro_rules! call {
    ($imp:expr, $name:ident, $name_on:ident ($($arg:expr),*)) => {
        match $imp {
            Impl::Reference => scalar::$name($($arg),*),
            Impl::Simd(b) => $name_on(b, $($arg),*),
        }
    };
}

/// Small deterministic PRNG (splitmix64), so failures are reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// A sorted, deduplicated array of roughly `len` values drawn from `lo..=hi`.
fn sorted_array(rng: &mut Rng, len: usize, lo: u16, hi: u16) -> Vec<u16> {
    let span = hi as u64 - lo as u64 + 1;
    let mut v: Vec<u16> = (0..len)
        .map(|_| (lo as u64 + rng.below(span)) as u16)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn array_cases() -> Vec<(Vec<u16>, Vec<u16>)> {
    let mut rng = Rng(0x5EED_0001);
    let mut cases: Vec<(Vec<u16>, Vec<u16>)> = Vec::new();
    let full: Vec<u16> = (0..=u16::MAX).collect();
    let evens: Vec<u16> = (0..=u16::MAX).step_by(2).collect();
    let odds: Vec<u16> = (1..=u16::MAX).step_by(2).collect();
    let fixed: Vec<Vec<u16>> = vec![
        vec![],
        vec![0],
        vec![u16::MAX],
        vec![0, u16::MAX],
        (0..7).collect(),
        (0..8).collect(),
        (0..9).collect(),
        (0..16).collect(),
        (0..17).collect(),
        (0..31).collect(),
        (0..32).collect(),
        (0..33).collect(),
        (65500..=u16::MAX).collect(),
        (0..4096).collect(),
        (0..4096).map(|x| x * 16).collect(),
        evens.clone(),
        odds.clone(),
        full.clone(),
    ];
    for a in &fixed {
        for b in &fixed {
            cases.push((a.clone(), b.clone()));
        }
    }
    let sizes = [
        0usize, 1, 2, 3, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 100, 500, 1000, 4096,
    ];
    for _ in 0..6 {
        for &la in &sizes {
            for &lb in &sizes {
                // Dense overlapping, sparse overlapping, and disjoint value ranges.
                cases.push((
                    sorted_array(&mut rng, la, 0, 2000),
                    sorted_array(&mut rng, lb, 0, 2000),
                ));
                cases.push((
                    sorted_array(&mut rng, la, 0, u16::MAX),
                    sorted_array(&mut rng, lb, 0, u16::MAX),
                ));
                cases.push((
                    sorted_array(&mut rng, la, 0, 30000),
                    sorted_array(&mut rng, lb, 30001, u16::MAX),
                ));
                // Nearly identical arrays.
                let a = sorted_array(&mut rng, la, 0, 10000);
                let mut b = a.clone();
                if !b.is_empty() {
                    let i = rng.below(b.len() as u64) as usize;
                    b.remove(i);
                }
                cases.push((a, b));
            }
        }
    }
    // Skewed sizes exercise galloping paths.
    for _ in 0..20 {
        let small_len = 1 + rng.below(40) as usize;
        let small = sorted_array(&mut rng, small_len, 0, u16::MAX);
        let large_len = 3000 + rng.below(3000) as usize;
        let large = sorted_array(&mut rng, large_len, 0, u16::MAX);
        cases.push((small.clone(), large.clone()));
        cases.push((large, small));
        cases.push((sorted_array(&mut rng, 5, 0, u16::MAX), full.clone()));
        cases.push((full.clone(), sorted_array(&mut rng, 5, 0, u16::MAX)));
    }
    cases
}

/// Runs an array kernel with an exact-size output buffer guarded by sentinels.
fn run_guarded(bound: usize, f: impl FnOnce(&mut [MaybeUninit<u16>]) -> usize) -> Vec<u16> {
    const GUARD: usize = 128;
    const SENTINEL: u16 = 0xA5A5;
    let mut buf = vec![MaybeUninit::new(SENTINEL); bound + GUARD];
    let n = f(&mut buf[..bound]);
    assert!(
        n <= bound,
        "kernel reported {n} values for a bound of {bound}"
    );
    for (i, g) in buf[bound..].iter().enumerate() {
        // SAFETY: the guard zone was initialized with the sentinel.
        assert_eq!(
            unsafe { g.assume_init() },
            SENTINEL,
            "write past the output bound at +{i}"
        );
    }
    // SAFETY: the kernel initialized the first `n` values.
    unsafe { assume_init(&buf, n) }.to_vec()
}

fn naive_intersect(a: &[u16], b: &[u16]) -> Vec<u16> {
    a.iter()
        .copied()
        .filter(|x| b.binary_search(x).is_ok())
        .collect()
}

fn naive_union(a: &[u16], b: &[u16]) -> Vec<u16> {
    let mut v: Vec<u16> = a.iter().chain(b).copied().collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn naive_difference(a: &[u16], b: &[u16]) -> Vec<u16> {
    a.iter()
        .copied()
        .filter(|x| b.binary_search(x).is_err())
        .collect()
}

fn naive_xor(a: &[u16], b: &[u16]) -> Vec<u16> {
    let mut v = naive_difference(a, b);
    v.extend(naive_difference(b, a));
    v.sort_unstable();
    v
}

#[test]
fn array_kernels_match_reference() {
    let cases = array_cases();
    for backend in backends() {
        for (a, b) in &cases {
            let ctx = || format!("backend={} |a|={} |b|={}", backend.name(), a.len(), b.len());
            let inter = naive_intersect(a, b);
            let got = run_guarded(a.len().min(b.len()), |out| {
                call!(backend, array_intersect, array_intersect_on(a, b, out))
            });
            assert_eq!(got, inter, "intersect {}", ctx());
            assert_eq!(
                call!(backend, array_intersect_card, array_intersect_card_on(a, b)),
                inter.len(),
                "intersect_card {}",
                ctx()
            );
            assert_eq!(
                call!(backend, array_intersects, array_intersects_on(a, b)),
                !inter.is_empty(),
                "intersects {}",
                ctx()
            );

            let got = run_guarded(a.len() + b.len(), |out| {
                call!(backend, array_union, array_union_on(a, b, out))
            });
            assert_eq!(got, naive_union(a, b), "union {}", ctx());

            let got = run_guarded(a.len(), |out| {
                call!(backend, array_difference, array_difference_on(a, b, out))
            });
            assert_eq!(got, naive_difference(a, b), "difference {}", ctx());

            let got = run_guarded(a.len() + b.len(), |out| {
                call!(backend, array_xor, array_xor_on(a, b, out))
            });
            assert_eq!(got, naive_xor(a, b), "xor {}", ctx());
        }
    }
}

fn bitmap_cases() -> Vec<Words> {
    let mut rng = Rng(0x5EED_0002);
    let mut cases: Vec<Words> = Vec::new();
    cases.push([0; BITMAP_WORDS]);
    cases.push([u64::MAX; BITMAP_WORDS]);
    cases.push([0xAAAA_AAAA_AAAA_AAAA; BITMAP_WORDS]);
    cases.push([0x5555_5555_5555_5555; BITMAP_WORDS]);
    cases.push([1; BITMAP_WORDS]);
    cases.push([1 << 63; BITMAP_WORDS]);
    let mut edges = [0u64; BITMAP_WORDS];
    edges[0] = 1;
    edges[BITMAP_WORDS - 1] = 1 << 63;
    cases.push(edges);
    let mut alternating_words = [0u64; BITMAP_WORDS];
    for (i, w) in alternating_words.iter_mut().enumerate() {
        *w = if i % 2 == 0 { u64::MAX } else { 0 };
    }
    cases.push(alternating_words);
    // Random densities from very sparse to very dense.
    for shift in [1u32, 2, 3, 4, 6, 8, 12, 16] {
        for _ in 0..3 {
            let mut w = [0u64; BITMAP_WORDS];
            for x in w.iter_mut() {
                let mut v = u64::MAX;
                for _ in 0..(shift / 2) {
                    v &= rng.next();
                }
                if shift % 2 == 1 {
                    v = !v;
                }
                *x = v;
            }
            cases.push(w);
        }
    }
    // A few long runs.
    let mut runs = [0u64; BITMAP_WORDS];
    for (i, w) in runs.iter_mut().enumerate() {
        *w = match i % 7 {
            0 | 1 => u64::MAX,
            2 => 0x0000_FFFF_FFFF_0000,
            3 => 0x8000_0000_0000_0001,
            _ => 0,
        };
    }
    cases.push(runs);
    cases
}

fn naive_popcount(a: &Words) -> u32 {
    a.iter().map(|w| w.count_ones()).sum()
}

fn naive_runs(a: &Words) -> u32 {
    let mut runs = 0;
    let mut prev = false;
    for i in 0..65536usize {
        let bit = a[i / 64] >> (i % 64) & 1 == 1;
        if bit && !prev {
            runs += 1;
        }
        prev = bit;
    }
    runs
}

#[test]
fn bitmap_kernels_match_reference() {
    let cases = bitmap_cases();
    let ops = [BitOp::And, BitOp::Or, BitOp::Xor, BitOp::AndNot];
    for backend in backends() {
        for (ia, a) in cases.iter().enumerate() {
            let card = naive_popcount(a);
            assert_eq!(
                call!(backend, bitmap_popcount, bitmap_popcount_on(a)),
                card,
                "{} popcount #{ia}",
                backend.name()
            );
            assert_eq!(
                call!(backend, bitmap_count_runs, bitmap_count_runs_on(a)),
                naive_runs(a),
                "{} runs #{ia}",
                backend.name()
            );
            let got = run_guarded(card as usize, |out| {
                call!(backend, bitmap_to_array, bitmap_to_array_on(a, out))
            });
            let expected: Vec<u16> = (0..=u16::MAX)
                .filter(|&i| a[i as usize / 64] >> (i % 64) & 1 == 1)
                .collect();
            assert_eq!(got, expected, "{} to_array #{ia}", backend.name());

            for (ib, b) in cases.iter().enumerate() {
                let ctx = || format!("backend={} a=#{ia} b=#{ib}", backend.name());
                for op in ops {
                    let mut expected = [0u64; BITMAP_WORDS];
                    for i in 0..BITMAP_WORDS {
                        expected[i] = op.apply(a[i], b[i]);
                    }
                    let exp_card = naive_popcount(&expected);

                    let mut out = [0u64; BITMAP_WORDS];
                    assert_eq!(
                        call!(backend, bitmap_op, bitmap_op_on(op, a, b, &mut out)),
                        exp_card,
                        "op {op:?} {}",
                        ctx()
                    );
                    assert!(out == expected, "op {op:?} output {}", ctx());

                    let mut inplace = *a;
                    assert_eq!(
                        call!(
                            backend,
                            bitmap_op_inplace,
                            bitmap_op_inplace_on(op, &mut inplace, b)
                        ),
                        exp_card,
                        "inplace {op:?} {}",
                        ctx()
                    );
                    assert!(inplace == expected, "inplace {op:?} output {}", ctx());

                    let mut lazy = *a;
                    call!(
                        backend,
                        bitmap_op_inplace_nocard,
                        bitmap_op_inplace_nocard_on(op, &mut lazy, b)
                    );
                    assert!(lazy == expected, "nocard {op:?} output {}", ctx());

                    assert_eq!(
                        call!(backend, bitmap_op_card, bitmap_op_card_on(op, a, b)),
                        exp_card,
                        "card {op:?} {}",
                        ctx()
                    );
                }
                let intersects = a.iter().zip(b.iter()).any(|(x, y)| x & y != 0);
                assert_eq!(
                    call!(backend, bitmap_intersects, bitmap_intersects_on(a, b)),
                    intersects,
                    "intersects {}",
                    ctx()
                );
                let subset = a.iter().zip(b.iter()).all(|(x, y)| x & !y == 0);
                assert_eq!(
                    call!(backend, bitmap_is_subset, bitmap_is_subset_on(a, b)),
                    subset,
                    "subset {}",
                    ctx()
                );
            }
        }
    }
}

#[test]
fn bitmap_single_bit_positions() {
    // Every single-bit bitmap position must survive the extraction and counting kernels.
    for backend in backends() {
        for bit in (0..65536usize).step_by(61).chain([0, 63, 64, 65535]) {
            let mut a = [0u64; BITMAP_WORDS];
            a[bit / 64] = 1 << (bit % 64);
            assert_eq!(call!(backend, bitmap_popcount, bitmap_popcount_on(&a)), 1);
            assert_eq!(
                call!(backend, bitmap_count_runs, bitmap_count_runs_on(&a)),
                1
            );
            let got = run_guarded(1, |out| {
                call!(backend, bitmap_to_array, bitmap_to_array_on(&a, out))
            });
            assert_eq!(got, vec![bit as u16], "{} bit {bit}", backend.name());
        }
    }
}

#[test]
fn backend_detection_is_supported() {
    assert!(simd_backend().is_supported());
}

#[test]
fn gallop_finds_lower_bound() {
    let a: Vec<u16> = (0..1000).map(|x| x * 3).collect();
    for from in [0usize, 1, 10, 500, 999, 1000] {
        for v in [0u16, 1, 2, 3, 299, 300, 301, 2996, 2997, 2998, u16::MAX] {
            let expected = from.max(a.partition_point(|&x| x < v)).min(a.len());
            let expected = if from >= a.len() { from } else { expected };
            assert_eq!(scalar::gallop(&a, from, v), expected, "from={from} v={v}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Backend selection, randomized differential cases and timings (added with the NEON kernels).
// ---------------------------------------------------------------------------------------------

/// With `ROARING128_EXPECT_BACKEND` set (`scripts/test-aarch64.sh` sets it to `neon`), the
/// selected backend must be that one, so emulated runs cannot silently test something else.
#[cfg(feature = "std")]
#[test]
fn expected_backend_is_selected() {
    if let Ok(expected) = std::env::var("ROARING128_EXPECT_BACKEND") {
        assert_eq!(simd_backend().name(), expected.to_ascii_lowercase());
    }
    #[cfg(target_arch = "aarch64")]
    {
        assert!(std::arch::is_aarch64_feature_detected!("neon"));
        assert!(SimdBackend::Neon.is_supported());
        assert_eq!(simd_backend(), SimdBackend::Neon);
    }
}

/// Lengths around the vector widths (8 and 16 lanes of `u16`, 32 and 64 for the wider units).
const EDGE_LENGTHS: [usize; 27] = [
    0, 1, 2, 3, 6, 7, 8, 9, 10, 15, 16, 17, 23, 24, 25, 31, 32, 33, 47, 48, 49, 63, 64, 65, 127,
    128, 129,
];

/// `len` distinct sorted values drawn from `lo..=hi` (all of them if the range is smaller).
fn exact_array(rng: &mut Rng, len: usize, lo: u16, hi: u16) -> Vec<u16> {
    let span = hi as usize - lo as usize + 1;
    if len >= span {
        return (lo..=hi).collect();
    }
    if len * 2 > span {
        // Dense: drop random values from the full range.
        let mut keep = vec![true; span];
        let mut drop = span - len;
        while drop > 0 {
            let i = rng.below(span as u64) as usize;
            if keep[i] {
                keep[i] = false;
                drop -= 1;
            }
        }
        return (0..span)
            .filter(|&i| keep[i])
            .map(|i| (lo as usize + i) as u16)
            .collect();
    }
    let mut v = sorted_array(rng, len, lo, hi);
    while v.len() < len {
        let extra = sorted_array(rng, len - v.len(), lo, hi);
        v.extend(extra);
        v.sort_unstable();
        v.dedup();
    }
    v
}

/// A random subset of `a` keeping roughly `per_mille / 1000` of its values.
fn subset(rng: &mut Rng, a: &[u16], per_mille: u64) -> Vec<u16> {
    a.iter()
        .copied()
        .filter(|_| rng.below(1000) < per_mille)
        .collect()
}

/// A random value range, often touching 0 or 65535.
fn random_range(rng: &mut Rng) -> (u16, u16) {
    match rng.below(5) {
        0 => (0, rng.below(3000) as u16 + 16),
        1 => (u16::MAX - 16 - rng.below(3000) as u16, u16::MAX),
        2 => (0, u16::MAX),
        _ => {
            let lo = rng.below(60000) as u16;
            (lo, lo + rng.below((u16::MAX - lo) as u64) as u16)
        }
    }
}

fn random_array_cases() -> Vec<(Vec<u16>, Vec<u16>)> {
    let mut rng = Rng(0x5EED_0103);
    let mut cases = Vec::new();
    // Edge lengths against edge lengths, overlapping in a small range (many matches) and spread
    // out (few matches), including the extreme values.
    for &la in &EDGE_LENGTHS {
        for &lb in &EDGE_LENGTHS {
            cases.push((
                exact_array(&mut rng, la, 0, 300),
                exact_array(&mut rng, lb, 0, 300),
            ));
            cases.push((
                exact_array(&mut rng, la, u16::MAX - 300, u16::MAX),
                exact_array(&mut rng, lb, u16::MAX - 300, u16::MAX),
            ));
            let (lo, hi) = random_range(&mut rng);
            cases.push((
                exact_array(&mut rng, la, lo, hi),
                exact_array(&mut rng, lb, lo, hi),
            ));
        }
    }
    // Random sizes up to 4096 with random densities.
    for _ in 0..300 {
        let (lo, hi) = random_range(&mut rng);
        let la = rng.below(4097) as usize;
        let lb = rng.below(4097) as usize;
        cases.push((
            exact_array(&mut rng, la, lo, hi),
            exact_array(&mut rng, lb, lo, hi),
        ));
    }
    // Subsets and supersets: the intersection fills its output buffer up to the last value.
    for _ in 0..150 {
        let len = rng.below(600) as usize + 1;
        let (lo, hi) = random_range(&mut rng);
        let a = exact_array(&mut rng, len, lo, hi);
        let per_mille = [1000, 999, 990, 900, 500, 100][rng.below(6) as usize];
        let b = subset(&mut rng, &a, per_mille);
        let extra_len = rng.below(20) as usize;
        let extra = exact_array(&mut rng, extra_len, lo, hi);
        let mut c = a.clone();
        c.extend(extra);
        c.sort_unstable();
        c.dedup();
        cases.push((a.clone(), b.clone()));
        cases.push((b, c));
    }
    // Alternating stretches of `run` consecutive values, so blocks advance in every pattern.
    for run in 1..=17u16 {
        for shift in [0u16, 1, run] {
            let (a, b): (Vec<u16>, Vec<u16>) = (0..2000u16)
                .map(|x| x + shift)
                .partition(|x| (x / run) % 2 == 0);
            cases.push((a.clone(), b.clone()));
            let dense: Vec<u16> = (0..2000).collect();
            cases.push((dense, b));
        }
    }
    // Skewed sizes around the galloping threshold.
    for _ in 0..60 {
        let small = rng.below(130) as usize + 1;
        let ratio = [GALLOP_RATIO - 1, GALLOP_RATIO, GALLOP_RATIO + 1, 100][rng.below(4) as usize];
        let large = (small * ratio + rng.below(3) as usize).min(65536);
        let (lo, hi) = random_range(&mut rng);
        let a = exact_array(&mut rng, small, lo, hi);
        let mut b = exact_array(&mut rng, large, lo, hi);
        // Make some of the small values common.
        let common = subset(&mut rng, &a, 500);
        b.extend(common);
        b.sort_unstable();
        b.dedup();
        cases.push((a, b));
    }
    cases
}

/// Checks every array kernel of `backend` against the naive definitions.
fn check_array_pair(backend: Impl, a: &[u16], b: &[u16]) {
    let ctx = || {
        format!(
            "backend={} |a|={} |b|={} a={:?}.. b={:?}..",
            backend.name(),
            a.len(),
            b.len(),
            &a[..a.len().min(12)],
            &b[..b.len().min(12)]
        )
    };
    let inter = naive_intersect(a, b);
    let got = run_guarded(a.len().min(b.len()), |out| {
        call!(backend, array_intersect, array_intersect_on(a, b, out))
    });
    assert_eq!(got, inter, "intersect {}", ctx());
    assert_eq!(
        call!(backend, array_intersect_card, array_intersect_card_on(a, b)),
        inter.len(),
        "intersect_card {}",
        ctx()
    );
    assert_eq!(
        call!(backend, array_intersects, array_intersects_on(a, b)),
        !inter.is_empty(),
        "intersects {}",
        ctx()
    );
    let got = run_guarded(a.len() + b.len(), |out| {
        call!(backend, array_union, array_union_on(a, b, out))
    });
    assert_eq!(got, naive_union(a, b), "union {}", ctx());
    let got = run_guarded(a.len(), |out| {
        call!(backend, array_difference, array_difference_on(a, b, out))
    });
    assert_eq!(got, naive_difference(a, b), "difference {}", ctx());
    let got = run_guarded(a.len() + b.len(), |out| {
        call!(backend, array_xor, array_xor_on(a, b, out))
    });
    assert_eq!(got, naive_xor(a, b), "xor {}", ctx());
}

#[test]
fn array_kernels_random_cases() {
    let cases = random_array_cases();
    for backend in backends() {
        for (a, b) in &cases {
            check_array_pair(backend, a, b);
            check_array_pair(backend, b, a);
        }
    }
}

/// Output buffers larger than the bound must give the same results.
#[test]
fn array_kernels_with_spare_output() {
    let mut rng = Rng(0x5EED_0104);
    for backend in backends() {
        for _ in 0..200 {
            let (lo, hi) = random_range(&mut rng);
            let len = EDGE_LENGTHS[rng.below(EDGE_LENGTHS.len() as u64) as usize] * 3;
            let a = exact_array(&mut rng, len, lo, hi);
            let b = subset(&mut rng, &a, 700);
            let spare = rng.below(20) as usize;
            let mut out = vec![MaybeUninit::new(0u16); a.len() + b.len() + spare];
            let only_a = naive_difference(&a, &b);
            let n = call!(
                backend,
                array_intersect,
                array_intersect_on(&a, &b, &mut out)
            );
            // SAFETY (all four): the kernel initialized the first `n` values.
            assert_eq!(
                unsafe { assume_init(&out, n) },
                &b[..],
                "{}",
                backend.name()
            );
            let n = call!(backend, array_union, array_union_on(&a, &b, &mut out));
            assert_eq!(
                unsafe { assume_init(&out, n) },
                &a[..],
                "{}",
                backend.name()
            );
            let n = call!(backend, array_xor, array_xor_on(&a, &b, &mut out));
            assert_eq!(
                unsafe { assume_init(&out, n) },
                &only_a[..],
                "{}",
                backend.name()
            );
            let n = call!(
                backend,
                array_difference,
                array_difference_on(&a, &b, &mut out)
            );
            assert_eq!(
                unsafe { assume_init(&out, n) },
                &only_a[..],
                "{}",
                backend.name()
            );
        }
    }
}

/// A bitmap whose bits are set with probability `per_mille / 1000`.
fn random_bitmap(rng: &mut Rng, per_mille: u64) -> Words {
    let mut w = [0u64; BITMAP_WORDS];
    for word in w.iter_mut() {
        for bit in 0..64 {
            if rng.below(1000) < per_mille {
                *word |= 1 << bit;
            }
        }
    }
    w
}

/// Random runs of random lengths (many crossing word and chunk boundaries).
fn runs_bitmap(rng: &mut Rng, max_len: u64) -> Words {
    let mut w = [0u64; BITMAP_WORDS];
    let mut pos = rng.below(100) as usize;
    while pos < 65536 {
        let len = rng.below(max_len) as usize + 1;
        for bit in pos..(pos + len).min(65536) {
            w[bit / 64] |= 1 << (bit % 64);
        }
        pos += len + rng.below(max_len) as usize + 1;
    }
    w
}

/// 64-byte chunks of mixed densities, so extraction switches between its code paths.
fn mixed_chunks_bitmap(rng: &mut Rng) -> Words {
    let mut w = [0u64; BITMAP_WORDS];
    for chunk in w.as_chunks_mut::<8>().0 {
        let kind = rng.below(6);
        for word in chunk.iter_mut() {
            *word = match kind {
                0 => 0,
                1 => 1 << rng.below(64),
                2 => rng.next() & rng.next() & rng.next() & rng.next(),
                3 => rng.next() & rng.next(),
                4 => rng.next(),
                _ => u64::MAX,
            };
        }
    }
    w
}

fn random_bitmap_cases() -> Vec<Words> {
    let mut rng = Rng(0x5EED_0105);
    let mut cases = Vec::new();
    for per_mille in [1, 5, 15, 31, 62, 100, 250, 500, 750, 900, 990, 999] {
        cases.push(random_bitmap(&mut rng, per_mille));
    }
    for max_len in [1, 2, 3, 7, 8, 9, 63, 64, 65, 200, 1000] {
        cases.push(runs_bitmap(&mut rng, max_len));
    }
    for _ in 0..6 {
        cases.push(mixed_chunks_bitmap(&mut rng));
    }
    // Set bits only in the last chunk / the first chunk.
    let mut last = [0u64; BITMAP_WORDS];
    last[BITMAP_WORDS - 8..].fill(u64::MAX);
    cases.push(last);
    let mut first = [0u64; BITMAP_WORDS];
    first[..8].fill(0x8000_0000_0000_0001);
    cases.push(first);
    cases
}

fn check_bitmap_single(backend: Impl, ia: usize, a: &Words) {
    let card = naive_popcount(a);
    assert_eq!(
        call!(backend, bitmap_popcount, bitmap_popcount_on(a)),
        card,
        "{} popcount #{ia}",
        backend.name()
    );
    assert_eq!(
        call!(backend, bitmap_count_runs, bitmap_count_runs_on(a)),
        naive_runs(a),
        "{} runs #{ia}",
        backend.name()
    );
    let expected: Vec<u16> = (0..=u16::MAX)
        .filter(|&i| a[i as usize / 64] >> (i % 64) & 1 == 1)
        .collect();
    for spare in [0, 1, 7, 8, 9, 64] {
        let got = run_guarded(card as usize + spare, |out| {
            call!(backend, bitmap_to_array, bitmap_to_array_on(a, out))
        });
        assert_eq!(
            got,
            expected,
            "{} to_array #{ia} spare={spare}",
            backend.name()
        );
    }
}

#[test]
fn bitmap_kernels_random_cases() {
    let cases = random_bitmap_cases();
    let ops = [BitOp::And, BitOp::Or, BitOp::Xor, BitOp::AndNot];
    for backend in backends() {
        for (ia, a) in cases.iter().enumerate() {
            check_bitmap_single(backend, ia, a);
        }
        // Neighbouring cases only (all pairs would be slow under emulation).
        for (ia, pair) in cases.windows(2).enumerate() {
            let (a, b) = (&pair[0], &pair[1]);
            for (x, y) in [(a, b), (b, a), (a, a)] {
                let ctx = || format!("backend={} pair #{ia}", backend.name());
                for op in ops {
                    let mut expected = [0u64; BITMAP_WORDS];
                    for i in 0..BITMAP_WORDS {
                        expected[i] = op.apply(x[i], y[i]);
                    }
                    let exp_card = naive_popcount(&expected);
                    let mut out = [0u64; BITMAP_WORDS];
                    assert_eq!(
                        call!(backend, bitmap_op, bitmap_op_on(op, x, y, &mut out)),
                        exp_card,
                        "op {op:?} {}",
                        ctx()
                    );
                    assert!(out == expected, "op {op:?} output {}", ctx());
                    let mut inplace = *x;
                    assert_eq!(
                        call!(
                            backend,
                            bitmap_op_inplace,
                            bitmap_op_inplace_on(op, &mut inplace, y)
                        ),
                        exp_card,
                        "inplace {op:?} {}",
                        ctx()
                    );
                    assert!(inplace == expected, "inplace {op:?} output {}", ctx());
                    let mut lazy = *x;
                    call!(
                        backend,
                        bitmap_op_inplace_nocard,
                        bitmap_op_inplace_nocard_on(op, &mut lazy, y)
                    );
                    assert!(lazy == expected, "nocard {op:?} output {}", ctx());
                    assert_eq!(
                        call!(backend, bitmap_op_card, bitmap_op_card_on(op, x, y)),
                        exp_card,
                        "card {op:?} {}",
                        ctx()
                    );
                }
                assert_eq!(
                    call!(backend, bitmap_intersects, bitmap_intersects_on(x, y)),
                    x.iter().zip(y.iter()).any(|(p, q)| p & q != 0),
                    "intersects {}",
                    ctx()
                );
                assert_eq!(
                    call!(backend, bitmap_is_subset, bitmap_is_subset_on(x, y)),
                    x.iter().zip(y.iter()).all(|(p, q)| p & !q == 0),
                    "subset {}",
                    ctx()
                );
            }
        }
    }
}

/// Constructed tails: a side whose last (partial) block holds outliers while the other side goes
/// on, values around the top of the range (where vector code may reserve padding values), and
/// long stretches between the values of a short side.
#[test]
fn array_kernels_constructed_tails() {
    let mut rng = Rng(0x5EED_0108);
    let mut cases: Vec<(Vec<u16>, Vec<u16>)> = Vec::new();
    for head in [8usize, 9, 15, 16, 17, 24] {
        for outliers in [
            &[60000u16][..],
            &[30000, 60000],
            &[65527],
            &[65528],
            &[65535],
        ] {
            let mut a: Vec<u16> = (0..head as u16).map(|x| x * 3).collect();
            a.extend_from_slice(outliers);
            for (lo, len) in [(0u16, 40usize), (10, 400), (100, 4000), (0, 64)] {
                let b: Vec<u16> = (lo..).step_by(7).take(len).collect();
                cases.push((a.clone(), b));
            }
        }
    }
    for top in [8usize, 9, 16, 30] {
        let a: Vec<u16> = (u16::MAX - top as u16 + 1..=u16::MAX).collect();
        let b = exact_array(&mut rng, 40, u16::MAX - 100, u16::MAX);
        let c = exact_array(&mut rng, 400, 0, u16::MAX);
        cases.push((a.clone(), b));
        cases.push((a, c));
    }
    for gap in [1u16, 31, 32, 33, 127, 128, 129, 200, 5000] {
        let small: Vec<u16> = (0..8u16).map(|x| x * gap).collect();
        let large = exact_array(&mut rng, 4096, 0, u16::MAX);
        let dense: Vec<u16> = (0..=u16::MAX).filter(|x| x % 3 != 0).collect();
        cases.push((small.clone(), large));
        cases.push((small, dense));
    }
    for backend in backends() {
        for (a, b) in &cases {
            check_array_pair(backend, a, b);
            check_array_pair(backend, b, a);
        }
    }
}

/// Early-exit kernels must look at every position: a single bit anywhere decides.
#[test]
fn bitmap_early_exit_every_word() {
    let full = [u64::MAX; BITMAP_WORDS];
    for backend in backends() {
        for word in 0..BITMAP_WORDS {
            for bit in [0usize, 31, 63] {
                let mut single = [0u64; BITMAP_WORDS];
                single[word] = 1 << bit;
                let mut holed = full;
                holed[word] &= !(1 << bit);
                let ctx = || format!("{} word {word} bit {bit}", backend.name());
                let intersects =
                    |x, y| call!(backend, bitmap_intersects, bitmap_intersects_on(x, y));
                let subset = |x, y| call!(backend, bitmap_is_subset, bitmap_is_subset_on(x, y));
                assert!(intersects(&single, &full), "{}", ctx());
                assert!(!intersects(&single, &holed), "{}", ctx());
                assert!(subset(&single, &full), "{}", ctx());
                assert!(!subset(&single, &holed), "{}", ctx());
                assert!(!subset(&full, &holed), "{}", ctx());
            }
        }
    }
}

/// A too small output buffer must panic without writing past its end (the kernels are reachable
/// from safe code).
#[cfg(feature = "std")]
#[test]
fn bitmap_to_array_short_output_panics_in_bounds() {
    const GUARD: usize = 128;
    const SENTINEL: u16 = 0x5A5A;
    let mut rng = Rng(0x5EED_0106);
    let cases = [
        random_bitmap(&mut rng, 3),
        random_bitmap(&mut rng, 500),
        mixed_chunks_bitmap(&mut rng),
    ];
    for backend in backends() {
        for a in &cases {
            let card = naive_popcount(a) as usize;
            for short in [1, 2, 9, card / 2] {
                let bound = card - short;
                let mut buf = vec![MaybeUninit::new(SENTINEL); bound + GUARD];
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    call!(
                        backend,
                        bitmap_to_array,
                        bitmap_to_array_on(a, &mut buf[..bound])
                    )
                }));
                assert!(
                    result.is_err(),
                    "{} accepted a short buffer",
                    backend.name()
                );
                for g in &buf[bound..] {
                    // SAFETY: the guard zone was initialized with the sentinel.
                    assert_eq!(unsafe { g.assume_init() }, SENTINEL, "{}", backend.name());
                }
            }
        }
    }
}

/// Rough per-kernel timings of every backend, for tuning on real hardware (emulated timings are
/// meaningless). Run with
/// `cargo test --release --lib simd::tests::kernel_timings -- --ignored --nocapture`.
#[cfg(feature = "std")]
#[test]
#[ignore = "timing report, run manually in release mode"]
fn kernel_timings() {
    use core::hint::black_box;
    use std::time::{Duration, Instant};

    fn time(label: &str, backend: Impl, mut f: impl FnMut() -> usize) {
        let mut iters = 1u64;
        loop {
            let start = Instant::now();
            for _ in 0..iters {
                black_box(f());
            }
            let elapsed = start.elapsed();
            if elapsed >= Duration::from_millis(50) || iters >= 1 << 30 {
                let ns = elapsed.as_nanos() as f64 / iters as f64;
                std::println!("{label:<40} {:<10} {ns:>12.1} ns", backend.name());
                return;
            }
            iters *= 4;
        }
    }

    let mut rng = Rng(0x5EED_0107);
    let sparse = random_bitmap(&mut rng, 10);
    let medium = random_bitmap(&mut rng, 62);
    let dense = random_bitmap(&mut rng, 500);
    let runs = runs_bitmap(&mut rng, 64);
    let mut disjoint = dense;
    for (x, y) in disjoint.iter_mut().zip(medium.iter()) {
        *x &= !y;
    }
    let evens: Vec<u16> = (0..4096).map(|x| x * 2).collect();
    let odds: Vec<u16> = (0..4096).map(|x| x * 2 + 1).collect();
    let arrays = [
        (
            "1000x1000 sparse",
            exact_array(&mut rng, 1000, 0, u16::MAX),
            exact_array(&mut rng, 1000, 0, u16::MAX),
        ),
        (
            "4096x4096 dense",
            exact_array(&mut rng, 4096, 0, 8191),
            exact_array(&mut rng, 4096, 0, 8191),
        ),
        ("4096x4096 disjoint", evens, odds),
        (
            "200x4000",
            exact_array(&mut rng, 200, 0, u16::MAX),
            exact_array(&mut rng, 4000, 0, u16::MAX),
        ),
        (
            "64x4096 skewed",
            exact_array(&mut rng, 64, 0, u16::MAX),
            exact_array(&mut rng, 4096, 0, u16::MAX),
        ),
        (
            "12x40",
            exact_array(&mut rng, 12, 0, 200),
            exact_array(&mut rng, 40, 0, 200),
        ),
    ];
    let mut words = [0u64; BITMAP_WORDS];
    let mut out = vec![MaybeUninit::new(0u16); 65536];
    // 16 inputs per density, cycled through: a single repeated input lets the branch predictors
    // learn it, which flatters branchy decoders.
    let mut cycle_rng = Rng(0x5EED_0109);
    let sparse_set: Vec<Words> = (0..16).map(|_| random_bitmap(&mut cycle_rng, 10)).collect();
    let medium_set: Vec<Words> = (0..16).map(|_| random_bitmap(&mut cycle_rng, 62)).collect();

    for backend in backends() {
        for (name, a) in [
            ("1%", &sparse),
            ("6%", &medium),
            ("50%", &dense),
            ("runs", &runs),
        ] {
            time(&format!("bitmap_popcount {name}"), backend, || {
                call!(backend, bitmap_popcount, bitmap_popcount_on(a)) as usize
            });
            time(&format!("bitmap_to_array {name}"), backend, || {
                call!(backend, bitmap_to_array, bitmap_to_array_on(a, &mut out))
            });
            time(&format!("bitmap_count_runs {name}"), backend, || {
                call!(backend, bitmap_count_runs, bitmap_count_runs_on(a)) as usize
            });
        }
        for (name, set) in [("1% x16", &sparse_set), ("6% x16", &medium_set)] {
            let mut next = 0;
            time(&format!("bitmap_to_array {name}"), backend, || {
                next = (next + 1) % set.len();
                call!(
                    backend,
                    bitmap_to_array,
                    bitmap_to_array_on(&set[next], &mut out)
                )
            });
        }
        time("bitmap_op and", backend, || {
            let op = BitOp::And;
            call!(
                backend,
                bitmap_op,
                bitmap_op_on(op, &dense, &medium, &mut words)
            ) as usize
        });
        time("bitmap_op_inplace or", backend, || {
            let op = BitOp::Or;
            call!(
                backend,
                bitmap_op_inplace,
                bitmap_op_inplace_on(op, &mut words, &medium)
            ) as usize
        });
        time("bitmap_op_inplace_nocard xor", backend, || {
            let op = BitOp::Xor;
            call!(
                backend,
                bitmap_op_inplace_nocard,
                bitmap_op_inplace_nocard_on(op, &mut words, &medium)
            );
            0
        });
        time("bitmap_op_card andnot", backend, || {
            let op = BitOp::AndNot;
            call!(
                backend,
                bitmap_op_card,
                bitmap_op_card_on(op, &dense, &medium)
            ) as usize
        });
        time("bitmap_intersects (false, full scan)", backend, || {
            call!(
                backend,
                bitmap_intersects,
                bitmap_intersects_on(&disjoint, &medium)
            ) as usize
        });
        time("bitmap_is_subset (true, full scan)", backend, || {
            call!(
                backend,
                bitmap_is_subset,
                bitmap_is_subset_on(&disjoint, &dense)
            ) as usize
        });
        for (name, a, b) in &arrays {
            let (a, b) = (&a[..], &b[..]);
            time(&format!("array_intersect {name}"), backend, || {
                call!(backend, array_intersect, array_intersect_on(a, b, &mut out))
            });
            time(&format!("array_intersect_card {name}"), backend, || {
                call!(backend, array_intersect_card, array_intersect_card_on(a, b))
            });
            time(&format!("array_intersects {name}"), backend, || {
                call!(backend, array_intersects, array_intersects_on(a, b)) as usize
            });
            time(&format!("array_union {name}"), backend, || {
                call!(backend, array_union, array_union_on(a, b, &mut out))
            });
            time(&format!("array_difference {name}"), backend, || {
                call!(
                    backend,
                    array_difference,
                    array_difference_on(a, b, &mut out)
                )
            });
            time(&format!("array_xor {name}"), backend, || {
                call!(backend, array_xor, array_xor_on(a, b, &mut out))
            });
        }
    }
}
