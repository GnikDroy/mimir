//! Single-pass strict-legal move generation.
//!
//! Builds a per-position [`LegalCtx`] (pins, king-danger, check-mask) and
//! AND-s each piece's pseudo-legal targets against it. The only fallback
//! to make/unmake is en passant, to catch the horizontal-pin edge case.

use crate::attack_table::ATTACK_TABLE;
use crate::bitboard::{BitBoard, BitBoardMethods};
use crate::core::*;
use crate::state::GameState;

/// Per-position legality context used to filter pseudo-legal moves in one pass.
struct LegalCtx {
    king_sq: Square,
    num_checkers: u32,
    /// Friendly pieces pinned to their `get_line(king_sq, from)` ray.
    pinned: BitBoard,
    /// Enemy attacks with the king removed from occupancy — squares the
    /// king may not move to.
    king_danger: BitBoard,
    /// Squares non-king pieces may land on: full, block-or-capture, or empty.
    check_mask: BitBoard,
}

impl LegalCtx {
    /// Compute the legality context for `state`'s side to move: king
    /// square, checker count, pin bitboard, king-danger squares, and
    /// the destination mask non-king pieces must respect.
    fn new(state: &GameState) -> LegalCtx {
        let side = state.side_to_move;
        let bitboards = &state.pieces[side as usize];
        let enemy_bbs = &state.pieces[side.opposite() as usize];

        let king_sq = {
            let mut king_bb = bitboards[Piece::King as usize];
            king_bb.pop_lsb().unwrap()
        };

        let all_occ =
            state.occupancies[Color::White as usize] | state.occupancies[Color::Black as usize];

        let enemy_pawns = enemy_bbs[Piece::Pawn as usize];
        let enemy_knights = enemy_bbs[Piece::Knight as usize];
        let enemy_bishops = enemy_bbs[Piece::Bishop as usize];
        let enemy_rooks = enemy_bbs[Piece::Rook as usize];
        let enemy_queens = enemy_bbs[Piece::Queen as usize];
        let enemy_king_sq = {
            let mut king_bb = enemy_bbs[Piece::King as usize];
            king_bb.pop_lsb().unwrap()
        };

        // king_danger uses occ_no_king so sliders see through the king —
        // otherwise the square behind a check ray looks safe.
        let occ_no_king = all_occ ^ bitboards[Piece::King as usize];

        let mut king_danger = ATTACK_TABLE.get_king(enemy_king_sq);
        for pawn in enemy_pawns.iter() {
            king_danger |= ATTACK_TABLE.get_pawn(pawn, side.opposite());
        }
        for knight in enemy_knights.iter() {
            king_danger |= ATTACK_TABLE.get_knight(knight);
        }
        for bishop in (enemy_bishops | enemy_queens).iter() {
            king_danger |= ATTACK_TABLE.get_bishop(bishop, occ_no_king);
        }
        for rook in (enemy_rooks | enemy_queens).iter() {
            king_danger |= ATTACK_TABLE.get_rook(rook, occ_no_king);
        }

        let checkers = (ATTACK_TABLE.get_pawn(king_sq, side) & enemy_pawns)
            | (ATTACK_TABLE.get_knight(king_sq) & enemy_knights)
            | (ATTACK_TABLE.get_bishop(king_sq, all_occ) & (enemy_bishops | enemy_queens))
            | (ATTACK_TABLE.get_rook(king_sq, all_occ) & (enemy_rooks | enemy_queens));

        let num_checkers = checkers.count_ones();

        // Sniper trick: slider attacks from the king using `enemy_occ` as
        // blockers stop at the first enemy, so intersecting with enemy
        // sliders yields exactly the sliders aligned with the king through
        // friendly-only blockers. A unique friendly between king and sniper
        // is pinned along that ray.
        let pinned = if num_checkers >= 2 {
            BitBoard::EMPTY
        } else {
            let friendly_occ = state.occupancies[side as usize];
            let enemy_occ = state.occupancies[side.opposite() as usize];

            let bishop_snipers =
                ATTACK_TABLE.get_bishop(king_sq, enemy_occ) & (enemy_bishops | enemy_queens);
            let rook_snipers =
                ATTACK_TABLE.get_rook(king_sq, enemy_occ) & (enemy_rooks | enemy_queens);
            let snipers = bishop_snipers | rook_snipers;

            let mut pinned = BitBoard::EMPTY;
            for sniper_sq in snipers.iter() {
                let between = ATTACK_TABLE.get_between(king_sq, sniper_sq) & friendly_occ;
                if between.count_ones() == 1 {
                    pinned |= between;
                }
            }
            pinned
        };

        // check_mask: block-or-capture for a single check. `get_between` is
        // 0 for knight/pawn checkers, so the mask collapses to just the
        // checker square — capture-only, no blocking.
        let check_mask = match num_checkers {
            0 => BitBoard::FULL,
            1 => {
                let checker_sq = {
                    let mut c = checkers;
                    c.pop_lsb().unwrap()
                };
                ATTACK_TABLE.get_between(king_sq, checker_sq) | checkers
            }
            _ => BitBoard::EMPTY,
        };

        LegalCtx {
            king_sq,
            num_checkers,
            pinned,
            king_danger,
            check_mask,
        }
    }
}

