//! Prints the SIMD backend selected by `roaring128` on this CPU.
//!
//! Usage: `backend-probe [--expect <avx2|avx512|neon>]`. With `--expect`, exits with status 1 when
//! the selected backend differs, so CI can assert that e.g. Intel SDE really exercises AVX-512.

use roaring128::{SimdBackend, simd_backend};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut expect = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--expect" => expect = Some(args.next().expect("--expect needs a backend name")),
            other => panic!("unknown argument {other:?}"),
        }
    }

    let forced = std::env::var("ROARING128_FORCE_BACKEND").unwrap_or_default();
    println!("arch: {}", std::env::consts::ARCH);
    println!(
        "ROARING128_FORCE_BACKEND: {}",
        if forced.is_empty() {
            "<unset>"
        } else {
            &forced
        }
    );
    for b in [SimdBackend::Avx2, SimdBackend::Avx512, SimdBackend::Neon] {
        println!("supported {:<7} {}", b.name(), b.is_supported());
    }
    let selected = simd_backend();
    println!("selected backend: {}", selected.name());

    if let Some(expect) = expect {
        if selected.name() != expect {
            eprintln!("error: expected backend {expect}, got {}", selected.name());
            std::process::exit(1);
        }
        println!("ok: backend is {expect}");
    }
}
