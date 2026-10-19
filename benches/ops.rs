//! Benchmarks against `roaring::RoaringTreemap` and `BTreeSet<u128>`.

use criterion::{Criterion, criterion_group, criterion_main};

fn placeholder(c: &mut Criterion) {
    c.bench_function("backend", |b| b.iter(roaring128::simd_backend));
}

criterion_group!(benches, placeholder);
criterion_main!(benches);