impl GameState {
    /// Counts legal leaves of the move tree at the given `depth`. Used as
    /// the canonical correctness check for move generation and
    /// make/unmake symmetry, and matches the UCI `go perft` semantics.
    pub fn perft(&mut self, depth: u8) -> u64 {
        if depth == 0 {
            return 1;
        }
        let mut moves = MoveList::default();
        self.generate_moves(&mut moves);
        if depth == 1 {
            return moves.len() as u64;
        }
        let mut count = 0u64;
        for &mv in moves.iter() {
            let undo = self.make_move(mv);
            count += self.perft(depth - 1);
            self.unmake_move(mv, &undo);
        }
        count
    }

    pub fn perft_divide(&mut self, depth: u8) -> Vec<(Move, u64)> {
        let mut results = Vec::new();

        if depth == 0 {
            return results;
        }

        let mut moves = MoveList::default();
        self.generate_moves_pseudo_legal(&mut moves);

        for move_encoded in moves {
            let undo_info = self.make_move(move_encoded);

            let count = if !self.is_in_check(self.side_to_move.opposite()) {
                self.perft(depth - 1)
            } else {
                0
            };

            self.unmake_move(move_encoded, &undo_info);
            results.push((move_encoded, count));
        }

        results
    }

    /// Strict-legal move generation in a single pass. Every emitted move
    /// is legal and every legal move is emitted; in double check only
    /// king moves come back, and castling is skipped whenever the king
    /// is in check or the transit squares are attacked.
    pub fn generate_moves(&self, moves: &mut MoveList) {
        let ctx = LegalCtx::new(self);
        let me = self.side_to_move;
        let friendly = self.occupancies[me as usize];
        let enemy = &self.pieces[me.opposite() as usize];

        // King moves first — they are the only legal moves under double check.
        self.add_legal_king_moves(&ctx, friendly, enemy, moves);

        if ctx.num_checkers >= 2 {
            return;
        }

        if ctx.num_checkers == 0 {
            self.add_legal_castling_moves(&ctx, moves);
        }

        self.add_legal_pawn_moves(&ctx, moves);
        self.add_legal_knight_moves(&ctx, friendly, enemy, moves);
        self.add_legal_slider_moves(&ctx, friendly, enemy, moves);
    }

    /// King moves restricted to squares not in [`LegalCtx::king_danger`].
    /// Independent of pin/check masks — the king is never pinned and
    /// resolves checks by moving away rather than landing on the mask.
    fn add_legal_king_moves(
        &self,
        ctx: &LegalCtx,
        friendly: BitBoard,
        enemy: &[BitBoard; 6],
        moves: &mut MoveList,
    ) {
        let targets = ATTACK_TABLE.get_king(ctx.king_sq) & !friendly & !ctx.king_danger;
        emit_piece_targets(ctx.king_sq, Piece::King, targets, enemy, moves);
    }

