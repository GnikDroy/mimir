//! 64-bit bitboard primitive and its core operations.
//!
//! A [`BitBoard`] is a `u64` where bit `i` represents the square with
//! index `i` under the A1 = 0, H8 = 63 ordering used throughout the
//! engine. Files run within a rank (low bits = A-file), ranks stack
//! upward (low bits = rank 1).
//!
//! The shift helpers move every set bit one step in a compass direction,
//! masking out the source rank/file first so bits cannot wrap onto the
//! opposite edge.
//!
//! The [`bitboard!`] macro lets tests express boards as ASCII grids — A8
//! is top-left, H1 is bottom-right, matching how a chess board is drawn.

use crate::core::*;

/// Bitboard: one bit per square, indexed by `Square as u64`.
///
/// Constants and helpers live on the [`BitBoardMethods`] trait, which is
/// implemented for `u64`. The alias keeps call sites self-documenting.
pub type BitBoard = u64;

/// Bitboard constants and operations.
///
/// Implemented for the [`BitBoard`] alias; trait form so the constants
/// and helpers can be accessed as `BitBoard::EMPTY`,
/// `bb.shift_north()`, etc.
pub trait BitBoardMethods {
    /// An empty board (no bits set).
    const EMPTY: Self;
    /// A fully-set board (every square present).
    const FULL: Self;
    /// Mask of rank 1.
    const FIRST_RANK: Self;
    /// Mask of rank 8.
    const LAST_RANK: Self;
    /// Mask of the A-file.
    const FIRST_FILE: Self;
    /// Mask of the H-file.
    const LAST_FILE: Self;

    /// Returns an 8×8 ASCII grid (`*` = set, `_` = unset) with rank 8 on
    /// top and rank 1 on the bottom. Intended for debugging output.
    fn to_grid(&self) -> String;

    /// Returns a bitboard with exactly `square` set.
    fn on(square: Square) -> BitBoard;

    /// Mirrors the board vertically (rank 1 ↔ rank 8). Equivalent to a
    /// byte-reversal of the underlying `u64`.
    fn flip_ranks(self) -> Self;

    /// Mirrors the board horizontally (A-file ↔ H-file) using a three-pass
    /// SWAR butterfly.
    fn flip_files(self) -> Self;

    /// Shifts every set bit one rank toward rank 8. Bits on rank 8 are
    /// dropped rather than wrapping.
    fn shift_north(&self) -> Self;

    /// Shifts every set bit one rank toward rank 1.
    fn shift_south(&self) -> Self;

    /// Shifts every set bit one file toward the H-file.
    fn shift_east(&self) -> Self;

    /// Shifts every set bit one file toward the A-file.
    fn shift_west(&self) -> Self;

    /// Shifts every set bit one diagonal step toward H8.
    fn shift_north_east(&self) -> Self;

    /// Shifts every set bit one diagonal step toward A8.
    fn shift_north_west(&self) -> Self;

    /// Shifts every set bit one diagonal step toward H1.
    fn shift_south_east(&self) -> Self;

    /// Shifts every set bit one diagonal step toward A1.
    fn shift_south_west(&self) -> Self;

    /// Pops and returns the least-significant set bit as a [`Square`].
    /// Returns [`None`] when the board is empty. Mutates `self` in place.
    fn pop_lsb(&mut self) -> Option<Square>;

    /// Returns an iterator over the set squares in least-significant
    /// bit order.
    fn iter(self) -> BitBoardIterator
    where
        Self: Sized + Into<BitBoard>,
    {
        BitBoardIterator {
            bitboard: self.into(),
        }
    }
}

impl BitBoardMethods for BitBoard {
    const EMPTY: Self = 0;
    const FULL: Self = !0;
    const FIRST_RANK: Self = 0x00000000000000ff;
    const LAST_RANK: Self = 0xff00000000000000;
    const FIRST_FILE: Self = 0x0101010101010101;
    const LAST_FILE: Self = 0x8080808080808080;

    fn to_grid(&self) -> String {
        let mut repr = String::new();
        for rank in 0..Rank::NUM {
            for file in 0..File::NUM {
                let idx = (Rank::NUM - rank - 1) * File::NUM + file;
                let piece = (self >> idx) & 1u64;
                repr.push(if piece == 0 { '_' } else { '*' });
            }
            repr.push('\n');
        }
        repr
    }

    fn flip_ranks(self) -> Self {
        // https://www.chessprogramming.org/Flipping_Mirroring_and_Rotating#Vertical
        self.swap_bytes()
    }

    fn flip_files(self) -> Self {
        // https://www.chessprogramming.org/Flipping_Mirroring_and_Rotating#Horizontal
        const K1: u64 = 0x5555555555555555;
        const K2: u64 = 0x3333333333333333;
        const K4: u64 = 0x0F0F0F0F0F0F0F0F;
        let mut flipped = self;
        flipped = ((flipped >> 1) & K1) | ((flipped & K1) << 1);
        flipped = ((flipped >> 2) & K2) | ((flipped & K2) << 2);
        flipped = ((flipped >> 4) & K4) | ((flipped & K4) << 4);
        flipped
    }

    fn on(square: Square) -> BitBoard {
        1 << square as u64
    }

