use num_enum::UnsafeFromPrimitive;

/// Piece kind, independent of colour.
///
/// Ordering is fixed: `King, Queen, Rook, Bishop, Knight, Pawn`. The
/// discriminant is the canonical piece index used by bitboards, PSTs, and
/// the packed [`Move`](crate::core::Move) encoding.
#[repr(u8)]
#[derive(UnsafeFromPrimitive, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Piece {
    King,
    Queen,
    Rook,
    Bishop,
    Knight,
    Pawn,
}

impl Piece {
    /// Number of piece kinds; usable as the size of per-piece lookup arrays.
    pub const NUM: usize = std::mem::variant_count::<Piece>();

    /// Reconstructs a [`Piece`] from its discriminant.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `index` is not in `0..NUM`. Release
    /// builds skip the check; passing an out-of-range value is undefined
    /// behaviour.
    #[inline(always)]
    pub fn index(index: u8) -> Self {
        debug_assert!(index < Self::NUM as u8);
        unsafe { Piece::unchecked_transmute_from(index) }
    }

    /// Iterator over every piece kind in discriminant order.
    #[inline(always)]
    pub fn all() -> impl ExactSizeIterator<Item = Piece> + DoubleEndedIterator + Clone {
        (0..Piece::NUM).map(|i| Self::index(i as u8))
    }
}
