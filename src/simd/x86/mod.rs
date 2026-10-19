//! x86_64 backends: AVX2 (minimum) and AVX-512.
//!
//! Each submodule exposes the same set of `unsafe` kernels as [`super::scalar`]. Callers must
//! only invoke them when the corresponding `has_*` function returns `true`.

mod arrays;
pub(crate) mod avx2;
pub(crate) mod avx512;
mod tables;

/// AVX2 tier (the `x86-64-v3` integer subset): POPCNT + SSE4.2 + AVX + AVX2 + BMI1 + BMI2.
#[inline]
pub(crate) fn has_avx2() -> bool {
    #[cfg(feature = "std")]
    {
        std::is_x86_feature_detected!("popcnt")
            && std::is_x86_feature_detected!("sse4.2")
            && std::is_x86_feature_detected!("avx")
            && std::is_x86_feature_detected!("avx2")
            && std::is_x86_feature_detected!("bmi1")
            && std::is_x86_feature_detected!("bmi2")
    }
    #[cfg(not(feature = "std"))]
    {
        cfg!(all(
            target_feature = "popcnt",
            target_feature = "sse4.2",
            target_feature = "avx",
            target_feature = "avx2",
            target_feature = "bmi1",
            target_feature = "bmi2"
        ))
    }
}

/// AVX-512 tier: AVX2 tier + AVX-512 F/BW/VL/DQ/VPOPCNTDQ/BITALG/VBMI2 (Ice Lake, Zen 4+).
#[inline]
pub(crate) fn has_avx512() -> bool {
    #[cfg(feature = "std")]
    {
        has_avx2()
            && std::is_x86_feature_detected!("avx512f")
            && std::is_x86_feature_detected!("avx512bw")
            && std::is_x86_feature_detected!("avx512vl")
            && std::is_x86_feature_detected!("avx512dq")
            && std::is_x86_feature_detected!("avx512vpopcntdq")
            && std::is_x86_feature_detected!("avx512bitalg")
            && std::is_x86_feature_detected!("avx512vbmi2")
    }
    #[cfg(not(feature = "std"))]
    {
        has_avx2()
            && cfg!(all(
                target_feature = "avx512f",
                target_feature = "avx512bw",
                target_feature = "avx512vl",
                target_feature = "avx512dq",
                target_feature = "avx512vpopcntdq",
                target_feature = "avx512bitalg",
                target_feature = "avx512vbmi2"
            ))
    }
}

/// Defines `unsafe` kernels that re-instantiate the scalar reference code with the given target
/// features enabled (compiler auto-vectorization only). Every kernel is specialized now; the
/// tests use this as the "auto-vectorized reference" baseline of the timing comparison.
#[cfg(test)]
macro_rules! scalar_kernels {
    ($features:literal => $($name:ident($($arg:ident : $ty:ty),*) $(-> $ret:ty)?;)*) => {
        $(
            #[target_feature(enable = $features)]
            #[allow(clippy::missing_safety_doc)]
            pub(crate) unsafe fn $name($($arg: $ty),*) $(-> $ret)? {
                crate::simd::scalar::$name($($arg),*)
            }
        )*
    };
}
#[cfg(test)]
pub(crate) use scalar_kernels;
