use crate::{MultiOps, RoaringBitmap128};

impl<I> MultiOps<RoaringBitmap128> for I
where
    I: IntoIterator<Item = RoaringBitmap128>,
{
    type Output = RoaringBitmap128;

    fn union(self) -> Self::Output {
        todo!()
    }

    fn intersection(self) -> Self::Output {
        todo!()
    }

    fn difference(self) -> Self::Output {
        todo!()
    }

    fn symmetric_difference(self) -> Self::Output {
        todo!()
    }
}

impl<I, E> MultiOps<Result<RoaringBitmap128, E>> for I
where
    I: IntoIterator<Item = Result<RoaringBitmap128, E>>,
{
    type Output = Result<RoaringBitmap128, E>;

    fn union(self) -> Self::Output {
        todo!()
    }

    fn intersection(self) -> Self::Output {
        todo!()
    }

    fn difference(self) -> Self::Output {
        todo!()
    }

    fn symmetric_difference(self) -> Self::Output {
        todo!()
    }
}

impl<'a, I> MultiOps<&'a RoaringBitmap128> for I
where
    I: IntoIterator<Item = &'a RoaringBitmap128>,
{
    type Output = RoaringBitmap128;

    fn union(self) -> Self::Output {
        todo!()
    }

    fn intersection(self) -> Self::Output {
        todo!()
    }

    fn difference(self) -> Self::Output {
        todo!()
    }

    fn symmetric_difference(self) -> Self::Output {
        todo!()
    }
}

impl<'a, I, E: 'a> MultiOps<Result<&'a RoaringBitmap128, E>> for I
where
    I: IntoIterator<Item = Result<&'a RoaringBitmap128, E>>,
{
    type Output = Result<RoaringBitmap128, E>;

    fn union(self) -> Self::Output {
        todo!()
    }

    fn intersection(self) -> Self::Output {
        todo!()
    }

    fn difference(self) -> Self::Output {
        todo!()
    }

    fn symmetric_difference(self) -> Self::Output {
        todo!()
    }
}
