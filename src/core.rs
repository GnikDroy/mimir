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

#[repr(u8)]
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
    pub fn index(index: u8) -> Self {
        unsafe { Piece::unchecked_transmute_from(index) }
    }
    pub fn all() -> impl Iterator<Item = Piece> {
        (0..Piece::NUM).map(|i| Self::index(i as u8))
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
#[derive(num_enum::UnsafeFromPrimitive, Debug, Clone, Copy, PartialEq, Eq)]
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
#[repr(u8)]
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

    pub fn index(index: u8) -> Self {
        unsafe { Square::unchecked_transmute_from(index as u8) }
    }

    pub fn all() -> impl Iterator<Item = Square> {
        (0..Square::NUM).map(|i| Self::index(i as u8))
    }

    pub fn coordinate(&self) -> (File, Rank) {
        let square = *self as usize;
        let file = square % File::NUM;
        let rank = square / File::NUM;
        (File::index(file), Rank::index(rank))
    }

    pub fn from_coordinate(file: File, rank: Rank) -> Self {
        Self::index(rank as u8 * File::NUM as u8 + file as u8)
    }

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
        format!("{}{}", file_char, rank_char)
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
    pub fn to_piece(&self) -> Piece {
        match self {
            PromotionPiece::Queen => Piece::Queen,
            PromotionPiece::Rook => Piece::Rook,
            PromotionPiece::Bishop => Piece::Bishop,
            PromotionPiece::Knight => Piece::Knight,
        }
    }
}

pub type Move = u32;

/*
32-bit move layout:
0–5   from square        (6)
6–11  to square          (6)
12–14 moved piece        (3)
15–17 captured piece     (3)
18–20 promotion piece    (3)
21    capture flag       (1)
22    promotion flag     (1)
23-24 special flags      (2)
25–31 reserved
*/

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
    fn debug_string(&self) -> String;

    fn from_quiet(from: Square, to: Square, moved: Piece) -> Self;

    fn from_double_pawn_push(from: Square, to: Square, moved: Piece) -> Self;

    fn from_castle(from: Square, to: Square, moved: Piece, kingside: bool) -> Self;

    fn from_capture(
        from: Square,
        to: Square,
        moved: Piece,
        captured: Piece,
        enpassant: bool,
    ) -> Self;

    fn from_promotion(
        from: Square,
        to: Square,
        moved: Piece,
        promotion: PromotionPiece,
        captured: Option<Piece>,
    ) -> Self;

    fn get_from(&self) -> Square;
    fn get_to(&self) -> Square;
    fn set_from(&mut self, from: Square);
    fn set_to(&mut self, to: Square);

    fn is_capture(&self) -> bool;
    fn is_promotion(&self) -> bool;
    fn is_quiet(&self) -> bool;
    fn is_double_pawn_push(&self) -> bool;
    fn is_enpassant(&self) -> bool;
    fn is_castle(&self) -> bool;
    fn is_kingside_castle(&self) -> bool;

    fn get_moved_piece(&self) -> Piece;
    fn get_captured_piece(&self) -> Option<Piece>;
    fn get_promotion_piece(&self) -> Option<PromotionPiece>;

    fn get_type(&self) -> MoveType;
}

impl MoveMethods for Move {
    #[inline(always)]
    fn from_quiet(from: Square, to: Square, moved: Piece) -> Self {
        (from as u32) | ((to as u32) << 6) | ((moved as u32) << 12)
    }

    #[inline(always)]
    fn from_double_pawn_push(from: Square, to: Square, moved: Piece) -> Self {
        let special = 1u32 << 23;
        (from as u32) | ((to as u32) << 6) | ((moved as u32) << 12) | special
    }

    #[inline(always)]
    fn from_castle(from: Square, to: Square, moved: Piece, kingside: bool) -> Self {
        let special = if kingside { 2u32 << 23 } else { 3u32 << 23 };
        (from as u32) | ((to as u32) << 6) | ((moved as u32) << 12) | special
    }

    #[inline(always)]
    fn from_capture(
        from: Square,
        to: Square,
        moved: Piece,
        captured: Piece,
        enpassant: bool,
    ) -> Self {
        let special = if enpassant { 1u32 << 23 } else { 0 };
        (from as u32)
            | ((to as u32) << 6)
            | ((moved as u32) << 12)
            | ((captured as u32) << 15)
            | (1 << 21)
            | special
    }

