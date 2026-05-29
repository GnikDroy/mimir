use std::time::Duration;

use crate::core::*;
use crate::evaluation::{evaluate, MATE_SCORE};
use crate::state::GameState;
use crate::time_control::TimeControl;
use crate::transposition_table::{TranspositionEntry, TranspositionFlag, TranspositionTable};

#[derive(Debug, Clone, Copy)]
pub struct SearchAnalytics {
    pub depth: u8,
    pub elapsed: Duration,
    pub nodes_searched: u64,
    pub quiescence_nodes_searched: u64,
    pub max_quiescence_depth_reached: u8,
    pub alpha_beta_cutoffs: u64,
    pub quiescence_alpha_beta_cutoffs: u64,
    pub transposition_table_hits: u64,
    pub transposition_table_cuts: u64,
}

impl SearchAnalytics {
    pub fn total_nodes(&self) -> u64 {
        self.nodes_searched + self.quiescence_nodes_searched
    }

    pub fn nodes_per_second(&self) -> u64 {
        if self.elapsed.as_secs_f64() == 0.0 {
            return 0;
        }
        (self.total_nodes() as f64 / self.elapsed.as_secs_f64()).round() as u64
    }
}

impl Default for SearchAnalytics {
    fn default() -> Self {
        Self {
            depth: 0,
            elapsed: Duration::from_secs(0),
            nodes_searched: 0,
            quiescence_nodes_searched: 0,
            max_quiescence_depth_reached: 0,
            alpha_beta_cutoffs: 0,
            quiescence_alpha_beta_cutoffs: 0,
            transposition_table_hits: 0,
            transposition_table_cuts: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SearchResult {
    pub best_move: Option<Move>,
    pub evaluation: i32,
    pub analytics: SearchAnalytics,
}

pub struct Searcher {
    move_pool: Vec<Vec<Move>>,
    transposition_table: TranspositionTable,
    time_control: TimeControl,
    analytics: SearchAnalytics,
}

const MAX_PLY: usize = 64;

// Check for timeouts every N nodes in alpha-beta and every M quiescence nodes.
const NODE_CHECK_INTERVAL: u64 = 256;
const QUIESCENCE_NODE_CHECK_INTERVAL: u64 = 128;

impl Searcher {
    pub fn new() -> Self {
        let move_pool = vec![Vec::with_capacity(256); MAX_PLY]; // Preallocate move storage for each depth

        let time_control = TimeControl::new(Duration::from_mins(1), Duration::from_secs(0));

        Searcher {
            move_pool,
            transposition_table: TranspositionTable::new(),
            analytics: SearchAnalytics::default(),
            time_control: time_control,
        }
    }

    pub fn update_clock(
        &mut self,
        wtime: Duration,
        btime: Duration,
        winc: Duration,
        binc: Duration,
        movestogo: Option<u32>,
        move_time: Option<Duration>,
    ) {
        self.time_control
            .update_clock(wtime, btime, winc, binc, movestogo, move_time);
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
    /// all of them are given bonuses from transposition table move if it matches, to ensure we search the tt move first``
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
    pub fn search(
        &mut self,
        state: &mut GameState,
        max_depth: u8,
        report_fn: Option<impl Fn(SearchResult)>,
    ) -> SearchResult {
        self.analytics = SearchAnalytics::default();
        let mut best_move = None;
        let mut best_eval = 0i32;

        self.time_control.set_search_deadline(state.side_to_move);
        for depth in 1..=max_depth {
            match self.alpha_beta(state, depth, 0, i32::MIN / 2, i32::MAX / 2) {
                Some((mv, eval)) => {
                    best_move = mv;
                    best_eval = eval;
                    self.analytics.depth = depth;
                }
                None => {
                    self.analytics.depth = depth - 1;
                }
            };

            // report info after each completed depth, if a reporting function is provided
            self.analytics.elapsed = self.time_control.get_elapsed();
            report_fn.as_ref().map(|f| {
                f(SearchResult {
                    best_move,
                    evaluation: best_eval,
                    analytics: self.analytics,
                })
            });

            // We ran out of time, so stop searching deeper
            if self.time_control.is_time_up() {
                break;
            }
        }

        self.time_control.clear_search_deadline();
        SearchResult {
            best_move,
            evaluation: best_eval,
            analytics: self.analytics,
        }
    }

    fn alpha_beta(
        &mut self,
        state: &mut GameState,
        depth: u8,
        ply: usize,
        mut alpha: i32,
        mut beta: i32,
    ) -> Option<(Option<Move>, i32)> {
        if depth == 0 {
            return self
                .quiescence(state, ply, alpha, beta)
                .map(|eval| (None, eval));
        }

        self.analytics.nodes_searched += 1;

        // Periodic timeout check to avoid calling Instant::now() every node.
        if self.analytics.nodes_searched % NODE_CHECK_INTERVAL == 0
            && self.time_control.is_time_up()
        {
            return None;
        }

        let original_alpha = alpha;
        let original_beta = beta;
        let tt_entry = self.transposition_table.probe(state.zobrist_hash);

        // Check if transposition table has a valid entry for this position and depth,
        // and use it to potentially cut off the search early
        if let Some(entry) = tt_entry.filter(|entry| entry.depth >= depth) {
            let tt_score = Self::score_from_tt(entry.score, ply);

            match entry.flag {
                TranspositionFlag::Exact => return Some((entry.best_move, tt_score)),
                TranspositionFlag::LowerBound => alpha = alpha.max(tt_score),
                TranspositionFlag::UpperBound => beta = beta.min(tt_score),
            }

            self.analytics.transposition_table_hits += 1;

            if alpha >= beta {
                self.analytics.transposition_table_cuts += 1;
                return Some((entry.best_move, tt_score));
            }
        }

        // sort moves by heuristic score (captures/promotions first, then tt move if available)
        // to improve alpha-beta efficiency
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
            // (alpha, beta) -> (-beta, -alpha) is standard in negamax
            let eval = match self.alpha_beta(state, depth - 1, ply + 1, -beta, -alpha) {
                Some((_, eval)) => {
                    state.unmake_move(mv, &undo_info);
                    eval
                }
                None => {
                    state.unmake_move(mv, &undo_info);
                    return None;
                }
            };

            // Core negamax idea. We negate the evaluation score returned.
            // This means each player is maximizing their own score.
            let eval = -eval;

            // min(-eval, -best_eval) = max(eval, best_eval)
            // So, this works for both players without needing to check which side is to move
            if eval > best_eval {
                best_eval = eval;
                best_move = Some(mv);
            }

            // update alpha.
            // alpha-beta on negamax, unlike minimax doesn't require updating beta.
            alpha = alpha.max(eval);

            // This is the core alpha-beta cutoff.
            if alpha >= beta {
                self.analytics.alpha_beta_cutoffs += 1;
                break;
            }
        }

        // checkmate and stalemate detection.
        if move_count == 0 {
            if state.is_in_check(state.side_to_move) {
                best_eval = -MATE_SCORE + (ply as i32);
            } else {
                best_eval = 0;
            }
        }

        // write to transposition table
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

        Some((best_move, best_eval))
    }

    /// Quiescence search: search captures until a quiet position is reached
    fn quiescence(
        &mut self,
        state: &mut GameState,
        ply: usize,
        mut alpha: i32,
        beta: i32,
    ) -> Option<i32> {
        self.analytics.max_quiescence_depth_reached =
            self.analytics.max_quiescence_depth_reached.max(ply as u8);
        self.analytics.quiescence_nodes_searched += 1;

        // Periodic timeout check in quiescence search
        if self.analytics.quiescence_nodes_searched % QUIESCENCE_NODE_CHECK_INTERVAL == 0
            && self.time_control.is_time_up()
        {
            return None;
        }

        // check for checkmate/stalemate before generating moves,
        // to avoid missing quiet mates or stalemates in quiescence search
        {
            let moves = &mut self.move_pool[ply];
            moves.clear();
            state.generate_valid_moves(moves);
            let move_count = moves.len();
            if move_count == 0 {
                if state.is_in_check(state.side_to_move) {
                    return Some(-MATE_SCORE + (ply as i32));
                } else {
                    return Some(0);
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
            self.analytics.quiescence_alpha_beta_cutoffs += 1;
            return Some(beta);
        }

        if stand_pat > alpha {
            alpha = stand_pat;
        }

        for i in 0..move_count {
            let mv = self.move_pool[ply][i];
            let undo_info = state.make_move(mv);
            let eval = self.quiescence(state, ply + 1, -beta, -alpha);
            if eval.is_none() {
                return None;
            }
            let eval = -eval.unwrap();
            state.unmake_move(mv, &undo_info);

            // update alpha.
            // alpha-beta on negamax, unlike minimax doesn't require updating beta.
            alpha = alpha.max(eval);

            // core alpha-beta cutoff.
            if alpha >= beta {
                self.analytics.quiescence_alpha_beta_cutoffs += 1;
                break;
            }
        }

        // Fail-soft quiescence: return the best score found,
        // which may exceed the original alpha bound.
        // A fail-hard implementation would instead return beta
        // immediately on cutoff.
        Some(alpha)
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
            let initial_time = std::time::Instant::now();
            let result = searcher.search(state, search_depth, None::<fn(SearchResult)>);
            let elapsed = initial_time.elapsed();
            println!(
                "Best Move: {}, Eval: {}, Nodes: {}, Max Depth: {}, Max QDepth: {}, Time: {:?}",
                result
                    .best_move
                    .map_or("None".to_string(), |mv| mv.repr_string()),
                result.evaluation,
                result.analytics.total_nodes(),
                result.analytics.depth,
                result.analytics.max_quiescence_depth_reached,
                elapsed
            );
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
