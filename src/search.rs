use std::time::Duration;

use crate::core::*;
use crate::evaluation::evaluate;
use crate::move_score::MoveScore;
use crate::move_scorer::MoveScorer;
use crate::pv_table::PvTable;
use crate::score;
use crate::stack_vec::StackVec;
use crate::state::GameState;
use crate::time_control::TimeControl;
use crate::transposition_table::{TranspositionEntry, TranspositionFlag, TranspositionTable};
use crate::zobrist::ZobristHash;

#[derive(Debug, Clone, Copy, Default)]
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
    pub transposition_table_hashfull: u32,
    pub pvs_re_searches: u64,
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

#[derive(Debug, Clone, Copy)]
pub struct SearchResult {
    pub evaluation: i32,
    pub analytics: SearchAnalytics,
    pub pv: PvList,
}

impl SearchResult {
    /// Head of the PV is the best move.
    pub fn best_move(&self) -> Option<Move> {
        self.pv.first().copied()
    }

    pub fn mate_in(&self) -> Option<i32> {
        score::mate_in_plies(self.evaluation)
    }
}

pub type ZobristHashList = StackVec<ZobristHash, MAX_PLY>;
pub type PvList = StackVec<Move, MAX_PLY>;

pub struct Searcher {
    position_history: Box<ZobristHashList>,
    transposition_table: TranspositionTable,
    time_control: TimeControl,
    analytics: SearchAnalytics,
    move_scorer: MoveScorer,
    pv: PvTable<MAX_PLY>,
}

pub const MAX_PLY: usize = 64;

// Check for timeouts every N nodes in alpha-beta and every M quiescence nodes.
const NODE_CHECK_INTERVAL: u64 = 256;
const QUIESCENCE_NODE_CHECK_INTERVAL: u64 = 128;

impl Searcher {
    pub fn new() -> Self {
        let time_control = TimeControl::new(Duration::from_mins(1), Duration::from_secs(0));

        Searcher {
            position_history: Box::new(ZobristHashList::default()),
            move_scorer: MoveScorer::new(),
            pv: PvTable::new(),
            transposition_table: TranspositionTable::new(),
            analytics: SearchAnalytics::default(),
            time_control,
        }
    }

    /// Snapshot the root PV into a `PvList` for reporting. After the
    /// triangular table runs out (TT cutoffs leave it short), continue by
    /// replaying TT entries until we hit a miss, a missing best_move, an
    /// illegal move (key collision), a repeated position, or `MAX_PLY`.
    fn root_pv(&self, state: &GameState) -> PvList {
        let mut pv = PvList::default();

        let mut working_state = *state;
        let mut seen = ZobristHashList::default();
        seen.push(working_state.zobrist_hash);

        // Copy the in-search PV into the output and advance working_state
        // to its leaf so the TT-replay loop below picks up from there.
        for &mv in self.pv.root() {
            pv.push(mv);
            working_state.make_move(mv);
            if seen.len() < MAX_PLY {
                seen.push(working_state.zobrist_hash);
            }
        }

        while pv.len() < MAX_PLY {
            let Some(entry) = self.transposition_table.probe(working_state.zobrist_hash) else {
                break;
            };
            let Some(mv) = entry.best_move else { break };

            // Guard against TT key collisions: the stored move may belong to
            // a different position that happened to hash to the same slot.
            let mut moves = MoveList::default();
            working_state.generate_moves(&mut moves);
            if !moves.contains(&mv) {
                break;
            }

            working_state.make_move(mv);

            // A TT-pointed cycle would loop here forever — stop on revisit.
            if seen.contains(&working_state.zobrist_hash) {
                break;
            }

            pv.push(mv);
            if seen.len() < MAX_PLY {
                seen.push(working_state.zobrist_hash);
            }
        }

        pv
    }