    fn shift_north(&self) -> BitBoard {
        (self & !Self::LAST_RANK) << File::NUM
    }
    fn shift_south(&self) -> BitBoard {
        (self & !Self::FIRST_RANK) >> File::NUM
    }
    fn shift_east(&self) -> BitBoard {
        (self & !Self::LAST_FILE) << 1
    }

    fn shift_west(&self) -> BitBoard {
        (self & !Self::FIRST_FILE) >> 1
    }

    fn shift_north_east(&self) -> BitBoard {
        (self & !Self::LAST_RANK & !Self::LAST_FILE) << (File::NUM + 1)
    }

    fn shift_north_west(&self) -> BitBoard {
        (self & !Self::LAST_RANK & !Self::FIRST_FILE) << (File::NUM - 1)
    }

    fn shift_south_east(&self) -> BitBoard {
        (self & !Self::FIRST_RANK & !Self::LAST_FILE) >> (File::NUM - 1)
    }

    fn shift_south_west(&self) -> BitBoard {
        (self & !Self::FIRST_RANK & !Self::FIRST_FILE) >> (File::NUM + 1)
    }

    fn pop_lsb(&mut self) -> Option<Square> {
        if *self == 0 {
            return None;
        }
        let idx = self.trailing_zeros() as u8;
        *self &= *self - 1;
        Some(Square::index(idx))
    }
}
/// Iterator yielding each set [`Square`] of a [`BitBoard`] in
/// least-significant bit order. Construct via
/// [`BitBoardMethods::iter`].
pub struct BitBoardIterator {
    bitboard: BitBoard,
}

impl Iterator for BitBoardIterator {
    type Item = Square;

    fn next(&mut self) -> Option<Square> {
        if self.bitboard == 0 {
            return None;
        }
        let idx = self.bitboard.trailing_zeros() as u8;
        self.bitboard &= self.bitboard - 1; // pop LSB
        Some(Square::index(idx))
    }
}

/// Constructs a `const`-evaluated [`BitBoard`] from an 8×8 ASCII grid.
///
/// The grid is written rank 8 first (top) down to rank 1 (bottom), with
/// each square as `X` (set) or `.` (unset). Internally the rows are
/// reversed so the resulting `u64` follows the A1 = 0 ordering used
/// everywhere else.
///
/// Compile errors fire on unknown tokens or wrong square counts.
///
/// ```ignore
/// let center = bitboard! {
///     . . . . . . . .
///     . . . . . . . .
///     . . . . . . . .
///     . . . X X . . .
///     . . . X X . . .
///     . . . . . . . .
///     . . . . . . . .
///     . . . . . . . .
/// };
/// ```
#[macro_export]
macro_rules! bitboard {
    (
        $a8:tt $b8:tt $c8:tt $d8:tt $e8:tt $f8:tt $g8:tt $h8:tt
        $a7:tt $b7:tt $c7:tt $d7:tt $e7:tt $f7:tt $g7:tt $h7:tt
        $a6:tt $b6:tt $c6:tt $d6:tt $e6:tt $f6:tt $g6:tt $h6:tt
        $a5:tt $b5:tt $c5:tt $d5:tt $e5:tt $f5:tt $g5:tt $h5:tt
        $a4:tt $b4:tt $c4:tt $d4:tt $e4:tt $f4:tt $g4:tt $h4:tt
        $a3:tt $b3:tt $c3:tt $d3:tt $e3:tt $f3:tt $g3:tt $h3:tt
        $a2:tt $b2:tt $c2:tt $d2:tt $e2:tt $f2:tt $g2:tt $h2:tt
        $a1:tt $b1:tt $c1:tt $d1:tt $e1:tt $f1:tt $g1:tt $h1:tt
    ) => {
        $crate::bitboard! { @__inner
            $a1 $b1 $c1 $d1 $e1 $f1 $g1 $h1
            $a2 $b2 $c2 $d2 $e2 $f2 $g2 $h2
            $a3 $b3 $c3 $d3 $e3 $f3 $g3 $h3
            $a4 $b4 $c4 $d4 $e4 $f4 $g4 $h4
            $a5 $b5 $c5 $d5 $e5 $f5 $g5 $h5
            $a6 $b6 $c6 $d6 $e6 $f6 $g6 $h6
            $a7 $b7 $c7 $d7 $e7 $f7 $g7 $h7
            $a8 $b8 $c8 $d8 $e8 $f8 $g8 $h8
        }
    };
    (@__inner $($occupied:tt)*) => {{
        const BITBOARD: $crate::BitBoard = {
            let mut index = 0;
            let mut bitboard = $crate::BitBoard::EMPTY;
            $(
                if $crate::bitboard!(@__square $occupied) {
                    bitboard |= 1 << index;
                }
                index += 1;
            )*
            let _ = index;
            bitboard
        };
        BITBOARD
    }};
    (@__square X) => { true };
    (@__square .) => { false };
    (@__square $token:tt) => {
        compile_error!(
            concat!(
                "Expected only `X` or `.` tokens, found `",
                stringify!($token),
                "`"
            )
        )
    };
    ($($token:tt)*) => {
        compile_error!("Expected 64 squares")
    };
}
