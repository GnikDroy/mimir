use num_enum::UnsafeFromPrimitive;

use crate::types::piece::Piece;
use crate::types::promotion_piece::PromotionPiece;
use crate::types::square::Square;

use crate::stack_vec::StackVec;

/// 4-bit tag identifying which kind of move this is.
///
/// The bit pattern is designed so that:
/// - bit 3 ⇒ promotion
/// - bit 2 ⇒ capture
/// - low 2 bits of any `Promotion*` discriminant select the promotion piece
#[derive(Copy, Clone, Eq, PartialEq, Debug, UnsafeFromPrimitive)]
#[repr(u8)]
#[rustfmt::skip]
pub enum MoveType {
    Normal                 = 0b0000,
    DoublePawnPush         = 0b0001,
    KingSideCastling       = 0b0010,
    QueenSideCastling      = 0b0011,
    PromotionKnight        = 0b1000,
    PromotionBishop        = 0b1001,
    PromotionRook          = 0b1010,
    PromotionQueen         = 0b1011,
    PromotionCaptureKnight = 0b1100,
    PromotionCaptureBishop = 0b1101,
    PromotionCaptureRook   = 0b1110,
    PromotionCaptureQueen  = 0b1111,
    Capture                = 0b0100,
    EnPassant              = 0b0101,
}

impl MoveType {
    /// Returns the promotion kind for `piece`, capturing or not.
    #[inline(always)]
    pub fn from_promotion(piece: PromotionPiece, capturing: bool) -> MoveType {
        use MoveType::*;
        match (piece, capturing) {
            (PromotionPiece::Knight, false) => PromotionKnight,
            (PromotionPiece::Bishop, false) => PromotionBishop,
            (PromotionPiece::Rook, false) => PromotionRook,
            (PromotionPiece::Queen, false) => PromotionQueen,
            (PromotionPiece::Knight, true) => PromotionCaptureKnight,
            (PromotionPiece::Bishop, true) => PromotionCaptureBishop,
            (PromotionPiece::Rook, true) => PromotionCaptureRook,
            (PromotionPiece::Queen, true) => PromotionCaptureQueen,
        }
    }

    /// Checks if the move type is a promotion
    #[inline(always)]
    pub fn is_promotion(&self) -> bool {
        let type_u8 = *self as u8;
        type_u8 & 0b1000 != 0
    }

    /// Checks if the move type is a capture
    #[inline(always)]
    pub fn is_capture(&self) -> bool {
        let type_u8 = *self as u8;
        type_u8 & 0b0100 != 0
    }

    /// Promotion target if this is a promotion kind, else `None`.
    #[inline(always)]
    pub fn get_promotion_piece(&self) -> Option<PromotionPiece> {
        use MoveType::*;
        match self {
            PromotionKnight | PromotionCaptureKnight => Some(PromotionPiece::Knight),
            PromotionBishop | PromotionCaptureBishop => Some(PromotionPiece::Bishop),
            PromotionRook | PromotionCaptureRook => Some(PromotionPiece::Rook),
            PromotionQueen | PromotionCaptureQueen => Some(PromotionPiece::Queen),
            _ => None,
        }
    }
}

/// Packed move representation. See the bit layout below and the
/// [`MoveMethods`] trait for construction and field access.
///
/// `Move` is a plain `u32` so it can be `Copy`-cheap, stored densely in
/// move lists, and compared with `==`. All structured access goes through
/// [`MoveMethods`].
///
///
/// BITS    USAGE             SIZE
/// 0–5     from square        (6)
/// 6–11    to square          (6)
/// 12–14   moved piece        (3)
/// 15–17   captured piece     (3)
/// 18–21   kind ([`MoveType`])(4)
/// 22–31   reserved
///
/// The 4-bit `kind` field is a [`MoveType`] discriminant. Its bit 3 is the
/// promotion flag and bit 2 is the capture flag, so generic predicates can
/// be tested with a single mask. For promotion kinds, the low two bits are
/// the promoted-to piece
pub type Move = u32;

// ---- Bit layout constants ------------------------------------------------

const FROM_SHIFT: u32 = 0;
const TO_SHIFT: u32 = 6;
const MOVED_SHIFT: u32 = 12;
const CAPTURED_SHIFT: u32 = 15;
const KIND_SHIFT: u32 = 18;

/// Construction and accessor surface for the packed [`Move`] type.
///
/// Implemented for `u32` so call sites can treat moves as ordinary values
/// while still going through typed helpers. All `from_*` constructors
/// produce bit patterns that round-trip through the matching accessors.
pub trait MoveMethods {
    /// Extracts a contiguous bit-field: `(self >> shift) & mask`.
    fn field(&self, shift: u32, mask: u32) -> u32;

    /// Returns a copy of `self` with the bit-field at `shift` (width
    /// implied by `mask`) replaced by `value`.
    fn with_field(&self, shift: u32, mask: u32, value: u32) -> Self;

