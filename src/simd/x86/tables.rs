//! Lookup tables shared by the x86 kernels, built at compile time.

/// Forces cache-line alignment of a table so that no entry straddles two lines.
#[repr(C, align(64))]
pub(crate) struct Aligned<T>(pub(crate) T);

/// `PACK16[m]` is a `pshufb` control that moves the 16-bit lanes selected by the bits of `m`
/// (lane `i` <=> bit `i`) to the front of the vector, in increasing lane order. Unused output
/// lanes are zeroed.
pub(crate) static PACK16: Aligned<[[u8; 16]; 256]> = Aligned(build_pack16());

/// `BIT_INDICES[m]` lists the positions of the set bits of the byte `m` in increasing order
/// (unused entries are zero). Widened to 16 bits and offset by the byte position it yields the
/// values encoded by one byte of a bitmap.
pub(crate) static BIT_INDICES: Aligned<[[u8; 8]; 256]> = Aligned(build_bit_indices());

const fn build_pack16() -> [[u8; 16]; 256] {
    let mut table = [[0x80u8; 16]; 256];
    let mut m = 0;
    while m < 256 {
        let mut out = 0;
        let mut lane = 0;
        while lane < 8 {
            if m & (1 << lane) != 0 {
                table[m][2 * out] = 2 * lane as u8;
                table[m][2 * out + 1] = 2 * lane as u8 + 1;
                out += 1;
            }
            lane += 1;
        }
        m += 1;
    }
    table
}

const fn build_bit_indices() -> [[u8; 8]; 256] {
    let mut table = [[0u8; 8]; 256];
    let mut m = 0;
    while m < 256 {
        let mut out = 0;
        let mut bit = 0;
        while bit < 8 {
            if m & (1 << bit) != 0 {
                table[m][out] = bit as u8;
                out += 1;
            }
            bit += 1;
        }
        m += 1;
    }
    table
}