    /// Knight moves filtered by [`LegalCtx::check_mask`]. Pinned knights
    /// are dropped wholesale because no knight jump lies on a pin ray.
    fn add_legal_knight_moves(
        &self,
        ctx: &LegalCtx,
        friendly: BitBoard,
        enemy: &[BitBoard; 6],
        moves: &mut MoveList,
    ) {
        // Pinned knights can never move — their jumps never lie on a ray.
        let knights = self.pieces[self.side_to_move as usize][Piece::Knight as usize] & !ctx.pinned;
        for from in knights.iter() {
            let targets = ATTACK_TABLE.get_knight(from) & !friendly & ctx.check_mask;
            emit_piece_targets(from, Piece::Knight, targets, enemy, moves);
        }
    }

    /// Bishop, rook, and queen moves, each routed through
    /// [`Self::emit_constrained`] for pin and check filtering.
    fn add_legal_slider_moves(
        &self,
        ctx: &LegalCtx,
        friendly: BitBoard,
        enemy: &[BitBoard; 6],
        moves: &mut MoveList,
    ) {
        let me = self.side_to_move;
        let occ = self.occupancies[2];

        let bishops = self.pieces[me as usize][Piece::Bishop as usize];
        for from in bishops.iter() {
            let attacks = ATTACK_TABLE.get_bishop(from, occ);
            self.emit_constrained(from, Piece::Bishop, attacks, ctx, friendly, enemy, moves);
        }

        let rooks = self.pieces[me as usize][Piece::Rook as usize];
        for from in rooks.iter() {
            let attacks = ATTACK_TABLE.get_rook(from, occ);
            self.emit_constrained(from, Piece::Rook, attacks, ctx, friendly, enemy, moves);
        }

        let queens = self.pieces[me as usize][Piece::Queen as usize];
        for from in queens.iter() {
            let attacks = ATTACK_TABLE.get_queen(from, occ);
            self.emit_constrained(from, Piece::Queen, attacks, ctx, friendly, enemy, moves);
        }
    }

