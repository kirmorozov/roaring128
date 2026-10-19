//! The native serialization format, see `FORMAT.md`.
//!
//! Writing goes through one reusable buffer flushed in [`WRITE_CHUNK`] pieces, so unbuffered
//! writers see few large writes. Reading validates the header, the key order and (for
//! `deserialize_from`) every container, reads in bounded chunks and never allocates more than a
//! small constant factor of what the input actually provided, whatever the announced counts.

use alloc::vec::Vec;
use std::io;

use crate::RoaringBitmap128;
use crate::container::{
    ARRAY_LIMIT, ArrayStore, BITMAP_BYTES, BitmapStore, Container, Interval, Kind, RunStore,
};
use crate::simd::Words;

/// `"RR18"` in memory order.
pub(crate) const COOKIE: u32 = 0x3831_5252;
/// Flags bit 0: a run-container bitset follows the header.
pub(crate) const FLAG_RUN_BITSET: u32 = 1;
/// Cookie, flags and container count.
pub(crate) const HEADER_BYTES: usize = 16;
/// One `u128` descriptor per container.
pub(crate) const DESCRIPTOR_BYTES: usize = 16;
/// Descriptors handled per read / write chunk (64 KiB).
pub(crate) const DESCRIPTOR_CHUNK: usize = 4096;
/// Write buffer flush threshold.
const WRITE_CHUNK: usize = 64 * 1024;
/// Largest single growth step when reading a variable sized section.
const READ_CHUNK: usize = 64 * 1024;

#[inline]
pub(crate) fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// The parsed fixed header.
pub(crate) struct Header {
    /// Number of containers.
    pub(crate) count: u64,
    /// Whether the run bitset is present.
    pub(crate) has_run_bitset: bool,
}

impl Header {
    /// Length of the run bitset in bytes.
    pub(crate) fn run_bitset_len(&self) -> io::Result<usize> {
        if !self.has_run_bitset {
            return Ok(0);
        }
        usize::try_from(self.count.div_ceil(8)).map_err(|_| invalid_data("too many containers"))
    }
}

/// Reads and checks the fixed header.
pub(crate) fn read_header<R: io::Read>(reader: &mut R) -> io::Result<Header> {
    let mut header = [0u8; HEADER_BYTES];
    reader.read_exact(&mut header)?;
    let cookie = u32::from_le_bytes(header[0..4].try_into().expect("4 bytes"));
    if cookie != COOKIE {
        return Err(invalid_data("unknown cookie value"));
    }
    let flags = u32::from_le_bytes(header[4..8].try_into().expect("4 bytes"));
    if flags & !FLAG_RUN_BITSET != 0 {
        return Err(invalid_data("unknown flags"));
    }
    let count = u64::from_le_bytes(header[8..16].try_into().expect("8 bytes"));
    Ok(Header {
        count,
        has_run_bitset: flags & FLAG_RUN_BITSET != 0,
    })
}

/// Reads exactly `len` bytes, growing the buffer only as data arrives (a hostile length cannot
/// trigger a large allocation without the matching input).
pub(crate) fn read_bytes<R: io::Read>(reader: &mut R, len: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while bytes.len() < len {
        let start = bytes.len();
        let chunk = (len - start).min(READ_CHUNK);
        bytes.resize(start + chunk, 0);
        reader.read_exact(&mut bytes[start..])?;
    }
    Ok(bytes)
}

/// Whether container `i` is a run container according to the run bitset.
#[inline]
pub(crate) fn is_run(run_bitset: &[u8], i: usize) -> bool {
    run_bitset
        .get(i / 8)
        .is_some_and(|byte| byte & (1 << (i % 8)) != 0)
}

/// Payload size of a container from its descriptor's low bits and run flag.
#[inline]
pub(crate) fn payload_size(low: u16, is_run: bool) -> usize {
    let n = low as usize + 1;
    if is_run {
        4 * n
    } else if n as u64 <= ARRAY_LIMIT {
        2 * n
    } else {
        BITMAP_BYTES
    }
}

/// Reads a bitmap payload straight into the store's own (64-byte aligned) block: no
/// intermediate buffer, no copy.
fn read_bitmap<R: io::Read>(reader: &mut R, len: u64, validate: bool) -> io::Result<BitmapStore> {
    let mut store = BitmapStore::new();
    let words: &mut Words = store.as_words_mut();
    // SAFETY: `Words` is `[u64; BITMAP_WORDS]`, exactly `BITMAP_BYTES` bytes of plain integers:
    // viewing it as bytes is in bounds, `u8` has no alignment requirement and every byte
    // pattern is a valid `u64`. `words` is not used while `bytes` is alive.
    let bytes: &mut [u8] =
        unsafe { core::slice::from_raw_parts_mut(words.as_mut_ptr().cast::<u8>(), BITMAP_BYTES) };
    reader.read_exact(bytes)?;
    // The payload is little-endian; this is a no-op on the supported (little-endian) targets.
    for word in words.iter_mut() {
        *word = u64::from_le(*word);
    }
    if validate {
        if store.recount() != len {
            return Err(invalid_data(
                "bitmap container cardinality does not match its bits",
            ));
        }
    } else {
        store.set_len(len);
    }
    Ok(store)
}

