use core::fmt;

use alloc::vec::Vec;

use crate::RoaringBitmap128;

impl fmt::Debug for RoaringBitmap128 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.len() < 16 {
            write!(
                f,
                "RoaringBitmap128<{:?}>",
                self.iter().collect::<Vec<u128>>()
            )
        } else {
            write!(
                f,
                "RoaringBitmap128<{:?} values between {:?} and {:?} in {:?} containers>",
                self.len(),
                self.min().unwrap(),
                self.max().unwrap(),
                self.map.len(),
            )
        }
    }
}
