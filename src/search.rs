use crate::core::*;
use crate::evaluation::{evaluate, MATE_SCORE};
use crate::state::GameState;
use crate::transposition_table::{TranspositionEntry, TranspositionFlag, TranspositionTable};

pub struct SearchResult {
    pub best_move: Option<Move>,
    pub evaluation: i32,
    pub depth: u8,
    pub nodes_searched: u64,
    pub max_quiescence_depth_reached: u8,
}

pub struct Searcher {
    nodes_searched: u64,
    max_quiescence_depth_reached: u8,
    move_pool: Vec<Vec<Move>>,
    transposition_table: TranspositionTable,
}

const MAX_PLY: usize = 64;

impl Searcher {
    pub fn new() -> Self {
        let move_pool = vec![Vec::with_capacity(256); MAX_PLY]; // Preallocate move storage for each depth
        Searcher {
            nodes_searched: 0,
            max_quiescence_depth_reached: 0,
            move_pool,
            transposition_table: TranspositionTable::new(),
        }
    }

    #[inline(always)]
    fn score_to_tt(score: i32, ply: usize) -> i32 {
        let ply = ply as i32;

        if score > MATE_SCORE - MAX_PLY as i32 {
            score + ply
        } else if score < -MATE_SCORE + MAX_PLY as i32 {
            score - ply
        } else {
            score
        }
    }

    #[inline(always)]
    fn score_from_tt(score: i32, ply: usize) -> i32 {
        let ply = ply as i32;

        if score > MATE_SCORE - MAX_PLY as i32 {
            score - ply
        } else if score < -MATE_SCORE + MAX_PLY as i32 {
            score + ply
        } else {
            score
        }
    }

    /// Simple static move ordering score: promotions highest, then captures (MVV-LVA),
    /// then castles, double pawn pushes, then quiet moves.
    fn score_move(mv: Move, _state: &GameState, tt_move: Option<Move>) -> i32 {
        // Material values aligned with `core::Piece` ordering: King, Queen, Rook, Bishop, Knight, Pawn
        const PIECE_VALUES: [i32; Piece::NUM] = [20000, 900, 500, 330, 320, 100];

        let tt_bonus = if Some(mv) == tt_move { 1_000_000 } else { 0 };

        match mv.get_type() {
            MoveType::Promotion { .. } => 20000 + tt_bonus,
            MoveType::Capture { .. } => {
                let captured = mv.get_captured_piece().unwrap_or(Piece::Pawn) as usize;
                let moved = mv.get_moved_piece() as usize;
                // MVV-LVA style: prefer capturing high-value pieces with low-value attackers
                ((PIECE_VALUES[captured] * 100) - (PIECE_VALUES[moved] as i32)) + tt_bonus
            }
            MoveType::Castle { .. } => 500 + tt_bonus,
            MoveType::DoublePawnPush => 50 + tt_bonus,
            _ => tt_bonus,
        }
    }

    /// Iterative deepening search: tries depths 1, 2, 3, ... until time/depth limit
    /// Returns the best move and evaluation found at the deepest completed depth
    pub fn search(&mut self, state: &mut GameState, max_depth: u8) -> SearchResult {
        self.nodes_searched = 0;
        self.max_quiescence_depth_reached = 0;
        let mut best_move = None;
        let mut best_eval = 0i32;

        for depth in 1..=max_depth {
            let (move_found, eval) = self.alpha_beta(state, depth, 0, i32::MIN / 2, i32::MAX / 2);
            if let Some(mv) = move_found {
                best_move = Some(mv);
                best_eval = eval;
            }
        }

        SearchResult {
            best_move,
            evaluation: best_eval,
            depth: max_depth,
            nodes_searched: self.nodes_searched,
            max_quiescence_depth_reached: self.max_quiescence_depth_reached,
        }
    }

