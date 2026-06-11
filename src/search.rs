use std::time::Duration;

use crate::core::*;
use crate::evaluation::{evaluate, MATE_SCORE};
use crate::move_generator::MoveList;
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
    pub best_move: Option<Move>,
    pub evaluation: i32,
    pub analytics: SearchAnalytics,
    pub pv: PvList,
}

impl SearchResult {
    pub fn mate_in(&self) -> Option<i32> {
        let ply = MATE_SCORE - self.evaluation.abs();
        if ply <= MAX_PLY as i32 {
            Some(ply * self.evaluation.signum())
        } else {
            None
        }
    }
}

pub type ZobristHashList = StackVec<ZobristHash, MAX_PLY>;
pub type PvList = StackVec<Move, MAX_PLY>;

pub struct Searcher {
    move_pool: Box<[MoveList; MAX_PLY]>,
    position_history: Box<ZobristHashList>,
    transposition_table: TranspositionTable,
    time_control: TimeControl,
    analytics: SearchAnalytics,
    killer_moves: Box<[[Move; 2]; MAX_PLY]>,
    history: Box<[[[Move; Square::NUM]; Square::NUM]; Color::NUM]>,
    // Triangular PV table: pv_table[ply][0..pv_length[ply]] is the PV
    // discovered at this ply. On a new best move at a node we write
    // the move into slot 0 and copy the child's PV after it.
    pv_table: Box<[[Move; MAX_PLY]; MAX_PLY]>,
    pv_length: Box<[u8; MAX_PLY]>,
}

const MAX_PLY: usize = 64;
const KILLER_MOVES_PER_PLY: usize = 2;
const HISTORY_BONUS_MULTIPLIER: u32 = 8;

// Check for timeouts every N nodes in alpha-beta and every M quiescence nodes.
const NODE_CHECK_INTERVAL: u64 = 256;
const QUIESCENCE_NODE_CHECK_INTERVAL: u64 = 128;

impl Searcher {
    pub fn new() -> Self {
        let move_pool = Box::new([MoveList::default(); MAX_PLY]);
        let killer_moves = Box::new([[0u32; KILLER_MOVES_PER_PLY]; MAX_PLY]);
        let history = Box::new([[[0u32; Square::NUM]; Square::NUM]; Color::NUM]);
        let pv_table = Box::new([[0u32; MAX_PLY]; MAX_PLY]);
        let pv_length = Box::new([0u8; MAX_PLY]);

        let time_control = TimeControl::new(Duration::from_mins(1), Duration::from_secs(0));

        Searcher {
            move_pool,
            position_history: Box::new(ZobristHashList::default()),
            killer_moves,
            history,
            pv_table,
            pv_length,
            transposition_table: TranspositionTable::new(),
            analytics: SearchAnalytics::default(),
            time_control,
        }
    }

    /// Apply `mv`, recurse into `alpha_beta` with negated bounds, undo,
    /// and return the score in the parent's frame. Returns `None` on timeout.
    #[inline]
    fn negamax_child(
        &mut self,
        state: &mut GameState,
        mv: Move,
        depth: u8,
        ply: usize,
        alpha: i32,
        beta: i32,
    ) -> Option<i32> {
        self.position_history.push(state.zobrist_hash);
        let undo = state.make_move(mv);
        let result = self
            .alpha_beta(state, depth - 1, ply + 1, -beta, -alpha)
            .map(|(_, e)| -e);
        state.unmake_move(mv, &undo);
        self.position_history.pop();
        result
    }

    /// Quiescence counterpart to `negamax_child`. Apply `mv`, recurse into
    /// `quiescence` with negated bounds, undo, and return the score in the
    /// parent's frame. Returns `None` on timeout.
    #[inline]
    fn quiescence_child(
        &mut self,
        state: &mut GameState,
        mv: Move,
        ply: usize,
        alpha: i32,
        beta: i32,
    ) -> Option<i32> {
        self.position_history.push(state.zobrist_hash);
        let undo = state.make_move(mv);
        let result = self.quiescence(state, ply + 1, -beta, -alpha).map(|e| -e);
        state.unmake_move(mv, &undo);
        self.position_history.pop();
        result
    }

