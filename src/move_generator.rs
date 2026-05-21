use crate::attack_table::{AttackTable, ATTACK_TABLE};
use crate::bitboard::{BitBoard, BitBoardMethods};
use crate::board::GameState;
use crate::core::*;
pub struct MoveGenerator {
    attacks: &'static AttackTable,
}

impl MoveGenerator {
    pub fn new() -> Self {
        MoveGenerator {
            attacks: &ATTACK_TABLE,
        }
    }

    pub fn generate_moves(&self, state: &GameState) -> Vec<Move> {
        let mut moves = Vec::with_capacity(256);
        let friendly = state.occupancies[state.side_to_move as usize];
        let enemy = state.occupancies[state.side_to_move.opposite() as usize];
        let occupancy = state.occupancies[2];

        // Generate moves for each piece type
        for piece in Piece::all() {
            let pieces = state.pieces[state.side_to_move as usize][piece as usize];
            for from in pieces.iter() {
                match piece {
                    Piece::Pawn => self.add_pawn_moves(state, from, &mut moves),
                    Piece::King => {
                        let attacks = self.attacks.get_king(from);
                        self.add_attack_moves(from, attacks & !friendly, &enemy, &mut moves);
                    }
                    Piece::Knight => {
                        let attacks = self.attacks.get_knight(from);
                        self.add_attack_moves(from, attacks & !friendly, &enemy, &mut moves);
                    }
                    Piece::Bishop => {
                        let attacks = self.attacks.get_bishop(from, occupancy);
                        self.add_attack_moves(from, attacks & !friendly, &enemy, &mut moves);
                    }
                    Piece::Rook => {
                        let attacks = self.attacks.get_rook(from, occupancy);
                        self.add_attack_moves(from, attacks & !friendly, &enemy, &mut moves);
                    }
                    Piece::Queen => {
                        let attacks = self.attacks.get_queen(from, occupancy);
                        self.add_attack_moves(from, attacks & !friendly, &enemy, &mut moves);
                    }
                }

                // castling moves (only for king)
                if piece == Piece::King {
                    self.add_castling_moves(state, from, &mut moves);
                }
            }
        }

        moves
    }

    fn get_attacked_squares(&self, state: &GameState, attacker: Color) -> BitBoard {
        let mut attacked = BitBoard::EMPTY;

        let attacker_boards = &state.pieces[attacker as usize];
        let pawn_board = attacker_boards[Piece::Pawn as usize];
        // It is faster to compute for all pawns at once instead of using attack table.
        // This includes en passant squares, since they are attacked by pawns as well.
        let pawn_attacks = match attacker {
            Color::White => pawn_board.shift_north_east() | pawn_board.shift_north_west(),
            Color::Black => pawn_board.shift_south_east() | pawn_board.shift_south_west(),
        };
        attacked |= pawn_attacks;

        for piece in [
            Piece::Knight,
            Piece::Bishop,
            Piece::Rook,
            Piece::Queen,
            Piece::King,
        ] {
            let board = attacker_boards[piece as usize];
            for from in board.iter() {
                let attacks = match piece {
                    Piece::Knight => self.attacks.get_knight(from),
                    Piece::Bishop => self.attacks.get_bishop(from, state.occupancies[2]),
                    Piece::Rook => self.attacks.get_rook(from, state.occupancies[2]),
                    Piece::Queen => self.attacks.get_queen(from, state.occupancies[2]),
                    Piece::King => self.attacks.get_king(from),
                    _ => unreachable!(),
                };
                attacked |= attacks;
            }
        }

        attacked
    }

    fn add_castling_moves(&self, state: &GameState, from: Square, moves: &mut Vec<Move>) {
        let color = state.side_to_move;

        // King must be on starting square to castle
        if (color == Color::White && from != Square::E1)
            || (color == Color::Black && from != Square::E8)
        {
            return;
        }

        let enemy = color.opposite();
        let attacked_by_enemy = self.get_attacked_squares(state, enemy);
        if (attacked_by_enemy & BitBoard::on(from)) != 0 {
            return;
        }

        if self.can_castle(state, color, true, attacked_by_enemy) {
            let to = match color {
                Color::White => Square::G1,
                Color::Black => Square::G8,
            };
            moves.push(Move::from_castle(from, to, true));
        }

        if self.can_castle(state, color, false, attacked_by_enemy) {
            let to = match color {
                Color::White => Square::C1,
                Color::Black => Square::C8,
            };
            moves.push(Move::from_castle(from, to, false));
        }
    }

