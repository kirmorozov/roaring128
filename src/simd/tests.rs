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
            check_array_pair(backend, a, b);
        }
    }
}

fn bitmap_cases() -> Vec<Words> {
    let mut rng = Rng(0x5EED_0002);
    let mut cases: Vec<Words> = vec![
        [0; BITMAP_WORDS],
        [u64::MAX; BITMAP_WORDS],
        [0xAAAA_AAAA_AAAA_AAAA; BITMAP_WORDS],
        [0x5555_5555_5555_5555; BITMAP_WORDS],
        [1; BITMAP_WORDS],
        [1 << 63; BITMAP_WORDS],
    ];
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

/// With `ROARING128_EXPECT_BACKEND=<name>` (`scripts/test-aarch64.sh` sets it to `neon`, the
/// Intel SDE jobs to `avx512`) the detected backend must be exactly that one, so an emulated run
/// that silently falls back to a lower tier fails. On AArch64 the backend is always NEON.
#[cfg(feature = "std")]
#[test]
fn expected_backend_is_selected() {
    #[cfg(target_arch = "aarch64")]
    {
        assert!(std::arch::is_aarch64_feature_detected!("neon"));
        assert!(SimdBackend::Neon.is_supported());
        assert_eq!(simd_backend(), SimdBackend::Neon);
    }
    let Ok(expected) = std::env::var("ROARING128_EXPECT_BACKEND") else {
        return;
    };
    #[cfg(target_arch = "x86_64")]
    {
        for (name, detected) in [
            ("avx2", std::is_x86_feature_detected!("avx2")),
            ("bmi2", std::is_x86_feature_detected!("bmi2")),
            ("avx512f", std::is_x86_feature_detected!("avx512f")),
            ("avx512bw", std::is_x86_feature_detected!("avx512bw")),
            ("avx512vl", std::is_x86_feature_detected!("avx512vl")),
            ("avx512dq", std::is_x86_feature_detected!("avx512dq")),
            (
                "avx512vpopcntdq",
                std::is_x86_feature_detected!("avx512vpopcntdq"),
            ),
            (
                "avx512bitalg",
                std::is_x86_feature_detected!("avx512bitalg"),
            ),
            ("avx512vbmi2", std::is_x86_feature_detected!("avx512vbmi2")),
        ] {
            std::eprintln!("cpu feature {name:<16} {detected}");
        }
    }
    let backend = simd_backend();
    std::eprintln!("simd backend: {}", backend.name());
    assert_eq!(
        backend.name(),
        expected.to_ascii_lowercase(),
        "unexpected SIMD backend"
    );
    assert!(backends().any(|b| b.name() == backend.name()));
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

/// Checks every array kernel of `backend` on `(a, b)` against the naive definitions, with
/// exact-size guarded output buffers.
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

// ---------------------------------------------------------------------------------------------
// Randomized shapes
// ---------------------------------------------------------------------------------------------

/// Exactly `len` distinct sorted values from `lo..=hi` (requires `len <= hi - lo + 1`). The
/// x86 kernel tests' generator; `exact_array` (NEON kernel tests) also accepts a short range.
fn exact_array_strict(rng: &mut Rng, len: usize, lo: u16, hi: u16) -> Vec<u16> {
    let span = hi as usize - lo as usize + 1;
    assert!(len <= span);
    if len * 2 > span {
        // Dense: drop random values from the full range.
        let mut v: Vec<u16> = (lo..=hi).collect();
        while v.len() > len {
            let i = rng.below(v.len() as u64) as usize;
            v.swap_remove(i);
        }
        v.sort_unstable();
        return v;
    }
    let mut seen = vec![false; span];
    let mut v = Vec::with_capacity(len);
    while v.len() < len {
        let x = rng.below(span as u64) as usize;
        if !seen[x] {
            seen[x] = true;
            v.push((lo as usize + x) as u16);
        }
    }
    v.sort_unstable();
    v
}

/// A random array of `len` values: random density, anywhere or clustered at either end.
fn random_array(rng: &mut Rng, len: usize) -> Vec<u16> {
    let len = len.min(65536);
    let span_for = |shift: u64| (len.max(1) << shift).clamp(len.max(1), 65536);
    match rng.below(6) {
        // Clustered at 0.
        0 => {
            let span = span_for(rng.below(4));
            exact_array_strict(rng, len, 0, (span - 1) as u16)
        }
        // Clustered at 65535.
        1 => {
            let span = span_for(rng.below(4));
            exact_array_strict(rng, len, (65536 - span) as u16, u16::MAX)
        }
        // Anywhere, random density.
        2 | 3 => {
            let span = span_for(rng.below(8));
            let lo = rng.below((65536 - span + 1) as u64) as u16;
            exact_array_strict(rng, len, lo, (lo as usize + span - 1) as u16)
        }
        // Full range.
        _ => exact_array_strict(rng, len, 0, u16::MAX),
    }
}

/// Derives an array of about `len` values from `a`: a random half of `a` plus fresh values.
fn related_array(rng: &mut Rng, a: &[u16], len: usize) -> Vec<u16> {
    let mut v: Vec<u16> = a.iter().copied().filter(|_| rng.below(2) == 0).collect();
    v.truncate(len);
    let extra = random_array(rng, len - v.len());
    v.extend(extra);
    v.sort_unstable();
    v.dedup();
    v
}

#[test]
fn array_kernels_random_shapes() {
    let mut rng = Rng(0x5EED_0003);
    let widths = [
        0usize, 1, 2, 7, 8, 9, 15, 16, 17, 23, 24, 25, 31, 32, 33, 63, 64, 65, 127, 128, 129,
    ];
    let mut cases: Vec<(Vec<u16>, Vec<u16>)> = Vec::new();
    // Lengths around the vector widths, independent and related values.
    for &la in &widths {
        for &lb in &widths {
            for _ in 0..2 {
                let a = random_array(&mut rng, la);
                let b = random_array(&mut rng, lb);
                let c = related_array(&mut rng, &a, lb);
                cases.push((a.clone(), b));
                cases.push((c.clone(), a.clone()));
                cases.push((a, c));
            }
        }
    }
    // Random sizes up to the array container limit.
    for _ in 0..300 {
        let la = rng.below(4097) as usize;
        let lb = rng.below(4097) as usize;
        let a = random_array(&mut rng, la);
        let b = if rng.below(2) == 0 {
            random_array(&mut rng, lb)
        } else {
            related_array(&mut rng, &a, lb)
        };
        cases.push((a, b));
    }
    // Skewed sizes around the galloping thresholds.
    for _ in 0..100 {
        let small = 1 + rng.below(200) as usize;
        let large = (small * (8 + rng.below(100) as usize)).min(8192);
        let a = random_array(&mut rng, small);
        let b = if rng.below(2) == 0 {
            random_array(&mut rng, large)
        } else {
            related_array(&mut rng, &a, large)
        };
        cases.push((b.clone(), a.clone()));
        cases.push((a, b));
    }
    // Interleaved values, so that both sides advance alternately.
    for step in [1u16, 2, 3, 8, 9] {
        let a: Vec<u16> = (0..4000u16)
            .map(|x| x * step)
            .take_while(|&x| x < 65000)
            .collect();
        let b: Vec<u16> = a.iter().map(|&x| x + 1).collect();
        cases.push((a.clone(), b.clone()));
        cases.push((b, a));
    }
    for backend in backends() {
        for (a, b) in &cases {
            check_array_pair(backend, a, b);
        }
    }
}

#[test]
fn bitmap_to_array_random_tails() {
    // Few set bits near the end of the container and at word boundaries: exercises the switch
    // from the unrolled stores to the bounded tail (the output has exactly the cardinality).
    let mut rng = Rng(0x5EED_0004);
    let mut cases: Vec<Words> = Vec::new();
    for card in [1usize, 7, 8, 9, 31, 32, 33, 63, 64, 65, 100, 200, 5000] {
        for start in [0usize, 65536 - 2048, 65536 - 128] {
            let mut w = [0u64; BITMAP_WORDS];
            let span = (65536 - start).min(card * 4).max(card);
            let start = start.min(65536 - span);
            for v in exact_array_strict(&mut rng, card, start as u16, (start + span - 1) as u16) {
                w[v as usize / 64] |= 1 << (v % 64);
            }
            cases.push(w);
        }
    }
    // Uniform densities around the decoder switch points (4 and 8 bits per word).
    for card in [3000usize, 4096, 4500, 6000, 8192, 9000, 12000] {
        let mut w = [0u64; BITMAP_WORDS];
        for v in exact_array_strict(&mut rng, card, 0, u16::MAX) {
            w[v as usize / 64] |= 1 << (v % 64);
        }
        cases.push(w);
    }
    for dense_words in [1usize, 2, 3, 15] {
        let mut w = [0u64; BITMAP_WORDS];
        for x in w[BITMAP_WORDS - dense_words..].iter_mut() {
            *x = rng.next() | 1;
        }
        w[3] = u64::MAX;
        cases.push(w);
    }
    for backend in backends() {
        for (ia, a) in cases.iter().enumerate() {
            let card = naive_popcount(a);
            let got = run_guarded(card as usize, |out| {
                call!(backend, bitmap_to_array, bitmap_to_array_on(a, out))
            });
            let expected: Vec<u16> = (0..=u16::MAX)
                .filter(|&i| a[i as usize / 64] >> (i % 64) & 1 == 1)
                .collect();
            assert_eq!(got, expected, "{} to_array case #{ia}", backend.name());
            assert_eq!(
                call!(backend, bitmap_count_runs, bitmap_count_runs_on(a)),
                naive_runs(a),
                "{} runs case #{ia}",
                backend.name()
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Out-of-bounds accesses
// ---------------------------------------------------------------------------------------------

/// Anonymous memory surrounded by inaccessible guard pages (raw Linux syscalls, no libc
/// dependency): any access outside the data region kills the test process.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod guard_pages {
    /// Size of the regions: a multiple of every Linux page size (4, 16 and 64 KiB), large
    /// enough for a full bitmap container decoded to an array.
    const UNIT: usize = 256 * 1024;
    const PROT_NONE: usize = 0;
    const PROT_READ_WRITE: usize = 1 | 2;
    const MAP_PRIVATE_ANONYMOUS: usize = 0x02 | 0x20;

    /// `mmap`, `mprotect`, `munmap`.
    #[cfg(target_arch = "x86_64")]
    const SYS: (usize, usize, usize) = (9, 10, 11);
    #[cfg(target_arch = "aarch64")]
    const SYS: (usize, usize, usize) = (222, 226, 215);

    unsafe fn syscall6(n: usize, a: [usize; 6]) -> isize {
        let ret: isize;
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::asm!(
                "syscall",
                inlateout("rax") n as isize => ret,
                in("rdi") a[0], in("rsi") a[1], in("rdx") a[2],
                in("r10") a[3], in("r8") a[4], in("r9") a[5],
                lateout("rcx") _, lateout("r11") _,
                options(nostack),
            );
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            core::arch::asm!(
                "svc 0",
                in("x8") n,
                inlateout("x0") a[0] as isize => ret,
                in("x1") a[1], in("x2") a[2], in("x3") a[3], in("x4") a[4], in("x5") a[5],
                options(nostack),
            );
        }
        ret
    }

    /// `[guard][data: LEN bytes][guard]`.
    pub(super) struct Guarded {
        base: *mut u8,
    }

    impl Guarded {
        /// Size of the accessible region in bytes.
        pub(super) const LEN: usize = UNIT;

        pub(super) fn new() -> Guarded {
            // SAFETY: an anonymous mapping and protection changes inside it.
            unsafe {
                let base = syscall6(
                    SYS.0,
                    [
                        0,
                        3 * UNIT,
                        PROT_READ_WRITE,
                        MAP_PRIVATE_ANONYMOUS,
                        usize::MAX,
                        0,
                    ],
                );
                assert!(base > 0, "mmap failed: {base}");
                let base = base as *mut u8;
                for guard in [base, base.add(2 * UNIT)] {
                    let r = syscall6(SYS.1, [guard as usize, UNIT, PROT_NONE, 0, 0, 0]);
                    assert_eq!(r, 0, "mprotect failed");
                }
                Guarded { base }
            }
        }

        /// The first accessible byte.
        pub(super) fn data(&mut self) -> *mut u8 {
            // SAFETY: inside the mapping.
            unsafe { self.base.add(UNIT) }
        }
    }

    impl Drop for Guarded {
        fn drop(&mut self) {
            // SAFETY: unmaps the region created in `new`.
            unsafe { syscall6(SYS.2, [self.base as usize, 3 * UNIT, 0, 0, 0, 0]) };
        }
    }
}

/// Every kernel runs on inputs placed directly after and directly before an inaccessible page,
/// with output buffers that end at an inaccessible page: reading or writing a single element
/// outside the slices crashes the test. (Masked AVX-512 loads and stores must not fault on
/// masked-off lanes either.)
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
fn kernels_stay_within_bounds() {
    use guard_pages::Guarded;
    /// Capacity of a region in `u16` values.
    const CAP: usize = Guarded::LEN / 2;

    /// Copies `src` to the start or to the end of the region.
    fn place<'a>(region: &'a mut Guarded, src: &[u16], at_end: bool) -> &'a [u16] {
        let offset = if at_end { CAP - src.len() } else { 0 };
        // SAFETY: the region holds `CAP` values.
        unsafe {
            let p = region.data().cast::<u16>().add(offset);
            core::ptr::copy_nonoverlapping(src.as_ptr(), p, src.len());
            core::slice::from_raw_parts(p, src.len())
        }
    }

    /// An output buffer of `len` values ending at the trailing guard page.
    fn out_slice(region: &mut Guarded, len: usize) -> &mut [MaybeUninit<u16>] {
        // SAFETY: the region holds `CAP` values.
        unsafe {
            let p = region.data().cast::<MaybeUninit<u16>>().add(CAP - len);
            core::slice::from_raw_parts_mut(p, len)
        }
    }

    /// A bitmap at the start or at the end of the region.
    fn words(region: &mut Guarded, at_end: bool) -> &mut Words {
        let offset = if at_end {
            Guarded::LEN - size_of::<Words>()
        } else {
            0
        };
        // SAFETY: inside the region and 8-byte aligned.
        unsafe { &mut *region.data().add(offset).cast::<Words>() }
    }

    let (mut ra, mut rb, mut ro) = (Guarded::new(), Guarded::new(), Guarded::new());
    let mut rng = Rng(0x5EED_0005);
    let mut lens = vec![
        0usize, 1, 2, 3, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 100, 1000, 4096,
    ];
    lens.extend((0..10).map(|_| rng.below(4097) as usize));
    let bitmaps = bitmap_cases();
    for backend in backends() {
        for &la in &lens {
            for &lb in &lens {
                let a0 = random_array(&mut rng, la);
                let b0 = if rng.below(2) == 0 {
                    random_array(&mut rng, lb)
                } else {
                    related_array(&mut rng, &a0, lb)
                };
                for (a_end, b_end) in [(true, true), (false, false), (true, false), (false, true)] {
                    let a = place(&mut ra, &a0, a_end);
                    let b = place(&mut rb, &b0, b_end);
                    let out = out_slice(&mut ro, a.len().min(b.len()));
                    let n = call!(backend, array_intersect, array_intersect_on(a, b, out));
                    assert_eq!(n, naive_intersect(a, b).len(), "{}", backend.name());
                    call!(backend, array_intersect_card, array_intersect_card_on(a, b));
                    call!(backend, array_intersects, array_intersects_on(a, b));
                    let out = out_slice(&mut ro, a.len() + b.len());
                    call!(backend, array_union, array_union_on(a, b, out));
                    let out = out_slice(&mut ro, a.len());
                    call!(backend, array_difference, array_difference_on(a, b, out));
                    let out = out_slice(&mut ro, a.len() + b.len());
                    call!(backend, array_xor, array_xor_on(a, b, out));
                }
            }
        }

        // Bitmaps at both ends of their regions.
        for (a0, b0) in bitmaps.iter().zip(bitmaps.iter().rev()) {
            for at_end in [false, true] {
                let b = words(&mut rb, !at_end);
                *b = *b0;
                let b: &Words = b;
                {
                    let a = words(&mut ra, at_end);
                    *a = *a0;
                    let a: &Words = a;
                    let card = call!(backend, bitmap_popcount, bitmap_popcount_on(a));
                    call!(backend, bitmap_count_runs, bitmap_count_runs_on(a));
                    call!(backend, bitmap_intersects, bitmap_intersects_on(a, b));
                    call!(backend, bitmap_is_subset, bitmap_is_subset_on(a, b));
                    for op in [BitOp::And, BitOp::Or, BitOp::Xor, BitOp::AndNot] {
                        call!(backend, bitmap_op_card, bitmap_op_card_on(op, a, b));
                        let out = words(&mut ro, at_end);
                        call!(backend, bitmap_op, bitmap_op_on(op, a, b, out));
                    }
                    let out = out_slice(&mut ro, card as usize);
                    let n = call!(backend, bitmap_to_array, bitmap_to_array_on(a, out));
                    assert_eq!(n, card as usize, "{}", backend.name());
                }
                let a = words(&mut ra, at_end);
                call!(
                    backend,
                    bitmap_op_inplace,
                    bitmap_op_inplace_on(BitOp::Xor, a, b)
                );
                call!(
                    backend,
                    bitmap_op_inplace_nocard,
                    bitmap_op_inplace_nocard_on(BitOp::Or, a, b)
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Timings
// ---------------------------------------------------------------------------------------------

/// The scalar reference re-instantiated with the AVX2 tier's target features: what the compiler
/// achieves with auto-vectorization alone (the previous placeholder backend).
#[cfg(target_arch = "x86_64")]
mod autovec {
    use super::*;

    crate::simd::x86::scalar_kernels! { "avx2,bmi1,bmi2,popcnt,sse4.2" =>
        bitmap_popcount(a: &Words) -> u32;
        bitmap_op(op: BitOp, a: &Words, b: &Words, out: &mut Words) -> u32;
        bitmap_op_inplace_nocard(op: BitOp, a: &mut Words, b: &Words);
        bitmap_op_card(op: BitOp, a: &Words, b: &Words) -> u32;
        bitmap_intersects(a: &Words, b: &Words) -> bool;
        bitmap_is_subset(a: &Words, b: &Words) -> bool;
        bitmap_to_array(a: &Words, out: &mut [MaybeUninit<u16>]) -> usize;
        bitmap_count_runs(a: &Words) -> u32;
        array_intersect(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize;
        array_intersect_card(a: &[u16], b: &[u16]) -> usize;
        array_intersects(a: &[u16], b: &[u16]) -> bool;
        array_union(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize;
        array_difference(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize;
        array_xor(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize;
    }
}

/// Best-of-N nanoseconds per call of `f`.
#[cfg(feature = "std")]
fn time_ns(mut f: impl FnMut()) -> f64 {
    use std::time::{Duration, Instant};
    for _ in 0..20 {
        f();
    }
    let start = Instant::now();
    let mut calls = 0u64;
    while start.elapsed() < Duration::from_millis(5) {
        f();
        calls += 1;
    }
    let per_round = calls.max(1);
    let mut best = f64::MAX;
    for _ in 0..15 {
        let start = Instant::now();
        for _ in 0..per_round {
            f();
        }
        best = best.min(start.elapsed().as_nanos() as f64 / per_round as f64);
    }
    best
}

/// Prints one row: the reference, the auto-vectorized reference and every supported backend.
#[cfg(feature = "std")]
macro_rules! bench_row {
    ($label:expr, $name:ident, $name_on:ident ($($arg:expr),*)) => {{
        use core::hint::black_box;
        let reference = time_ns(|| {
            black_box(scalar::$name($(black_box($arg)),*));
        });
        let mut row = std::format!("{:<44} {:>10.1}", $label, reference);
        #[cfg(target_arch = "x86_64")]
        {
            // SAFETY: the AVX2 tier is the x86_64 minimum (checked by `backends()` users).
            let auto = time_ns(|| unsafe {
                black_box(autovec::$name($(black_box($arg)),*));
            });
            row += &std::format!(" {:>10.1}", auto);
        }
        for backend in ALL_BACKENDS.into_iter().filter(|b| b.is_supported()) {
            let t = time_ns(|| {
                black_box($name_on(backend, $(black_box($arg)),*));
            });
            row += &std::format!(" {:>10.1} ({:>5.1}x)", t, reference / t);
        }
        std::println!("{row}");
    }};
}

/// Kernel timings against the scalar reference, in ns per call. Run with
/// `cargo test --release --lib simd -- --ignored --nocapture`.
#[cfg(feature = "std")]
#[test]
#[ignore = "timing report, run explicitly in release mode"]
fn kernel_speedups() {
    let mut rng = Rng(0x5EED_0006);
    let mut header = std::format!("{:<44} {:>10}", "kernel / shape (ns per call)", "reference");
    #[cfg(target_arch = "x86_64")]
    {
        header += &std::format!(" {:>10}", "autovec");
    }
    for backend in ALL_BACKENDS.into_iter().filter(|b| b.is_supported()) {
        header += &std::format!(" {:>10} {:>7}", backend.name(), "speedup");
    }
    std::println!("{header}");

    let random_words = |rng: &mut Rng, ands: u32| -> Words {
        let mut w = [0u64; BITMAP_WORDS];
        for x in w.iter_mut() {
            *x = (0..ands).fold(u64::MAX, |acc, _| acc & rng.next());
        }
        w
    };
    let dense_a = random_words(&mut rng, 1);
    let dense_b = random_words(&mut rng, 1);
    let mut sparse = [0u64; BITMAP_WORDS];
    for v in exact_array_strict(&mut rng, 5000, 0, u16::MAX) {
        sparse[v as usize / 64] |= 1 << (v % 64);
    }
    let mut out_words = [0u64; BITMAP_WORDS];
    let mut out = vec![MaybeUninit::new(0u16); 65536 + 64];
    let disjoint = {
        let mut w = dense_a;
        for x in w.iter_mut() {
            *x = !*x;
        }
        w
    };

    bench_row!(
        "bitmap_popcount",
        bitmap_popcount,
        bitmap_popcount_on(&dense_a)
    );
    bench_row!(
        "bitmap_op And (with card)",
        bitmap_op,
        bitmap_op_on(BitOp::And, &dense_a, &dense_b, &mut out_words)
    );
    bench_row!(
        "bitmap_op Xor (with card)",
        bitmap_op,
        bitmap_op_on(BitOp::Xor, &dense_a, &dense_b, &mut out_words)
    );
    bench_row!(
        "bitmap_op_card And",
        bitmap_op_card,
        bitmap_op_card_on(BitOp::And, &dense_a, &dense_b)
    );
    bench_row!(
        "bitmap_op_inplace_nocard Or",
        bitmap_op_inplace_nocard,
        bitmap_op_inplace_nocard_on(BitOp::Or, &mut out_words, &dense_b)
    );
    bench_row!(
        "bitmap_intersects (disjoint, full scan)",
        bitmap_intersects,
        bitmap_intersects_on(&dense_a, &disjoint)
    );
    bench_row!(
        "bitmap_is_subset (subset, full scan)",
        bitmap_is_subset,
        bitmap_is_subset_on(&dense_a, &dense_a)
    );
    bench_row!(
        "bitmap_to_array dense (32k bits)",
        bitmap_to_array,
        bitmap_to_array_on(&dense_a, &mut out)
    );
    bench_row!(
        "bitmap_to_array sparse (5000 bits)",
        bitmap_to_array,
        bitmap_to_array_on(&sparse, &mut out)
    );
    bench_row!(
        "bitmap_count_runs dense",
        bitmap_count_runs,
        bitmap_count_runs_on(&dense_a)
    );

    let a1000 = exact_array_strict(&mut rng, 1000, 0, 1999);
    let b1000 = exact_array_strict(&mut rng, 1000, 0, 1999);
    let a4000 = exact_array_strict(&mut rng, 4000, 0, 7999);
    let b4000 = exact_array_strict(&mut rng, 4000, 0, 7999);
    let a64 = exact_array_strict(&mut rng, 64, 0, 7999);
    let a2000 = exact_array_strict(&mut rng, 2000, 0, 7999);
    let b2000 = exact_array_strict(&mut rng, 2000, 0, 7999);
    let s4000 = exact_array_strict(&mut rng, 4000, 0, u16::MAX);
    let t4000 = exact_array_strict(&mut rng, 4000, 0, u16::MAX);
    let a200 = exact_array_strict(&mut rng, 200, 0, 7999);
    let (a1000, b1000, a4000, b4000, a64, a2000, b2000) = (
        &a1000[..],
        &b1000[..],
        &a4000[..],
        &b4000[..],
        &a64[..],
        &a2000[..],
        &b2000[..],
    );
    let (s4000, t4000, a200) = (&s4000[..], &t4000[..], &a200[..]);

    bench_row!(
        "array_intersect 1000x1000 (50% dense)",
        array_intersect,
        array_intersect_on(a1000, b1000, &mut out)
    );
    bench_row!(
        "array_intersect 4000x4000 (50% dense)",
        array_intersect,
        array_intersect_on(a4000, b4000, &mut out)
    );
    bench_row!(
        "array_intersect 4000x4000 (sparse)",
        array_intersect,
        array_intersect_on(s4000, t4000, &mut out)
    );
    bench_row!(
        "array_intersect 200x4000",
        array_intersect,
        array_intersect_on(a200, b4000, &mut out)
    );
    bench_row!(
        "array_intersect 64x4000",
        array_intersect,
        array_intersect_on(a64, b4000, &mut out)
    );
    bench_row!(
        "array_intersect_card 4000x4000",
        array_intersect_card,
        array_intersect_card_on(a4000, b4000)
    );
    bench_row!(
        "array_intersects 4000x4000 sparse",
        array_intersects,
        array_intersects_on(s4000, t4000)
    );
    bench_row!(
        "array_union 2000x2000",
        array_union,
        array_union_on(a2000, b2000, &mut out)
    );
    bench_row!(
        "array_union 4000x4000 (sparse)",
        array_union,
        array_union_on(s4000, t4000, &mut out)
    );
    bench_row!(
        "array_union 200x4000",
        array_union,
        array_union_on(a200, b4000, &mut out)
    );
    bench_row!(
        "array_union 64x4000",
        array_union,
        array_union_on(a64, b4000, &mut out)
    );
    bench_row!(
        "array_difference 2000x2000",
        array_difference,
        array_difference_on(a2000, b2000, &mut out)
    );
    bench_row!(
        "array_difference 4000x200",
        array_difference,
        array_difference_on(b4000, a200, &mut out)
    );
    bench_row!(
        "array_difference 200x4000",
        array_difference,
        array_difference_on(a200, b4000, &mut out)
    );
    bench_row!(
        "array_xor 2000x2000",
        array_xor,
        array_xor_on(a2000, b2000, &mut out)
    );
    bench_row!(
        "array_xor 200x4000",
        array_xor,
        array_xor_on(a200, b4000, &mut out)
    );
}
