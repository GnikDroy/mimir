//! Pseudo-legal move generation.
//!
//! [`GameState::generate_moves_pseudo_legal`] emits every move that respects piece
//! geometry, captures, and friendly blockers, but does *not* filter
//! moves that leave the king in check.
use crate::attack_table::ATTACK_TABLE;
use crate::bitboard::{BitBoard, BitBoardMethods};
use crate::core::*;
use crate::state::GameState;

impl GameState {
    /// Counts legal leaves of the move tree at the given `depth`. Used as
    /// the canonical correctness check for move generation and
    /// make/unmake symmetry, and matches the UCI `go perft` semantics.
    pub fn perft_pseudo_legal(&mut self, depth: u8) -> u64 {
        if depth == 0 {
            return 1;
        }

        let mut current_moves = MoveList::default();
        Self::generate_moves_pseudo_legal(self, &mut current_moves);

        let mut count = 0u64;

        for &mv in current_moves.iter() {
            let undo_info = self.make_move(mv);

            // Only count legal moves
            if !self.is_in_check(self.side_to_move.opposite()) {
                count += self.perft_pseudo_legal(depth - 1);
            }

            self.unmake_move(mv, &undo_info);
        }

        count
    }

    pub fn perft_divide_pseudo_legal(&mut self, depth: u8) -> Vec<(Move, u64)> {
        let mut results = Vec::new();

        if depth == 0 {
            return results;
        }

        let mut moves = MoveList::default();
        self.generate_moves_pseudo_legal(&mut moves);

        for move_encoded in moves {
            let undo_info = self.make_move(move_encoded);

            let count = if !self.is_in_check(self.side_to_move.opposite()) {
                self.perft_pseudo_legal(depth - 1)
            } else {
                0
            };

            self.unmake_move(move_encoded, &undo_info);
            results.push((move_encoded, count));
        }

        results
    }

    /// Pseudo-legal moves for the side to move: every move that obeys
    /// piece geometry, captures, and friendly blockers. Moves that leave
    /// the king in check are **not** filtered out — callers must do that
    /// themselves, or use [`Self::generate_moves`].
    pub fn generate_moves_pseudo_legal(&self, moves: &mut MoveList) {
        let friendly = self.occupancies[self.side_to_move as usize];
        let occupancy = self.occupancies[2];

        // Generate moves for each piece type
        for piece in Piece::all() {
            let pieces = self.pieces[self.side_to_move as usize][piece as usize];
            let enemy = &self.pieces[self.side_to_move.opposite() as usize];
            for from in pieces.iter() {
                match piece {
                    Piece::Pawn => Self::add_pawn_moves(self, from, moves),
                    Piece::King => {
                        let attacks = ATTACK_TABLE.get_king(from);
                        Self::add_attack_moves(
                            from,
                            Piece::King,
                            attacks & !friendly,
                            enemy,
                            moves,
                        );
                    }
                    Piece::Knight => {
                        let attacks = ATTACK_TABLE.get_knight(from);
                        Self::add_attack_moves(
                            from,
                            Piece::Knight,
                            attacks & !friendly,
                            enemy,
                            moves,
                        );
                    }
                    Piece::Bishop => {
                        let attacks = ATTACK_TABLE.get_bishop(from, occupancy);
                        Self::add_attack_moves(
                            from,
                            Piece::Bishop,
                            attacks & !friendly,
                            enemy,
                            moves,
                        );
                    }
                    Piece::Rook => {
                        let attacks = ATTACK_TABLE.get_rook(from, occupancy);
                        Self::add_attack_moves(
                            from,
                            Piece::Rook,
                            attacks & !friendly,
                            enemy,
                            moves,
                        );
                    }
                    Piece::Queen => {
                        let attacks = ATTACK_TABLE.get_queen(from, occupancy);
                        Self::add_attack_moves(
                            from,
                            Piece::Queen,
                            attacks & !friendly,
                            enemy,
                            moves,
                        );
                    }
                }

                // castling moves (only for king)
                if piece == Piece::King {
                    self.add_castling_moves(from, moves);
                }
            }
        }
    }

    /// Emit kingside and queenside castles when rights, paths, and
    /// square-attack checks all permit them.
    fn add_castling_moves(&self, from: Square, moves: &mut MoveList) {
        let color = self.side_to_move;
        let enemy = color.opposite();

        // King must be on starting square
        if (color == Color::White && from != Square::E1)
            || (color == Color::Black && from != Square::E8)
        {
            return;
        }

        // King cannot be in check
        if self.is_square_attacked(from, enemy) {
            return;
        }

        // Kingside
        if self.can_castle(color, true, enemy) {
            let to = match color {
                Color::White => Square::G1,
                Color::Black => Square::G8,
            };
            moves.push(Move::from_kingside_castle(from, to));
        }

        // Queenside
        if self.can_castle(color, false, enemy) {
            let to = match color {
                Color::White => Square::C1,
                Color::Black => Square::C8,
            };
            moves.push(Move::from_queenside_castle(from, to));
        }
    }

