use crate::{core::*, BitBoardMethods, GameState};
use rand::prelude::*;

pub type ZobristHash = u64;

pub struct ZobristHasher {
    pub square: [[[ZobristHash; Square::NUM]; Piece::NUM]; Color::NUM],
    pub side_is_black: ZobristHash,
    pub castling_rights: [ZobristHash; 16],
    pub en_passant: [ZobristHash; Square::NUM],
}

pub static ZOBRIST_HASHER: once_cell::sync::Lazy<ZobristHasher> =
    once_cell::sync::Lazy::new(|| ZobristHasher::new());

impl ZobristHasher {
    fn new() -> ZobristHasher {
        let mut rng = rand::rng();
        let table = std::array::from_fn(|_| {
            std::array::from_fn(|_| std::array::from_fn(|_| rng.random::<u64>()))
        });

        let side_is_black = rng.random::<u64>();
        let castling_rights = std::array::from_fn(|_| rng.random::<u64>());
        let en_passant = std::array::from_fn(|_| rng.random::<u64>());

        ZobristHasher {
            square: table,
            side_is_black,
            castling_rights,
            en_passant,
        }
    }

    pub fn hash(&self, state: &GameState) -> ZobristHash {
        let mut key = 0;

        state
            .pieces
            .iter()
            .enumerate()
            .for_each(|(color, piece_bitboards)| {
                piece_bitboards
                    .iter()
                    .enumerate()
                    .for_each(|(piece, &bitboard)| {
                        for square in bitboard.iter() {
                            key ^= self.square[color][piece][square as usize];
                        }
                    });
            });

        if state.side_to_move == Color::Black {
            key ^= self.side_is_black;
        }

        let castling_rights_index = (state.castling_rights & 0b1111) as usize;
        key ^= self.castling_rights[castling_rights_index];

        if let Some(ep_square) = state.en_passant {
            let file = ep_square.coordinate().0;
            key ^= self.en_passant[file as usize];
        }

        key
    }
}

// Note that GameState already hashes itself.
// If we want to test the ZobristHasher, we should test it against the GameState's hash to ensure they match.
#[cfg(test)]
mod tests {
    use crate::move_generator::MoveList;

    use super::*;

    #[test]
    fn test_zobrist_hash() {
        let state = GameState::new();
        assert_eq!(state.zobrist_hash, ZOBRIST_HASHER.hash(&state));
    }

    fn assert_hash_equal_after_move(state: &mut GameState, mv: Move) {
        let before_hash = state.zobrist_hash;
        let undo = state.make_move(mv);
        assert_eq!(state.zobrist_hash, ZOBRIST_HASHER.hash(&state));
        state.unmake_move(mv, &undo);
        assert_eq!(
            state.zobrist_hash, before_hash,
            "Zobrist hash differs after unmake"
        );
    }

    #[test]
    fn test_zobrist_hash_idempotency() {
        let mut state = GameState::new();
        let mv = Move::from_quiet(Square::E2, Square::E4, Piece::Pawn);
        assert_hash_equal_after_move(&mut state, mv);
    }

    // This function is same as perft.
    // Gamestate maintains a running Zobrist hash, so if the hash is correct.
    // We compare this to the Zobrist hash of the current state, and if they match,
    // we can be confident that the Zobrist hash is correct in make_move and unmake_move.
    fn zobrist_perft(state: &mut GameState, depth: u8, moves_list: &mut [MoveList]) {
        if depth == 0 {
            return;
        }

        let (current_moves, rest) = moves_list.split_first_mut().unwrap();

        current_moves.clear();
        state.generate_valid_moves(current_moves);

        for &mv in current_moves.iter() {
            let prev_hash = state.zobrist_hash;
            let undo_info = state.make_move(mv);
            zobrist_perft(state, depth - 1, rest);
            state.unmake_move(mv, &undo_info);
            assert!(
                state.zobrist_hash == prev_hash,
                "Zobrist hash mismatch after unmake_move at depth {}: expected {:016x}, got {:016x}",
                depth,
                prev_hash,
                state.zobrist_hash
            );
        }
    }

    #[test]
    fn test_zobrist_perft_standard() {
        let mut state = GameState::new();
        let depth: u8 = 5;
        let mut moves_list = vec![MoveList::default(); depth as usize + 1];
        zobrist_perft(&mut state, depth, &mut moves_list);
    }

    #[test]
    fn test_zobrist_perft_complex_middlegame() {
        let mut state = GameState::from_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        )
        .unwrap();
        let depth: u8 = 5;
        let mut moves_list = vec![MoveList::default(); depth as usize + 1];
        zobrist_perft(&mut state, depth, &mut moves_list);
    }

    #[test]
    fn test_zobrist_perft_endgame() {
        let mut state = GameState::from_fen("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1").unwrap();
        let depth: u8 = 5;
        let mut moves_list = vec![MoveList::default(); depth as usize + 1];
        zobrist_perft(&mut state, depth, &mut moves_list);
    }
}
