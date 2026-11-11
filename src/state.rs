//! Mutable chess position and its move-application API.
//!
//! [`GameState`] is the engine's central type: piece bitboards, derived
//! occupancies, side to move, castling rights, en-passant target,
//! 50-move and full-move clocks, plus a running [`ZobristHash`].
//!
//! [`GameState::make_move`] returns an [`UndoInfo`] snapshot of the
//! fields that can't be recovered from the move alone. Passing the same
//! pair to [`GameState::unmake_move`] restores the position exactly.
//! Search and perft rely on this symmetry.
//!
//! `make_move` does not validate legality.
use crate::attack_table::ATTACK_TABLE;
use crate::bitboard::*;
use crate::core::*;
use crate::nnue::{self, Accumulator};
use crate::zobrist::{ZobristHash, ZOBRIST_HASHER};

/// Snapshot of the fields [`GameState::make_move`] cannot recompute
/// from the move alone. Must be passed back to
/// [`GameState::unmake_move`] to reverse the move exactly.
#[derive(Debug, Clone, Copy)]
pub struct UndoInfo {
    /// Piece captured by this move, if any. For en-passant this is the
    /// pawn removed from behind the destination square, not a piece on
    /// the destination square itself.
    pub captured_piece: Option<Piece>,
    /// Kind of the moving piece. Recorded so unmake can place it back
    /// without re-deriving from the (now-mutated) board.
    pub moved_piece: Piece,
    /// En-passant target square before the move, or [`None`].
    pub en_passant_before: Option<Square>,
    /// Castling-rights bitmask before the move. See
    /// [`GameState::castling_rights`] for the layout.
    pub castling_rights_before: u8,
    /// 50-move halfmove clock before the move.
    pub halfmove_clock_before: u8,
    /// Fullmove number before the move.
    pub fullmove_number_before: u16,
}

/// Full chess position: bitboards, occupancies, clocks, and the running
/// Zobrist hash. Constructed via [`GameState::new`] (start position),
/// [`GameState::empty`] (no pieces), or [`GameState::from_fen`].
#[derive(Debug, Clone, Copy)]
pub struct GameState {
    /// Piece bitboards indexed `[color][piece_kind]`.
    pub pieces: [[BitBoard; Piece::NUM]; Color::NUM],
    /// Per-side occupancies plus a combined occupancy at index
    /// `Color::NUM` (i.e. `occupancies[2]`).
    pub occupancies: [BitBoard; Color::NUM + 1],

    /// Side whose turn it is to move next.
    pub side_to_move: Color,
    /// Castling availability as a 4-bit mask (LSB first):
    /// bit 0 = white kingside, bit 1 = white queenside,
    /// bit 2 = black kingside, bit 3 = black queenside.
    pub castling_rights: u8,
    /// Square *behind* a pawn that just made a double push, i.e. the
    /// square an enemy pawn would capture *to* via en passant. [`None`]
    /// when no en-passant capture is currently legal.
    pub en_passant: Option<Square>,
    /// Halfmoves since the last pawn move or capture (50-move rule).
    pub halfmove_clock: u8,
    /// 1-based fullmove counter. Increments after every black move.
    pub fullmove_number: u16,
    /// Incrementally maintained Zobrist key for this position.
    pub zobrist_hash: ZobristHash,
    /// NNUE feature accumulators, one per perspective
    /// (`[Color::White as usize]` = white-to-move, `[Color::Black as usize]` = black).
    /// Kept up-to-date by `make_move`/`unmake_move`. Consumed by [`crate::nnue::evaluate`].
    pub accumulators: [Accumulator; Color::NUM],
}

impl GameState {
    /// Creates an empty board: no pieces, white to move, no castling
    /// rights, no en-passant, clocks reset. The Zobrist hash is
    /// computed from scratch to match this state.
    pub fn empty() -> Self {
        let mut state = GameState {
            pieces: [[0; Piece::NUM]; Color::NUM],
            occupancies: [0; Color::NUM + 1],
            side_to_move: Color::White,
            castling_rights: 0,
            en_passant: None,
            halfmove_clock: 0,
            fullmove_number: 1,
            zobrist_hash: 0,
            accumulators: nnue::empty_accumulators(),
        };
        state.zobrist_hash = ZOBRIST_HASHER.hash(&state);
        state
    }

    /// Creates the standard chess starting position.
    pub fn new() -> Self {
        GameState::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap()
    }
}

impl Default for GameState {
    fn default() -> Self {
        Self::new()
    }
}