    /// Apply pin and check masks to a slider/knight attack set and emit moves.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn emit_constrained(
        &self,
        from: Square,
        piece: Piece,
        attacks: BitBoard,
        ctx: &LegalCtx,
        friendly: BitBoard,
        enemy: &[BitBoard; 6],
        moves: &mut MoveList,
    ) {
        let pin_mask = pin_mask(ctx, from);
        let targets = attacks & !friendly & ctx.check_mask & pin_mask;
        emit_piece_targets(from, piece, targets, enemy, moves);
    }

    /// Pawn moves with pin and check constraints applied to pushes,
    /// captures, promotions, and en passant. EP candidates fall through
    /// to a make/unmake simulation to catch the horizontal-pin case.
    fn add_legal_pawn_moves(&self, ctx: &LegalCtx, moves: &mut MoveList) {
        let me = self.side_to_move;
        let opp = me.opposite();
        let pawns = self.pieces[me as usize][Piece::Pawn as usize];
        let all_occ = self.occupancies[2];
        let enemy_occ = self.occupancies[opp as usize];
        let enemy = &self.pieces[opp as usize];

        for from in pawns.iter() {
            let from_bb = BitBoard::on(from);
            let pin_mask = pin_mask(ctx, from);
            let allowed = ctx.check_mask & pin_mask;

            let single = match me {
                Color::White => from_bb.shift_north() & !all_occ,
                Color::Black => from_bb.shift_south() & !all_occ,
            };
            for to in (single & allowed).iter() {
                add_pawn_move(from, to, me, None, moves);
            }

            // Double push derives from the unfiltered `single` so the
            // intermediate square's emptiness is enforced independent of
            // the check / pin filter applied at the destination.
            if from.is_pawn_start_square(me) {
                let double = match me {
                    Color::White => single.shift_north() & !all_occ,
                    Color::Black => single.shift_south() & !all_occ,
                };
                for to in (double & allowed).iter() {
                    moves.push(Move::from_double_pawn_push(from, to));
                }
            }

            let attacks = ATTACK_TABLE.get_pawn(from, me);
            for to in (attacks & enemy_occ & allowed).iter() {
                let captured = captured_piece_at(enemy, to);
                add_pawn_move(from, to, me, captured, moves);
            }

            // EP needs a simulate-and-check to catch the horizontal-pin
            // case, which `pinned` cannot see.
            if let Some(ep_sq) = self.en_passant {
                if (attacks & BitBoard::on(ep_sq)) != 0 {
                    let mv = Move::from_capture(from, ep_sq, Piece::Pawn, Piece::Pawn, true);
                    if self.ep_move_is_legal(mv) {
                        moves.push(mv);
                    }
                }
            }
        }
    }

    /// Emit kingside and queenside castles when the king is on its
    /// starting square and [`Self::can_castle_legal`] approves the path.
    fn add_legal_castling_moves(&self, ctx: &LegalCtx, moves: &mut MoveList) {
        let color = self.side_to_move;

        let start = match color {
            Color::White => Square::E1,
            Color::Black => Square::E8,
        };
        if ctx.king_sq != start {
            return;
        }

        if self.can_castle_legal(color, true, ctx) {
            let to = match color {
                Color::White => Square::G1,
                Color::Black => Square::G8,
            };
            moves.push(Move::from_kingside_castle(ctx.king_sq, to));
        }
        if self.can_castle_legal(color, false, ctx) {
            let to = match color {
                Color::White => Square::C1,
                Color::Black => Square::C8,
            };
            moves.push(Move::from_queenside_castle(ctx.king_sq, to));
        }
    }

    /// Castling availability check: rights set, path empty, the friendly
    /// rook on its starting square, and the king's transit squares free
    /// of `king_danger`. Callers must ensure the king is not in check.
    fn can_castle_legal(&self, color: Color, kingside: bool, ctx: &LegalCtx) -> bool {
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

        // empty_squares: king↔rook path must be unoccupied.
        // transit_squares: king's transit + destination, must be unattacked.
        // The start square is implicit — callers only reach here with num_checkers == 0.
        let (rook_square, empty_squares, transit_squares) = match (color, kingside) {
            (Color::White, true) => (
                Square::H1,
                BitBoard::on(Square::F1) | BitBoard::on(Square::G1),
                BitBoard::on(Square::F1) | BitBoard::on(Square::G1),
            ),
            (Color::White, false) => (
                Square::A1,
                BitBoard::on(Square::B1) | BitBoard::on(Square::C1) | BitBoard::on(Square::D1),
                BitBoard::on(Square::C1) | BitBoard::on(Square::D1),
            ),
            (Color::Black, true) => (
                Square::H8,
                BitBoard::on(Square::F8) | BitBoard::on(Square::G8),
                BitBoard::on(Square::F8) | BitBoard::on(Square::G8),
            ),
            (Color::Black, false) => (
                Square::A8,
                BitBoard::on(Square::B8) | BitBoard::on(Square::C8) | BitBoard::on(Square::D8),
                BitBoard::on(Square::C8) | BitBoard::on(Square::D8),
            ),
        };

        if (self.occupancies[2] & empty_squares) != 0 {
            return false;
        }

        let rook_bb = self.pieces[color as usize][Piece::Rook as usize];
        if (rook_bb & BitBoard::on(rook_square)) == 0 {
            return false;
        }

        if (transit_squares & ctx.king_danger) != 0 {
            return false;
        }

        true
    }

    /// Simulate an EP move on a stack copy and check the mover's king for
    /// self-check. Handles the horizontal-pin case `pinned` can't detect.
    #[inline(always)]
    fn ep_move_is_legal(&self, mv: Move) -> bool {
        let mut sim = *self;
        let _ = sim.make_move(mv);
        !sim.is_in_check(self.side_to_move)
    }
}

/// Pin-aware destination mask: the pin ray if pinned, otherwise `FULL`.
#[inline(always)]
fn pin_mask(ctx: &LegalCtx, from: Square) -> BitBoard {
    if (ctx.pinned & BitBoard::on(from)) != 0 {
        ATTACK_TABLE.get_line(ctx.king_sq, from)
    } else {
        BitBoard::FULL
    }
}