    /// Castling availability check: rights set, path empty, friendly rook
    /// on its starting square, and the king's transit squares unattacked.
    fn can_castle(&self, color: Color, kingside: bool, enemy: Color) -> bool {
        let rights = if kingside {
            match color {
                Color::White => (self.castling_rights & 0b0001) != 0,
                Color::Black => (self.castling_rights & 0b0100) != 0,
            }
        } else {
            match color {
                Color::White => (self.castling_rights & 0b0010) != 0,
                Color::Black => (self.castling_rights & 0b1000) != 0,
            }
        };

        if !rights {
            return false;
        }

        let (rook_square, empty_squares, path_squares) = match (color, kingside) {
            (Color::White, true) => (
                Square::H1,
                BitBoard::on(Square::F1) | BitBoard::on(Square::G1),
                [Square::F1, Square::G1],
            ),
            (Color::White, false) => (
                Square::A1,
                BitBoard::on(Square::B1) | BitBoard::on(Square::C1) | BitBoard::on(Square::D1),
                [Square::D1, Square::C1],
            ),
            (Color::Black, true) => (
                Square::H8,
                BitBoard::on(Square::F8) | BitBoard::on(Square::G8),
                [Square::F8, Square::G8],
            ),
            (Color::Black, false) => (
                Square::A8,
                BitBoard::on(Square::B8) | BitBoard::on(Square::C8) | BitBoard::on(Square::D8),
                [Square::D8, Square::C8],
            ),
        };

        // Empty squares between king and rook
        if self.occupancies[2] & empty_squares != 0 {
            return false;
        }

        // Rook must exist
        let rook_bb = self.pieces[color as usize][Piece::Rook as usize];
        if rook_bb & BitBoard::on(rook_square) == 0 {
            return false;
        }

        for sq in path_squares {
            if self.is_square_attacked(sq, enemy) {
                return false;
            }
        }

        true
    }

    /// Emit pseudo-legal pawn moves from `from`: single/double push,
    /// captures, en passant, and promotions on the back rank.
    #[inline(always)]
    fn add_pawn_moves(&self, from: Square, moves: &mut MoveList) {
        Self::add_pawn_moves_for_color(self, from, self.side_to_move, moves);
    }

    /// Color-parameterised pawn emission. The push direction and promotion
    /// rank fall out of `color` so the same body handles white and black.
    #[inline(always)]
    fn add_pawn_moves_for_color(&self, from: Square, color: Color, moves: &mut MoveList) {
        let from_board = BitBoard::on(from);
        let occupancy = self.occupancies[2];

        // Single push
        let single_push = match color {
            Color::White => from_board.shift_north() & !occupancy,
            Color::Black => from_board.shift_south() & !occupancy,
        };

        for to in single_push.iter() {
            Self::add_pawn_move(from, to, color, None, moves);
        }

        // Double push if on starting rank
        if from.is_pawn_start_square(color) {
            let double_push = match color {
                Color::White => single_push.shift_north() & !occupancy,
                Color::Black => single_push.shift_south() & !occupancy,
            };
            for to in double_push.iter() {
                moves.push(Move::from_double_pawn_push(from, to));
            }
        }
        // Captures
        let enemy = &self.pieces[self.side_to_move.opposite() as usize];
        let enemy_occ = self.occupancies[self.side_to_move.opposite() as usize];
        let attacks = ATTACK_TABLE.get_pawn(from, color);
        let captures = attacks & enemy_occ;

        for to in captures.iter() {
            let captured = Self::get_captured_piece(enemy, to);
            Self::add_pawn_move(from, to, color, captured, moves);
        }
        // En passant
        if let Some(ep_sq) = self.en_passant {
            if (attacks & BitBoard::on(ep_sq)) != 0 {
                moves.push(Move::from_capture(
                    from,
                    ep_sq,
                    Piece::Pawn,
                    Piece::Pawn,
                    true,
                ));
            }
        }
    }

    /// Push one pawn move into `moves`, expanding to four promotions when
    /// `to` is a promotion square and otherwise emitting a single quiet
    /// move or capture.
    #[inline(always)]
    fn add_pawn_move(
        from: Square,
        to: Square,
        color: Color,
        captured: Option<Piece>,
        moves: &mut MoveList,
    ) {
        if to.is_promotion_square(color) {
            for promo in [
                PromotionPiece::Queen,
                PromotionPiece::Rook,
                PromotionPiece::Bishop,
                PromotionPiece::Knight,
            ] {
                moves.push(Move::from_promotion(from, to, Piece::Pawn, promo, captured));
            }
        } else if let Some(c) = captured {
            moves.push(Move::from_capture(from, to, Piece::Pawn, c, false));
        } else {
            moves.push(Move::from_quiet(from, to, Piece::Pawn));
        }
    }