    fn can_castle(
        &self,
        state: &GameState,
        color: Color,
        kingside: bool,
        attacked_by_enemy: BitBoard,
    ) -> bool {
        // Check castling rights
        let rights = if kingside {
            match color {
                Color::White => (state.castling_rights & 0b0001) != 0,
                Color::Black => (state.castling_rights & 0b0100) != 0,
            }
        } else {
            match color {
                Color::White => (state.castling_rights & 0b0010) != 0,
                Color::Black => (state.castling_rights & 0b1000) != 0,
            }
        };
        if !rights {
            return false;
        }

        // Define rook square, empty squares, and king path squares
        let (rook_square, empty_between, through_mask) = match (color, kingside) {
            (Color::White, true) => (
                Square::H1,
                BitBoard::on(Square::F1) | BitBoard::on(Square::G1),
                BitBoard::on(Square::F1) | BitBoard::on(Square::G1),
            ),
            (Color::White, false) => (
                Square::A1,
                BitBoard::on(Square::B1) | BitBoard::on(Square::C1) | BitBoard::on(Square::D1),
                BitBoard::on(Square::D1) | BitBoard::on(Square::C1),
            ),
            (Color::Black, true) => (
                Square::H8,
                BitBoard::on(Square::F8) | BitBoard::on(Square::G8),
                BitBoard::on(Square::F8) | BitBoard::on(Square::G8),
            ),
            (Color::Black, false) => (
                Square::A8,
                BitBoard::on(Square::B8) | BitBoard::on(Square::C8) | BitBoard::on(Square::D8),
                BitBoard::on(Square::D8) | BitBoard::on(Square::C8),
            ),
        };

        // Check that the path between king and rook is empty
        if (state.occupancies[2] & empty_between) != 0 {
            return false;
        }

        // Check that the rook is in place
        let rook_board = state.pieces[color as usize][Piece::Rook as usize];
        if (rook_board & BitBoard::on(rook_square)) == 0 {
            return false;
        }

        // Check that the king's path squares are not attacked
        (attacked_by_enemy & through_mask) == 0
    }

    /// Generate pawn moves (push, captures, promotions, en passant)
    fn add_pawn_moves(&self, state: &GameState, from: Square, moves: &mut Vec<Move>) {
        self.add_pawn_moves_for_color(state, from, state.side_to_move, moves);
    }

    fn add_pawn_moves_for_color(
        &self,
        state: &GameState,
        from: Square,
        color: Color,
        moves: &mut Vec<u16>,
    ) {
        let from_board = BitBoard::on(from);
        let occupancy = state.occupancies[2];
        let enemy = state.occupancies[color.opposite() as usize];

        // Single push
        let single_push = match color {
            Color::White => from_board.shift_north() & !occupancy,
            Color::Black => from_board.shift_south() & !occupancy,
        };

        for to in single_push.iter() {
            self.add_pawn_move(from, to, color, false, moves);
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
        let attacks = self.attacks.get_pawn(from, color);
        let captures = attacks & enemy;
        for to in captures.iter() {
            self.add_pawn_move(from, to, color, true, moves);
        }

        // En passant
        if let Some(ep_sq) = state.en_passant {
            if (attacks & BitBoard::on(ep_sq)) != 0 {
                moves.push(Move::from_capture(from, ep_sq, true));
            }
        }
    }

    fn add_pawn_move(
        &self,
        from: Square,
        to: Square,
        color: Color,
        is_capture: bool,
        moves: &mut Vec<Move>,
    ) {
        if to.is_promotion_square(color) {
            for promo in [
                PromotionPiece::Queen,
                PromotionPiece::Rook,
                PromotionPiece::Bishop,
                PromotionPiece::Knight,
            ] {
                moves.push(Move::from_promotion(from, to, promo, is_capture));
            }
        } else if is_capture {
            moves.push(Move::from_capture(from, to, false));
        } else {
            moves.push(Move::from_quiet(from, to));
        }
    }

    /// Generate moves for any piece, separating quiet moves from captures
    fn add_attack_moves(
        &self,
        from: Square,
        targets: BitBoard,
        enemy: &BitBoard,
        moves: &mut Vec<Move>,
    ) {
        for to in targets.iter() {
            let to_board = BitBoard::on(to);

            if (to_board & enemy) != 0 {
                moves.push(Move::from_capture(from, to, false));
            } else {
                moves.push(Move::from_quiet(from, to));
            }
        }
    }
}

impl Default for MoveGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::MoveGenerator;
    use crate::bitboard::*;
    use crate::board::GameState;
    use crate::core::*;

