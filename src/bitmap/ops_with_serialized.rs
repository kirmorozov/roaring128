use alloc::vec::Vec;
use std::io;

use super::serialization::{
    DESCRIPTOR_BYTES, DESCRIPTOR_CHUNK, HEADER_BYTES, PayloadReader, invalid_data, is_run,
    payload_size, read_bytes, read_header,
};
use crate::RoaringBitmap128;
use crate::container::Container;

impl RoaringBitmap128 {
    /// Computes the intersection between a materialized [`RoaringBitmap128`] and a serialized
    /// one.
    ///
    /// This is faster and more space efficient when you only need the intersection result: only
    /// the containers whose keys are present in `self` are read from `other`.
    /// The serialized bitmap is not validated (see
    /// [`deserialize_unchecked_from`](Self::deserialize_unchecked_from)).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    /// use std::io::Cursor;
    ///
    /// let rb1: RoaringBitmap128 = (1..4).collect();
    /// let rb2: RoaringBitmap128 = (3..5).collect();
    ///
    /// let mut serialized_bytes_rb2 = Vec::new();
    /// rb2.serialize_into(&mut serialized_bytes_rb2).unwrap();
    ///
    /// let intersection = rb1.intersection_with_serialized_unchecked(Cursor::new(serialized_bytes_rb2)).unwrap();
    ///
    /// assert_eq!(rb1 & rb2, intersection);
    /// ```
    pub fn intersection_with_serialized_unchecked<R>(
        &self,
        other: R,
    ) -> io::Result<RoaringBitmap128>
    where
        R: io::Read + io::Seek,
    {
        let mut reader = other;
        let header = read_header(&mut reader)?;
        let run_bitset_len = header.run_bitset_len()?;
        let run_bitset = read_bytes(&mut reader, run_bitset_len)?;
        let count = header.count;

        // Walk the descriptors (in bounded chunks) and the keys of `self` together, computing
        // the payload offset of every container; remember the containers whose keys we have.
        // Reading stops as soon as `self` has no greater key.
        let mut ours = self.map.iter().peekable();
        let mut matches: Vec<(u128, u16, bool, u64, &Container)> = Vec::new();
        let mut payload_offset = 0u64;
        let mut read = 0u64;
        let mut chunk = Vec::new();
        while read < count && ours.peek().is_some() {
            let take = (count - read).min(DESCRIPTOR_CHUNK as u64) as usize;
            chunk.resize(take * DESCRIPTOR_BYTES, 0);
            reader.read_exact(&mut chunk)?;
            for (j, &bytes) in chunk.as_chunks::<DESCRIPTOR_BYTES>().0.iter().enumerate() {
                let descriptor = u128::from_le_bytes(bytes);
                let (key, low) = (descriptor >> 16, descriptor as u16);
                let run = is_run(&run_bitset, (read as usize) + j);
                while ours.next_if(|(k, _)| **k < key).is_some() {}
                if let Some((_, container)) = ours.next_if(|(k, _)| **k == key) {
                    matches.push((key, low, run, payload_offset, container));
                }
                payload_offset = payload_offset.saturating_add(payload_size(low, run) as u64);
            }
            read += take as u64;
        }

        // Stream positions relative to the start of the serialized bitmap.
        let too_large = || invalid_data("serialized bitmap is too large");
        let payloads_start = count
            .checked_mul(DESCRIPTOR_BYTES as u64)
            .and_then(|d| d.checked_add((HEADER_BYTES + run_bitset_len) as u64))
            .ok_or_else(too_large)?;
        let mut position = (HEADER_BYTES + run_bitset_len) as u64 + read * DESCRIPTOR_BYTES as u64;

        let mut payloads = PayloadReader::new();
        let mut entries = Vec::new();
        let mut len = 0;
        for (key, low, run, offset, ours) in matches {
            let target = payloads_start.checked_add(offset).ok_or_else(too_large)?;
            if target != position {
                let delta = i64::try_from(i128::from(target) - i128::from(position))
                    .map_err(|_| too_large())?;
                reader.seek(io::SeekFrom::Current(delta))?;
            }
            let mut container = payloads.read(&mut reader, low, run, false)?;
            position = target.saturating_add(payload_size(low, run) as u64);
            container.intersect_with(ours);
            if !container.is_empty() {
                len += container.len();
                entries.push((key, container));
            }
        }
        Ok(RoaringBitmap128::from_sorted_entries_with_len(entries, len))
    }
}
