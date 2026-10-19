//! Differential tests: every supported backend must agree with the scalar reference.
//!
//! Output buffers are allocated with exactly the documented bound followed by a guard zone; a
//! kernel that writes past its bound corrupts the guard and fails the test.

use alloc::vec::Vec;
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