    #[inline(always)]
    fn from_promotion(
        from: Square,
        to: Square,
        moved: Piece,
        promotion: PromotionPiece,
        captured: Option<Piece>,
    ) -> Self {
        let mut m = (from as u32)
            | ((to as u32) << 6)
            | ((moved as u32) << 12)
            | ((promotion as u32) << 18)
            | (1 << 22);

        if let Some(c) = captured {
            m |= 1 << 21;
            m |= (c as u32) << 15;
        }

        m
    }

    #[inline(always)]
    fn get_from(&self) -> Square {
        Square::index((self & 0b111111) as u8)
    }

    #[inline(always)]
    fn get_to(&self) -> Square {
        Square::index(((self >> 6) & 0b111111) as u8)
    }

    #[inline(always)]
    fn set_from(&mut self, from: Square) {
        *self = (*self & !0b111111) | (from as u32);
    }

    #[inline(always)]
    fn set_to(&mut self, to: Square) {
        *self = (*self & !(0b111111 << 6)) | ((to as u32) << 6);
    }

    #[inline(always)]
    fn is_capture(&self) -> bool {
        (self & (1 << 21)) != 0
    }

    #[inline(always)]
    fn is_promotion(&self) -> bool {
        (self & (1 << 22)) != 0
    }

    #[inline(always)]
    fn is_quiet(&self) -> bool {
        ((self >> 21) & 0b1111) == 0
    }

    #[inline(always)]
    fn is_double_pawn_push(&self) -> bool {
        ((self >> 21) & 0b1111) == 0b0100
    }

    #[inline(always)]
    fn is_enpassant(&self) -> bool {
        ((self >> 21) & 0b1111) == 0b0101
    }

    fn is_castle(&self) -> bool {
        ((self >> 23) & 0b11) >= 0b10
    }

    fn is_kingside_castle(&self) -> bool {
        (self >> 23) & 0b11 == 0b10
    }

    #[inline(always)]
    fn get_moved_piece(&self) -> Piece {
        Piece::index(((self >> 12) & 0b111) as u8)
    }

    #[inline(always)]
    fn get_captured_piece(&self) -> Option<Piece> {
        if (self & (1 << 21)) == 0 {
            return None;
        }
        Some(Piece::index(((self >> 15) & 0b111) as u8))
    }

    #[inline(always)]
    fn get_promotion_piece(&self) -> Option<PromotionPiece> {
        if (self & (1 << 22)) == 0 {
            return None;
        }
        Some(PromotionPiece::index(((self >> 18) & 0b111) as u8))
    }

    #[inline(always)]
    fn get_type(&self) -> MoveType {
        let capture = (self & (1 << 21)) != 0;
        let promo = (self & (1 << 22)) != 0;
        let special = (self >> 23) & 0b11;

        if promo {
            return MoveType::Promotion {
                piece: PromotionPiece::index(((self >> 18) & 0b111) as u8),
                is_capture: capture,
            };
        }

        if capture {
            return MoveType::Capture {
                enpassant: special == 0b01,
            };
        }

        if special == 0b01 {
            return MoveType::DoublePawnPush;
        } else if special != 0 {
            return MoveType::Castle {
                kingside: special == 0b10,
            };
        }

        MoveType::Quiet
    }

    fn repr_string(&self) -> String {
        let from = self.get_from();
        let to = self.get_to();

        let mut s = format!("{}{}", from.to_algebraic(), to.to_algebraic());
        if self.is_promotion() {
            let piece = self.get_promotion_piece().unwrap();
            let char = match piece {
                PromotionPiece::Queen => 'q',
                PromotionPiece::Rook => 'r',
                PromotionPiece::Bishop => 'b',
                PromotionPiece::Knight => 'n',
            };
            s.push(char);
        }

        s
    }

    fn debug_string(&self) -> String {
        let mut s = self.repr_string();

        match self.get_type() {
            MoveType::Quiet => {}
            MoveType::DoublePawnPush => s.push_str(" (double pawn push)"),
            MoveType::Castle { kingside } => {
                if kingside {
                    s.push_str(" (kingside castle)");
                } else {
                    s.push_str(" (queenside castle)");
                }
            }
            MoveType::Capture { enpassant } => {
                if enpassant {
                    s.push_str(" (en passant capture)");
                } else {
                    s.push_str(" (capture)");
                }
            }
            MoveType::Promotion { piece, is_capture } => {
                if is_capture {
                    s.push_str(&format!(" (capture promo to {:?})", piece));
                } else {
                    s.push_str(&format!(" (promo to {:?})", piece));
                }
            }
        }
        s
    }
}