impl GameState {
    /// Returns the colour and kind of the piece standing on `square`,
    /// or [`None`] when the square is empty.
    pub fn piece_at(&self, square: Square) -> Option<(Color, Piece)> {
        let mask = BitBoard::on(square);
        if self.occupancies[2] & mask == 0 {
            return None;
        }

        for color in Color::all() {
            if self.occupancies[color as usize] & mask != 0 {
                for piece in Piece::all() {
                    if self.pieces[color as usize][piece as usize] & mask != 0 {
                        return Some((color, piece));
                    }
                }
            }
        }
        None
    }

    /// Returns `true` if any piece of color `attacker` attacks `sq` in
    /// the current position. Used for both check detection and castling
    /// safety. Does not require `sq` to be occupied or empty.
    #[inline]
    pub fn is_square_attacked(&self, sq: Square, attacker: Color) -> bool {
        let occ = self.occupancies[2];
        let enemy = &self.pieces[attacker as usize];

        let sq_bb = BitBoard::on(sq);

        let rook_attackers = enemy[Piece::Rook as usize] | enemy[Piece::Queen as usize];
        if (ATTACK_TABLE.get_rook(sq, occ) & rook_attackers) != 0 {
            return true;
        }

        let bishop_attackers = enemy[Piece::Bishop as usize] | enemy[Piece::Queen as usize];
        if (ATTACK_TABLE.get_bishop(sq, occ) & bishop_attackers) != 0 {
            return true;
        }

        if (ATTACK_TABLE.get_knight(sq) & enemy[Piece::Knight as usize]) != 0 {
            return true;
        }

        let pawn_attackers = match attacker {
            Color::White => sq_bb.shift_south_east() | sq_bb.shift_south_west(),
            Color::Black => sq_bb.shift_north_east() | sq_bb.shift_north_west(),
        };

        if (pawn_attackers & enemy[Piece::Pawn as usize]) != 0 {
            return true;
        }
        if (ATTACK_TABLE.get_king(sq) & enemy[Piece::King as usize]) != 0 {
            return true;
        }

        false
    }

    /// Returns `true` if `color`'s king is attacked. Assumes exactly
    /// one king of that color is on the board (always true for legal
    /// positions reached via [`make_move`](Self::make_move)).
    #[inline(always)]
    pub fn is_in_check(&self, color: Color) -> bool {
        let enemy = color.opposite();
        let king_bb = self.pieces[color as usize][Piece::King as usize];
        self.is_square_attacked(Square::index(king_bb.trailing_zeros() as u8), enemy)
    }

    // Checks if no legal moves exist.
    fn no_moves(&self) -> bool {
        let mut moves = MoveList::default();
        self.generate_moves(&mut moves);
        moves.is_empty()
    }

    /// `true` when the side to move is in check and has no legal reply.
    /// Generates legal moves internally, so callers should cache the
    /// result rather than re-querying inside a hot loop.
    #[inline(always)]
    pub fn is_checkmate(&self) -> bool {
        self.is_in_check(self.side_to_move) && self.no_moves()
    }

    /// `true` when the side to move has no legal reply but is *not* in
    /// check. Like [`is_checkmate`](Self::is_checkmate), generates
    /// legal moves internally.
    #[inline(always)]
    pub fn is_stalemate(&self) -> bool {
        !self.is_in_check(self.side_to_move) && self.no_moves()
    }

    /// Mirrors the position so that it is other side to play
    ///
    /// Computed through FEN rather than editing bitboards so the derived
    /// state (Zobrist hash, NNUE accumulators) is rebuilt correctly.
    pub fn flipped(&self) -> Option<GameState> {
        GameState::from_fen(&crate::notation::fen::flip(&self.to_fen())?).ok()
    }

