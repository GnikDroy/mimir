use num_enum::UnsafeFromPrimitive;

use crate::types::piece::Piece;

/// A piece a pawn may promote to. Listed best-to-worst by typical search
/// value so move ordering iterates queens first.
#[repr(u8)]
#[derive(UnsafeFromPrimitive, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromotionPiece {
    Queen,
    Rook,
    Bishop,
    Knight,
}

impl PromotionPiece {
    /// Number of promotion targets (always 4).
    pub const NUM: u8 = std::mem::variant_count::<PromotionPiece>() as u8;

    /// Reconstructs a [`PromotionPiece`] from its discriminant.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `index` is not in `0..NUM`. Release
    /// builds skip the check; passing an out-of-range value is undefined
    /// behaviour.
    #[inline(always)]
    pub fn index(index: u8) -> Self {
        debug_assert!(index < Self::NUM);
        unsafe { PromotionPiece::unchecked_transmute_from(index) }
    }

    /// Iterator over every promotion target in discriminant order.
    #[inline(always)]
    pub fn all() -> impl ExactSizeIterator<Item = PromotionPiece> + DoubleEndedIterator + Clone {
        (0..PromotionPiece::NUM).map(Self::index)
    }

    /// Widens the promotion choice to the corresponding [`Piece`] kind.
    #[inline(always)]
    pub fn to_piece(&self) -> Piece {
        match self {
            PromotionPiece::Queen => Piece::Queen,
            PromotionPiece::Rook => Piece::Rook,
            PromotionPiece::Bishop => Piece::Bishop,
            PromotionPiece::Knight => Piece::Knight,
        }
    }
}