/// Emit quiet moves and captures from a pre-filtered `targets` bitboard.
#[inline(always)]
fn emit_piece_targets(
    from: Square,
    piece: Piece,
    targets: BitBoard,
    enemy: &[BitBoard; 6],
    moves: &mut MoveList,
) {
    let enemy_occ = enemy[0] | enemy[1] | enemy[2] | enemy[3] | enemy[4] | enemy[5];
    let captures = targets & enemy_occ;
    let quiets = targets ^ captures;

    for to in captures.iter() {
        let captured = captured_piece_at(enemy, to).unwrap();
        moves.push(Move::from_capture(from, to, piece, captured, false));
    }
    for to in quiets.iter() {
        moves.push(Move::from_quiet(from, to, piece));
    }
}

/// Resolve which enemy piece occupies `to`, or `None` if the square is empty.
fn captured_piece_at(enemy: &[BitBoard; 6], to: Square) -> Option<Piece> {
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

/// Push one pawn move, expanding to four promotions on the back rank
/// and otherwise emitting a single quiet or capture.
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
            let count = state.perft(depth as u8 + 1);
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

    #[test]
    fn test_starting_position_generates_twenty_white_moves() {
        let state = GameState::new();
        let mut moves = MoveList::default();
        state.generate_moves(&mut moves);
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
        state.generate_moves(&mut moves);

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
        state.generate_moves(&mut moves);

        assert!(moves.contains(&Move::from_capture(
            Square::E5,
            Square::D6,
            Piece::Pawn,
            Piece::Pawn,
            true,
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
        state.generate_moves(&mut moves);
        assert!(moves.contains(&Move::from_kingside_castle(Square::E1, Square::G1)));
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
        state.generate_moves(&mut moves);
        assert!(!moves.contains(&Move::from_kingside_castle(Square::E1, Square::G1)));
    }

    #[test]
    fn test_castling_is_skipped_when_king_in_check() {
        let mut state = GameState::empty();
        state.side_to_move = Color::White;
        state.castling_rights = 0b0001;

        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::White, Piece::Rook, Square::H1);
        set_piece(&mut state, Color::Black, Piece::King, Square::A8);
        set_piece(&mut state, Color::Black, Piece::Rook, Square::E8);

        let mut moves = MoveList::default();
        state.generate_moves(&mut moves);
        assert!(!moves.contains(&Move::from_kingside_castle(Square::E1, Square::G1)));
    }

    #[test]
    fn test_king_does_not_walk_into_check() {
        // Black rook on E8 — king can't step along the E-file, even though
        // the rook is currently blocked by the king on E1.
        let mut state = GameState::empty();
        state.side_to_move = Color::White;
        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::Black, Piece::King, Square::A8);
        set_piece(&mut state, Color::Black, Piece::Rook, Square::E8);

        let mut moves = MoveList::default();
        state.generate_moves(&mut moves);

        assert!(moves.contains(&Move::from_quiet(Square::E1, Square::D1, Piece::King)));
        assert!(moves.contains(&Move::from_quiet(Square::E1, Square::F1, Piece::King)));
        assert!(!moves.contains(&Move::from_quiet(Square::E1, Square::E2, Piece::King)));
    }

    #[test]
    fn test_pinned_knight_cannot_move() {
        let mut state = GameState::empty();
        state.side_to_move = Color::White;
        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::White, Piece::Knight, Square::E2);
        set_piece(&mut state, Color::Black, Piece::King, Square::A8);
        set_piece(&mut state, Color::Black, Piece::Rook, Square::E8);

        let mut moves = MoveList::default();
        state.generate_moves(&mut moves);

        for mv in moves.iter() {
            let m: Move = *mv;
            assert_ne!(
                m.get_moved_piece(),
                Piece::Knight,
                "pinned knight produced a move: {}",
                m.debug_string()
            );
        }
    }

    #[test]
    fn test_pinned_rook_moves_only_along_pin_ray() {
        let mut state = GameState::empty();
        state.side_to_move = Color::White;
        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::White, Piece::Rook, Square::E4);
        set_piece(&mut state, Color::Black, Piece::King, Square::A8);
        set_piece(&mut state, Color::Black, Piece::Rook, Square::E8);

        let mut moves = MoveList::default();
        state.generate_moves(&mut moves);

        assert!(moves.contains(&Move::from_quiet(Square::E4, Square::E5, Piece::Rook)));
        assert!(moves.contains(&Move::from_capture(
            Square::E4,
            Square::E8,
            Piece::Rook,
            Piece::Rook,
            false,
        )));
        assert!(!moves.contains(&Move::from_quiet(Square::E4, Square::D4, Piece::Rook)));
        assert!(!moves.contains(&Move::from_quiet(Square::E4, Square::F4, Piece::Rook)));
    }

    #[test]
    fn test_double_check_allows_only_king_moves() {
        // Both E8 rook and H4 bishop check E1. The knight could block the
        // rook ray in a single check, but never under double check.
        let mut state = GameState::empty();
        state.side_to_move = Color::White;
        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::Black, Piece::King, Square::A8);
        set_piece(&mut state, Color::Black, Piece::Rook, Square::E8);
        set_piece(&mut state, Color::Black, Piece::Bishop, Square::H4);
        set_piece(&mut state, Color::White, Piece::Knight, Square::C3);

        let mut moves = MoveList::default();
        state.generate_moves(&mut moves);

        for mv in moves.iter() {
            let m: Move = *mv;
            assert_eq!(
                m.get_moved_piece(),
                Piece::King,
                "non-king move emitted under double check: {}",
                m.debug_string()
            );
        }
    }

    #[test]
    fn test_check_can_be_blocked_by_interposing_piece() {
        // E8 rook checks E1; the F3 knight can interpose on E5.
        let mut state = GameState::empty();
        state.side_to_move = Color::White;
        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::White, Piece::Knight, Square::F3);
        set_piece(&mut state, Color::Black, Piece::King, Square::A8);
        set_piece(&mut state, Color::Black, Piece::Rook, Square::E8);

        let mut moves = MoveList::default();
        state.generate_moves(&mut moves);

        assert!(moves.contains(&Move::from_quiet(Square::F3, Square::E5, Piece::Knight)));
        assert!(!moves.contains(&Move::from_quiet(Square::F3, Square::G5, Piece::Knight)));
        assert!(!moves.contains(&Move::from_quiet(Square::F3, Square::D4, Piece::Knight)));
    }

    #[test]
    fn test_en_passant_is_blocked_by_horizontal_pin() {
        // e5xd6 removes both pawns from rank 5, exposing the white king
        // on a5 to the black rook on h5.
        let state = GameState::from_fen("8/8/8/K2pP2r/8/8/8/4k3 w - d6 0 1").unwrap();

        let mut moves = MoveList::default();
        state.generate_moves(&mut moves);

        let ep = Move::from_capture(Square::E5, Square::D6, Piece::Pawn, Piece::Pawn, true);
        assert!(!moves.contains(&ep), "horizontal-pin EP must be illegal");
    }

    #[test]
    fn test_starting_position_perft_matches_known_values() {
        assert_perft_case(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            &[20, 400, 8902, 197281, 4865609, 119060324],
        );
    }

    #[test]
    fn test_kiwipete_perft_matches_known_values() {
        assert_perft_case(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            &[48, 2039, 97862, 4085603, 193690690],
        );
    }

    #[test]
    fn test_endgame_perft_matches_known_values() {
        assert_perft_case(
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            &[14, 191, 2812, 43238, 674624, 11030083, 178633661],
        );
    }

    #[test]
    fn test_perft_divide_output_matches_depth_two_total() {
        let mut state =
            GameState::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
                .unwrap();
        let divide = state.perft_divide(2);

        assert_eq!(divide.len(), 20);
        assert_eq!(divide.iter().map(|(_, count)| count).sum::<u64>(), 400);
    }
}