    fn empty_state() -> GameState {
        GameState::new()
    }

    fn set_piece(state: &mut GameState, color: Color, piece: Piece, square: Square) {
        let board = BitBoard::on(square);
        state.pieces[color as usize][piece as usize] |= board;
        state.occupancies[color as usize] |= board;
        state.occupancies[2] |= board;
    }

    fn moves_for(state: GameState) -> Vec<u16> {
        MoveGenerator::new().generate_moves(&state)
    }

    #[test]
    fn starting_position_generates_twenty_white_moves() {
        let state = GameState::starting_position();
        let moves = moves_for(state);

        assert_eq!(moves.len(), 20);
        assert!(moves.contains(&Move::from_quiet(Square::E2, Square::E3)));
        assert!(moves.contains(&Move::from_double_pawn_push(Square::E2, Square::E4)));
        assert!(moves.contains(&Move::from_quiet(Square::G1, Square::F3)));
        assert!(moves.contains(&Move::from_quiet(Square::B1, Square::C3)));
        assert!(!moves.contains(&Move::from_castle(Square::E1, Square::G1, true)));
        assert!(!moves.contains(&Move::from_castle(Square::E1, Square::C1, false)));
    }

    #[test]
    fn pawn_promotions_generate_all_promotion_moves() {
        let mut state = empty_state();
        state.side_to_move = Color::White;
        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::Black, Piece::King, Square::C8);
        set_piece(&mut state, Color::White, Piece::Pawn, Square::E7);

        let moves = moves_for(state);

        for promotion in PromotionPiece::all() {
            assert!(moves.contains(&Move::from_promotion(
                Square::E7,
                Square::E8,
                promotion,
                false,
            )));
        }
    }

    #[test]
    fn en_passant_move_is_generated() {
        let mut state = empty_state();
        state.side_to_move = Color::White;
        state.en_passant = Some(Square::D6);

        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::Black, Piece::King, Square::E8);
        set_piece(&mut state, Color::White, Piece::Pawn, Square::E5);
        set_piece(&mut state, Color::Black, Piece::Pawn, Square::D5);

        let moves = moves_for(state);

        assert!(moves.contains(&Move::from_capture(Square::E5, Square::D6, true)));
    }

    #[test]
    fn castling_is_generated_when_path_is_clear_and_unattacked() {
        let mut state = empty_state();
        state.side_to_move = Color::White;
        state.castling_rights = 0b0001;

        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::White, Piece::Rook, Square::H1);
        set_piece(&mut state, Color::Black, Piece::King, Square::E8);

        let moves = moves_for(state);

        assert!(moves.contains(&Move::from_castle(Square::E1, Square::G1, true)));
    }

    #[test]
    fn castling_is_blocked_when_path_is_attacked() {
        let mut state = empty_state();
        state.side_to_move = Color::White;
        state.castling_rights = 0b0001;

        set_piece(&mut state, Color::White, Piece::King, Square::E1);
        set_piece(&mut state, Color::White, Piece::Rook, Square::H1);
        set_piece(&mut state, Color::Black, Piece::King, Square::E8);
        set_piece(&mut state, Color::Black, Piece::Rook, Square::F8);

        let moves = moves_for(state);

        assert!(!moves.contains(&Move::from_castle(Square::E1, Square::G1, true)));
    }
}