    /// Detect repetition: returns true if `hash` matches any position
    /// in `position_history` within reach of the current reversible-move
    /// window.
    ///
    /// Convention: `position_history` contains the parent states leading
    /// to the current state; the current state's hash is NOT in the
    /// stack.
    ///
    /// `halfmove_clock` is a safe upper bound on lookback — captures,
    /// pawn moves, and castling all alter the zobrist hash in a way no
    /// prior position can match, so positions before any such move
    /// cannot be a repetition. (Castling doesn't reset `halfmove_clock`,
    /// so this bound is loose rather than tight, but that's harmless.)
    ///
    /// Only same-side-to-move positions can match, so we step back by 2.
    /// The minimum cycle length is 4 plies (each side moves and moves
    /// back), so the closest viable match is at index `n - 4`.
    #[inline]
    fn is_repetition(&self, hash: ZobristHash, halfmove_clock: u8) -> bool {
        let n = self.position_history.len();
        let reversible = halfmove_clock as usize;
        if reversible < 4 || n < 4 {
            return false;
        }
        // Earliest index reachable without crossing an irreversible move.
        let earliest = n.saturating_sub(reversible);
        (earliest..=n - 4)
            .rev()
            .step_by(2)
            .any(|i| self.position_history[i] == hash)
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

    /// Iterative deepening search: tries depths 1, 2, 3, ... until time/depth limit
    /// Returns the best move and evaluation found at the deepest completed depth
    pub fn search(
        &mut self,
        state: &mut GameState,
        max_depth: u8,
        report_fn: Option<impl Fn(SearchResult)>,
    ) -> SearchResult {
        // Clear analytics
        self.analytics = SearchAnalytics::default();

        // Clear killer moves
        self.move_scorer.clear_killers();

        self.pv.clear();

        // Reset position history: repetition detection only considers
        // positions visited within this search tree.
        self.position_history.clear();

        let mut best_eval = 0i32;
        let mut pv = PvList::default();

        self.time_control.set_search_deadline(state.side_to_move);
        for depth in 1..=max_depth {
            match self.alpha_beta(state, depth, 0, i32::MIN / 2, i32::MAX / 2) {
                Some((_, eval)) => {
                    best_eval = eval;
                    pv = self.root_pv(state);
                    self.analytics.depth = depth;
                }
                None => {
                    self.analytics.depth = depth - 1;
                }
            };

            // report info after each completed depth, if a reporting function is provided
            self.analytics.elapsed = self.time_control.get_elapsed();
            self.analytics.transposition_table_hashfull = self.transposition_table.hashfull();
            if let Some(f) = report_fn.as_ref() {
                f(SearchResult {
                    evaluation: best_eval,
                    analytics: self.analytics,
                    pv,
                })
            }

            // We ran out of time, so stop searching deeper
            if self.time_control.is_time_up() {
                break;
            }
        }

        self.time_control.clear_search_deadline();
        SearchResult {
            evaluation: best_eval,
            analytics: self.analytics,
            pv,
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
        // Reset this ply's PV. Early returns (draw, depth==0, TT cutoff) will
        // leave it empty; the parent then sees an empty child PV and truncates.
        self.pv.clear_ply(ply);

        // Draw detection: 50-move rule and repetition.
        // Only at ply > 0 so the root still produces a best move
        if ply > 0
            && (state.halfmove_clock >= 100
                || self.is_repetition(state.zobrist_hash, state.halfmove_clock))
        {
            return Some((None, 0));
        }

        if depth == 0 {
            return self
                .quiescence(state, ply, alpha, beta)
                .map(|eval| (None, eval));
        }

        self.analytics.nodes_searched += 1;

        // Timeout check
        if self
            .analytics
            .nodes_searched
            .is_multiple_of(NODE_CHECK_INTERVAL)
            && self.time_control.is_time_up()
        {
            return None;
        }

        // We store these so that we can store in the transposition table later.
        let original_alpha = alpha;
        let original_beta = beta;

        // Check if transposition table has a valid entry for this position and depth,
        // and use it to potentially cut off the search early
        let tt_entry = self.transposition_table.probe(state.zobrist_hash);
        if let Some(entry) = tt_entry.filter(|entry| entry.depth >= depth) {
            let tt_score = score::decode_tt_score(entry.score, ply);

            match entry.flag {
                TranspositionFlag::Exact => return Some((entry.best_move, tt_score)),
                TranspositionFlag::LowerBound => alpha = alpha.max(tt_score),
                TranspositionFlag::UpperBound => beta = beta.min(tt_score),
            }

            self.analytics.transposition_table_hits += 1;

            // If the tt entry causes a cutoff, we can skip searching this node entirely.
            if alpha >= beta {
                self.analytics.transposition_table_cuts += 1;
                return Some((entry.best_move, tt_score));
            }
        }

        // Generate and score moves into a stack-local buffer.
        let tt_move = tt_entry.and_then(|entry| entry.best_move);
        let mut buf = MoveScore::new();
        state.generate_moves(buf.moves_mut());
        buf.score_main(&self.move_scorer, state, tt_move, ply);

        let mut best_move = None;
        let mut best_eval = i32::MIN / 2;
        for (i, mv) in buf.ordered().enumerate() {
            // Principal Variation Search: full window on the first move, null-window
            // probe on the rest, re-search only when a probe falls inside (alpha, beta).
            let eval = if i == 0 {
                self.position_history.push(state.zobrist_hash);
                let undo = state.make_move(mv);
                let eval = self
                    .alpha_beta(state, depth - 1, ply + 1, -beta, -alpha)
                    .map(|(_, e)| -e);
                state.unmake_move(mv, &undo);
                self.position_history.pop();
                eval?
            } else {
                self.position_history.push(state.zobrist_hash);
                let undo = state.make_move(mv);
                let scout = self
                    .alpha_beta(state, depth - 1, ply + 1, -alpha - 1, -alpha)
                    .map(|(_, e)| -e);
                state.unmake_move(mv, &undo);
                self.position_history.pop();
                let scout = scout?;

                if scout > alpha && scout < beta {
                    self.analytics.pvs_re_searches += 1;
                    self.position_history.push(state.zobrist_hash);
                    let undo = state.make_move(mv);
                    let eval = self
                        .alpha_beta(state, depth - 1, ply + 1, -beta, -alpha)
                        .map(|(_, e)| -e);
                    state.unmake_move(mv, &undo);
                    self.position_history.pop();
                    eval?
                } else {
                    scout
                }
            };

            // min(-eval, -best_eval) = max(eval, best_eval)
            // So, this works for both players without needing to check which side is to move
            if eval > best_eval {
                best_eval = eval;
                best_move = Some(mv);
                self.pv.update(ply, mv);
            }

            // update alpha.
            // alpha-beta on negamax, unlike minimax doesn't require updating beta.
            alpha = alpha.max(eval);

            // This is the core alpha-beta cutoff.
            if alpha >= beta {
                self.analytics.alpha_beta_cutoffs += 1;
                if !mv.is_capture() && !mv.is_promotion() {
                    // Store killer moves (per ply) if it's a quiet move
                    // that cause a cutoff at this depth.
                    self.move_scorer.update_killer(mv, ply);

                    // History heuristic tracks quiet moves that lead to cutoffs regardless of depth.
                    // Even a shallow cutoff can indicate a move that is good in general.
                    self.move_scorer
                        .update_history(mv, depth, state.side_to_move);
                }
                break;
            }
        }

        // checkmate and stalemate detection.
        if buf.is_empty() {
            if state.is_in_check(state.side_to_move) {
                best_eval = -score::MATE_SCORE + (ply as i32);
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
            score: score::encode_tt_score(best_eval, ply),
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

        // Timeout check
        if self
            .analytics
            .quiescence_nodes_searched
            .is_multiple_of(QUIESCENCE_NODE_CHECK_INTERVAL)
            && self.time_control.is_time_up()
        {
            return None;
        }

        // Draw detection (50-move rule and repetition) — cheap and
        // doesn't need generated moves, so do it before movegen.
        // Quiescence is normally only captures (which reset halfmove_clock)
        // but check evasions can include quiet moves, so the check still matters.
        if state.halfmove_clock >= 100
            || self.is_repetition(state.zobrist_hash, state.halfmove_clock)
        {
            return Some(0);
        }

        // Generate moves so we can detect checkmate/stalemate before any
        // quiet-move filtering would hide them.
        let mut buf = MoveScore::new();
        state.generate_moves(buf.moves_mut());
        if buf.is_empty() {
            if state.is_in_check(state.side_to_move) {
                return Some(-score::MATE_SCORE + (ply as i32));
            } else {
                return Some(0);
            }
        }

        let in_check = state.is_in_check(state.side_to_move);

        // only filter captures/promotions if not in check
        // otherwise we might miss important evasions
        if !in_check {
            buf.moves_mut()
                .retain(|&mv| mv.is_capture() || mv.is_promotion());
        }

        // Move scoring
        buf.score_quiescence();

        // Stand-pat is only valid when not in check
        if !in_check {
            let stand_pat = evaluate(state);
            if stand_pat >= beta {
                self.analytics.quiescence_alpha_beta_cutoffs += 1;
                return Some(beta);
            }
            alpha = alpha.max(stand_pat);
        }

        for mv in buf.ordered() {
            self.position_history.push(state.zobrist_hash);
            let undo = state.make_move(mv);
            let eval = self.quiescence(state, ply + 1, -beta, -alpha).map(|e| -e);
            state.unmake_move(mv, &undo);
            self.position_history.pop();
            let eval = eval?;

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
                    .best_move()
                    .map_or("None".to_string(), |mv| mv.to_uci()),
                result.evaluation,
                result.analytics.total_nodes(),
                result.analytics.depth,
                result.analytics.max_quiescence_depth_reached,
                elapsed
            );
            if let Some(best_move) = result.best_move() {
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
            assert_eq!(mv.to_uci(), expected_moves[i]);
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
    fn test_search_returns_full_pv_for_mate_in_two() {
        let mut state =
            GameState::from_fen("5rk1/5ppp/2p5/1p6/1Q1p1P2/2Pq4/bP2R2P/rNK1R3 w - - 0 24").unwrap();
        let mut searcher = Searcher::new();
        let result = searcher.search(&mut state, 4, None::<fn(SearchResult)>);
        let pv: Vec<String> = result.pv.iter().map(|m| m.to_uci()).collect();
        assert_eq!(pv, vec!["b4f8", "g8f8", "e2e8"]);
        assert_eq!(
            result.best_move().map(|m| m.to_uci()),
            Some("b4f8".to_string())
        );
    }

    #[test]
    fn test_search_returns_full_pv_for_mate_in_three() {
        // Mate-in-3 (5 plies). At depth 5 the in-search PV may truncate at TT
        // cutoffs; TT-replay should extend it back to the full forced line.
        let mut state =
            GameState::from_fen("4k1r1/R6p/4Nb2/4n3/6Pq/2P4P/3Q3K/5R2 w - - 2 2").unwrap();
        let mut searcher = Searcher::new();
        let result = searcher.search(&mut state, 5, None::<fn(SearchResult)>);
        let pv: Vec<String> = result.pv.iter().map(|m| m.to_uci()).collect();
        assert_eq!(pv, vec!["d2d8", "f6d8", "f1f8", "g8f8", "e6g7"]);
    }

    #[test]
    fn test_search_mate_in_three() {
        let state = GameState::from_fen("4k1r1/R6p/4Nb2/4n3/6Pq/2P4P/3Q3K/5R2 w - - 2 2").unwrap();
        let best_moves_expected = ["d2d8", "f6d8", "f1f8", "g8f8", "e6g7"];
        assert_move_sequence(state, &best_moves_expected, 5);
    }

    fn play_moves(state: &mut GameState, uci_moves: &[&str]) -> Box<ZobristHashList> {
        let mut history = Box::new(ZobristHashList::default());
        for &uci in uci_moves {
            let mut moves = MoveList::default();
            state.generate_moves(&mut moves);
            let mv = moves
                .into_iter()
                .find(|m| m.to_uci() == uci)
                .unwrap_or_else(|| panic!("illegal move {uci}"));
            history.push(state.zobrist_hash);
            state.make_move(mv);
        }
        history
    }

    #[test]
    fn test_is_repetition_detects_match_at_minimum_cycle() {
        // Mirror knight moves out and back: position after 4 plies equals start.
        let mut state = GameState::new();
        let history = play_moves(&mut state, &["b1c3", "b8c6", "c3b1", "c6b8"]);
        let mut searcher = Searcher::new();
        searcher.position_history = history;
        assert!(searcher.is_repetition(state.zobrist_hash, state.halfmove_clock));
    }

    #[test]
    fn test_is_repetition_no_match() {
        // Four distinct development moves: current position is novel.
        let mut state = GameState::new();
        let history = play_moves(&mut state, &["e2e4", "e7e5", "g1f3", "g8f6"]);
        let mut searcher = Searcher::new();
        searcher.position_history = history;
        assert!(!searcher.is_repetition(state.zobrist_hash, 100));
    }

    #[test]
    fn test_is_repetition_too_few_entries() {
        // Less than 4 entries: a 4-ply cycle is impossible.
        let mut state = GameState::new();
        let history = play_moves(&mut state, &["b1c3", "b8c6", "c3b1"]);
        let mut searcher = Searcher::new();
        searcher.position_history = history;
        assert!(!searcher.is_repetition(state.zobrist_hash, 100));
    }

    #[test]
    fn test_is_repetition_short_circuits_below_4_halfmoves() {
        // A real 4-ply cycle is present, but halfmove_clock < 4 short-circuits.
        let mut state = GameState::new();
        let history = play_moves(&mut state, &["b1c3", "b8c6", "c3b1", "c6b8"]);
        let mut searcher = Searcher::new();
        searcher.position_history = history;
        assert!(!searcher.is_repetition(state.zobrist_hash, 3));
    }

    #[test]
    fn test_is_repetition_bounded_by_halfmove_clock() {
        // 8-ply cycle: both pairs of knights go out and come back.
        // No 2/4/6-ply prefix matches the starting position.
        let mut state = GameState::new();
        let history = play_moves(
            &mut state,
            &[
                "b1c3", "b8c6", "g1f3", "g8f6", "c3b1", "c6b8", "f3g1", "f6g8",
            ],
        );
        let mut searcher = Searcher::new();
        searcher.position_history = history;
        // halfmove_clock=4 only looks back 4 plies, missing the 8-ply match.
        assert!(!searcher.is_repetition(state.zobrist_hash, 4));
        // The real halfmove_clock (=8) reaches the match.
        assert!(searcher.is_repetition(state.zobrist_hash, state.halfmove_clock));
    }

    #[test]
    fn test_is_repetition_detects_longer_cycle() {
        // King triangulation: each side returns home after 3 moves (6 plies).
        // Knights can't return in 3 moves (color parity), so we use bare-king
        // positions — no castling rights or en passant to taint the hash.
        let mut state = GameState::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let history = play_moves(
            &mut state,
            &["e1d1", "e8d8", "d1e2", "d8e7", "e2e1", "e7e8"],
        );
        let mut searcher = Searcher::new();
        searcher.position_history = history;
        assert!(searcher.is_repetition(state.zobrist_hash, state.halfmove_clock));
    }

    #[test]
    fn test_search_fifty_move_rule_returns_zero() {
        // K+R vs K is normally winning for White, but with halfmove_clock=99
        // every legal first move (all non-capture, non-pawn) pushes the clock
        // to 100. At depth >= 1, every child node hits the 50-move draw check
        // and returns 0, which propagates back to the root.
        let mut state = GameState::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 99 50").unwrap();
        let mut searcher = Searcher::new();
        let result = searcher.search(&mut state, 3, None::<fn(SearchResult)>);
        assert_eq!(result.evaluation, 0);
    }

    #[test]
    fn test_search_finds_forced_repetition_draw() {
        // White is down significant material but the only drawing line is a forced
        // perpetual starting with a bishop sacrifice:
        let mut state =
            GameState::from_fen("1krqr3/p1p2n2/2Q3b1/p3n3/1b6/8/4BB2/5K2 w - - 0 1").unwrap();
        let mut searcher = Searcher::new();
        let result = searcher.search(&mut state, 7, None::<fn(SearchResult)>);
        assert_eq!(result.evaluation, 0);
        assert_eq!(
            result.best_move().map(|m| m.to_uci()),
            Some("f2a7".to_string())
        );
    }

    #[test]
    fn test_search_stalemate_returns_zero() {
        // Black to move with no legal moves and not in check: white queen on f7
        // covers g8/g7/h7, white king on f6 covers g5/g6/g7. Black king on h8
        // has no escape and is not attacked.
        let mut state = GameState::from_fen("7k/5Q2/5K2/8/8/8/8/8 b - - 0 1").unwrap();
        let mut searcher = Searcher::new();
        let result = searcher.search(&mut state, 1, None::<fn(SearchResult)>);
        assert_eq!(result.evaluation, 0);
    }

    /// Profiling target: a single fixed-depth search from the kiwipete
    /// position with no time control and no reporting. `#[ignore]` keeps
    /// it out of regular runs; invoke with
    /// `cargo test --release profile_search_startpos_depth_10 -- --ignored --nocapture`
    /// (or wrap with `samply` / `cargo flamegraph`).
    #[test]
    #[ignore]
    fn profile_search_startpos_depth_10() {
        let mut state = GameState::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1").unwrap();
        let mut searcher = Searcher::new();
        let start = std::time::Instant::now();
        let result = searcher.search(&mut state, 10, None::<fn(SearchResult)>);
        let elapsed = start.elapsed();
        let a = result.analytics;
        eprintln!(
            "depth={} time={:?} nodes={} qnodes={} nps={}",
            a.depth,
            elapsed,
            a.nodes_searched,
            a.quiescence_nodes_searched,
            a.nodes_per_second(),
        );
    }
}
