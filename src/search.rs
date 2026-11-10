use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::analytics::SearchAnalytics;
use crate::core::*;
use crate::late_move_reduction_table::reduction;
use crate::move_score::MoveScore;
use crate::move_scorer::{MoveScorer, PIECE_VALUES};
use crate::nnue::{evaluate, Cp};
use crate::pv_table::PvTable;
use crate::score;
use crate::search_status::SearchStatus;
use crate::see::see_ge;
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

/// SearchLimits passed into [`Searcher::search`] per call.
/// Unset fields mean no bound of that kind.
#[derive(Debug, Clone, Copy, Default)]
pub struct SearchLimits {
    /// Maximum iterative-deepening depth (`go depth N`).
    pub depth: Option<u8>,
    /// Cap on total nodes searched (`go nodes N`).
    pub nodes: Option<u64>,
    /// White's remaining clock time (`go wtime N`).
    pub wtime: Option<Duration>,
    /// Black's remaining clock time (`go btime N`).
    pub btime: Option<Duration>,
    /// White's increment per move (`go winc N`).
    pub winc: Option<Duration>,
    /// Black's increment per move (`go binc N`).
    pub binc: Option<Duration>,
    /// Moves remaining until the next time control (`go movestogo N`).
    pub movestogo: Option<u32>,
    /// Max time spent on this move (`go movetime N`).
    pub movetime: Option<Duration>,
}

impl SearchLimits {
    /// Limits for a fixed-depth search with no other bounds.
    pub fn to_depth(depth: u8) -> Self {
        Self {
            depth: Some(depth),
            ..Self::default()
        }
    }

    /// Limits for a node-capped search with no other bounds.
    pub fn to_nodes(nodes: u64) -> Self {
        Self {
            nodes: Some(nodes),
            ..Self::default()
        }
    }
}

/// Only last 100 game positions matter for repetition
/// due to irreversibility enforced at 50th move.
pub const MAX_GAME_HISTORY: usize = 100;
pub const HISTORY_CAP: usize = MAX_PLY + MAX_GAME_HISTORY;

pub type ZobristHashList = StackVec<ZobristHash, HISTORY_CAP>;
pub type PvList = StackVec<Move, MAX_PLY>;

pub struct Searcher {
    position_history: Box<ZobristHashList>,
    transposition_table: TranspositionTable,
    time_control: TimeControl,
    analytics: SearchAnalytics,
    move_scorer: MoveScorer,
    pv: PvTable<MAX_PLY>,
    /// External abort signal. The Searcher only reads it. The owner
    /// (e.g. the UCI adapter) flips it from another thread to interrupt
    /// the in-flight search.
    stop: Arc<AtomicBool>,
    limits: SearchLimits,
}

pub const MAX_PLY: usize = 128;

// Check for timeouts every N nodes in alpha-beta and every M quiescence nodes.
const NODE_CHECK_INTERVAL: u64 = 256;
const QUIESCENCE_NODE_CHECK_INTERVAL: u64 = 128;

impl Searcher {
    /// Builds a [`Searcher`] that consults `stop` between node-interval
    /// checks. The caller owns the flag. The Searcher only reads it.
    /// Flipping `stop` to `true` from another thread asks the in-flight
    /// search to return [`SearchStatus::Stopped`] at the next check.
    pub fn new(options: EngineOptions, stop: Arc<AtomicBool>) -> Self {
        Searcher {
            position_history: Box::new(ZobristHashList::default()),
            move_scorer: MoveScorer::new(),
            pv: PvTable::new(),
            transposition_table: TranspositionTable::with_size_mb(options.hash_mb),
            analytics: SearchAnalytics::default(),
            time_control: TimeControl::with_overhead(options.move_overhead),
            stop,
            limits: SearchLimits::default(),
        }
    }