    #[inline]
    pub fn is_draw_by_insufficient_material(&self) -> bool {
        // Having major pieces or pawns means no draw.
        let pawns = self.pieces[Color::White as usize][Piece::Pawn as usize]
            | self.pieces[Color::Black as usize][Piece::Pawn as usize];
        let rooks = self.pieces[Color::White as usize][Piece::Rook as usize]
            | self.pieces[Color::Black as usize][Piece::Rook as usize];
        let queens = self.pieces[Color::White as usize][Piece::Queen as usize]
            | self.pieces[Color::Black as usize][Piece::Queen as usize];

        if (pawns | rooks | queens) != 0 {
            return false;
        }

        let bishops = self.pieces[Color::White as usize][Piece::Bishop as usize]
            | self.pieces[Color::Black as usize][Piece::Bishop as usize];
        let knights = self.pieces[Color::White as usize][Piece::Knight as usize]
            | self.pieces[Color::Black as usize][Piece::Knight as usize];

        // If exactly 0 or 1 minors then it is a draw.
        let minors = bishops | knights;
        if minors.count_ones() < 2 {
            return true;
        }

        // If only bishops exist and all lie on the same color complex it is a draw.
        // otherwise it is not a draw by insufficient material
        let light_bishops = bishops & BitBoard::LIGHT_SQUARES;
        let dark_bishops = bishops & BitBoard::DARK_SQUARES;

        knights == 0 && (light_bishops == 0 || dark_bishops == 0)
    }

    /// Adds a piece to the board at `sq`. `sq_bb` must equal
    /// `BitBoard::on(sq)` (passed in so callers can reuse it).
    ///
    /// Does **not** touch `occupancies[2]`: make/unmake recompute it
    /// once at the bottom.
    #[inline(always)]
    fn place_piece(
        &mut self,
        color: Color,
        piece: Piece,
        sq: Square,
        sq_bb: BitBoard,
        key: &mut ZobristHash,
    ) {
        self.pieces[color as usize][piece as usize] |= sq_bb;
        self.occupancies[color as usize] |= sq_bb;
        *key ^= ZOBRIST_HASHER.square[color as usize][piece as usize][sq as usize];
        nnue::add_piece(&mut self.accumulators, color, piece, sq);
    }

    /// Removes a piece from the board at `sq`. See [`place_piece`] for
    /// the `sq_bb` convention and the `occupancies[2]` invariant.
    #[inline(always)]
    fn remove_piece(
        &mut self,
        color: Color,
        piece: Piece,
        sq: Square,
        sq_bb: BitBoard,
        key: &mut ZobristHash,
    ) {
        self.pieces[color as usize][piece as usize] &= !sq_bb;
        self.occupancies[color as usize] &= !sq_bb;
        *key ^= ZOBRIST_HASHER.square[color as usize][piece as usize][sq as usize];
        nnue::remove_piece(&mut self.accumulators, color, piece, sq);
    }

    /// Moves a piece from `from` to `to`. `mask` must equal
    /// `BitBoard::on(from) | BitBoard::on(to)`.
    ///
    /// The XOR form requires `from` set and `to` clear at entry, so the
    /// caller must remove any enemy piece on `to` first. Swapping `from`
    /// and `to` reverses the call exactly (used by [`unmake_move`]).
    #[inline(always)]
    fn move_piece(
        &mut self,
        color: Color,
        piece: Piece,
        from: Square,
        to: Square,
        mask: BitBoard,
        key: &mut ZobristHash,
    ) {
        self.pieces[color as usize][piece as usize] ^= mask;
        self.occupancies[color as usize] ^= mask;
        *key ^= ZOBRIST_HASHER.square[color as usize][piece as usize][from as usize];
        *key ^= ZOBRIST_HASHER.square[color as usize][piece as usize][to as usize];
        nnue::remove_piece(&mut self.accumulators, color, piece, from);
        nnue::add_piece(&mut self.accumulators, color, piece, to);
    }

