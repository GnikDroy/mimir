use crate::core::*;
use crate::evaluation::evaluate;
use crate::state::GameState;

pub struct SearchResult {
    pub best_move: Option<Move>,
    pub evaluation: i32,
    pub depth: u8,
    pub nodes_searched: u64,
}

pub struct Searcher {
    nodes_searched: u64,
    move_pool: Vec<Vec<Move>>,
}

impl Searcher {
    pub fn new() -> Self {
        Searcher { nodes_searched: 0 }
    }

    /// Simple static move ordering score: promotions highest, then captures (MVV-LVA),
    /// then castles, double pawn pushes, then quiet moves.
    fn score_move(&self, mv: Move, _state: &GameState) -> i32 {
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
        let mut best_move = None;
        let mut best_eval = 0i32;

        for depth in 1..=max_depth {
            let (move_found, eval) = self.alpha_beta(state, depth, i32::MIN / 2, i32::MAX / 2);
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
        }
    }

    /// Alpha-beta pruning: negamax variant with alphabeta window
    /// Returns (best_move, evaluation)
    fn alpha_beta(
        &mut self,
        state: &mut GameState,
        depth: u8,
        mut alpha: i32,
        mut beta: i32,
    ) -> (Option<Move>, i32) {
        // Quiescence search at leaf nodes to stabilize evaluation
        if depth == 0 {
            return (None, self.quiescence(state, alpha, beta));
        }

        self.nodes_searched += 1;

        let mut moves = Vec::with_capacity(256);
        state.generate_valid_moves(&mut moves);
        // Order moves to improve alpha-beta pruning: higher score first
        moves.sort_by_key(|&mv| -self.score_move(mv, state));

        let mut best_move = None;
        let mut best_eval = i32::MIN / 2;

        for mv in moves {
            let undo_info = state.make_move(mv);

            let (_, eval) = self.alpha_beta(state, depth - 1, -beta, -alpha);
            let eval = -eval;

            state.unmake_move(mv, &undo_info);

            if eval > best_eval {
                best_eval = eval;
                best_move = Some(mv);
            }

            alpha = alpha.max(eval);
            if alpha >= beta {
                break; // Beta cutoff
            }
        }

        if best_move.is_none() {
            // No legal moves: checkmate or stalemate
            if state.is_checkmate() {
                best_eval = -100000; // Loss for side to move
            } else {
                best_eval = 0; // Draw
            }
        }

        (best_move, best_eval)
    }

    /// Quiescence search: search captures and checks until a "quiet" position is reached
    /// Helps stabilize evaluation near leaf nodes
    fn quiescence(&mut self, state: &mut GameState, mut alpha: i32, beta: i32) -> i32 {
        self.nodes_searched += 1;

        if state.is_checkmate() {
            return -100000; // Loss for side to move
        }

        if state.is_stalemate() {
            return 0; // Draw
        }

        // Evaluate the current position
        let stand_pat = evaluate(state);
        if stand_pat >= beta {
            return beta;
        }
        if stand_pat > alpha {
            alpha = stand_pat;
        }

        // Generate capture moves only and order them
        let mut moves = Vec::with_capacity(256);
        state.generate_valid_moves(&mut moves);
        moves.retain(|&mv| mv.is_capture());
        moves.sort_by_key(|&mv| -self.score_move(mv, state));

        for mv in moves {
            let undo_info = state.make_move(mv);

            let eval = -self.quiescence(state, -beta, -alpha);

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
