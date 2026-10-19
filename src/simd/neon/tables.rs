//! Lookup tables of the NEON kernels, built at compile time (about 6.3 KiB in total).

/// Cache line aligned wrapper, so that no table entry straddles two cache lines.
#[repr(C, align(64))]
pub(super) struct Aligned<T>(pub(super) T);

/// `SHUFFLE[m]` are the `vqtbl1q_u8` indices that move the `u16` lanes selected by the 8-bit mask
/// `m` to the front of a vector, in lane order. The lanes after them are zeroed.
pub(super) static SHUFFLE: Aligned<[[u8; 16]; 256]> = Aligned(shuffle_table());

/// Number of set bits of every byte value.
pub(super) static POPCOUNT: Aligned<[u8; 256]> = Aligned(popcount_table());

/// Twice the number of set bits of every byte value: the output advance in bytes when every set
/// bit becomes one `u16`.
pub(super) static POPCOUNT_X2: Aligned<[u8; 256]> = Aligned(popcount_x2_table());

/// `BIT_POSITIONS[b]` holds the indices (`0..8`) of the set bits of byte `b` in increasing
/// order, followed by zeros.
pub(super) static BIT_POSITIONS: Aligned<[[u8; 8]; 256]> = Aligned(bit_positions_table());

const fn shuffle_table() -> [[u8; 16]; 256] {
    // Out-of-range indices make `tbl` produce zero bytes.
    let mut table = [[0xFF; 16]; 256];
    let mut mask = 0;
    while mask < 256 {
        let mut lane = 0;
        let mut n = 0;
        while lane < 8 {
            if mask & (1 << lane) != 0 {
                table[mask][2 * n] = 2 * lane as u8;
                table[mask][2 * n + 1] = 2 * lane as u8 + 1;
                n += 1;
            }
            lane += 1;
        }
        mask += 1;
    }
    table
}

const fn popcount_table() -> [u8; 256] {
    let mut table = [0; 256];
    let mut b = 0;
    while b < 256 {
        table[b] = (b as u8).count_ones() as u8;
        b += 1;
    }
    table
}

const fn popcount_x2_table() -> [u8; 256] {
    let mut table = [0; 256];
    let mut b = 0;
    while b < 256 {
        table[b] = 2 * (b as u8).count_ones() as u8;
        b += 1;
    }
    table
}

const fn bit_positions_table() -> [[u8; 8]; 256] {
    let mut table = [[0; 8]; 256];
    let mut b = 0;
    while b < 256 {
        let mut bit = 0;
        let mut n = 0;
        while bit < 8 {
            if b & (1 << bit) != 0 {
                table[b][n] = bit as u8;
                n += 1;
            }
            bit += 1;
        }
        b += 1;
    }
    table
}