    /// Applies `move_encoded` and returns the [`UndoInfo`] that
    /// [`unmake_move`](Self::unmake_move) needs to reverse it.
    ///
    /// **Does not validate legality.** The caller must pass a legal or
    /// pseudo-legal move and filter out moves that leave the moving
    /// side in check.
    pub fn make_move(&mut self, move_encoded: Move) -> UndoInfo {
        let from = move_encoded.get_from();
        let to = move_encoded.get_to();

        let from_board = BitBoard::on(from);
        let to_board = BitBoard::on(to);

        let moving_color = self.side_to_move;
        let enemy_color = moving_color.opposite();

        // Save undo info before making changes
        let en_passant_before = self.en_passant;
        let castling_rights_before = self.castling_rights;
        let halfmove_clock_before = self.halfmove_clock;
        let fullmove_number_before = self.fullmove_number;

        // Find the moving piece
        let moving_piece = move_encoded.get_moved_piece();

        // Prepare incremental zobrist key update
        let mut key = self.zobrist_hash;

        // remove castling & en passant old keys
        let castling_index_before = (castling_rights_before & 0b1111) as usize;
        key ^= ZOBRIST_HASHER.castling_rights[castling_index_before];
        if let Some(ep_sq) = en_passant_before {
            let file = ep_sq.coordinate().0 as usize;
            key ^= ZOBRIST_HASHER.en_passant[file];
        }

        let captured_piece = move_encoded.get_captured_piece();
        let mover_mask = from_board | to_board;

        // Remove the captured piece first so `move_piece` below sees a
        // clear `to` square. En passant has its own capture square,
        // handled further down.
        if let Some(captured) = captured_piece {
            if !move_encoded.is_enpassant() {
                self.remove_piece(enemy_color, captured, to, to_board, &mut key);
            }
        }

        // Move the mover. Promotions are asymmetric (pawn at `from`,
        // promoted piece at `to`) so they split into remove + place.
        if move_encoded.is_promotion() {
            let piece = move_encoded.get_promotion_piece().unwrap().to_piece();
            self.remove_piece(moving_color, Piece::Pawn, from, from_board, &mut key);
            self.place_piece(moving_color, piece, to, to_board, &mut key);
        } else {
            self.move_piece(moving_color, moving_piece, from, to, mover_mask, &mut key);
        }

        // En passant: pawn capture happens on a different square from
        // the mover's destination.
        if move_encoded.is_enpassant() {
            let capture_square = match moving_color {
                Color::White => Square::index(to as u8 - 8),
                Color::Black => Square::index(to as u8 + 8),
            };
            let capture_board = BitBoard::on(capture_square);
            self.remove_piece(
                enemy_color,
                Piece::Pawn,
                capture_square,
                capture_board,
                &mut key,
            );
        }

        // Update castling rights
        if moving_piece == Piece::King {
            let bits_to_clear = match moving_color {
                Color::White => 0b0011,
                Color::Black => 0b1100,
            };
            self.castling_rights &= !bits_to_clear;
        } else if moving_piece == Piece::Rook {
            match (moving_color, from) {
                (Color::White, Square::A1) => self.castling_rights &= !0b0010,
                (Color::White, Square::H1) => self.castling_rights &= !0b0001,
                (Color::Black, Square::A8) => self.castling_rights &= !0b1000,
                (Color::Black, Square::H8) => self.castling_rights &= !0b0100,
                _ => {}
            }
        }

        // Also lose castling rights if a rook is captured
        if let Some(Piece::Rook) = captured_piece {
            match (enemy_color, to) {
                (Color::White, Square::A1) => self.castling_rights &= !0b0010,
                (Color::White, Square::H1) => self.castling_rights &= !0b0001,
                (Color::Black, Square::A8) => self.castling_rights &= !0b1000,
                (Color::Black, Square::H8) => self.castling_rights &= !0b0100,
                _ => {}
            }
        }

        // Update en passant
        self.en_passant = if move_encoded.is_double_pawn_push() {
            let ep_square = match moving_color {
                Color::White => Square::index(to as u8 - 8),
                Color::Black => Square::index(to as u8 + 8),
            };
            Some(ep_square)
        } else {
            None
        };

        // XOR in new en passant if present
        if let Some(ep_sq) = self.en_passant {
            let file = ep_sq.coordinate().0 as usize;
            key ^= ZOBRIST_HASHER.en_passant[file];
        }

        // Handle castling
        if move_encoded.is_castle() {
            let kingside = move_encoded.is_kingside_castle();
            let (rook_from, rook_to) = match (moving_color, kingside) {
                (Color::White, true) => (Square::H1, Square::F1),
                (Color::White, false) => (Square::A1, Square::D1),
                (Color::Black, true) => (Square::H8, Square::F8),
                (Color::Black, false) => (Square::A8, Square::D8),
            };
            let rook_mask = BitBoard::on(rook_from) | BitBoard::on(rook_to);
            self.move_piece(
                moving_color,
                Piece::Rook,
                rook_from,
                rook_to,
                rook_mask,
                &mut key,
            );
        }

        self.occupancies[2] =
            self.occupancies[moving_color as usize] | self.occupancies[enemy_color as usize];

        // XOR in new castling rights
        let castling_index_after = (self.castling_rights & 0b1111) as usize;
        key ^= ZOBRIST_HASHER.castling_rights[castling_index_after];

        // Toggle side to move (and xor side key)
        self.side_to_move = enemy_color;
        key ^= ZOBRIST_HASHER.side_is_black;

        if self.side_to_move == Color::White {
            self.fullmove_number += 1;
        }

        if moving_piece == Piece::Pawn || move_encoded.is_capture() {
            self.halfmove_clock = 0;
        } else {
            self.halfmove_clock += 1;
        }

        // store updated hash
        self.zobrist_hash = key;

        UndoInfo {
            captured_piece,
            moved_piece: moving_piece,
            en_passant_before,
            castling_rights_before,
            halfmove_clock_before,
            fullmove_number_before,
        }
    }

