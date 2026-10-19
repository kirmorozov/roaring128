//! Reports the SIMD backend selected on this machine (visible in CI logs with `--nocapture`, and
//! always printed on failure). If `ROARING128_EXPECT_BACKEND` is set (`avx2`, `avx512`, `neon`),
//! the selected backend must have that name, e.g. to check `ROARING128_FORCE_BACKEND` or a CI
//! runner's CPU.

use roaring128::{RoaringBitmap128, simd_backend};

#[test]
fn backend() {
    let backend = simd_backend();
    println!("roaring128 SIMD backend: {} ({backend:?})", backend.name());
    eprintln!("roaring128 SIMD backend: {} ({backend:?})", backend.name());
    assert!(backend.is_supported(), "the selected backend {backend:?} is not supported by this CPU");
    // the selection is stable
    assert_eq!(simd_backend(), backend);
    if let Ok(expected) = std::env::var("ROARING128_EXPECT_BACKEND") {
        assert_eq!(
            backend.name(),
            expected.trim(),
            "ROARING128_EXPECT_BACKEND={expected} but the selected backend is {backend:?}"
        );
    }
}

#[test]
fn backend_is_used_by_bitmap_operations() {
    // a smoke test through kernels of the selected backend (arrays, bitmaps, runs)
    let a: RoaringBitmap128 = (0..20_000u128).map(|v| (1 << 100) + 3 * v).collect();
    let b: RoaringBitmap128 = (0..20_000u128).map(|v| (1 << 100) + 2 * v).collect();
    let c = &a & &b;
    assert_eq!(c.len(), (0..60_000u128).filter(|v| v % 6 == 0 && *v < 40_000).count() as u64);
    assert_eq!((&a | &b).len(), a.len() + b.len() - c.len());
    assert!(c.internal_validate().is_ok());
}
