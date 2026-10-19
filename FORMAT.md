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

`deserialize_from` validates everything above (cookie, flags, key order, sorted array values,
bitmap cardinality equal to the popcount, well-formed runs) and never allocates more than the
input actually provides. `deserialize_unchecked_from` skips the content validation of
containers.
