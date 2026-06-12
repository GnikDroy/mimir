use num_enum::UnsafeFromPrimitive;

/// Board rank (row). `First` is white's back rank, `Eighth` is black's.
///
/// Discriminant matches the FEN rank number minus one (`First = 0`,
/// ..., `Eighth = 7`).
#[repr(u8)]
#[derive(UnsafeFromPrimitive, Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Rank {
    First,
    Second,
    Third,
    Fourth,
    Fifth,
    Sixth,
    Seventh,
    Eighth,
}

impl Rank {
    /// Number of ranks (always 8).
    pub const NUM: usize = std::mem::variant_count::<Rank>();

    /// Reconstructs a [`Rank`] from its discriminant.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `index` is not in `0..NUM`. Release
    /// builds skip the check; passing an out-of-range value is undefined
    /// behaviour.
    #[inline(always)]
    pub fn index(index: usize) -> Self {
        debug_assert!(index < Self::NUM);
        unsafe { Rank::unchecked_transmute_from(index as u8) }
    }

    /// Iterator over every rank from `First` to `Eighth`.
    #[inline(always)]
    pub fn all() -> impl ExactSizeIterator<Item = Rank> + DoubleEndedIterator + Clone {
        (0..Rank::NUM).map(Self::index)
    }
}
