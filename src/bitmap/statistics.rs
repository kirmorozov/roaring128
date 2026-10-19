use crate::RoaringBitmap128;

/// Detailed statistics on the composition of a bitmap.
#[derive(Clone, Copy, PartialEq, Debug)]
#[non_exhaustive]
pub struct Statistics {
    /// Number of containers in the bitmap
    pub n_containers: u64,
    /// Number of array containers in the bitmap
    pub n_array_containers: u64,
    /// Number of run containers in the bitmap
    pub n_run_containers: u64,
    /// Number of bitset containers in the bitmap
    pub n_bitset_containers: u64,
    /// Number of values stored in array containers
    pub n_values_array_containers: u64,
    /// Number of values stored in run containers
    pub n_values_run_containers: u64,
    /// Number of values stored in bitset containers
    pub n_values_bitset_containers: u64,
    /// Number of bytes used by array containers
    pub n_bytes_array_containers: u64,
    /// Number of bytes used by run containers
    pub n_bytes_run_containers: u64,
    /// Number of bytes used by bitset containers
    pub n_bytes_bitset_containers: u64,
    /// Maximum value stored in the bitmap
    pub max_value: Option<u128>,
    /// Minimum value stored in the bitmap
    pub min_value: Option<u128>,
    /// Number of values stored in the bitmap
    pub cardinality: u64,
}

impl RoaringBitmap128 {
    /// Returns statistics about the composition of a roaring bitmap.
    ///
    /// ```
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut bitmap: RoaringBitmap128 = (1..100).collect();
    /// let statistics = bitmap.statistics();
    ///
    /// assert_eq!(statistics.n_containers, 1);
    /// assert_eq!(statistics.n_array_containers, 1);
    /// assert_eq!(statistics.n_run_containers, 0);
    /// assert_eq!(statistics.n_bitset_containers, 0);
    /// assert_eq!(statistics.n_values_array_containers, 99);
    /// assert_eq!(statistics.n_values_run_containers, 0);
    /// assert_eq!(statistics.n_values_bitset_containers, 0);
    /// assert_eq!(statistics.max_value, Some(99));
    /// assert_eq!(statistics.min_value, Some(1));
    /// assert_eq!(statistics.cardinality, 99);
    /// ```
    pub fn statistics(&self) -> Statistics {
        todo!()
    }
}