/// Reusable buffers for reading container payloads.
pub(crate) struct PayloadReader {
    bytes: Vec<u8>,
    values: Vec<u16>,
    intervals: Vec<Interval>,
}

impl PayloadReader {
    pub(crate) fn new() -> PayloadReader {
        PayloadReader {
            bytes: Vec::new(),
            values: Vec::new(),
            intervals: Vec::new(),
        }
    }

    /// Reads one container payload. `validate` checks the container contents (sorted arrays,
    /// bitmap cardinalities, well-formed runs); the run lengths are always checked so that no
    /// interval can end past index `0xFFFF`.
    pub(crate) fn read<R: io::Read>(
        &mut self,
        reader: &mut R,
        low: u16,
        is_run: bool,
        validate: bool,
    ) -> io::Result<Container> {
        let n = low as usize + 1;
        if is_run {
            self.bytes.resize(4 * n, 0);
            reader.read_exact(&mut self.bytes)?;
            self.intervals.clear();
            for run in self.bytes.as_chunks::<4>().0 {
                let start = u16::from_le_bytes([run[0], run[1]]);
                let length_minus_one = u16::from_le_bytes([run[2], run[3]]);
                let end = start
                    .checked_add(length_minus_one)
                    .ok_or_else(|| invalid_data("run ends past the end of its container"))?;
                self.intervals.push(Interval::new_unchecked(start, end));
            }
            let store = if validate {
                RunStore::try_from_intervals(&self.intervals).map_err(invalid_data)?
            } else {
                RunStore::from_intervals_unchecked(&self.intervals)
            };
            Ok(Container::Run(store))
        } else if n as u64 <= ARRAY_LIMIT {
            self.bytes.resize(2 * n, 0);
            reader.read_exact(&mut self.bytes)?;
            self.values.clear();
            self.values.extend(
                self.bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&v| u16::from_le_bytes(v)),
            );
            let store = if validate {
                ArrayStore::try_from_sorted(&self.values).map_err(invalid_data)?
            } else {
                ArrayStore::from_sorted_unchecked(&self.values)
            };
            Ok(Container::Array(store))
        } else {
            // `n > ARRAY_LIMIT`, so the store is a valid bitmap container once its cardinality
            // matches the bits.
            Ok(Container::Bitmap(read_bitmap(reader, n as u64, validate)?))
        }
    }
}

/// Appends `bytes`-producing writes to `buf`, flushing it once it is large enough.
#[inline]
fn flush_if_full<W: io::Write>(writer: &mut W, buf: &mut Vec<u8>) -> io::Result<()> {
    if buf.len() >= WRITE_CHUNK {
        writer.write_all(buf)?;
        buf.clear();
    }
    Ok(())
}

impl RoaringBitmap128 {
    /// Return the size in bytes of the serialized output (see `FORMAT.md`).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let mut bytes = Vec::with_capacity(rb1.serialized_size());
    /// rb1.serialize_into(&mut bytes).unwrap();
    /// let rb2 = RoaringBitmap128::deserialize_from(&bytes[..]).unwrap();
    ///
    /// assert_eq!(rb1, rb2);
    /// ```
    pub fn serialized_size(&self) -> usize {
        let n = self.map.len();
        let mut has_runs = false;
        let payloads: usize = self
            .map
            .values()
            .map(|c| {
                has_runs |= c.kind() == Kind::Run;
                c.serialized_payload_size()
            })
            .sum();
        let run_bitset = if has_runs { n.div_ceil(8) } else { 0 };
        HEADER_BYTES + run_bitset + n * DESCRIPTOR_BYTES + payloads
    }

