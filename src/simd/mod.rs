//! Vectorized kernels with runtime dispatch.
//!
//! Supported hardware (there is deliberately no fallback for older CPUs):
//!
//! * `x86_64`: AVX2 + BMI1/BMI2 + POPCNT is the minimum (Haswell / Zen 1 and newer, the
//!   `x86-64-v3` level). AVX-512 F/BW/VL/DQ/VPOPCNTDQ/BITALG/VBMI2 (Ice Lake, Zen 4 and newer) is
//!   used when present. Running on a CPU without AVX2 panics on first use of the bitmap.
//! * `aarch64`: NEON (baseline on every AArch64 CPU).
//! * Other architectures are rejected at compile time.
//!
//! [`scalar`] holds the portable reference implementation of every kernel. It is the semantic
//! specification checked by the differential tests and supplies building blocks (tails,
//! galloping searches) to the vector code; it is never selected as a backend on its own.
//!
//! Contracts shared by all array kernels: inputs are strictly increasing `u16` slices and the
//! output slice must be at least as long as the documented bound. Kernels never write past the
//! end of `out` and never read past the end of their inputs.

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU8, Ordering};

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("roaring128 requires x86_64 (AVX2 or newer) or aarch64 (NEON)");

#[cfg(target_arch = "aarch64")]
pub(crate) mod neon;
pub(crate) mod scalar;
#[cfg(target_arch = "x86_64")]
pub(crate) mod x86;

/// Number of 64-bit words in a bitmap container.
pub(crate) const BITMAP_WORDS: usize = 1024;

/// The 65536 bits of a bitmap container.
pub(crate) type Words = [u64; BITMAP_WORDS];

/// The vectorized code path used by this process.
///
/// The backend is detected once, on first use. On `x86_64` with the `std` feature the detection
/// is done at runtime, so a single binary uses AVX-512 on machines that support it and AVX2
/// elsewhere. Without `std` the backend is chosen from the target features enabled at compile
/// time: build with at least `-C target-cpu=x86-64-v3`. The AVX-512 backend additionally needs
/// AVX-512 VPOPCNTDQ, BITALG and VBMI2, which `x86-64-v4` does not include; use for example
/// `-C target-cpu=icelake-server` or `-C target-cpu=znver4`.
///
/// For testing, the `ROARING128_FORCE_BACKEND` environment variable (`avx2`, `avx512` or `neon`)
/// can downgrade the selection. Requests for a backend that the CPU does not support are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SimdBackend {
    /// x86_64 AVX2 + BMI1/BMI2 + POPCNT (`x86-64-v3`).
    Avx2,
    /// x86_64 AVX-512 F/BW/VL/DQ/VPOPCNTDQ/BITALG/VBMI2 (Ice Lake, Zen 4 and newer).
    Avx512,
    /// AArch64 Advanced SIMD (NEON).
    Neon,
}

impl SimdBackend {
    /// A short human readable name of the backend.
    pub fn name(self) -> &'static str {
        match self {
            SimdBackend::Avx2 => "avx2",
            SimdBackend::Avx512 => "avx512",
            SimdBackend::Neon => "neon",
        }
    }

    const fn to_u8(self) -> u8 {
        match self {
            SimdBackend::Avx2 => 1,
            SimdBackend::Avx512 => 2,
            SimdBackend::Neon => 3,
        }
    }

    const fn from_u8(v: u8) -> SimdBackend {
        match v {
            1 => SimdBackend::Avx2,
            2 => SimdBackend::Avx512,
            _ => SimdBackend::Neon,
        }
    }

    /// Whether this backend can run on the current CPU.
    pub fn is_supported(self) -> bool {
        match self {
            #[cfg(target_arch = "x86_64")]
            SimdBackend::Avx2 => x86::has_avx2(),
            #[cfg(target_arch = "x86_64")]
            SimdBackend::Avx512 => x86::has_avx512(),
            #[cfg(target_arch = "aarch64")]
            SimdBackend::Neon => neon::has_neon(),
            #[allow(unreachable_patterns)]
            _ => false,
        }
    }
}

