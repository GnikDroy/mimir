extern crate num_enum;
use num_enum::UnsafeFromPrimitive;

#[repr(u8)]
#[derive(num_enum::UnsafeFromPrimitive, Debug, Clone, Copy)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub const NUM: usize = std::mem::variant_count::<Color>();
    pub fn index(index: usize) -> Self {
        unsafe { Color::unchecked_transmute_from(index as u8) }
    }
    pub fn all() -> impl Iterator<Item = Color> {
        (0..Color::NUM).map(|i| Self::index(i))
    }
}

#[repr(usize)]
#[derive(num_enum::UnsafeFromPrimitive, Debug, Clone, Copy)]
pub enum Piece {
    King,
    Queen,
    Rook,
    Bishop,
    Knight,
    Pawn,
}

impl Piece {
    pub const NUM: usize = std::mem::variant_count::<Piece>();
    pub fn index(index: usize) -> Self {
        unsafe { Piece::unchecked_transmute_from(index) }
    }
    pub fn all() -> impl Iterator<Item = Piece> {
        (0..Piece::NUM).map(|i| Self::index(i))
    }
}

#[repr(u8)]
#[derive(num_enum::UnsafeFromPrimitive, Debug, Clone, Copy)]
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
    pub const NUM: usize = std::mem::variant_count::<Rank>();
    pub fn index(index: usize) -> Self {
        unsafe { Rank::unchecked_transmute_from(index as u8) }
    }
    pub fn all() -> impl Iterator<Item = Rank> {
        (0..Rank::NUM).map(|i| Self::index(i))
    }
}

#[repr(u8)]
#[derive(num_enum::UnsafeFromPrimitive, Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum File {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
}
impl File {
    pub const NUM: usize = std::mem::variant_count::<File>();
    pub fn index(index: usize) -> Self {
        unsafe { File::unchecked_transmute_from(index as u8) }
    }
    pub fn all() -> impl Iterator<Item = File> {
        (0..File::NUM).map(|i| Self::index(i))
    }
}

#[rustfmt::skip]
#[repr(usize)]
#[derive(num_enum::UnsafeFromPrimitive, Debug, Clone, Copy)]
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
    pub const NUM: usize = std::mem::variant_count::<Square>();
    pub fn index(index: usize) -> Self {
        unsafe { Square::unchecked_transmute_from(index) }
    }
    pub fn all() -> impl Iterator<Item = Square> {
        (0..Square::NUM).map(|i| Self::index(i))
    }
    pub fn coordinate(&self) -> (File, Rank) {
        let square = *self as usize;
        let file = square % File::NUM;
        let rank = square / File::NUM;
        (File::index(file), Rank::index(rank))
    }
    pub fn from_coordinate(file: File, rank: Rank) -> Self {
        Self::index(rank as usize * File::NUM + file as usize)
    }
}
