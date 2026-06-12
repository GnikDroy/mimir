use num_enum::UnsafeFromPrimitive;

use crate::types::color::Color;
use crate::types::file::File;
use crate::types::rank::Rank;

/// A board square, with discriminant `A1 = 0` increasing by file first then
/// rank (so `H1 = 7`, `A2 = 8`, ..., `H8 = 63`).
///
/// This ordering is the foundation of the bitboard layout: bit `n` in any
/// [`BitBoard`](crate::bitboard::BitBoard) corresponds to `Square::index(n)`.
#[rustfmt::skip]
#[repr(u8)]
#[derive(UnsafeFromPrimitive, Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Square {
    A1, B1, C1, D1, E1, F1, G1, H1,
    A2, B2, C2, D2, E2, F2, G2, H2,
    A3, B3, C3, D3, E3, F3, G3, H3,
    A4, B4, C4, D4, E4, F4, G4, H4,
    A5, B5, C5, D5, E5, F5, G5, H5,
    A6, B6, C6, D6, E6, F6, G6, H6,
    A7, B7, C7, D7, E7, F7, G7, H7,
    A8, B8, C8, D8, E8, F8, G8, H8,
}

impl Square {
    /// Number of squares (always 64).
    pub const NUM: usize = std::mem::variant_count::<Square>();

    /// Reconstructs a [`Square`] from its discriminant.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `index` is not in `0..NUM`. Release
    /// builds skip the check; passing an out-of-range value is undefined
    /// behaviour.
    #[inline(always)]
    pub fn index(index: u8) -> Self {
        debug_assert!((index as usize) < Self::NUM);
        unsafe { Square::unchecked_transmute_from(index) }
    }

    /// Iterator over every square from `A1` to `H8` in discriminant order.
    #[inline(always)]
    pub fn all() -> impl Iterator<Item = Square> {
        (0..Square::NUM).map(|i| Self::index(i as u8))
    }

    /// Decomposes the square into its `(file, rank)` coordinates.
    #[inline(always)]
    pub fn coordinate(&self) -> (File, Rank) {
        let square = *self as usize;
        let file = square % File::NUM;
        let rank = square / File::NUM;
        (File::index(file), Rank::index(rank))
    }

    /// Composes a square from `(file, rank)` coordinates using the
    /// `A1 = 0`, file-first encoding.
    #[inline(always)]
    pub fn from_coordinate(file: File, rank: Rank) -> Self {
        Self::index(rank as u8 * File::NUM as u8 + file as u8)
    }

    /// Parses a two-character algebraic square like `"e4"` (case-insensitive
    /// in the file letter). Returns `None` if the input is not exactly two
    /// characters or contains a non-file / non-rank symbol.
    pub fn from_algebraic(algebraic: &str) -> Option<Self> {
        if algebraic.len() != 2 {
            return None;
        }
        let mut chars = algebraic.chars();
        let file_char = chars.next().unwrap();
        let rank_char = chars.next().unwrap();

        let file = match file_char {
            'a' | 'A' => File::A,
            'b' | 'B' => File::B,
            'c' | 'C' => File::C,
            'd' | 'D' => File::D,
            'e' | 'E' => File::E,
            'f' | 'F' => File::F,
            'g' | 'G' => File::G,
            'h' | 'H' => File::H,
            _ => return None,
        };

        let rank = match rank_char {
            '1' => Rank::First,
            '2' => Rank::Second,
            '3' => Rank::Third,
            '4' => Rank::Fourth,
            '5' => Rank::Fifth,
            '6' => Rank::Sixth,
            '7' => Rank::Seventh,
            '8' => Rank::Eighth,
            _ => return None,
        };

        Some(Self::from_coordinate(file, rank))
    }

    /// Renders the square as two lowercase characters (e.g. `"e4"`).
    pub fn to_algebraic(&self) -> String {
        let (file, rank) = self.coordinate();
        let file_char = match file {
            File::A => 'a',
            File::B => 'b',
            File::C => 'c',
            File::D => 'd',
            File::E => 'e',
            File::F => 'f',
            File::G => 'g',
            File::H => 'h',
        };
        let rank_char = match rank {
            Rank::First => '1',
            Rank::Second => '2',
            Rank::Third => '3',
            Rank::Fourth => '4',
            Rank::Fifth => '5',
            Rank::Sixth => '6',
            Rank::Seventh => '7',
            Rank::Eighth => '8',
        };
        String::from_iter([file_char, rank_char])
    }

    /// Mirrors the square across the horizontal axis (rank 1 ↔ rank 8,
    /// rank 2 ↔ rank 7, ...). Used to look up black-side PST values from
    /// white-relative tables.
    pub fn flip_vertical(&self) -> Self {
        let (file, rank) = self.coordinate();
        Self::from_coordinate(file, Rank::index(Rank::NUM - 1 - rank as usize))
    }

    /// Returns `true` if a pawn of the given colour reaching this square is
    /// promoting (rank 8 for white, rank 1 for black).
    pub fn is_promotion_square(&self, color: Color) -> bool {
        let (_, rank) = self.coordinate();
        match color {
            Color::White => rank == Rank::Eighth,
            Color::Black => rank == Rank::First,
        }
    }

    /// Returns `true` if this is the starting square of a pawn of the given
    /// colour (rank 2 for white, rank 7 for black). Used to gate double
    /// pawn pushes.
    pub fn is_pawn_start_square(&self, color: Color) -> bool {
        let (_, rank) = self.coordinate();
        match color {
            Color::White => rank == Rank::Second,
            Color::Black => rank == Rank::Seventh,
        }
    }
}