    /// Append `mv` followed by the child's PV into this ply's PV slot.
    #[inline]
    fn update_pv(&mut self, ply: usize, mv: Move) {
        if ply + 1 >= MAX_PLY {
            self.pv_table[ply][0] = mv;
            self.pv_length[ply] = 1;
            return;
        }
        let child_len = self.pv_length[ply + 1] as usize;
        let (lo, hi) = self.pv_table.split_at_mut(ply + 1);
        lo[ply][0] = mv;
        lo[ply][1..=child_len].copy_from_slice(&hi[0][..child_len]);
        self.pv_length[ply] = (child_len + 1) as u8;
    }

    /// Snapshot the root PV into a `PvList` for reporting. After the
    /// triangular table runs out (TT cutoffs leave it short), continue by
    /// replaying TT entries until we hit a miss, a missing best_move, an
    /// illegal move (key collision), a repeated position, or `MAX_PLY`.
    fn root_pv(&self, state: &GameState) -> PvList {
        let mut pv = PvList::default();
        let len = self.pv_length[0] as usize;
        for i in 0..len {
            pv.push(self.pv_table[0][i]);
        }

        let mut working_state = *state;
        let mut seen = ZobristHashList::default();
        seen.push(working_state.zobrist_hash);
        for i in 0..len {
            working_state.make_move(pv[i]);
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
            working_state.generate_valid_moves(&mut moves);
            if !moves.iter().any(|&m| m == mv) {
                break;
            }

            working_state.make_move(mv);

            // A TT-pointed cycle would loop here forever — stop on revisit.
            if seen.iter().any(|&h| h == working_state.zobrist_hash) {
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
        let mut i = n - 4;
        loop {
            if self.position_history[i] == hash {
                return true;
            }
            if i < earliest + 2 {
                break;
            }
            i -= 2;
        }
        false
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
    /// then castles, double pawn pushes, then quiet moves with history bonus, and killer moves.
    /// all of them are given bonuses from transposition table move if it matches, to ensure we search the tt move first
    fn score_move_main(
        mv: Move,
        state: &GameState,
        tt_move: Option<Move>,
        killer_moves: &[Move; KILLER_MOVES_PER_PLY],
        history: &[[[Move; Square::NUM]; Square::NUM]; Color::NUM],
    ) -> i32 {
        // Material values aligned with `core::Piece` ordering: King, Queen, Rook, Bishop, Knight, Pawn
        const PIECE_VALUES: [i32; Piece::NUM] = [20_000, 900, 500, 330, 320, 100];

        let tt_bonus = if Some(mv) == tt_move { 1_000_000 } else { 0 };

        // Check if move is a killer move (quiet move that caused cutoff at this depth)
        let killer_bonus = if killer_moves.contains(&mv) { 9_000 } else { 0 };

        // Get history bonus for this move. History tracks quiet moves that caused cutoffs.
        let from = mv.get_from() as usize;
        let to = mv.get_to() as usize;
        let history_bonus =
            (history[state.side_to_move as usize][from][to] / HISTORY_BONUS_MULTIPLIER) as i32;

        match mv.get_type() {
            MoveType::Promotion { .. } => 100_000 + tt_bonus,
            MoveType::Capture { .. } => {
                let captured = mv.get_captured_piece().unwrap_or(Piece::Pawn) as usize;
                let moved = mv.get_moved_piece() as usize;
                // MVV-LVA style: prefer capturing high-value pieces with low-value attackers
                ((PIECE_VALUES[captured] * 100) - PIECE_VALUES[moved]) + tt_bonus
            }
            _ => history_bonus + killer_bonus + tt_bonus,
        }
    }

    /// Simple static move ordering score: promotions highest, then captures (MVV-LVA),
    /// then castles, double pawn pushes, then quiet moves with history bonus, and killer moves.
    /// all of them are given bonuses from transposition table move if it matches, to ensure we search the tt move first
    fn score_move_quiescence(mv: Move, tt_move: Option<Move>) -> i32 {
        // Material values aligned with `core::Piece` ordering: King, Queen, Rook, Bishop, Knight, Pawn
        const PIECE_VALUES: [i32; Piece::NUM] = [20_000, 900, 500, 330, 320, 100];

        let tt_bonus = if Some(mv) == tt_move { 1_000_000 } else { 0 };

        match mv.get_type() {
            MoveType::Promotion { .. } => 100_000 + tt_bonus,
            MoveType::Capture { .. } => {
                let captured = mv.get_captured_piece().unwrap_or(Piece::Pawn) as usize;
                let moved = mv.get_moved_piece() as usize;
                // MVV-LVA style: prefer capturing high-value pieces with low-value attackers
                ((PIECE_VALUES[captured] * 100) - PIECE_VALUES[moved]) + tt_bonus
            }
            _ => tt_bonus,
        }
    }

    /// Add a killer move at the given ply. Maintains up to 2 killer moves per ply.
    /// When a new killer move is added, the first one becomes the second and the new one becomes first.
    #[inline]
    fn update_killer(&mut self, mv: Move, ply: usize) {
        let killers = &mut self.killer_moves[ply];

        // Don't add if it's already the primary killer
        if killers[0] == mv {
            return;
        }

        // Shift and add new killer
        killers[1] = killers[0];
        killers[0] = mv;
    }

    /// Update history score for a move that caused a beta cutoff.
    /// History heuristic tracks quiet moves that lead to cutoffs and improves their move ordering.
    /// The history bonus is scaled and capped to prevent overflow and control its influence.
    #[inline]
    fn update_history(&mut self, mv: Move, depth: u8, side: Color) {
        let from = mv.get_from() as usize;
        let to = mv.get_to() as usize;

        // Add a bonus based on depth (deeper moves that cause cutoffs are more valuable)
        let bonus = (depth as u32) * (depth as u32) * HISTORY_BONUS_MULTIPLIER;

        // Cap history values to prevent overflow and unbounded growth
        const HISTORY_MAX: u32 = u32::MAX / 2;
        self.history[side as usize][from][to] = self.history[side as usize][from][to]
            .saturating_add(bonus)
            .min(HISTORY_MAX);
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
        self.killer_moves.fill([0u32; KILLER_MOVES_PER_PLY]);

        // Clear PV table lengths. The data array doesn't need clearing —
        // pv_length controls which slots are read.
        self.pv_length.fill(0);

        // Reset position history: repetition detection only considers
        // positions visited within this search tree.
        self.position_history.clear();

        let mut best_move = None;
        let mut best_eval = 0i32;
        let mut pv = PvList::default();

        self.time_control.set_search_deadline(state.side_to_move);
        for depth in 1..=max_depth {
            match self.alpha_beta(state, depth, 0, i32::MIN / 2, i32::MAX / 2) {
                Some((mv, eval)) => {
                    best_move = mv;
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
                    best_move,
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
            best_move,
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
        self.pv_length[ply] = 0;

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
            let tt_score = Self::score_from_tt(entry.score, ply);

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

        // We order moves to improve alpha-beta cutoffs.
        let move_count = {
            let moves = &mut self.move_pool[ply];
            moves.clear();
            state.generate_valid_moves(moves);

            let tt_move = tt_entry.and_then(|entry| entry.best_move);
            let killer_moves = &self.killer_moves[ply];
            moves.sort_by_key(|&mv| {
                -Searcher::score_move_main(mv, state, tt_move, killer_moves, &self.history)
            });

            moves.len()
        };

        let mut best_move = None;
        let mut best_eval = i32::MIN / 2;
        for i in 0..move_count {
            let mv = self.move_pool[ply][i];

            // Principal Variation Search: full window on the first move, null-window
            // probe on the rest, re-search only when a probe falls inside (alpha, beta).
            let eval = if i == 0 {
                self.negamax_child(state, mv, depth, ply, alpha, beta)?
            } else {
                let scout = self.negamax_child(state, mv, depth, ply, alpha, alpha + 1)?;
                if scout > alpha && scout < beta {
                    self.analytics.pvs_re_searches += 1;
                    self.negamax_child(state, mv, depth, ply, alpha, beta)?
                } else {
                    scout
                }
            };

            // min(-eval, -best_eval) = max(eval, best_eval)
            // So, this works for both players without needing to check which side is to move
            if eval > best_eval {
                best_eval = eval;
                best_move = Some(mv);
                self.update_pv(ply, mv);
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
                    self.update_killer(mv, ply);

                    // History heuristic tracks quiet moves that lead to cutoffs regardless of depth.
                    // Even a shallow cutoff can indicate a move that is good in general.
                    self.update_history(mv, depth, state.side_to_move);
                }
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

        // Timeout check
        if self
            .analytics
            .quiescence_nodes_searched
            .is_multiple_of(QUIESCENCE_NODE_CHECK_INTERVAL)
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

        // Draw detection 50-move rule and repetition.
        // Quiescence is normally only captures (which reset halfmove_clock)
        // but check evasions can include quiet moves, so the check still matters.
        if state.halfmove_clock >= 100
            || self.is_repetition(state.zobrist_hash, state.halfmove_clock)
        {
            return Some(0);
        }

        let in_check = state.is_in_check(state.side_to_move);

        let move_count = {
            let moves = &mut self.move_pool[ply];
            // only filter captures/promotions if not in check
            // otherwise we might miss important evasions
            if !in_check {
                moves.retain(|&mv| mv.is_capture() || mv.is_promotion());
            }
            moves.sort_by_key(|&mv| -Searcher::score_move_quiescence(mv, None));
            moves.len()
        };

        // Stand-pat is only valid when not in check
        if !in_check {
            let stand_pat = evaluate(state);
            if stand_pat >= beta {
                self.analytics.quiescence_alpha_beta_cutoffs += 1;
                return Some(beta);
            }
            alpha = alpha.max(stand_pat);
        }

        for i in 0..move_count {
            let mv = self.move_pool[ply][i];
            let eval = self.quiescence_child(state, mv, ply, alpha, beta)?;

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
    fn test_search_returns_full_pv_for_mate_in_two() {
        let mut state =
            GameState::from_fen("5rk1/5ppp/2p5/1p6/1Q1p1P2/2Pq4/bP2R2P/rNK1R3 w - - 0 24").unwrap();
        let mut searcher = Searcher::new();
        let result = searcher.search(&mut state, 4, None::<fn(SearchResult)>);
        let pv: Vec<String> = result.pv.iter().map(|m| m.repr_string()).collect();
        assert_eq!(pv, vec!["b4f8", "g8f8", "e2e8"]);
        assert_eq!(result.best_move.map(|m| m.repr_string()), Some("b4f8".to_string()));
    }

    #[test]
    fn test_search_returns_full_pv_for_mate_in_three() {
        // Mate-in-3 (5 plies). At depth 5 the in-search PV may truncate at TT
        // cutoffs; TT-replay should extend it back to the full forced line.
        let mut state =
            GameState::from_fen("4k1r1/R6p/4Nb2/4n3/6Pq/2P4P/3Q3K/5R2 w - - 2 2").unwrap();
        let mut searcher = Searcher::new();
        let result = searcher.search(&mut state, 5, None::<fn(SearchResult)>);
        let pv: Vec<String> = result.pv.iter().map(|m| m.repr_string()).collect();
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
            state.generate_valid_moves(&mut moves);
            let mv = moves
                .into_iter()
                .find(|m| m.repr_string() == uci)
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
            result.best_move.map(|m| m.repr_string()),
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
