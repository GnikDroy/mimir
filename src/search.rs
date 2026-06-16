use std::time::Duration;

use crate::analytics::SearchAnalytics;
use crate::core::*;
use crate::late_move_reduction_table::reduction;
use crate::move_score::MoveScore;
use crate::move_scorer::MoveScorer;
use crate::nnue::evaluate;
use crate::pv_table::PvTable;
use crate::score;
use crate::stack_vec::StackVec;
use crate::state::GameState;
use crate::time_control::TimeControl;
use crate::transposition_table::{TranspositionEntry, TranspositionFlag, TranspositionTable};
use crate::uci::options::EngineOptions;
use crate::zobrist::ZobristHash;

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
        Self::with_options(EngineOptions::default())
    }

    /// Builds a [`Searcher`] whose transposition table and time control
    /// reflect `options`. Per-search state reset on every
    /// [`search`](Self::search) call.
    pub fn with_options(options: EngineOptions) -> Self {
        Searcher {
            position_history: Box::new(ZobristHashList::default()),
            move_scorer: MoveScorer::new(),
            pv: PvTable::new(),
            transposition_table: TranspositionTable::with_size_mb(options.hash_mb),
            analytics: SearchAnalytics::default(),
            time_control: TimeControl::with_overhead(options.move_overhead),
        }
    }

    /// Replaces the transposition table with one sized to `mb` mebibytes,
    /// discarding existing entries.
    pub fn resize_tt(&mut self, mb: usize) {
        self.transposition_table.resize(mb);
    }

    /// Updates the time-control safety buffer.
    pub fn set_move_overhead(&mut self, overhead: Duration) {
        self.time_control.set_move_overhead(overhead);
    }

    /// Clears the transposition table without reallocating.
    pub fn clear_tt(&mut self) {
        self.transposition_table.clear();
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
        wtime: Option<Duration>,
        btime: Option<Duration>,
        winc: Option<Duration>,
        binc: Option<Duration>,
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
            match self.alpha_beta(state, depth, 0, i32::MIN / 2, i32::MAX / 2, true) {
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

            // Soft cutoff: refuse to start the next iteration once we
            // cross the soft deadline. The next iteration would almost
            // certainly abort against the hard deadline and produce no
            // usable result, so its CPU spend is pure waste.
            if self.time_control.is_soft_time_up() {
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
        do_null: bool,
    ) -> Option<(Option<Move>, i32)> {
        // Reset this ply's PV. Early returns (draw, depth==0, TT cutoff) will
        // leave it empty; the parent then sees an empty child PV and truncates.
        self.pv.clear_ply(ply);

        self.analytics.max_ply_reached = self.analytics.max_ply_reached.max(ply as u8);

        // Draw detection: 50-move rule and repetition.
        // Only at ply > 0 so the root still produces a best move
        if ply > 0
            && (state.halfmove_clock >= 100
                || self.is_repetition(state.zobrist_hash, state.halfmove_clock))
        {
            return Some((None, 0));
        }

        // Check extension: spend one extra ply when the side to move is in check.
        // Applied before quiescence so we never drop into quiescence while in check.
        let in_check = state.is_in_check(state.side_to_move);
        let depth = depth + in_check as u8;

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
            && self.time_control.is_hard_time_up()
        {
            return None;
        }

        // We store these so that we can store in the transposition table later.
        let original_alpha = alpha;
        let original_beta = beta;

        // Check if transposition table has a valid entry for this position and depth,
        // and use it to potentially cut off the search early
        let tt_entry = self.transposition_table.probe(state.zobrist_hash);
        if tt_entry.is_some() {
            self.analytics.transposition_table_entries_found += 1;
        }
        if let Some(entry) = tt_entry.filter(|entry| entry.depth >= depth) {
            let tt_score = score::decode_tt_score(entry.score, ply);

            self.analytics.transposition_table_hits += 1;

            match entry.flag {
                TranspositionFlag::Exact => {
                    self.analytics.transposition_table_cuts += 1;
                    return Some((entry.best_move, tt_score));
                }
                TranspositionFlag::LowerBound => alpha = alpha.max(tt_score),
                TranspositionFlag::UpperBound => beta = beta.min(tt_score),
            }

            // If the bound update causes a cutoff, skip searching this node.
            if alpha >= beta {
                self.analytics.transposition_table_cuts += 1;
                return Some((entry.best_move, tt_score));
            }
        }

        // Static eval feeds the NMP eval guard below. Skip when in check:
        // NMP refuses to fire there, and the eval is meaningless while the
        // king is under attack.
        let static_eval = if !in_check { Some(evaluate(state)) } else { None };

        // Null move pruning: pass the turn and search at reduced depth with
        // a null window around beta. If the opponent still can't beat us
        // after a free tempo, our position is so good we can cut without
        // searching real moves.
        //
        // Guards:
        // - `ply > 0`: root must produce a best move.
        // - `do_null`: prevents consecutive null moves (a double pass gives
        //   the opponent two tempi and breaks soundness).
        // - `!in_check`: a null while in check is an illegal pass.
        // - `depth >= NMP_MIN_DEPTH`: at very low depth the reduced search
        //   would collapse into qsearch, defeating the point.
        // - `position_suitable_for_null_move`: zugzwang guard for king-and-pawn endings.
        // - `beta` not a mate score: mate-bound comparisons are meaningless.
        // - `static_eval >= beta`: if we're already losing on static eval,
        //   passing the move is very unlikely to fail high.
        const NMP_MIN_DEPTH: u8 = 3;

        // Historically 2 gives good reduction, but can do adaptive reduction later.
        const NMP_REDUCTION: u8 = 2;
        if ply > 0
            && do_null
            && !in_check
            && depth >= NMP_MIN_DEPTH
            && state.position_suitable_for_null_move()
            && score::mate_in_plies(beta).is_none()
            && static_eval.is_some_and(|e| e >= beta)
        {
            self.analytics.null_move_attempts += 1;
            let undo = state.make_null_move();
            let null_score = self
                .alpha_beta(
                    state,
                    depth - 1 - NMP_REDUCTION,
                    ply + 1,
                    -beta,
                    -beta + 1,
                    false,
                )
                .map(|(_, e)| -e);
            state.unmake_null_move(&undo);
            let null_score = null_score?;

            if null_score >= beta {
                self.analytics.null_move_cutoffs += 1;
                // Don't return mate scores from NMP — we never proved a real
                // mate exists, only that passing didn't lose.
                let clamped = if score::mate_in_plies(null_score).is_some() {
                    beta
                } else {
                    null_score
                };
                return Some((None, clamped));
            }
        }

        // Generate and score moves into a stack-local buffer.
        let tt_move = tt_entry.and_then(|entry| entry.best_move);
        let mut buf = MoveScore::new();
        state.generate_moves(buf.moves_mut());
        buf.score_main(&self.move_scorer, state, tt_move, ply);

        // Snapshot the move-ordering signal that depends on the buffer's
        // contents but not its order — pull it before `ordered()` borrows
        // `buf` mutably.
        let history_top = self.move_scorer.find_history_top(state.side_to_move, &buf);
        let mut history_top_tried = false;

        let mut best_move = None;
        let mut best_eval = i32::MIN / 2;
        for (i, mv) in buf.ordered().enumerate() {
            // Track which ordering signal this move corresponds to.
            if Some(mv) == tt_move {
                self.analytics.tt_move_tried += 1;
            }
            if self.move_scorer.is_killer(mv, ply) {
                self.analytics.killer_move_tried += 1;
            }
            if Some(mv) == history_top {
                history_top_tried = true;
            }

            // Principal Variation Search: full window on the first move, null-window
            // probe on the rest, re-search only when a probe falls inside (alpha, beta).
            let eval = if i == 0 {
                self.position_history.push(state.zobrist_hash);
                let undo = state.make_move(mv);
                let eval = self
                    .alpha_beta(state, depth - 1, ply + 1, -beta, -alpha, true)
                    .map(|(_, e)| -e);
                state.unmake_move(mv, &undo);
                self.position_history.pop();
                eval?
            } else {
                self.analytics.pvs_scouts += 1;
                // A scout taken when beta == alpha + 1 cannot ever
                // re-search (the math forbids it)
                if beta > alpha + 1 {
                    self.analytics.pvs_open_window_scouts += 1;
                }

                // Late Move Reductions: search late quiet moves at reduced
                // depth. The reduced scout's job is to cheaply confirm the
                // move isn't better than alpha; if it fails high, we
                // re-search at full depth to verify.
                const LMR_MIN_DEPTH: u8 = 3;
                const LMR_MIN_MOVES: usize = 3;
                let can_reduce = depth >= LMR_MIN_DEPTH
                    && i >= LMR_MIN_MOVES
                    && !in_check
                    && !mv.is_capture()
                    && !mv.is_promotion()
                    && !self.move_scorer.is_killer(mv, ply);
                let r = if can_reduce { reduction(depth, i) } else { 0 };
                let reduced_depth = (depth - 1).saturating_sub(r);
                if r > 0 {
                    self.analytics.lmr_attempts += 1;
                }

                // Stage 1: null-window scout (reduced depth if LMR fired).
                self.position_history.push(state.zobrist_hash);
                let undo = state.make_move(mv);
                let scout = self
                    .alpha_beta(state, reduced_depth, ply + 1, -alpha - 1, -alpha, true)
                    .map(|(_, e)| -e);
                state.unmake_move(mv, &undo);
                self.position_history.pop();
                let mut score = scout?;

                // Stage 2: if LMR was applied and the reduced scout failed
                // high, verify at full depth (still null window). Reduced
                // searches can't be trusted for cutoffs without this re-check.
                if r > 0 && score > alpha {
                    self.analytics.lmr_re_searches_depth += 1;
                    self.position_history.push(state.zobrist_hash);
                    let undo = state.make_move(mv);
                    let s = self
                        .alpha_beta(state, depth - 1, ply + 1, -alpha - 1, -alpha, true)
                        .map(|(_, e)| -e);
                    state.unmake_move(mv, &undo);
                    self.position_history.pop();
                    score = s?;
                } else if r > 0 {
                    self.analytics.lmr_successes += 1;
                }

                // Stage 3: PVS open-window re-search when the score lands
                // inside (alpha, beta) — only possible at PV nodes.
                if score > alpha && score < beta {
                    self.analytics.pvs_re_searches += 1;
                    self.position_history.push(state.zobrist_hash);
                    let undo = state.make_move(mv);
                    let s = self
                        .alpha_beta(state, depth - 1, ply + 1, -beta, -alpha, true)
                        .map(|(_, e)| -e);
                    state.unmake_move(mv, &undo);
                    self.position_history.pop();
                    score = s?;
                }

                score
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
                if Some(mv) == tt_move {
                    self.analytics.tt_move_cutoffs += 1;
                }
                if self.move_scorer.is_killer(mv, ply) {
                    self.analytics.killer_move_cutoffs += 1;
                }
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

        // Resolve history-top stats once the move loop ends. Only count
        // the node if a top-history move existed (i.e. at least one quiet
        // move had nonzero history); otherwise the sample is meaningless.
        if history_top.is_some() {
            if history_top_tried {
                self.analytics.history_top_tried += 1;
            }
            if best_move == history_top {
                self.analytics.history_top_best += 1;
            }
        }

        // checkmate and stalemate detection.
        if buf.is_empty() {
            if state.is_in_check(state.side_to_move) {
                best_eval = score::mated(ply);
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
        self.analytics.max_ply_reached = self.analytics.max_ply_reached.max(ply as u8);
        self.analytics.quiescence_nodes_searched += 1;

        // Timeout check
        if self
            .analytics
            .quiescence_nodes_searched
            .is_multiple_of(QUIESCENCE_NODE_CHECK_INTERVAL)
            && self.time_control.is_hard_time_up()
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
                return Some(score::mated(ply));
            } else {
                return Some(0);
            }
        }

        let in_check = state.is_in_check(state.side_to_move);

        // If not in check, keep only captures and promotions
        if !in_check {
            buf.moves_mut()
                .retain(|&mv| mv.is_capture() || mv.is_promotion());
        }

        // Stand-pat is only valid when not in check
        if !in_check {
            let stand_pat = evaluate(state);
            if stand_pat >= beta {
                self.analytics.quiescence_stand_pat_cutoffs += 1;
                return Some(beta);
            }
            alpha = alpha.max(stand_pat);
        }

        // Move scoring
        buf.score_quiescence();

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
                "Best Move: {}, Eval: {}, Nodes: {}, Max Depth: {}, Deepest Ply: {}, Time: {:?}",
                result
                    .best_move()
                    .map_or("None".to_string(), |mv| mv.to_uci()),
                result.evaluation,
                result.analytics.total_nodes(),
                result.analytics.depth,
                result.analytics.max_ply_reached,
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

    fn assert_best_move_sequence(mut state: GameState, expected_moves: &[&str], search_depth: u8) {
        let best_moves = get_best_moves_till_limit(&mut state, search_depth, expected_moves.len());
        assert_move_sequence(&best_moves, expected_moves);
    }

    fn assert_move_sequence(best_moves: &[Move], expected_moves: &[&str]) {
        assert_eq!(best_moves.len(), expected_moves.len());
        for (i, mv) in best_moves.iter().enumerate() {
            if expected_moves[i] != "-" {
                assert_eq!(mv.to_uci(), expected_moves[i]);
            }
        }
    }

    #[test]
    fn test_search_mate_in_one() {
        let state = GameState::from_fen("3r4/1K6/2Nb4/2kb4/8/8/3PB3/8 w - - 0 1").unwrap();
        let best_moves_expected = ["d2d4"];
        assert_best_move_sequence(state, &best_moves_expected, 4);
    }

    #[test]
    fn test_search_mate_in_two() {
        let state =
            GameState::from_fen("5rk1/5ppp/2p5/1p6/1Q1p1P2/2Pq4/bP2R2P/rNK1R3 w - - 0 24").unwrap();
        let best_moves_expected = ["b4f8", "g8f8", "e2e8"];
        assert_best_move_sequence(state, &best_moves_expected, 4);
    }

    #[test]
    fn test_search_mate_in_three() {
        let state = GameState::from_fen("4k1r1/R6p/4Nb2/4n3/6Pq/2P4P/3Q3K/5R2 w - - 2 2").unwrap();
        let best_moves_expected = ["d2d8", "f6d8", "f1f8", "g8f8", "e6g7"];
        assert_best_move_sequence(state, &best_moves_expected, 5);
    }

    #[test]
    fn test_search_mate_in_four() {
        let state =
            GameState::from_fen("3qr2k/1p3rbp/2p3p1/p7/P2pBNn1/1P3n2/6P1/B1Q1RR1K b - - 1 30")
                .unwrap();
        let best_moves_expected = ["d8h4", "f4h3", "h4g3", "c1f4", "f7f4", "-", "g3h2"];
        assert_best_move_sequence(state, &best_moves_expected, 7);
    }

    #[test]
    fn test_search_returns_full_pv_for_mate_in_two() {
        let mut state =
            GameState::from_fen("5rk1/5ppp/2p5/1p6/1Q1p1P2/2Pq4/bP2R2P/rNK1R3 w - - 0 24").unwrap();
        let mut searcher = Searcher::new();
        let result = searcher.search(&mut state, 4, None::<fn(SearchResult)>);
        let best_moves: Vec<Move> = result.pv.iter().copied().collect();
        assert_move_sequence(&best_moves, &["b4f8", "g8f8", "e2e8"]);
    }

    #[test]
    fn test_search_returns_full_pv_for_mate_in_three() {
        let mut state =
            GameState::from_fen("4k1r1/R6p/4Nb2/4n3/6Pq/2P4P/3Q3K/5R2 w - - 2 2").unwrap();
        let mut searcher = Searcher::new();
        let result = searcher.search(&mut state, 5, None::<fn(SearchResult)>);
        let best_moves: Vec<Move> = result.pv.iter().copied().collect();
        assert_move_sequence(&best_moves, &["d2d8", "f6d8", "f1f8", "g8f8", "e6g7"]);
    }

    #[test]
    fn test_search_returns_full_pv_for_mate_in_four() {
        let mut state =
            GameState::from_fen("3qr2k/1p3rbp/2p3p1/p7/P2pBNn1/1P3n2/6P1/B1Q1RR1K b - - 1 30")
                .unwrap();
        let mut searcher = Searcher::new();
        let result = searcher.search(&mut state, 7, None::<fn(SearchResult)>);
        let best_moves: Vec<Move> = result.pv.iter().copied().collect();
        let best_moves_expected = ["d8h4", "f4h3", "h4g3", "c1f4", "f7f4", "-", "g3h2"];
        assert_move_sequence(&best_moves, &best_moves_expected);
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
        let result = searcher.search(&mut state, 9, None::<fn(SearchResult)>);
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
}