    /// Raw 4-bit `kind` field as an integer. Mostly useful for the bit-mask
    /// predicates (`is_capture`, `is_promotion`).
    fn type_bits(&self) -> u32;

    /// Assembles a [`Move`] from its `from`/`to`/`moved`/`kind` fields.
    /// Other fields (captured piece) are OR'd on by the specific
    /// constructor.
    fn encode(from: Square, to: Square, moved: Piece, kind: MoveType) -> Self;

    /// Renders the move in UCI long-algebraic form (e.g. `"e2e4"`,
    /// `"e7e8q"`). Suitable for `bestmove` output and PGN move lists.
    fn to_uci(&self) -> String;

    /// Like [`to_uci`](Self::to_uci), but annotates the move with
    /// its [`MoveType`] (e.g. `"e1g1 (kingside castle)"`). For diagnostics.
    fn debug_string(&self) -> String;

    /// Builds a quiet (non-capture, non-special) move.
    fn from_quiet(from: Square, to: Square, moved: Piece) -> Self;

    /// Builds a pawn double-push.
    fn from_double_pawn_push(from: Square, to: Square) -> Self;

    /// Builds a kingside castle move.
    fn from_kingside_castle(from: Square, to: Square) -> Self;

    /// Builds a queenside castle move.
    fn from_queenside_castle(from: Square, to: Square) -> Self;

    /// Builds a capture. Set `enpassant` for the special pawn capture; the
    /// `captured` piece in that case is the pawn being removed (not the
    /// piece on `to`).
    fn from_capture(
        from: Square,
        to: Square,
        moved: Piece,
        captured: Piece,
        enpassant: bool,
    ) -> Self;

    /// Builds a promotion. Pass `captured = Some(_)` to encode a promoting
    /// capture; `None` for a non-capturing promotion.
    fn from_promotion(
        from: Square,
        to: Square,
        moved: Piece,
        promotion: PromotionPiece,
        captured: Option<Piece>,
    ) -> Self;

    /// Origin square of the moved piece.
    fn get_from(&self) -> Square;
    /// Destination square of the moved piece.
    fn get_to(&self) -> Square;
    /// Overwrites the `from` field in place, preserving all other bits.
    fn set_from(&mut self, from: Square);
    /// Overwrites the `to` field in place, preserving all other bits.
    fn set_to(&mut self, to: Square);

    /// `true` if this is any capture, including en-passant and promoting
    /// captures.
    fn is_capture(&self) -> bool;

    /// `true` if this is a promotion (capturing or not).
    fn is_promotion(&self) -> bool;

    /// `true` if this is a normal move not (capture, promotion, double-push, en-passant or castle).
    fn is_normal(&self) -> bool;

    /// `true` if this is a pawn double advance.
    fn is_double_pawn_push(&self) -> bool;

    /// `true` if this is the special en-passant capture.
    fn is_enpassant(&self) -> bool;

    /// `true` if this is any castling move.
    fn is_castle(&self) -> bool;

    /// `true` if this is specifically a kingside castle. Undefined unless
    /// [`is_castle`](Self::is_castle) returns `true`.
    fn is_kingside_castle(&self) -> bool;

    /// Piece kind of the mover.
    fn get_moved_piece(&self) -> Piece;

    /// Captured piece kind, or `None` for non-captures. For en-passant,
    /// returns the captured pawn (not the piece on `to`).
    fn get_captured_piece(&self) -> Option<Piece>;

    /// Promotion target, or `None` for non-promotions.
    fn get_promotion_piece(&self) -> Option<PromotionPiece>;

    /// Returns the [`MoveType`] tag for this move.
    fn get_type(&self) -> MoveType;
}

impl MoveMethods for Move {
    #[inline(always)]
    fn field(&self, shift: u32, mask: u32) -> u32 {
        (*self >> shift) & mask
    }

    #[inline(always)]
    fn with_field(&self, shift: u32, mask: u32, value: u32) -> Self {
        (*self & !(mask << shift)) | ((value & mask) << shift)
    }

    #[inline(always)]
    fn type_bits(&self) -> u32 {
        self.field(KIND_SHIFT, 0b1111)
    }

    #[inline(always)]
    fn encode(from: Square, to: Square, moved: Piece, kind: MoveType) -> Self {
        ((from as u32) << FROM_SHIFT)
            | ((to as u32) << TO_SHIFT)
            | ((moved as u32) << MOVED_SHIFT)
            | ((kind as u32) << KIND_SHIFT)
    }

    #[inline(always)]
    fn from_quiet(from: Square, to: Square, moved: Piece) -> Self {
        Self::encode(from, to, moved, MoveType::Normal)
    }

    #[inline(always)]
    fn from_double_pawn_push(from: Square, to: Square) -> Self {
        Self::encode(from, to, Piece::Pawn, MoveType::DoublePawnPush)
    }