    fn alpha_beta(
        &mut self,
        state: &mut GameState,
        depth: u8,
        ply: usize,
        mut alpha: i32,
        mut beta: i32,
    ) -> (Option<Move>, i32) {
        if depth == 0 {
            return (None, self.quiescence(state, ply, alpha, beta));
        }

        self.nodes_searched += 1;

        let original_alpha = alpha;
        let original_beta = beta;
        let tt_entry = self.transposition_table.probe(state.zobrist_hash);

        if let Some(entry) = tt_entry.filter(|entry| entry.depth >= depth) {
            let tt_score = Self::score_from_tt(entry.score, ply);

            match entry.flag {
                TranspositionFlag::Exact => return (entry.best_move, tt_score),
                TranspositionFlag::LowerBound => alpha = alpha.max(tt_score),
                TranspositionFlag::UpperBound => {
                    if tt_score < beta {
                        beta = tt_score;
                    }
                }
            }

            if alpha >= beta {
                return (entry.best_move, tt_score);
            }
        }

        let move_count = {
            let moves = &mut self.move_pool[ply];

            moves.clear();
            state.generate_valid_moves(moves);

            let tt_move = tt_entry.and_then(|entry| entry.best_move);
            moves.sort_by_key(|&mv| -Searcher::score_move(mv, state, tt_move));

            moves.len()
        };

        let mut best_move = None;
        let mut best_eval = i32::MIN / 2;

        for i in 0..move_count {
            let mv = self.move_pool[ply][i];
            let undo_info = state.make_move(mv);
            let (_, eval) = self.alpha_beta(state, depth - 1, ply + 1, -beta, -alpha);
            let eval = -eval;

            state.unmake_move(mv, &undo_info);

            if eval > best_eval {
                best_eval = eval;
                best_move = Some(mv);
            }

            alpha = alpha.max(eval);

            if alpha >= beta {
                break;
            }
        }

        if move_count == 0 {
            if state.is_in_check(state.side_to_move) {
                best_eval = -MATE_SCORE + (ply as i32);
            } else {
                best_eval = 0;
            }
        }

        let flag = if best_eval <= original_alpha {
            TranspositionFlag::UpperBound
        } else if best_eval >= original_beta {
            TranspositionFlag::LowerBound
        } else {
            TranspositionFlag::Exact
        };

        self.transposition_table.store(TranspositionEntry {
            key: state.zobrist_hash,
            depth,
            score: Self::score_to_tt(best_eval, ply),
            flag,
            best_move,
        });

        (best_move, best_eval)
    }

    /// Quiescence search: search captures until a quiet position is reached
    fn quiescence(&mut self, state: &mut GameState, ply: usize, mut alpha: i32, beta: i32) -> i32 {
        self.max_quiescence_depth_reached = self.max_quiescence_depth_reached.max(ply as u8);
        self.nodes_searched += 1;

        {
            let moves = &mut self.move_pool[ply];
            moves.clear();
            state.generate_valid_moves(moves);
            let move_count = moves.len();
            if move_count == 0 {
                if state.is_in_check(state.side_to_move) {
                    return -MATE_SCORE + (ply as i32);
                } else {
                    return 0;
                }
            }
        }

        let move_count = {
            let moves = &mut self.move_pool[ply];
            let in_check = state.is_in_check(state.side_to_move);
            // only filter captures/promotions if not in check
            // otherwise we might miss important evasions
            if !in_check {
                moves.retain(|&mv| mv.is_capture() || mv.is_promotion());
            }
            moves.sort_by_key(|&mv| -Searcher::score_move(mv, state, None));
            moves.len()
        };

        let stand_pat = evaluate(state);

        if stand_pat >= beta {
            return beta;
        }

        if stand_pat > alpha {
            alpha = stand_pat;
        }

        for i in 0..move_count {
            let mv = self.move_pool[ply][i];
            let undo_info = state.make_move(mv);
            let eval = -self.quiescence(state, ply + 1, -beta, -alpha);
            state.unmake_move(mv, &undo_info);

            if eval >= beta {
                return beta;
            }

            if eval > alpha {
                alpha = eval;
            }
        }

        alpha
    }
}

impl Default for Searcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_best_moves_till_limit(
        state: &mut GameState,
        search_depth: u8,
        max_moves: usize,
    ) -> Vec<Move> {
        let mut searcher = Searcher::new();
        let mut history = vec![];

        for _ in 0..max_moves {
            if state.is_checkmate() || state.is_stalemate() {
                break;
            }
            let result = searcher.search(state, search_depth);
            if let Some(best_move) = result.best_move {
                history.push(best_move);
                state.make_move(best_move);
            } else {
                break;
            }
        }

        history
    }

    fn assert_move_sequence(mut state: GameState, expected_moves: &[&str], search_depth: u8) {
        let best_moves = get_best_moves_till_limit(&mut state, search_depth, expected_moves.len());
        assert_eq!(best_moves.len(), expected_moves.len());
        for (i, mv) in best_moves.iter().enumerate() {
            println!("Move {}: {}", i + 1, mv.repr_string());
            assert_eq!(mv.repr_string(), expected_moves[i]);
        }
    }

    #[test]
    fn test_search_mate_in_one() {
        let state = GameState::from_fen("3r4/1K6/2Nb4/2kb4/8/8/3PB3/8 w - - 0 1").unwrap();
        let best_moves_expected = ["d2d4"];
        assert_move_sequence(state, &best_moves_expected, 4);
    }

    #[test]
    fn test_search_mate_in_two() {
        let state =
            GameState::from_fen("5rk1/5ppp/2p5/1p6/1Q1p1P2/2Pq4/bP2R2P/rNK1R3 w - - 0 24").unwrap();
        let best_moves_expected = ["b4f8", "g8f8", "e2e8"];
        assert_move_sequence(state, &best_moves_expected, 4);
    }

    #[test]
    fn test_search_mate_in_three() {
        let state = GameState::from_fen("4k1r1/R6p/4Nb2/4n3/6Pq/2P4P/3Q3K/5R2 w - - 2 2").unwrap();
        let best_moves_expected = ["d2d8", "f6d8", "f1f8", "g8f8", "e6g7"];
        assert_move_sequence(state, &best_moves_expected, 5);
    }
}
