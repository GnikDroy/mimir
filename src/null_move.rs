use crate::core::*;
use crate::state::GameState;
use crate::zobrist::ZOBRIST_HASHER;

/// State needed to undo a null move.
pub struct NullUndoInfo {
    pub en_passant_before: Option<Square>,
    pub halfmove_clock_before: u8,
}

impl GameState {
    /// Whether the side to move can safely be pruned by null move
    /// pruning. Currently: requires non-pawn material to avoid
    /// zugzwang in king-and-pawn endgames, where passing the turn
    /// would falsely look winning.
    #[inline]
    pub fn position_suitable_for_null_move(&self) -> bool {
        let me = self.side_to_move as usize;
        let non_pawn = self.pieces[me][Piece::Knight as usize]
            | self.pieces[me][Piece::Bishop as usize]
            | self.pieces[me][Piece::Rook as usize]
            | self.pieces[me][Piece::Queen as usize];
        non_pawn != 0
    }

    /// Applies a "null move": pass the turn without making a move. Used
    /// by null move pruning in search.
    pub fn make_null_move(&mut self) -> NullUndoInfo {
        let undo = NullUndoInfo {
            en_passant_before: self.en_passant,
            halfmove_clock_before: self.halfmove_clock,
        };

        if let Some(ep_sq) = self.en_passant {
            let file = ep_sq.coordinate().0 as usize;
            self.zobrist_hash ^= ZOBRIST_HASHER.en_passant[file];
            self.en_passant = None;
        }

        self.side_to_move = self.side_to_move.opposite();
        self.zobrist_hash ^= ZOBRIST_HASHER.side_is_black;

        self.halfmove_clock = self.halfmove_clock.saturating_add(1);
        if self.side_to_move == Color::White {
            self.fullmove_number += 1;
        }

        undo
    }

    pub fn unmake_null_move(&mut self, undo: &NullUndoInfo) {
        if self.side_to_move == Color::White {
            self.fullmove_number -= 1;
        }
        self.side_to_move = self.side_to_move.opposite();
        self.zobrist_hash ^= ZOBRIST_HASHER.side_is_black;

        self.halfmove_clock = undo.halfmove_clock_before;

        if let Some(ep_sq) = undo.en_passant_before {
            let file = ep_sq.coordinate().0 as usize;
            self.zobrist_hash ^= ZOBRIST_HASHER.en_passant[file];
        }
        self.en_passant = undo.en_passant_before;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zobrist::ZOBRIST_HASHER;

    #[test]
    fn test_null_move_restores_state() {
        let mut state = GameState::new();
        let hash_before = state.zobrist_hash;
        let side_before = state.side_to_move;
        let ep_before = state.en_passant;
        let halfmove_before = state.halfmove_clock;
        let fullmove_before = state.fullmove_number;

        let undo = state.make_null_move();
        state.unmake_null_move(&undo);

        assert_eq!(state.zobrist_hash, hash_before);
        assert_eq!(state.side_to_move, side_before);
        assert_eq!(state.en_passant, ep_before);
        assert_eq!(state.halfmove_clock, halfmove_before);
        assert_eq!(state.fullmove_number, fullmove_before);
    }

    #[test]
    fn test_null_move_flips_side_and_zobrist() {
        let mut state = GameState::new();
        let hash_before = state.zobrist_hash;
        let side_before = state.side_to_move;

        let _undo = state.make_null_move();

        assert_eq!(state.side_to_move, side_before.opposite());
        assert_eq!(
            state.zobrist_hash,
            hash_before ^ ZOBRIST_HASHER.side_is_black
        );
    }

    #[test]
    fn test_null_move_clears_en_passant() {
        // 1. e2-e4 leaves an en passant square on e3.
        let mut state = crate::state::GameState::from_fen(
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/BNBQKBNR b KQkq e3 0 1",
        )
        .unwrap();
        assert!(state.en_passant.is_some());

        let undo = state.make_null_move();
        assert!(state.en_passant.is_none());

        state.unmake_null_move(&undo);
        assert!(state.en_passant.is_some());
    }

    #[test]
    fn test_position_suitable_for_null_move_starting_position() {
        let state = GameState::new();
        assert!(state.position_suitable_for_null_move());
    }

    #[test]
    fn test_position_suitable_for_null_move_kp_endgame() {
        // King and pawns only — zugzwang risk, NMP must be disabled.
        let state = GameState::from_fen("4k3/p7/8/8/8/8/P7/4K3 w - - 0 1").unwrap();
        assert!(!state.position_suitable_for_null_move());
    }

    #[test]
    fn test_position_suitable_for_null_move_lone_king() {
        let state = GameState::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert!(!state.position_suitable_for_null_move());
    }

    #[test]
    fn test_position_suitable_for_null_move_minor_piece() {
        // A single knight is enough non-pawn material to avoid zugzwang.
        let state = GameState::from_fen("4k3/8/8/8/8/8/8/4KN2 w - - 0 1").unwrap();
        assert!(state.position_suitable_for_null_move());
    }

    #[test]
    fn test_null_move_zobrist_matches_full_recompute() {
        let mut state = crate::state::GameState::from_fen(
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/BNBQKBNR b KQkq e3 0 1",
        )
        .unwrap();

        let _undo = state.make_null_move();
        assert_eq!(state.zobrist_hash, ZOBRIST_HASHER.hash(&state));
    }
}