    /// Resolve which enemy piece occupies `to`, or `None` if the square
    /// is empty.
    #[inline(always)]
    fn get_captured_piece(enemy: &[BitBoard; 6], to: Square) -> Option<Piece> {
        let bb = BitBoard::on(to);

        if enemy[Piece::Pawn as usize] & bb != 0 {
            return Some(Piece::Pawn);
        }
        if enemy[Piece::Knight as usize] & bb != 0 {
            return Some(Piece::Knight);
        }
        if enemy[Piece::Bishop as usize] & bb != 0 {
            return Some(Piece::Bishop);
        }
        if enemy[Piece::Rook as usize] & bb != 0 {
            return Some(Piece::Rook);
        }
        if enemy[Piece::Queen as usize] & bb != 0 {
            return Some(Piece::Queen);
        }
        None
    }

    /// Split a non-pawn piece's `targets` bitboard into captures and
    /// quiets and push the corresponding [`Move`]s.
    #[inline(always)]
    fn add_attack_moves(
        from: Square,
        from_piece: Piece,
        targets: BitBoard,
        enemy: &[BitBoard; 6],
        moves: &mut MoveList,
    ) {
        let captures = targets & (enemy[0] | enemy[1] | enemy[2] | enemy[3] | enemy[4] | enemy[5]);

        let quiets = targets ^ captures;

        for to in captures.iter() {
            let captured = Self::get_captured_piece(enemy, to);
            moves.push(Move::from_capture(
                from,
                to,
                from_piece,
                captured.unwrap(),
                false,
            ));
        }

        for to in quiets.iter() {
            moves.push(Move::from_quiet(from, to, from_piece));
        }
    }
}

#[cfg(test)]
mod tests {
    use once_cell::sync::Lazy;

    use crate::attack_table::ATTACK_TABLE;
    use crate::bitboard::*;
    use crate::core::*;
    use crate::state::GameState;
    use std::time::Instant;

    fn set_piece(state: &mut GameState, color: Color, piece: Piece, square: Square) {
        let board = BitBoard::on(square);
        state.pieces[color as usize][piece as usize] |= board;
        state.occupancies[color as usize] |= board;
        state.occupancies[2] |= board;
    }

    fn assert_perft_case(fen: &str, expected_perft: &[u64]) {
        Lazy::force(&ATTACK_TABLE);
        let mut state = GameState::from_fen(fen).unwrap();

        for (depth, perft_count) in expected_perft.iter().enumerate() {
            let start_time = Instant::now();
            let count = state.perft_pseudo_legal(depth as u8 + 1);
            let elapsed = start_time.elapsed();
            println!(
                "Depth {}: {} nodes (calculated in {:.2?} at {:.2}M nodes/sec)",
                depth + 1,
                count,
                elapsed,
                count as f64 / 1_000_000 as f64 / elapsed.as_secs_f64()
            );
            assert_eq!(
                count,
                *perft_count,
                "Perft count mismatch at depth {}: expected {}, got {}",
                depth + 1,
                perft_count,
                count
            );
        }
    }

    fn assert_state_eq(left: &GameState, right: &GameState) {
        assert_eq!(left.pieces, right.pieces, "piece boards differ");
        assert_eq!(left.occupancies, right.occupancies, "occupancies differ");
        assert_eq!(
            left.side_to_move, right.side_to_move,
            "side to move differs"
        );
        assert_eq!(
            left.castling_rights, right.castling_rights,
            "castling rights differ"
        );
        assert_eq!(left.en_passant, right.en_passant, "en passant differs");
        assert_eq!(
            left.halfmove_clock, right.halfmove_clock,
            "halfmove clock differs"
        );
        assert_eq!(
            left.fullmove_number, right.fullmove_number,
            "fullmove number differs"
        );
    }

    #[test]
    fn test_starting_position_generates_twenty_white_moves() {
        let state = GameState::new();
        let mut moves = MoveList::default();
        state.generate_moves_pseudo_legal(&mut moves);

        assert_eq!(moves.len(), 20);
    }

    #[test]
    fn test_pawn_promotions_generate_all_promotion_moves() {
        let mut state = GameState::empty();
        state.side_to_move = Color::White;
        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::Black, Piece::King, Square::C8);
        set_piece(&mut state, Color::White, Piece::Pawn, Square::E7);