/// Returns the vectorized code path selected for the running CPU.
///
/// # Panics
///
/// Panics if the CPU does not meet the minimum requirements (AVX2 on `x86_64`).
pub fn simd_backend() -> SimdBackend {
    backend()
}

static BACKEND: AtomicU8 = AtomicU8::new(0);

#[inline(always)]
pub(crate) fn backend() -> SimdBackend {
    match BACKEND.load(Ordering::Relaxed) {
        0 => init_backend(),
        v => SimdBackend::from_u8(v),
    }
}

#[cold]
#[inline(never)]
fn init_backend() -> SimdBackend {
    #[allow(unused_mut)]
    let mut best = detect();
    #[cfg(feature = "std")]
    if let Ok(forced) = std::env::var("ROARING128_FORCE_BACKEND") {
        let requested = match forced.to_ascii_lowercase().as_str() {
            "avx2" => Some(SimdBackend::Avx2),
            "avx512" | "avx-512" => Some(SimdBackend::Avx512),
            "neon" => Some(SimdBackend::Neon),
            _ => None,
        };
        if let Some(requested) = requested
            && requested.is_supported()
        {
            best = requested;
        }
    }
    BACKEND.store(best.to_u8(), Ordering::Relaxed);
    best
}

fn detect() -> SimdBackend {
    #[cfg(target_arch = "x86_64")]
    {
        if x86::has_avx512() {
            return SimdBackend::Avx512;
        }
        if x86::has_avx2() {
            return SimdBackend::Avx2;
        }
        panic!(
            "roaring128 requires an x86_64 CPU with AVX2, BMI1, BMI2 and POPCNT \
             (x86-64-v3: Intel Haswell / AMD Zen or newer)"
        );
    }
    #[cfg(target_arch = "aarch64")]
    {
        if neon::has_neon() {
            return SimdBackend::Neon;
        }
        panic!("roaring128 requires an aarch64 CPU with NEON");
    }
}

