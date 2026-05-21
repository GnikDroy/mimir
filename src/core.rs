extern crate num_enum;
use num_enum::UnsafeFromPrimitive;

#[repr(u8)]
#[derive(num_enum::UnsafeFromPrimitive, Debug, Clone, Copy, PartialEq, Eq)]
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
    pub fn opposite(&self) -> Self {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}

#[repr(usize)]
#[derive(num_enum::UnsafeFromPrimitive, Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
#[derive(num_enum::UnsafeFromPrimitive, Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(num_enum::UnsafeFromPrimitive, Debug, Clone, Copy, PartialEq, Eq)]
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

    pub fn flip_vertical(&self) -> Self {
        let (file, rank) = self.coordinate();
        Self::from_coordinate(file, Rank::index(Rank::NUM - 1 - rank as usize))
    }

    pub fn is_promotion_square(&self, color: Color) -> bool {
        let (_file, rank) = self.coordinate();
        match color {
            Color::White => rank == Rank::Eighth,
            Color::Black => rank == Rank::First,
        }
    }

    pub fn is_pawn_start_square(&self, color: Color) -> bool {
        let (_file, rank) = self.coordinate();
        match color {
            Color::White => rank == Rank::Second,
            Color::Black => rank == Rank::Seventh,
        }
    }
}

#[repr(u8)]
#[derive(num_enum::UnsafeFromPrimitive, Debug, Clone, Copy)]
pub enum PromotionPiece {
    Queen,
    Rook,
    Bishop,
    Knight,
}

impl PromotionPiece {
    pub const NUM: u8 = std::mem::variant_count::<PromotionPiece>() as u8;
    pub fn index(index: u8) -> Self {
        unsafe { PromotionPiece::unchecked_transmute_from(index) }
    }
    pub fn all() -> impl Iterator<Item = PromotionPiece> {
        (0..PromotionPiece::NUM).map(|i| Self::index(i as u8))
    }
}

// Encode move as 16 bits: 6 bits for from square, 6 bits for to square
// [0..5] bits for from square, [6..11] bits for to square, 12th bit for if promotion, 13th bit for if capture, [14..15] bits for special flags.
// 1 bit for if capture, 1 bit for if promotion, 2 bits for special flags.
// if capture but no promotion, special flags are used to detect en passant.
// if capture and promotion, special flags are used to encode which piece the pawn promotes to.
// if no capture and promotion, special flags are used to encode which piece the pawn promotes to.
// if no capture and no promotion, special flags are used to encode double pawn push, and castling (kingside or queenside).
pub type Move = u16;

pub enum MoveType {
    Quiet,
    DoublePawnPush,
    Castle {
        kingside: bool,
    },
    Capture {
        enpassant: bool,
    },
    Promotion {
        piece: PromotionPiece,
        is_capture: bool,
    },
}
pub trait MoveMethods {
    fn repr_string(&self) -> String;
    fn from_quiet(from: Square, to: Square) -> Self;
    fn from_double_pawn_push(from: Square, to: Square) -> Self;
    fn from_castle(from: Square, to: Square, kingside: bool) -> Self;
    fn from_capture(from: Square, to: Square, enpassant: bool) -> Self;
    fn from_promotion(
        from: Square,
        to: Square,
        promotion: PromotionPiece,
        is_capture: bool,
    ) -> Self;

    fn get_from(&self) -> Square;
    fn get_to(&self) -> Square;
    fn get_type(&self) -> MoveType;
}

impl MoveMethods for Move {
    fn repr_string(&self) -> String {
        let from = self.get_from();
        let to = self.get_to();
        let move_type = self.get_type();

        let mut repr = format!("{:?}{:?}", from, to);
        match move_type {
            MoveType::Quiet => {}
            MoveType::DoublePawnPush => repr.push_str(" (double pawn push)"),
            MoveType::Castle { kingside } => {
                if kingside {
                    repr.push_str(" (kingside castle)");
                } else {
                    repr.push_str(" (queenside castle)");
                }
            }
            MoveType::Capture { enpassant } => {
                if enpassant {
                    repr.push_str(" (en passant capture)");
                } else {
                    repr.push_str(" (capture)");
                }
            }
            MoveType::Promotion { piece, is_capture } => {
                if is_capture {
                    repr.push_str(&format!(" (capture and promote to {:?})", piece));
                } else {
                    repr.push_str(&format!(" (promote to {:?})", piece));
                }
            }
        }
        repr
    }
    fn from_quiet(from: Square, to: Square) -> Self {
        let from_bits = (from as u16) & 0b111111; // 6 bits for from square
        let to_bits = ((to as u16) & 0b111111) << 6; // 6 bits for to square
        from_bits | to_bits
    }

    fn from_double_pawn_push(from: Square, to: Square) -> Self {
        let from_bits = (from as u16) & 0b111111; // 6 bits for from square
        let to_bits = ((to as u16) & 0b111111) << 6; // 6 bits for to square
        let special_bits = 1u16 << 14; // 1 bit for if double pawn push
        from_bits | to_bits | special_bits
    }

    fn from_castle(from: Square, to: Square, kingside: bool) -> Self {
        let from_bits = (from as u16) & 0b111111; // 6 bits for from square
        let to_bits = ((to as u16) & 0b111111) << 6; // 6 bits for to square
        let special_bits = if kingside { 2u16 << 14 } else { 3u16 << 14 };
        from_bits | to_bits | special_bits
    }

    fn from_capture(from: Square, to: Square, enpassant: bool) -> Self {
        let from_bits = (from as u16) & 0b111111; // 6 bits for from square
        let to_bits = ((to as u16) & 0b111111) << 6; // 6 bits for to square
        let is_capture_bit = 1u16 << 13; // 1 bit for if capture
        let special_bits = if enpassant { 1u16 << 14 } else { 0 };
        from_bits | to_bits | is_capture_bit | special_bits
    }

    fn from_promotion(from: Square, to: Square, piece: PromotionPiece, is_capture: bool) -> Self {
        let from_bits = (from as u16) & 0b111111; // 6 bits for from square
        let to_bits = ((to as u16) & 0b111111) << 6; // 6 bits for to square
        let is_promotion_bit = 1u16 << 12;
        let is_capture_bit = if is_capture { 1u16 << 13 } else { 0 };
        let special_bits = (piece as u16) << 14;
        from_bits | to_bits | is_promotion_bit | is_capture_bit | special_bits
    }

    fn get_from(&self) -> Square {
        let from_bits = self & 0b111111; // 6 bits for from square
        Square::index(from_bits as usize)
    }

    fn get_to(&self) -> Square {
        let to_bits = (self >> 6) & 0b111111; // 6 bits for to square
        Square::index(to_bits as usize)
    }

    fn get_type(&self) -> MoveType {
        let is_promotion_bit = (self >> 12) & 1; // 1 bit for if promotion
        let is_capture_bit = (self >> 13) & 1; // 1 bit for if capture
        let special_bits = ((self >> 14) & 0b11) as u8; // 2 bits for special moves

        if is_promotion_bit == 1 {
            MoveType::Promotion {
                piece: PromotionPiece::index(special_bits),
                is_capture: is_capture_bit == 1,
            }
        } else if is_capture_bit == 1 {
            MoveType::Capture {
                enpassant: special_bits == 1,
            }
        } else if special_bits == 0 {
            MoveType::Quiet
        } else if special_bits == 1 {
            MoveType::DoublePawnPush
        } else {
            MoveType::Castle {
                kingside: special_bits == 2,
            }
        }
    }
}