        let mut moves = MoveList::default();
        state.generate_moves_pseudo_legal(&mut moves);

        for promotion in PromotionPiece::all() {
            assert!(moves.contains(&Move::from_promotion(
                Square::E7,
                Square::E8,
                Piece::Pawn,
                promotion,
                None,
            )));
        }
    }

    #[test]
    fn test_en_passant_move_is_generated() {
        let mut state = GameState::empty();
        state.side_to_move = Color::White;
        state.en_passant = Some(Square::D6);

        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::Black, Piece::King, Square::E8);
        set_piece(&mut state, Color::White, Piece::Pawn, Square::E5);
        set_piece(&mut state, Color::Black, Piece::Pawn, Square::D5);

        let mut moves = MoveList::default();
        state.generate_moves_pseudo_legal(&mut moves);

        assert!(moves.contains(&Move::from_capture(
            Square::E5,
            Square::D6,
            Piece::Pawn,
            Piece::Pawn,
            true
        )));
    }

    #[test]
    fn test_castling_is_generated_when_path_is_clear_and_not_attacked() {
        let mut state = GameState::empty();
        state.side_to_move = Color::White;
        state.castling_rights = 0b0001;

        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::White, Piece::Rook, Square::H1);
        set_piece(&mut state, Color::Black, Piece::King, Square::E8);

        let mut moves = MoveList::default();
        state.generate_moves_pseudo_legal(&mut moves);
        assert!(moves.contains(&Move::from_kingside_castle(Square::E1, Square::G1,)));
    }

    #[test]
    fn test_castling_is_blocked_when_path_is_attacked() {
        let mut state = GameState::empty();
        state.side_to_move = Color::White;
        state.castling_rights = 0b0001;

        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::White, Piece::Rook, Square::H1);
        set_piece(&mut state, Color::Black, Piece::King, Square::E8);
        set_piece(&mut state, Color::Black, Piece::Rook, Square::F8);

        let mut moves = MoveList::default();
        state.generate_moves_pseudo_legal(&mut moves);

        assert!(!moves.contains(&Move::from_kingside_castle(Square::E1, Square::G1,)));
    }

    #[test]
    fn test_en_passant_make_and_unmake_restores_state() {
        let mut state = GameState::empty();
        state.side_to_move = Color::White;
        state.en_passant = Some(Square::D6);

        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::Black, Piece::King, Square::E8);
        set_piece(&mut state, Color::White, Piece::Pawn, Square::E5);
        set_piece(&mut state, Color::Black, Piece::Pawn, Square::D5);

        let before = state;
        let mv = Move::from_capture(Square::E5, Square::D6, Piece::Pawn, Piece::Pawn, true);
        let undo = state.make_move(mv);
        state.unmake_move(mv, &undo);

        assert_state_eq(&state, &before);
    }

    #[test]
    fn test_castling_make_and_unmake_restores_state() {
        let mut state = GameState::empty();
        state.side_to_move = Color::White;
        state.castling_rights = 0b0001;

        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::White, Piece::Rook, Square::H1);
        set_piece(&mut state, Color::Black, Piece::King, Square::E8);

        let before = state;
        let mv = Move::from_kingside_castle(Square::E1, Square::G1);
        let undo = state.make_move(mv);
        state.unmake_move(mv, &undo);

        assert_state_eq(&state, &before);
    }

    #[test]
    fn test_promotion_make_and_unmake_restores_state() {
        let mut state = GameState::empty();
        state.side_to_move = Color::White;

        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::Black, Piece::King, Square::E8);
        set_piece(&mut state, Color::White, Piece::Pawn, Square::E7);

        let before = state;
        let mv = Move::from_promotion(
            Square::E7,
            Square::E8,
            Piece::Pawn,
            PromotionPiece::Queen,
            None,
        );
        let undo = state.make_move(mv);
        state.unmake_move(mv, &undo);

        assert_state_eq(&state, &before);
    }

    #[test]
    fn test_starting_position_perft_matches_known_values() {
        assert_perft_case(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            &[20, 400, 8902, 197281, 4865609],
        );
    }

    #[test]
    fn test_kiwipete_perft_matches_known_values() {
        assert_perft_case(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            &[48, 2039, 97862, 4085603],
        );
    }

    #[test]
    fn test_endgame_perft_matches_known_values() {
        assert_perft_case(
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            &[14, 191, 2812, 43238, 674624, 11030083],
        );
    }

    #[test]
    fn test_perft_divide_output_matches_depth_two_total() {
        let mut state =
            GameState::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
                .unwrap();
        let divide = state.perft_divide_pseudo_legal(2);

        assert_eq!(divide.len(), 20);
        assert_eq!(divide.iter().map(|(_, count)| count).sum::<u64>(), 400);
    }
}