    #[inline(always)]
    fn from_kingside_castle(from: Square, to: Square) -> Self {
        Self::encode(from, to, Piece::King, MoveType::KingSideCastling)
    }

    #[inline(always)]
    fn from_queenside_castle(from: Square, to: Square) -> Self {
        Self::encode(from, to, Piece::King, MoveType::QueenSideCastling)
    }

    #[inline(always)]
    fn from_capture(
        from: Square,
        to: Square,
        moved: Piece,
        captured: Piece,
        enpassant: bool,
    ) -> Self {
        let k = if enpassant {
            MoveType::EnPassant
        } else {
            MoveType::Capture
        };
        Self::encode(from, to, moved, k) | ((captured as u32) << CAPTURED_SHIFT)
    }

    #[inline(always)]
    fn from_promotion(
        from: Square,
        to: Square,
        moved: Piece,
        promotion: PromotionPiece,
        captured: Option<Piece>,
    ) -> Self {
        let k = MoveType::from_promotion(promotion, captured.is_some());
        let mut m = Self::encode(from, to, moved, k);
        if let Some(c) = captured {
            m |= (c as u32) << CAPTURED_SHIFT;
        }
        m
    }

    #[inline(always)]
    fn get_from(&self) -> Square {
        Square::index(self.field(FROM_SHIFT, 0b11_1111) as u8)
    }

    #[inline(always)]
    fn get_to(&self) -> Square {
        Square::index(self.field(TO_SHIFT, 0b11_1111) as u8)
    }

    #[inline(always)]
    fn set_from(&mut self, from: Square) {
        *self = self.with_field(FROM_SHIFT, 0b11_1111, from as u32);
    }

    #[inline(always)]
    fn set_to(&mut self, to: Square) {
        *self = self.with_field(TO_SHIFT, 0b11_1111, to as u32);
    }

    #[inline(always)]
    fn is_capture(&self) -> bool {
        self.get_type().is_capture()
    }

    #[inline(always)]
    fn is_promotion(&self) -> bool {
        self.get_type().is_promotion()
    }

    #[inline(always)]
    fn is_normal(&self) -> bool {
        self.get_type() == MoveType::Normal
    }

    #[inline(always)]
    fn is_double_pawn_push(&self) -> bool {
        self.get_type() == MoveType::DoublePawnPush
    }

    #[inline(always)]
    fn is_enpassant(&self) -> bool {
        self.get_type() == MoveType::EnPassant
    }

    #[inline(always)]
    fn is_castle(&self) -> bool {
        let k = self.get_type();
        k == MoveType::KingSideCastling || k == MoveType::QueenSideCastling
    }

    #[inline(always)]
    fn is_kingside_castle(&self) -> bool {
        self.get_type() == MoveType::KingSideCastling
    }

    #[inline(always)]
    fn get_moved_piece(&self) -> Piece {
        Piece::index(self.field(MOVED_SHIFT, 0b111) as u8)
    }

    #[inline(always)]
    fn get_captured_piece(&self) -> Option<Piece> {
        if !self.is_capture() {
            return None;
        }
        Some(Piece::index(self.field(CAPTURED_SHIFT, 0b111) as u8))
    }

    #[inline(always)]
    fn get_promotion_piece(&self) -> Option<PromotionPiece> {
        self.get_type().get_promotion_piece()
    }

    #[inline(always)]
    fn get_type(&self) -> MoveType {
        // SAFETY: every Move is constructed via `encode`, which writes a
        // valid MoveType discriminant into the kind field.
        unsafe { MoveType::unchecked_transmute_from(self.type_bits() as u8) }
    }

    fn to_uci(&self) -> String {
        let from = self.get_from();
        let to = self.get_to();

        let mut s = format!("{}{}", from.to_algebraic(), to.to_algebraic());
        if let Some(piece) = self.get_promotion_piece() {
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
        let mut s = self.to_uci();

        match self.get_type() {
            MoveType::Normal => {}
            MoveType::DoublePawnPush => s.push_str(" (double pawn push)"),
            MoveType::KingSideCastling => s.push_str(" (kingside castle)"),
            MoveType::QueenSideCastling => s.push_str(" (queenside castle)"),
            MoveType::Capture => s.push_str(" (capture)"),
            MoveType::EnPassant => s.push_str(" (en passant capture)"),
            k => {
                let piece = k.get_promotion_piece().unwrap();
                if self.is_capture() {
                    s.push_str(&format!(" (capture promo to {:?})", piece));
                } else {
                    s.push_str(&format!(" (promo to {:?})", piece));
                }
            }
        }
        s
    }
}

/// The maximum number of possible chess moves in any given legal position
pub const MAX_MOVE_COUNT: usize = 256;

/// This type is used to store possible chess moves in a position
/// WARN: Do not use this to store moves in a game as it is insufficient.
pub type MoveList = StackVec<Move, MAX_MOVE_COUNT>;