    /// Serialize this bitmap into the native 128-bit Roaring format described in `FORMAT.md`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let mut bytes = vec![];
    /// rb1.serialize_into(&mut bytes).unwrap();
    /// let rb2 = RoaringBitmap128::deserialize_from(&bytes[..]).unwrap();
    ///
    /// assert_eq!(rb1, rb2);
    /// ```
    pub fn serialize_into<W: io::Write>(&self, mut writer: W) -> io::Result<()> {
        let n = self.map.len();
        let has_runs = self.map.values().any(|c| c.kind() == Kind::Run);

        // The buffer is flushed at WRITE_CHUNK; small bitmaps get a small buffer (header,
        // descriptors and at least two bytes of payload per container).
        let estimate = HEADER_BYTES + n.div_ceil(8) + n * (DESCRIPTOR_BYTES + 2);
        let mut buf = Vec::with_capacity(WRITE_CHUNK.min(estimate) + 64);
        buf.extend_from_slice(&COOKIE.to_le_bytes());
        let flags = if has_runs { FLAG_RUN_BITSET } else { 0 };
        buf.extend_from_slice(&flags.to_le_bytes());
        buf.extend_from_slice(&(n as u64).to_le_bytes());

        if has_runs {
            let mut byte = 0u8;
            for (i, container) in self.map.values().enumerate() {
                byte |= ((container.kind() == Kind::Run) as u8) << (i % 8);
                if i % 8 == 7 {
                    buf.push(byte);
                    byte = 0;
                    flush_if_full(&mut writer, &mut buf)?;
                }
            }
            if !n.is_multiple_of(8) {
                buf.push(byte);
            }
        }

        for (&key, container) in &self.map {
            let descriptor = (key << 16) | container.descriptor() as u128;
            buf.extend_from_slice(&descriptor.to_le_bytes());
            flush_if_full(&mut writer, &mut buf)?;
        }

        for container in self.map.values() {
            container.write_payload(&mut buf);
            flush_if_full(&mut writer, &mut buf)?;
        }

        if !buf.is_empty() {
            writer.write_all(&buf)?;
        }
        Ok(())
    }

    /// Deserialize a bitmap from the native 128-bit Roaring format, validating the input.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let mut bytes = vec![];
    /// rb1.serialize_into(&mut bytes).unwrap();
    /// let rb2 = RoaringBitmap128::deserialize_from(&bytes[..]).unwrap();
    ///
    /// assert_eq!(rb1, rb2);
    /// ```
    pub fn deserialize_from<R: io::Read>(reader: R) -> io::Result<RoaringBitmap128> {
        RoaringBitmap128::deserialize_impl(reader, true)
    }

    /// Deserialize a bitmap from the native 128-bit Roaring format without validating the
    /// container contents. Only use this on trusted input; invalid input results in an invalid
    /// bitmap (but never in undefined behaviour).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let mut bytes = vec![];
    /// rb1.serialize_into(&mut bytes).unwrap();
    /// let rb2 = RoaringBitmap128::deserialize_unchecked_from(&bytes[..]).unwrap();
    ///
    /// assert_eq!(rb1, rb2);
    /// ```
    pub fn deserialize_unchecked_from<R: io::Read>(reader: R) -> io::Result<RoaringBitmap128> {
        RoaringBitmap128::deserialize_impl(reader, false)
    }

    fn deserialize_impl<R: io::Read>(
        mut reader: R,
        validate: bool,
    ) -> io::Result<RoaringBitmap128> {
        let header = read_header(&mut reader)?;
        let run_bitset = read_bytes(&mut reader, header.run_bitset_len()?)?;
        // Bits past the last container must be zero.
        if validate
            && let Some(&last) = run_bitset.last()
            && !header.count.is_multiple_of(8)
            && last >> (header.count % 8) != 0
        {
            return Err(invalid_data(
                "run bitset has bits set past the last container",
            ));
        }

        // Descriptors, read in bounded chunks; the vector only grows with data actually read.
        // Key order is structural and checked even without validation.
        let mut descriptors: Vec<u128> = Vec::new();
        let mut remaining = header.count;
        let mut chunk = Vec::new();
        let mut previous_key: Option<u128> = None;
        while remaining > 0 {
            let take = remaining.min(DESCRIPTOR_CHUNK as u64) as usize;
            chunk.resize(take * DESCRIPTOR_BYTES, 0);
            reader.read_exact(&mut chunk)?;
            descriptors.reserve(take);
            for &bytes in chunk.as_chunks::<DESCRIPTOR_BYTES>().0 {
                let descriptor = u128::from_le_bytes(bytes);
                let key = descriptor >> 16;
                if previous_key.is_some_and(|previous| key <= previous) {
                    return Err(invalid_data("container keys are not strictly increasing"));
                }
                previous_key = Some(key);
                descriptors.push(descriptor);
            }
            remaining -= take as u64;
        }
        drop(chunk);

        let mut entries = Vec::with_capacity(descriptors.len());
        let mut payloads = PayloadReader::new();
        let mut len = 0u64;
        for (i, descriptor) in descriptors.into_iter().enumerate() {
            let container = payloads.read(
                &mut reader,
                descriptor as u16,
                is_run(&run_bitset, i),
                validate,
            )?;
            len = len
                .checked_add(container.len())
                .ok_or_else(|| invalid_data("cardinality overflows u64"))?;
            entries.push((descriptor >> 16, container));
        }
        Ok(RoaringBitmap128::from_sorted_entries_with_len(entries, len))
    }
}