    /// True once the node cap has been hit.
    fn is_node_limit_reached(&self) -> bool {
        self.limits
            .nodes
            .is_some_and(|limit| self.analytics.total_nodes() >= limit)
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

    /// Reset state for fresh game.
    pub fn new_game(&mut self) {
        self.transposition_table.clear();
        self.move_scorer.clear_history();
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

            // TT cycle would cause it to loop forever.
            // Stop on cycle or if we have 3 fold repetition from previous hashes.
            if seen.contains(&working_state.zobrist_hash)
                || self.is_repetition(working_state.zobrist_hash, working_state.halfmove_clock)
            {
                break;
            }

            pv.push(mv);
            if seen.len() < MAX_PLY {
                seen.push(working_state.zobrist_hash);
            }
        }

        pv
    }

    /// Detect repetition: true if `hash` matches any position in
    /// `position_history` within the current reversible-move window.
    ///
    /// Convention: the stack holds the positions leading to the current
    /// state. The current state's hash is NOT in it.
    ///
    /// `halfmove_clock` bounds the lookback: no position from before the
    /// last capture or pawn move can repeat. The bound is loose (castling
    /// and e.p. loss don't reset the clock) but that is harmless.
    ///
    /// Only same-side-to-move positions can match, so step back by 2.
    /// The shortest cycle is 4 plies, so start at index `n - 4`.
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

