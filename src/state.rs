//! Mutable chess position and its move-application API.
//!
//! [`GameState`] is the engine's central type: piece bitboards, derived
//! occupancies, side to move, castling rights, en-passant target,
//! 50-move and full-move clocks, plus a running [`ZobristHash`].
//!
//! Moves are applied with [`GameState::make_move`], which returns an
//! [`UndoInfo`] snapshot of the fields that can't be recovered from the
//! move alone. Passing the same `(move, undo_info)` pair to
//! [`GameState::unmake_move`] restores the position byte-for-byte and
//! reverses every Zobrist XOR — search and perft rely on this exact
//! symmetry.
//!
//! `make_move` does not validate legality.
use crate::attack_table::ATTACK_TABLE;
use crate::bitboard::*;
use crate::core::*;
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
    /// Square *behind* a pawn that just made a double push — i.e. the
    /// square an enemy pawn would capture *to* via en passant. [`None`]
    /// when no en-passant capture is currently legal.
    pub en_passant: Option<Square>,
    /// Halfmoves since the last pawn move or capture (50-move rule).
    pub halfmove_clock: u8,
    /// 1-based fullmove counter; increments after every black move.
    pub fullmove_number: u16,
    /// Incrementally maintained Zobrist key for this position.
    pub zobrist_hash: ZobristHash,
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
    /// Returns `true` if any piece of color `attacker` attacks `sq` in
    /// the current position. Used for both check detection and castling
    /// safety. Does not require `sq` to be occupied or empty.
    #[inline(always)]
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

    /// Applies `move_encoded` to the position and returns an
    /// [`UndoInfo`] that [`unmake_move`](Self::unmake_move) needs to
    /// reverse it.
    ///
    /// Updates bitboards, occupancies, castling rights, en-passant,
    /// the 50-move and fullmove clocks, side to move, and the Zobrist
    /// hash incrementally. Handles promotions, normal captures, en
    /// passant, both castling sides, and rook-capture / king-move
    /// effects on castling rights.
    ///
    /// **Does not validate legality.** The caller is responsible for
    /// passing a legal/pseudo-legal move and (where required) filtering out
    /// moves that leave the moving side in check.
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

        // Remove piece from source
        self.pieces[moving_color as usize][moving_piece as usize] &= !from_board;
        self.occupancies[moving_color as usize] &= !from_board;

        // XOR out moving piece from source square
        key ^= ZOBRIST_HASHER.square[moving_color as usize][moving_piece as usize][from as usize];

        // Place piece at destination
        if move_encoded.is_promotion() {
            let piece = move_encoded.get_promotion_piece().unwrap().to_piece();
            self.pieces[moving_color as usize][piece as usize] |= to_board;
            self.occupancies[moving_color as usize] |= to_board;

            // promotion: add promoted piece at destination
            key ^= ZOBRIST_HASHER.square[moving_color as usize][piece as usize][to as usize];
        } else {
            self.pieces[moving_color as usize][moving_piece as usize] |= to_board;
            self.occupancies[moving_color as usize] |= to_board;

            // add moved piece at destination
            key ^= ZOBRIST_HASHER.square[moving_color as usize][moving_piece as usize][to as usize];
        }

        let captured_piece = move_encoded.get_captured_piece();

        // Handle normal captures (en passant is handled separately)
        if let Some(captured) = captured_piece {
            if !move_encoded.is_enpassant() {
                self.pieces[enemy_color as usize][captured as usize] &= !to_board;
                self.occupancies[enemy_color as usize] &= !to_board;

                // XOR out captured piece on destination
                key ^= ZOBRIST_HASHER.square[enemy_color as usize][captured as usize][to as usize];
            }
        }

        // Handle en passant captures
        if move_encoded.is_enpassant() {
            let capture_square = match moving_color {
                Color::White => Square::index(to as u8 - 8),
                Color::Black => Square::index(to as u8 + 8),
            };
            let capture_board = BitBoard::on(capture_square);
            self.pieces[enemy_color as usize][Piece::Pawn as usize] &= !capture_board;
            self.occupancies[enemy_color as usize] &= !capture_board;

            // XOR out the captured pawn for en passant
            key ^= ZOBRIST_HASHER.square[enemy_color as usize][Piece::Pawn as usize]
                [capture_square as usize];
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
            let rook_from_board = BitBoard::on(rook_from);
            let rook_to_board = BitBoard::on(rook_to);
            self.pieces[moving_color as usize][Piece::Rook as usize] &= !rook_from_board;
            self.pieces[moving_color as usize][Piece::Rook as usize] |= rook_to_board;
            self.occupancies[moving_color as usize] &= !rook_from_board;
            self.occupancies[moving_color as usize] |= rook_to_board;

            // XOR rook move for castling
            key ^= ZOBRIST_HASHER.square[moving_color as usize][Piece::Rook as usize]
                [rook_from as usize];
            key ^= ZOBRIST_HASHER.square[moving_color as usize][Piece::Rook as usize]
                [rook_to as usize];
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

    /// Reverses [`make_move`](Self::make_move) using the [`UndoInfo`]
    /// it returned. Restores every field including the Zobrist hash to
    /// byte-equality with the pre-move state.
    ///
    /// `move_encoded` and `undo_info` must be the exact pair produced
    /// by the matching `make_move`; using them out of order or with a
    /// different move corrupts the position.
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

        // Remove piece from destination and restore pawn at source for promotions
        // Or for regular moves, just find and move the piece back
        if move_encoded.is_promotion() {
            let piece = move_encoded.get_promotion_piece().unwrap().to_piece();
            self.pieces[moving_color as usize][piece as usize] &= !to_board;
            self.pieces[moving_color as usize][Piece::Pawn as usize] |= from_board;
            self.occupancies[moving_color as usize] &= !to_board;
            self.occupancies[moving_color as usize] |= from_board;

            // XOR out promoted piece at destination, xor in pawn at source
            key ^= ZOBRIST_HASHER.square[moving_color as usize][piece as usize][to as usize];
            key ^=
                ZOBRIST_HASHER.square[moving_color as usize][Piece::Pawn as usize][from as usize];
        } else {
            let moving_piece = undo_info.moved_piece;
            self.pieces[moving_color as usize][moving_piece as usize] &= !to_board;
            self.pieces[moving_color as usize][moving_piece as usize] |= from_board;
            self.occupancies[moving_color as usize] &= !to_board;
            self.occupancies[moving_color as usize] |= from_board;

            // XOR out moved piece at destination, xor in at source
            key ^= ZOBRIST_HASHER.square[moving_color as usize][moving_piece as usize][to as usize];
            key ^=
                ZOBRIST_HASHER.square[moving_color as usize][moving_piece as usize][from as usize];
        }

        // Handle en passant & capture piece restore
        if move_encoded.is_enpassant() {
            let capture_square = match moving_color {
                Color::White => Square::index(to as u8 - File::NUM as u8),
                Color::Black => Square::index(to as u8 + File::NUM as u8),
            };
            let capture_board = BitBoard::on(capture_square);
            self.pieces[enemy_color as usize][Piece::Pawn as usize] |= capture_board;
            self.occupancies[enemy_color as usize] |= capture_board;

            // XOR in the restored pawn for en passant
            key ^= ZOBRIST_HASHER.square[enemy_color as usize][Piece::Pawn as usize]
                [capture_square as usize];
        } else if move_encoded.is_capture() {
            let captured = undo_info.captured_piece.unwrap();
            self.pieces[enemy_color as usize][captured as usize] |= to_board;
            self.occupancies[enemy_color as usize] |= to_board;

            // XOR in the restored captured piece
            key ^= ZOBRIST_HASHER.square[enemy_color as usize][captured as usize][to as usize];
        }

        // Handle castling unmake
        if move_encoded.is_castle() {
            let (rook_from, rook_to) = match (moving_color, move_encoded.is_kingside_castle()) {
                (Color::White, true) => (Square::H1, Square::F1),
                (Color::White, false) => (Square::A1, Square::D1),
                (Color::Black, true) => (Square::H8, Square::F8),
                (Color::Black, false) => (Square::A8, Square::D8),
            };
            let rook_from_board = BitBoard::on(rook_from);
            let rook_to_board = BitBoard::on(rook_to);
            self.pieces[moving_color as usize][Piece::Rook as usize] &= !rook_to_board;
            self.pieces[moving_color as usize][Piece::Rook as usize] |= rook_from_board;
            self.occupancies[moving_color as usize] &= !rook_to_board;
            self.occupancies[moving_color as usize] |= rook_from_board;

            // XOR rook move reversal: remove rook at to, add at from
            key ^= ZOBRIST_HASHER.square[moving_color as usize][Piece::Rook as usize]
                [rook_to as usize];
            key ^= ZOBRIST_HASHER.square[moving_color as usize][Piece::Rook as usize]
                [rook_from as usize];
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