/// Generates a dispatching wrapper around a kernel implemented by every backend module.
///
/// For a kernel `name` this defines `name(args)` (uses the detected backend) and
/// `name_on(backend, args)` (uses an explicit backend, which must be supported by the CPU; used by
/// the differential tests).
macro_rules! dispatch {
    ($(#[$meta:meta])* $vis:vis fn $name:ident / $name_on:ident ($($arg:ident : $ty:ty),* $(,)?) $(-> $ret:ty)?) => {
        $(#[$meta])*
        #[inline]
        $vis fn $name($($arg: $ty),*) $(-> $ret)? {
            // Miri cannot execute vendor intrinsics; it checks the memory safety of the
            // container code against the reference kernels instead. Never used in real builds.
            #[cfg(miri)]
            {
                scalar::$name($($arg),*)
            }
            #[cfg(not(miri))]
            {
                $name_on(backend(), $($arg),*)
            }
        }

        #[inline(always)]
        #[allow(dead_code)]
        $vis fn $name_on(backend: SimdBackend, $($arg: $ty),*) $(-> $ret)? {
            debug_assert!(backend.is_supported());
            match backend {
                // SAFETY: only supported backends are ever selected.
                #[cfg(target_arch = "x86_64")]
                SimdBackend::Avx512 => unsafe { x86::avx512::$name($($arg),*) },
                #[cfg(target_arch = "x86_64")]
                SimdBackend::Avx2 => unsafe { x86::avx2::$name($($arg),*) },
                #[cfg(target_arch = "aarch64")]
                SimdBackend::Neon => unsafe { neon::$name($($arg),*) },
                #[allow(unreachable_patterns)]
                _ => unreachable!("unsupported SIMD backend"),
            }
        }
    };
}

/// Bitwise operation applied to two bitmap containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BitOp {
    And,
    Or,
    Xor,
    AndNot,
}

impl BitOp {
    #[inline(always)]
    #[cfg_attr(not(any(test, miri)), allow(dead_code))] // Used by the reference kernels.
    pub(crate) fn apply(self, a: u64, b: u64) -> u64 {
        match self {
            BitOp::And => a & b,
            BitOp::Or => a | b,
            BitOp::Xor => a ^ b,
            BitOp::AndNot => a & !b,
        }
    }
}

dispatch! {
    /// Number of set bits.
    pub(crate) fn bitmap_popcount / bitmap_popcount_on(a: &Words) -> u32
}

dispatch! {
    /// `out = a op b`, returns the cardinality of `out`.
    pub(crate) fn bitmap_op / bitmap_op_on(op: BitOp, a: &Words, b: &Words, out: &mut Words) -> u32
}

dispatch! {
    /// `a = a op b`, returns the cardinality of the new `a`.
    pub(crate) fn bitmap_op_inplace / bitmap_op_inplace_on(op: BitOp, a: &mut Words, b: &Words) -> u32
}

dispatch! {
    /// `a = a op b` without computing the cardinality.
    pub(crate) fn bitmap_op_inplace_nocard / bitmap_op_inplace_nocard_on(op: BitOp, a: &mut Words, b: &Words)
}

dispatch! {
    /// Cardinality of `a op b`.
    pub(crate) fn bitmap_op_card / bitmap_op_card_on(op: BitOp, a: &Words, b: &Words) -> u32
}

dispatch! {
    /// Whether `a & b` is non empty.
    pub(crate) fn bitmap_intersects / bitmap_intersects_on(a: &Words, b: &Words) -> bool
}

dispatch! {
    /// Whether every bit of `a` is set in `b`.
    pub(crate) fn bitmap_is_subset / bitmap_is_subset_on(a: &Words, b: &Words) -> bool
}

dispatch! {
    /// Writes the indices of all set bits in increasing order, returns the count.
    /// `out` must be able to hold the cardinality of `a`.
    pub(crate) fn bitmap_to_array / bitmap_to_array_on(a: &Words, out: &mut [MaybeUninit<u16>]) -> usize
}

dispatch! {
    /// Number of runs of consecutive set bits.
    pub(crate) fn bitmap_count_runs / bitmap_count_runs_on(a: &Words) -> u32
}

dispatch! {
    /// Sorted intersection, `out.len() >= min(a.len(), b.len())`.
    pub(crate) fn array_intersect / array_intersect_on(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize
}

dispatch! {
    /// Cardinality of the intersection.
    pub(crate) fn array_intersect_card / array_intersect_card_on(a: &[u16], b: &[u16]) -> usize
}

dispatch! {
    /// Whether the two arrays share at least one value.
    pub(crate) fn array_intersects / array_intersects_on(a: &[u16], b: &[u16]) -> bool
}

dispatch! {
    /// Sorted union, `out.len() >= a.len() + b.len()`.
    pub(crate) fn array_union / array_union_on(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize
}

dispatch! {
    /// Sorted difference `a - b`, `out.len() >= a.len()`.
    pub(crate) fn array_difference / array_difference_on(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize
}

dispatch! {
    /// Sorted symmetric difference, `out.len() >= a.len() + b.len()`.
    pub(crate) fn array_xor / array_xor_on(a: &[u16], b: &[u16], out: &mut [MaybeUninit<u16>]) -> usize
}

/// Arrays at least this many times smaller than the other side are intersected with galloping
/// (exponential search) instead of a linear merge.
#[cfg_attr(not(any(test, miri)), allow(dead_code))] // Used by the reference kernels.
pub(crate) const GALLOP_RATIO: usize = 32;

/// Asserts the output-length contract of an array kernel.
#[inline(always)]
pub(crate) fn check_out(out: &[MaybeUninit<u16>], bound: usize) {
    assert!(out.len() >= bound, "kernel output buffer too small");
}

/// Reinterprets the initialized prefix of an output buffer.
///
/// # Safety
/// The first `len` elements of `out` must have been initialized.
#[inline(always)]
pub(crate) unsafe fn assume_init(out: &[MaybeUninit<u16>], len: usize) -> &[u16] {
    debug_assert!(len <= out.len());
    unsafe { core::slice::from_raw_parts(out.as_ptr().cast(), len) }
}

#[cfg(test)]
mod tests;