    /// Iterative deepening search: tries depths 1, 2, 3, ... until a
    /// limit from `limits` fires (depth, nodes, or clock deadline).
    /// game_history holds hashes of positions played before
    /// Returns the best move and evaluation found at the deepest completed depth.
    pub fn search(
        &mut self,
        state: &mut GameState,
        limits: SearchLimits,
        game_history: &[ZobristHash],
        report_fn: Option<impl Fn(SearchResult)>,
    ) -> SearchResult {
        self.limits = limits;
        self.time_control.update_clock(
            limits.wtime,
            limits.btime,
            limits.winc,
            limits.binc,
            limits.movestogo,
            limits.movetime,
        );
        let max_depth = limits.depth.unwrap_or(MAX_PLY as u8);

        // No need to search if checkmate or stalemate.
        let is_drawn = state.halfmove_clock >= 100
            || state.is_draw_by_insufficient_material()
            || state.is_stalemate();
        if state.is_checkmate() || is_drawn {
            return SearchResult {
                evaluation: 0,
                analytics: SearchAnalytics::default(),
                pv: PvList::default(),
            };
        }

        // Clear analytics
        self.analytics = SearchAnalytics::default();

        // Clear killer moves
        self.move_scorer.clear_killers();

        self.pv.clear();

        // Seed the repetition stack with the game's past positions.
        self.position_history.clear();
        let keep = game_history.len().min(MAX_GAME_HISTORY);
        for &hash in &game_history[game_history.len() - keep..] {
            self.position_history.push(hash);
        }

        let mut best_eval = 0i32;
        let mut pv = PvList::default();

        self.time_control.set_search_deadline(state.side_to_move);
        for depth in 1..=max_depth {
            let status = self.aspiration_search(state, depth, best_eval);
            let aborted = !matches!(status, SearchStatus::Complete(_));
            match status {
                SearchStatus::Complete((_, eval)) => {
                    best_eval = eval;
                    pv = self.root_pv(state);
                    self.analytics.depth = depth;
                }
                SearchStatus::Stopped | SearchStatus::TimedOut => {
                    self.analytics.depth = depth - 1;
                }
            }

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

            // Bail on abort: a Stopped/TimedOut search can't produce a
            // usable deeper result, and without a soft deadline (e.g.
            // `go infinite`) the loop below wouldn't otherwise terminate.
            if aborted {
                break;
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

    /// Aspiration window wrapper around the root alpha-beta call.
    ///
    /// At sufficient depth we expect the new score to land near the
    /// previous iteration's score, so we search a narrow window centered
    /// on `prev_eval`. A successful search returns the score directly;
    /// fail-low / fail-high re-searches widen only the failing bound,
    /// doubling `delta` each time, and fall back to a full window once
    /// `delta` exceeds [`ASPIRATION_MAX_DELTA`].
    ///
    /// Skipped at low depth (eval is too noisy) and when `prev_eval` is
    /// a mate score (mate-bound windows are meaningless).
    fn aspiration_search(
        &mut self,
        state: &mut GameState,
        depth: u8,
        prev_eval: i32,
    ) -> SearchStatus<(Option<Move>, i32)> {
        const FULL_ALPHA: i32 = i32::MIN / 2;
        const FULL_BETA: i32 = i32::MAX / 2;
        const ASPIRATION_MIN_DEPTH: u8 = 4;
        const ASPIRATION_INITIAL_DELTA: i32 =
            Cp(PIECE_VALUES[Piece::Pawn as usize]).to_internal() / 6;
        const ASPIRATION_MAX_DELTA: i32 = Cp(1000).to_internal();

        if depth < ASPIRATION_MIN_DEPTH || score::mate_in_plies(prev_eval).is_some() {
            return self.alpha_beta(state, depth, 0, FULL_ALPHA, FULL_BETA, true);
        }

        self.analytics.aspiration_attempts += 1;
        let mut delta = ASPIRATION_INITIAL_DELTA;
        let mut alpha = prev_eval - delta;
        let mut beta = prev_eval + delta;

        loop {
            let (best_move, eval) = self.alpha_beta(state, depth, 0, alpha, beta, true)?;

            if eval <= alpha {
                self.analytics.aspiration_fail_low += 1;
                delta = delta.saturating_mul(2);
                alpha = if delta > ASPIRATION_MAX_DELTA {
                    FULL_ALPHA
                } else {
                    prev_eval - delta
                };
            } else if eval >= beta {
                self.analytics.aspiration_fail_high += 1;
                delta = delta.saturating_mul(2);
                beta = if delta > ASPIRATION_MAX_DELTA {
                    FULL_BETA
                } else {
                    prev_eval + delta
                };
            } else {
                return SearchStatus::Complete((best_move, eval));
            }
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
    ) -> SearchStatus<(Option<Move>, i32)> {
        // Reset this ply's PV. Early returns (draw, depth==0, TT cutoff) will
        // leave it empty. The parent then sees an empty child PV and truncates.
        self.pv.clear_ply(ply);

        self.analytics.max_ply_reached = self.analytics.max_ply_reached.max(ply as u8);

        // Draw detection: 50-move rule and repetition.
        // Only at ply > 0 so the root still produces a best move
        if ply > 0
            && (state.halfmove_clock >= 100
                || state.is_draw_by_insufficient_material()
                || self.is_repetition(state.zobrist_hash, state.halfmove_clock))
        {
            return SearchStatus::Complete((None, 0));
        }

        // Max ply reached
        if ply >= MAX_PLY - 1 {
            return SearchStatus::Complete((None, evaluate(state)));
        }

        // Mate distance pruning
        if ply > 0 {
            alpha = alpha.max(score::mated(ply));
            beta = beta.min(score::mate(ply + 1));
            if alpha >= beta {
                return SearchStatus::Complete((None, alpha));
            }
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

        // Abort check: external stop signal takes precedence over time-up
        // so we report the cause faithfully.
        if self
            .analytics
            .nodes_searched
            .is_multiple_of(NODE_CHECK_INTERVAL)
        {
            if self.stop.load(Ordering::Relaxed) {
                return SearchStatus::Stopped;
            }
            if self.time_control.is_hard_time_up() {
                return SearchStatus::TimedOut;
            }
            if self.is_node_limit_reached() {
                return SearchStatus::TimedOut;
            }
        }

        // We store these so that we can store in the transposition table later.
        let original_alpha = alpha;
        let original_beta = beta;

        // Check if transposition table has a valid entry for this position and depth,
        // and use it to potentially cut off the search early
        let is_pv = beta > alpha + 1;
        let tt_entry = self.transposition_table.probe(state.zobrist_hash);
        if tt_entry.is_some() {
            self.analytics.transposition_table_entries_found += 1;
        }
        if let Some(entry) = tt_entry.filter(|entry| entry.depth >= depth) {
            let tt_score = score::decode_tt_score(entry.score, ply);

            self.analytics.transposition_table_hits += 1;

            // Skip TT cuts at PV nodes so an Exact/bound entry doesn't
            // return before the PV row at this ply is populated. That
            // would truncate the reported PV. The entry's best_move is
            // still consumed below for move ordering.
            if !is_pv {
                match entry.flag {
                    TranspositionFlag::Exact => {
                        self.analytics.transposition_table_cuts += 1;
                        return SearchStatus::Complete((entry.best_move, tt_score));
                    }
                    TranspositionFlag::LowerBound => alpha = alpha.max(tt_score),
                    TranspositionFlag::UpperBound => beta = beta.min(tt_score),
                }

                // If the bound update causes a cutoff, skip searching this node.
                if alpha >= beta {
                    self.analytics.transposition_table_cuts += 1;
                    return SearchStatus::Complete((entry.best_move, tt_score));
                }
            }
        }

        // Static eval feeds the NMP eval guard below. Skip when in check:
        // NMP refuses to fire there, and the eval is meaningless while the
        // king is under attack.
        let static_eval = if !in_check {
            Some(evaluate(state))
        } else {
            None
        };

        // Reverse futility pruning (a.k.a. static null move pruning): at
        // shallow depth in non-PV nodes, if the static eval already exceeds
        // beta by a depth-scaled margin, we assume no reasonable move drops
        // us below beta and cut without searching.
        //
        // Guards:
        // - `ply > 0`: root must produce a best move.
        // - `!is_pv`: PV nodes need a real score. The margin heuristic is unsound there.
        // - `!in_check`: static eval is meaningless while the king is under attack.
        // - `depth <= RFP_MAX_DEPTH`: deeper nodes are too unstable for a static cutoff.
        // - `beta` not a mate score: mate-bound comparisons are meaningless.
        const RFP_MAX_DEPTH: u8 = 5;
        const RFP_MARGIN_PER_DEPTH: i32 = Cp(100).to_internal();
        if ply > 0
            && !is_pv
            && !in_check
            && depth <= RFP_MAX_DEPTH
            && score::mate_in_plies(beta).is_none()
        {
            if let Some(eval) = static_eval {
                self.analytics.rfp_attempts += 1;
                let margin = RFP_MARGIN_PER_DEPTH * depth as i32;
                if eval - margin >= beta {
                    self.analytics.rfp_cutoffs += 1;
                    return SearchStatus::Complete((None, eval - margin));
                }
            }
        }

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

        if ply > 0
            && do_null
            && !in_check
            && depth >= NMP_MIN_DEPTH
            && state.position_suitable_for_null_move()
            && score::mate_in_plies(beta).is_none()
            && static_eval.is_some_and(|e| e >= beta)
        {
            self.analytics.null_move_attempts += 1;

            // Adaptive NMP reduction
            const NMP_BASE_REDUCTION: u8 = 3;
            const NMP_DEPTH_DIVISOR: u8 = 4;
            let reduction = NMP_BASE_REDUCTION + depth / NMP_DEPTH_DIVISOR;
            let null_depth = depth.saturating_sub(1 + reduction);

            let undo = state.make_null_move();
            let null_status = self
                .alpha_beta(state, null_depth, ply + 1, -beta, -beta + 1, false)
                .map(|(_, e)| -e);
            state.unmake_null_move(&undo);
            let null_score = null_status?;

            if null_score >= beta {
                self.analytics.null_move_cutoffs += 1;
                // Don't return mate scores from NMP. We didn't prove a real
                // mate exists, only that passing didn't lose.
                let clamped = if score::mate_in_plies(null_score).is_some() {
                    beta
                } else {
                    null_score
                };
                return SearchStatus::Complete((None, clamped));
            }
        }

        // Generate and score moves into a stack-local buffer.
        let tt_move = tt_entry.and_then(|entry| entry.best_move);
        let mut buf = MoveScore::new();
        state.generate_moves(buf.moves_mut());
        buf.score_main(&self.move_scorer, state, tt_move, ply);

        // Snapshot the move-ordering signal that depends on the buffer's
        // contents but not its order, pull it before `ordered()` borrows
        // `buf` mutably.
        let history_top = self.move_scorer.find_history_top(state.side_to_move, &buf);
        let mut history_top_tried = false;

        // Quiet moves searched for history malus.
        let mut tried_quiets: StackVec<Move, MAX_MOVE_COUNT> = StackVec::default();

        let mut best_move = None;
        let mut best_eval = i32::MIN / 2;
        for (i, mv) in buf.ordered().enumerate() {
            // Futility pruning. At shallow non-PV nodes, skip a quiet move
            // when the static eval is below than alpha such that the move
            // can't realistically raise it. First move is always searched.
            const FUTILITY_MAX_DEPTH: u8 = 6;
            const FUTILITY_MARGIN_PER_DEPTH: i32 = Cp(120).to_internal();
            if !is_pv
                && !in_check
                && i > 0
                && depth <= FUTILITY_MAX_DEPTH
                && !mv.is_capture()
                && !mv.is_promotion()
                && score::mate_in_plies(alpha).is_none()
                && static_eval
                    .is_some_and(|se| se + FUTILITY_MARGIN_PER_DEPTH * depth as i32 <= alpha)
            {
                continue;
            }

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
                // move isn't better than alpha. If it fails high, we
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
                // inside (alpha, beta), only possible at PV nodes.
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
                        .reward_history(mv, depth, state.side_to_move);

                    // Penalize using history malus.
                    for &quiet in tried_quiets.iter() {
                        self.move_scorer
                            .penalize_history(quiet, depth, state.side_to_move);
                    }
                }
                break;
            }

            if !mv.is_capture() && !mv.is_promotion() {
                tried_quiets.push(mv);
            }
        }

        // Resolve history-top stats once the move loop ends. Only count
        // the node if a top-history move existed (i.e. at least one quiet
        // move had nonzero history). Otherwise the sample is meaningless.
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

        let stored_move = if best_eval > original_alpha {
            best_move
        } else {
            None
        };

        self.transposition_table.store(TranspositionEntry {
            key: state.zobrist_hash,
            depth,
            score: score::encode_tt_score(best_eval, ply),
            flag,
            best_move: stored_move,
        });

        SearchStatus::Complete((best_move, best_eval))
    }

    /// Quiescence search: search captures until a quiet position is reached
    fn quiescence(
        &mut self,
        state: &mut GameState,
        ply: usize,
        mut alpha: i32,
        beta: i32,
    ) -> SearchStatus<i32> {
        self.analytics.max_ply_reached = self.analytics.max_ply_reached.max(ply as u8);
        self.analytics.quiescence_nodes_searched += 1;

        // Max ply reached
        if ply >= MAX_PLY - 1 {
            return SearchStatus::Complete(evaluate(state));
        }

        // Abort check: external stop signal takes precedence over time-up
        // so we report the cause faithfully.
        if self
            .analytics
            .quiescence_nodes_searched
            .is_multiple_of(QUIESCENCE_NODE_CHECK_INTERVAL)
        {
            if self.stop.load(Ordering::Relaxed) {
                return SearchStatus::Stopped;
            }
            if self.time_control.is_hard_time_up() {
                return SearchStatus::TimedOut;
            }
            if self.is_node_limit_reached() {
                return SearchStatus::TimedOut;
            }
        }

        // Draw detection (50-move rule and repetition), cheap and
        // doesn't need generated moves, so do it before movegen.
        // Quiescence is normally only captures (which reset halfmove_clock)
        // but check evasions can include quiet moves, so the check still matters.
        if state.halfmove_clock >= 100
            || state.is_draw_by_insufficient_material()
            || self.is_repetition(state.zobrist_hash, state.halfmove_clock)
        {
            return SearchStatus::Complete(0);
        }

        let in_check = state.is_in_check(state.side_to_move);

        // Stand-pat before movegen.
        // The cost is stalemate blindness, a stalemate node is
        // scored by static eval instead of 0. Stalemates behind quiet
        // moves were cannot be visible in quiescence search.
        let stand_pat = if !in_check {
            let sp = evaluate(state);
            self.analytics.quiescence_stand_pat_attempts += 1;
            if sp >= beta {
                self.analytics.quiescence_stand_pat_cutoffs += 1;
                return SearchStatus::Complete(beta);
            }
            alpha = alpha.max(sp);
            Some(sp)
        } else {
            None
        };

        let mut buf = MoveScore::new();
        state.generate_moves(buf.moves_mut());

        if in_check {
            // We need to ensure that there is no checkmate
            if buf.is_empty() {
                return SearchStatus::Complete(score::mated(ply));
            }
        } else {
            // Keep only captures and promotions.
            buf.moves_mut()
                .retain(|&mv| mv.is_capture() || mv.is_promotion());
        }

        // Move scoring
        buf.score_quiescence();

        const DELTA_MARGIN: i32 = Cp(2 * PIECE_VALUES[Piece::Pawn as usize]).to_internal();

        // Eligibility: stand-pat must be valid (not in check) and alpha
        // must not be a mate score (else we could mask a forced tactic).
        let delta_baseline = stand_pat.filter(|_| score::mate_in_plies(alpha).is_none());

        for mv in buf.ordered() {
            // Delta pruning: skip captures whose best-case material gain still
            // can't reach alpha.
            // TODO: You want to disable delta pruning in the endgame.
            if let Some(sp) = delta_baseline {
                self.analytics.delta_pruning_attempts += 1;
                let captured = mv
                    .get_captured_piece()
                    .map_or(0, |p| Cp(PIECE_VALUES[p as usize]).to_internal());
                let promo_gain = if mv.is_promotion() {
                    Cp(PIECE_VALUES[Piece::Queen as usize] - PIECE_VALUES[Piece::Pawn as usize])
                        .to_internal()
                } else {
                    0
                };
                if sp + captured + promo_gain + DELTA_MARGIN < alpha {
                    self.analytics.delta_prunings += 1;
                    continue;
                }
            }

            // SEE pruning: skip captures that lose material in the static
            // exchange on `to`. Same eligibility as delta (`delta_baseline`
            // is `Some` iff not in check and alpha is not a mate score) so
            // we never prune forced check evasions or mate-tactic captures.
            if delta_baseline.is_some() && mv.is_capture() {
                self.analytics.see_pruning_attempts += 1;
                if !see_ge(state, mv, 0) {
                    self.analytics.see_prunings += 1;
                    continue;
                }
            }

            // Quiescence search recursion
            self.position_history.push(state.zobrist_hash);
            let undo = state.make_move(mv);
            let status = self.quiescence(state, ply + 1, -beta, -alpha).map(|e| -e);
            state.unmake_move(mv, &undo);
            self.position_history.pop();
            let eval = status?;

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
        SearchStatus::Complete(alpha)
    }
}

impl Default for Searcher {
    fn default() -> Self {
        Self::new(EngineOptions::default(), Arc::new(AtomicBool::new(false)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search_to_nodes(state: &mut GameState, nodes: u64) -> SearchResult {
        let mut searcher = Searcher::default();
        searcher.search(
            state,
            SearchLimits::to_nodes(nodes),
            &[],
            None::<fn(SearchResult)>,
        )
    }

    const MATE_IN_THREE_FEN: &str = "3r1r1k/1p3p1p/p2p4/4n1NN/6bQ/1BPq4/P3p1PP/1R5K w - - 0 1";

    const HARD_MATE_IN_THREE_FEN: &str = "4k1r1/R6p/4Nb2/4n3/6Pq/2P4P/3Q3K/5R2 w - - 2 2";

    const HARD_MATE_IN_FOUR_FEN: &str =
        "3qr2k/1p3rbp/2p3p1/p7/P2pBNn1/1P3n2/6P1/B1Q1RR1K b - - 1 30";

    fn assert_finds_mate(fen: &str, plies: i32, node_limit: u64) {
        let mut state = GameState::from_fen(fen).unwrap();
        let result = search_to_nodes(&mut state, node_limit);
        assert_eq!(
            result.mate_in(),
            Some(plies),
            "expected mate in {plies} plies, got eval {}",
            Cp::from_internal(result.evaluation),
        );
    }

    fn get_best_moves_till_limit(
        state: &mut GameState,
        node_limit: u64,
        max_moves: usize,
    ) -> Vec<Move> {
        let mut searcher = Searcher::default();
        let mut history = vec![];

        for _ in 0..max_moves {
            if state.is_checkmate() || state.is_stalemate() {
                break;
            }
            let initial_time = std::time::Instant::now();
            let result = searcher.search(
                state,
                SearchLimits::to_nodes(node_limit),
                &[],
                None::<fn(SearchResult)>,
            );
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

    fn assert_best_move_sequence(mut state: GameState, expected_moves: &[&str], node_limit: u64) {
        let best_moves = get_best_moves_till_limit(&mut state, node_limit, expected_moves.len());
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
        assert_best_move_sequence(state, &best_moves_expected, 500_000);
    }

    #[test]
    fn test_search_mate_in_two() {
        let state =
            GameState::from_fen("5rk1/5ppp/2p5/1p6/1Q1p1P2/2Pq4/bP2R2P/rNK1R3 w - - 0 24").unwrap();
        let best_moves_expected = ["b4f8", "g8f8", "e2e8"];
        assert_best_move_sequence(state, &best_moves_expected, 500_000);
    }

    #[test]
    fn test_search_mate_in_three() {
        assert_finds_mate(MATE_IN_THREE_FEN, 5, 200_000);
    }

    /// Mates that the current search only finds after a lot of nodes.
    #[test]
    #[ignore]
    fn test_search_finds_hard_mates() {
        assert_finds_mate(HARD_MATE_IN_THREE_FEN, 5, 30_000_000);
        assert_finds_mate(HARD_MATE_IN_FOUR_FEN, 7, 30_000_000);
    }

    #[test]
    fn test_search_returns_full_pv_for_mate_in_two() {
        let mut state =
            GameState::from_fen("5rk1/5ppp/2p5/1p6/1Q1p1P2/2Pq4/bP2R2P/rNK1R3 w - - 0 24").unwrap();
        let result = search_to_nodes(&mut state, 500_000);
        let best_moves: Vec<Move> = result.pv.iter().copied().collect();
        assert_move_sequence(&best_moves, &["b4f8", "g8f8", "e2e8"]);
    }

    #[test]
    fn test_search_returns_full_pv_for_mate_in_three() {
        let mut state = GameState::from_fen(MATE_IN_THREE_FEN).unwrap();
        let result = search_to_nodes(&mut state, 200_000);
        assert_eq!(result.mate_in(), Some(5));
        assert!(result.pv.len() >= 2);
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
        let mut searcher = Searcher::default();
        searcher.position_history = history;
        assert!(searcher.is_repetition(state.zobrist_hash, state.halfmove_clock));
    }

    #[test]
    fn test_is_repetition_no_match() {
        // Four distinct development moves: current position is novel.
        let mut state = GameState::new();
        let history = play_moves(&mut state, &["e2e4", "e7e5", "g1f3", "g8f6"]);
        let mut searcher = Searcher::default();
        searcher.position_history = history;
        assert!(!searcher.is_repetition(state.zobrist_hash, 100));
    }

    #[test]
    fn test_is_repetition_too_few_entries() {
        // Less than 4 entries: a 4-ply cycle is impossible.
        let mut state = GameState::new();
        let history = play_moves(&mut state, &["b1c3", "b8c6", "c3b1"]);
        let mut searcher = Searcher::default();
        searcher.position_history = history;
        assert!(!searcher.is_repetition(state.zobrist_hash, 100));
    }

    #[test]
    fn test_is_repetition_short_circuits_below_4_halfmoves() {
        // A real 4-ply cycle is present, but halfmove_clock < 4 short-circuits.
        let mut state = GameState::new();
        let history = play_moves(&mut state, &["b1c3", "b8c6", "c3b1", "c6b8"]);
        let mut searcher = Searcher::default();
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
        let mut searcher = Searcher::default();
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
        // positions, no castling rights or en passant to taint the hash.
        let mut state = GameState::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let history = play_moves(
            &mut state,
            &["e1d1", "e8d8", "d1e2", "d8e7", "e2e1", "e7e8"],
        );
        let mut searcher = Searcher::default();
        searcher.position_history = history;
        assert!(searcher.is_repetition(state.zobrist_hash, state.halfmove_clock));
    }

    #[test]
    fn test_losing_side_seeks_game_history_repetition() {
        let mut state = GameState::from_fen("3q3k/8/8/8/8/8/8/KN6 w - - 0 1").unwrap();
        let history = play_moves(&mut state, &["b1c3", "d8d7", "c3b1", "d7d8"]);
        let mut searcher = Searcher::default();
        let result = searcher.search(
            &mut state,
            SearchLimits::to_depth(6),
            &history,
            None::<fn(SearchResult)>,
        );
        assert_eq!(result.evaluation, 0);
        assert_eq!(
            result.best_move().map(|m| m.to_uci()),
            Some("b1c3".to_string())
        );
    }

    #[test]
    fn test_winning_side_avoids_game_history_repetition() {
        let mut state = GameState::from_fen("7k/8/8/8/8/8/8/K2Q4 w - - 0 1").unwrap();
        let history = play_moves(&mut state, &["d1e1", "h8g8", "e1d1", "g8h8"]);
        let mut searcher = Searcher::default();
        let result = searcher.search(
            &mut state,
            SearchLimits::to_depth(6),
            &history,
            None::<fn(SearchResult)>,
        );
        assert!(
            result.evaluation > Cp(300).to_internal(),
            "eval = {} ({} cp)",
            result.evaluation,
            Cp::from_internal(result.evaluation)
        );
        assert_ne!(
            result.best_move().map(|m| m.to_uci()),
            Some("d4e4".to_string())
        );
    }

    #[test]
    fn test_without_game_history_repetition_is_invisible() {
        let mut state = GameState::from_fen("3q3k/8/8/8/8/8/8/KN6 w - - 0 1").unwrap();
        play_moves(&mut state, &["b1c3", "d8d7", "c3b1", "d7d8"]);
        let mut searcher = Searcher::default();
        let result = searcher.search(
            &mut state,
            SearchLimits::to_depth(6),
            &[],
            None::<fn(SearchResult)>,
        );
        assert!(
            result.evaluation < Cp(-300).to_internal(),
            "eval = {} ({} cp)",
            result.evaluation,
            Cp::from_internal(result.evaluation)
        );
    }

    #[test]
    fn test_search_fifty_move_rule_returns_zero() {
        // K+R vs K is normally winning for White, but with halfmove_clock=99
        // every legal first move (all non-capture, non-pawn) pushes the clock
        // to 100. At depth >= 1, every child node hits the 50-move draw check
        // and returns 0, which propagates back to the root.
        let mut state = GameState::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 99 50").unwrap();
        let mut searcher = Searcher::default();
        let result = searcher.search(
            &mut state,
            SearchLimits::to_depth(3),
            &[],
            None::<fn(SearchResult)>,
        );
        assert_eq!(result.evaluation, 0);
    }

    #[test]
    fn test_search_finds_forced_repetition_draw() {
        // White is down significant material but the only drawing line is a forced
        // perpetual starting with a bishop sacrifice:
        let mut state =
            GameState::from_fen("1krqr3/p1p2n2/2Q3b1/p3n3/1b6/8/4BB2/5K2 w - - 0 1").unwrap();
        let mut searcher = Searcher::default();
        let result = searcher.search(
            &mut state,
            SearchLimits::to_depth(9),
            &[],
            None::<fn(SearchResult)>,
        );
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
        let mut searcher = Searcher::default();
        let result = searcher.search(
            &mut state,
            SearchLimits::to_depth(1),
            &[],
            None::<fn(SearchResult)>,
        );
        assert_eq!(result.evaluation, 0);
    }
}
