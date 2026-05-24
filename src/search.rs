use crate::core::*;
use crate::evaluation::{evaluate, MATE_SCORE};
use crate::state::GameState;

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
}

const MAX_PLY: usize = 64;

impl Searcher {
    pub fn new() -> Self {
        let move_pool = vec![Vec::with_capacity(256); MAX_PLY]; // Preallocate move storage for each depth
        Searcher {
            nodes_searched: 0,
            max_quiescence_depth_reached: 0,
            move_pool,
        }
    }

    /// Simple static move ordering score: promotions highest, then captures (MVV-LVA),
    /// then castles, double pawn pushes, then quiet moves.
    fn score_move(mv: Move, _state: &GameState) -> i32 {
        // Material values aligned with `core::Piece` ordering: King, Queen, Rook, Bishop, Knight, Pawn
        const PIECE_VALUES: [i32; Piece::NUM] = [20000, 900, 500, 330, 320, 100];

        match mv.get_type() {
            MoveType::Promotion { .. } => 20000,
            MoveType::Capture { .. } => {
                let captured = mv.get_captured_piece().unwrap_or(Piece::Pawn) as usize;
                let moved = mv.get_moved_piece() as usize;
                // MVV-LVA style: prefer capturing high-value pieces with low-value attackers
                (PIECE_VALUES[captured] * 100) - (PIECE_VALUES[moved] as i32)
            }
            MoveType::Castle { .. } => 500,
            MoveType::DoublePawnPush => 50,
            _ => 0,
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
        beta: i32,
    ) -> (Option<Move>, i32) {
        if depth == 0 {
            return (None, self.quiescence(state, ply, alpha, beta));
        }

        self.nodes_searched += 1;

        let move_count = {
            let moves = &mut self.move_pool[ply];

            moves.clear();
            state.generate_valid_moves(moves);

            moves.sort_by_key(|&mv| -Searcher::score_move(mv, state));

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
            // only consider captures if not in check, otherwise we might miss important evasions
            if !in_check {
                moves.retain(|&mv| mv.is_capture());
            }
            moves.sort_by_key(|&mv| -Searcher::score_move(mv, state));
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