    /// Reverses [`make_move`](Self::make_move). The `(move, undo_info)`
    /// pair must be exactly what the matching `make_move` produced.
    /// Using them out of order corrupts the position.
    pub fn unmake_move(&mut self, move_encoded: Move, undo_info: &UndoInfo) {
        let from = move_encoded.get_from();
        let to = move_encoded.get_to();

        // Restore side to move first (so we identify the moving side correctly)
        self.side_to_move = self.side_to_move.opposite();

        // Prepare incremental zobrist update
        let mut key = self.zobrist_hash;
        // flipping side: xor side key
        key ^= ZOBRIST_HASHER.side_is_black;

        // remove current castling & en passant keys (we will xor in the previous ones later)
        let castling_index_current = (self.castling_rights & 0b1111) as usize;
        key ^= ZOBRIST_HASHER.castling_rights[castling_index_current];
        if let Some(ep_sq) = self.en_passant {
            let file = ep_sq.coordinate().0 as usize;
            key ^= ZOBRIST_HASHER.en_passant[file];
        }

        let from_board = BitBoard::on(from);
        let to_board = BitBoard::on(to);

        let moving_color = self.side_to_move;
        let enemy_color = moving_color.opposite();

        let mover_mask = from_board | to_board;

        // Reverse the mover: bring it back to `from`. Promotions are
        // asymmetric (promoted piece at `to`, pawn at `from`) so they
        // split into remove + place.
        if move_encoded.is_promotion() {
            let piece = move_encoded.get_promotion_piece().unwrap().to_piece();
            self.remove_piece(moving_color, piece, to, to_board, &mut key);
            self.place_piece(moving_color, Piece::Pawn, from, from_board, &mut key);
        } else {
            let moving_piece = undo_info.moved_piece;
            // Swap `from`/`to` to reverse the original make_move call.
            self.move_piece(moving_color, moving_piece, to, from, mover_mask, &mut key);
        }

        // Restore the captured piece. For en passant the capture
        // square sits behind `to`.
        if move_encoded.is_enpassant() {
            let capture_square = match moving_color {
                Color::White => Square::index(to as u8 - File::NUM as u8),
                Color::Black => Square::index(to as u8 + File::NUM as u8),
            };
            let capture_board = BitBoard::on(capture_square);
            self.place_piece(
                enemy_color,
                Piece::Pawn,
                capture_square,
                capture_board,
                &mut key,
            );
        } else if move_encoded.is_capture() {
            let captured = undo_info.captured_piece.unwrap();
            self.place_piece(enemy_color, captured, to, to_board, &mut key);
        }

        // Handle castling unmake, swap rook back.
        if move_encoded.is_castle() {
            let (rook_from, rook_to) = match (moving_color, move_encoded.is_kingside_castle()) {
                (Color::White, true) => (Square::H1, Square::F1),
                (Color::White, false) => (Square::A1, Square::D1),
                (Color::Black, true) => (Square::H8, Square::F8),
                (Color::Black, false) => (Square::A8, Square::D8),
            };
            let rook_mask = BitBoard::on(rook_from) | BitBoard::on(rook_to);
            self.move_piece(
                moving_color,
                Piece::Rook,
                rook_to,
                rook_from,
                rook_mask,
                &mut key,
            );
        }

        self.occupancies[2] =
            self.occupancies[moving_color as usize] | self.occupancies[enemy_color as usize];

        // Restore game state
        // XOR in previous castling & en passant
        let castling_index_prev = (undo_info.castling_rights_before & 0b1111) as usize;
        key ^= ZOBRIST_HASHER.castling_rights[castling_index_prev];

        self.en_passant = undo_info.en_passant_before;
        if let Some(ep_sq) = self.en_passant {
            let file = ep_sq.coordinate().0 as usize;
            key ^= ZOBRIST_HASHER.en_passant[file];
        }

        self.castling_rights = undo_info.castling_rights_before;
        self.halfmove_clock = undo_info.halfmove_clock_before;
        self.fullmove_number = undo_info.fullmove_number_before;

        // store updated hash
        self.zobrist_hash = key;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLIP_CASES: [&str; 5] = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "rnbqkb1r/1p2pppp/p2p1n2/8/3NP3/2N5/PPP2PPP/R1BQKB1R w KQkq - 0 6",
        "rnbqkbnr/pppp1ppp/8/4p3/8/8/PPPPPPPP/RNBQKBNR w KQkq e6 0 2",
        "4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    ];

    #[test]
    fn test_flip_twice_is_the_original_position() {
        for fen in FLIP_CASES {
            let state = GameState::from_fen(fen).unwrap();
            let there_and_back = state.flipped().unwrap().flipped().unwrap();
            assert_eq!(there_and_back.to_fen(), fen, "flipping twice changed {fen}");
        }
    }

    #[test]
    fn test_flip_swaps_side_to_move() {
        for fen in FLIP_CASES {
            let state = GameState::from_fen(fen).unwrap();
            assert_eq!(
                state.flipped().unwrap().side_to_move,
                state.side_to_move.opposite(),
                "fen: {fen}",
            );
        }
    }

    #[test]
    fn test_flip_mirrors_castling_rights() {
        let state = GameState::from_fen("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        assert_eq!(
            state.flipped().unwrap().to_fen().split(' ').nth(2),
            Some("kq")
        );
    }

    #[test]
    fn test_flip_reflects_en_passant_square() {
        let state =
            GameState::from_fen("rnbqkbnr/pppp1ppp/8/4p3/8/8/PPPPPPPP/RNBQKBNR w KQkq e6 0 2")
                .unwrap();
        assert_eq!(
            state.flipped().unwrap().to_fen().split(' ').nth(3),
            Some("e3")
        );
    }

    #[test]
    fn test_evaluation_is_colour_symmetric() {
        for fen in FLIP_CASES {
            let state = GameState::from_fen(fen).unwrap();
            let flipped = state.flipped().unwrap();
            assert_eq!(
                crate::nnue::evaluate(&state),
                crate::nnue::evaluate(&flipped),
                "evaluation is not symmetric for {fen}",
            );
        }
    }

    fn assert_insufficient(fen: &str, expected: bool) {
        let state = GameState::from_fen(fen).unwrap_or_else(|_| panic!("bad fen: {fen}"));
        assert_eq!(
            state.is_draw_by_insufficient_material(),
            expected,
            "fen: {fen}"
        );
    }

    #[test]
    fn test_is_draw_by_insufficient_material_draws() {
        let draws = [
            // K vs K
            "8/8/8/8/8/8/8/K6k w - - 0 1",
            // K+B vs K (lone minor)
            "8/8/8/8/8/8/8/KB5k w - - 0 1",
            // K+N vs K (lone minor)
            "8/8/8/8/8/8/8/KN5k w - - 0 1",
            // K+B vs K+B with bishops on the same color complex (C1 + F8, both dark)
            "5b1k/8/8/8/8/8/8/K1B5 w - - 0 1",
            // K+B+B vs K with both bishops light (B1, D1)
            "7k/8/8/8/8/8/8/KB1B4 w - - 0 1",
            // Three bishops across both sides, all on the light complex
            "6bk/8/8/8/8/8/8/KB1B4 w - - 0 1",
        ];
        for fen in draws {
            assert_insufficient(fen, true);
        }
    }

    #[test]
    fn test_is_draw_by_insufficient_material_not_draws() {
        let not_draws = [
            // Starting position
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            // Pawn / rook / queen present
            "8/8/8/8/8/8/P7/K6k w - - 0 1",
            "8/8/8/8/8/8/8/KR5k b - - 0 1",
            "8/8/8/8/8/8/8/KQ5k b - - 0 1",
            // K+B vs K+B with bishops on opposite color complexes (B1 light, C1 dark)
            "7k/8/8/8/8/8/8/KBb5 w - - 0 1",
            // K+N vs K+N, two knights, not a draw
            "7k/8/6n1/8/8/8/8/KN6 w - - 0 1",
            // K+N+N vs K, mate is possible (not forced), not a draw here
            "7k/8/8/8/8/8/8/KNN5 w - - 0 1",
            // K+B+N vs K, forced win
            "7k/8/8/8/8/8/8/KBN5 w - - 0 1",
            // K+B+B vs K with bishops on opposite colors (A1 dark, B1 light)
            "7k/8/8/8/8/8/8/BB5K b - - 0 1",
        ];
        for fen in not_draws {
            assert_insufficient(fen, false);
        }
    }
}
