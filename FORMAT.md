# roaring128 serialization format (version 1)

There is no standard Roaring format for 128-bit values, so `roaring128` defines one that stays as
close as possible to the [32-bit Roaring format specification](https://github.com/RoaringBitmap/RoaringFormatSpec)
while making every container's position computable from the header alone (no offset table, so
sparse bitmaps — one value per container, the typical UUID case — cost 18 bytes per value).

All integers are little-endian.

```
+--------------------------------------------------------------------------------+
| cookie            u32   0x3831_5252  ("RR18" in memory order: 'R','R','1','8')  |
| flags             u32   bit 0: a run-container bitset follows; other bits zero  |
| container count   u64   n                                                       |
+--------------------------------------------------------------------------------+
| run bitset        ceil(n / 8) bytes, only if flags bit 0 is set;               |
|                   bit (i % 8) of byte (i / 8) is set if container i is a run   |
+--------------------------------------------------------------------------------+
| descriptors       n × u128                                                      |
|                   descriptor = (key << 16) | low                                |
|                     key  : the 112 high bits shared by the container's values  |
|                     low  : cardinality - 1   for array and bitmap containers    |
|                            run count - 1     for run containers                 |
|                   keys are strictly increasing                                  |
+--------------------------------------------------------------------------------+
| payloads          one per container, in descriptor order, no padding:           |
|                     array  (cardinality <= 4096, not a run): cardinality × u16 |
|                            strictly increasing values                           |
|                     bitmap (cardinality >  4096, not a run): 1024 × u64        |
|                            bit i of word j is the value 64 * j + i              |
|                     run:   run count × (start: u16, length - 1: u16)           |
|                            sorted, non overlapping, non adjacent intervals     |
+--------------------------------------------------------------------------------+
```

A value `v` belongs to the container with key `v >> 16` and is stored there as the 16-bit
index `v & 0xFFFF`.

Because the payload size of every container is known from its descriptor and the run bitset
(`2 × cardinality`, `8192` or `4 × run count` bytes), readers can compute the offset of any
container after reading the header, which is what
`RoaringBitmap128::intersection_with_serialized_unchecked` does.

## Validity rules

A serialized bitmap is valid if and only if all of the following hold:

1. The cookie is `0x3831_5252` and only bit 0 of the flags may be set.
2. If the run bitset is present, its padding bits (bits `n % 8 ..= 7` of the last byte when
   `n` is not a multiple of 8) are zero. Writers only emit the bitset when at least one
   container is a run, but a bitset without any run bit set is valid.
3. Keys are strictly increasing (so no key is repeated).
4. Array payloads (`cardinality <= 4096`, not a run) are strictly increasing.
5. Bitmap payloads (`cardinality > 4096`, not a run) have exactly `cardinality` bits set.
6. Every run satisfies `start + (length - 1) <= 0xFFFF`; runs are sorted, non overlapping and
   non adjacent (`start` of a run is greater than `end + 1` of the previous one).
7. The total cardinality (the sum over all containers) fits in a `u64`.

The input may continue after the last payload: readers stop there and leave the rest unread.

`deserialize_from` checks every rule and never allocates more than a small constant factor of
what the input actually provides, whatever the announced container count.

`deserialize_unchecked_from` is meant for trusted input. It still checks rules 1, 3, 6 (only the
bound of each run) and 7, which keep the in-memory index consistent, but not rule 2 nor the
contents of the containers (rules 4, 5 and the ordering part of 6). Invalid contents yield an
invalid bitmap, never undefined behaviour; debug builds may panic on them.

`intersection_with_serialized_unchecked` checks rule 1, stops reading descriptors (read in
chunks of 4096) once the in-memory operand has no greater key, and reads only the payloads of the
containers whose keys the in-memory operand has. Its reader needs `Seek`; seeks are relative to
the position where the serialized bitmap starts.
