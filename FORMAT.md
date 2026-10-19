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
2. Flags bit 0 and the run bitset: writers set bit 0 (and emit the bitset) only when at least
   one container is a run. Readers also accept bit 0 with a bitset in which no container is
   marked as a run. The padding bits of the bitset (bits `n % 8 ..= 7` of its last byte when
   `n` is not a multiple of 8) must be zero.
3. Keys are strictly increasing (so no key is repeated).
4. Array payloads (`cardinality <= 4096`, not a run) are strictly increasing.
5. Bitmap payloads (`cardinality > 4096`, not a run) have exactly `cardinality` bits set.
6. Every run satisfies `start + (length - 1) <= 0xFFFF`; runs are sorted, non overlapping and
   non adjacent (`start` of a run is greater than `end + 1` of the previous one).
7. The total cardinality (the sum over all containers) fits in a `u64`.
8. The input holds every byte announced by the header and the descriptors (no truncation).

## Readers

All readers start at the current position of their reader. The input may continue after the
last payload: readers stop there and leave the rest unread. Errors are reported as
`io::Error`s (`UnexpectedEof` for truncated input, `InvalidData` for broken rules); no input
makes a reader panic, and none of them allocates more than a small constant factor of what the
input actually provides, whatever the announced container count.

* `deserialize_from` checks every rule.
* `deserialize_unchecked_from` is meant for trusted input and skips the two checks that cost
  more than reading the data: the bitmap cardinality (rule 5, the announced cardinality is
  trusted) and the padding bits of the run bitset (part of rule 2). A bitmap payload that does
  not match its cardinality yields an invalid bitmap (in release and debug builds alike, never a
  panic nor undefined behaviour). Every other rule is checked, in particular truncation, the
  cookie and the flags.
* `intersection_with_serialized_unchecked` needs `Seek` in addition; all seeks are relative to
  the position where the serialized bitmap starts. It checks rule 1 and reads the descriptors
  (in chunks of 4096) only until the in-memory operand has no greater key, then reads only the
  payloads of the containers whose keys the in-memory operand has; those payloads are fully
  validated (rules 4 to 6). Truncation inside the data it reads is an error; truncation past
  the last byte it needs is not detected.
