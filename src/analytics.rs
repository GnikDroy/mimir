//! Aggregated counters and timings collected during a single search.
//!
//! Populated incrementally inside [`crate::search::Searcher`]; consumed
//! by the UCI adapter for `info` lines and by debug callers via
//! [`SearchAnalytics::print`].

use std::fmt;
use std::time::Duration;

#[derive(Debug, Clone, Copy, Default)]
pub struct SearchAnalytics {
    pub depth: u8,
    /// Deepest ply reached from the root, including any quiescence
    /// extension. (Renamed from `max_quiescence_depth_reached` — the
    /// stored value is the absolute ply, not the depth into quiescence.)
    pub max_ply_reached: u8,
    pub elapsed: Duration,
    pub nodes_searched: u64,
    pub quiescence_nodes_searched: u64,
    pub alpha_beta_cutoffs: u64,
    /// Move-loop beta cutoffs inside quiescence (after make_move).
    /// Does NOT include stand-pat — see `quiescence_stand_pat_cutoffs`.
    pub quiescence_alpha_beta_cutoffs: u64,
    /// Stand-pat fail-high cutoffs inside quiescence: the static eval
    /// already meets or exceeds beta, so the position is at least as
    /// good as the current bound and we skip generating recaptures.
    pub quiescence_stand_pat_cutoffs: u64,
    /// Probes that returned an entry, regardless of stored depth. Includes
    /// shallow entries that feed move ordering but can't directly cut.
    pub transposition_table_entries_found: u64,
    /// Subset of entries_found whose stored depth is sufficient to use
    /// for bound updates or an Exact cutoff.
    pub transposition_table_hits: u64,
    pub transposition_table_cuts: u64,
    pub transposition_table_hashfull: u32,
    /// Null-window scout searches performed (one per non-first move at
    /// each PVS node).
    pub pvs_scouts: u64,
    /// Subset of `pvs_scouts` taken when the window was still open
    /// (`beta > alpha + 1`). At a non-PV node, `beta = alpha + 1` on
    /// entry and the re-search condition `scout > alpha && scout < beta`
    /// is mathematically impossible — those scouts are eligibility-zero
    /// and shouldn't dilute the re-search rate.
    pub pvs_open_window_scouts: u64,
    pub pvs_re_searches: u64,
    /// Times null move pruning was attempted (passed all guards).
    pub null_move_attempts: u64,
    /// Subset of `null_move_attempts` where the reduced null-window search
    /// failed high and we cut.
    pub null_move_cutoffs: u64,
    /// Times late move reductions were applied (passed all guards and the
    /// table returned a nonzero reduction).
    pub lmr_attempts: u64,
    /// Subset of `lmr_attempts` where the reduced search confirmed the move
    /// wasn't better than alpha — the reduction held, no re-search needed.
    pub lmr_successes: u64,
    /// Subset of `lmr_attempts` where the reduced scout failed high and we
    /// re-searched at full depth.
    pub lmr_re_searches_depth: u64,
    /// Times the TT-suggested move was reached in the move loop.
    pub tt_move_tried: u64,
    /// Times the TT-suggested move caused the beta cutoff.
    pub tt_move_cutoffs: u64,
    /// Times a killer-table move was reached in the move loop. Counted
    /// per try, so multiple killers tried at one node increment multiply.
    pub killer_move_tried: u64,
    /// Times a killer-table move caused the beta cutoff.
    pub killer_move_cutoffs: u64,
    /// Nodes where the highest-history quiet move existed AND the search
    /// reached it before cutoff.
    pub history_top_tried: u64,
    /// Subset of `history_top_tried` where the highest-history quiet
    /// move ended up as the node's best move.
    pub history_top_best: u64,
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

    /// Prints a multi-line summary to stderr. Stdout is reserved for UCI.
    pub fn print(&self) {
        eprintln!("{self}");
    }
}

fn pct(numer: u64, denom: u64) -> f64 {
    if denom == 0 {
        0.0
    } else {
        (numer as f64) * 100.0 / (denom as f64)
    }
}

/// Renders a non-negative integer with `,` separators every three digits.
fn with_commas(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(b as char);
    }
    out
}

/// Width reserved for the label column in the multi-line Display layout.
/// Picked to leave a ~3-character gap after the longest label
/// (`Transposition table entries found:`).
const LABEL_WIDTH: usize = 38;

impl fmt::Display for SearchAnalytics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Top-level search metadata
        writeln!(f, "{:<w$}{}", "Depth reached:", self.depth, w = LABEL_WIDTH)?;
        writeln!(
            f,
            "{:<w$}{}",
            "Deepest ply reached:",
            self.max_ply_reached,
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{:.3}s",
            "Elapsed time:",
            self.elapsed.as_secs_f64(),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{}",
            "Nodes per second:",
            with_commas(self.nodes_per_second()),
            w = LABEL_WIDTH,
        )?;
        writeln!(f)?;

        // Node counts
        writeln!(
            f,
            "{:<w$}{}",
            "Main-search nodes:",
            with_commas(self.nodes_searched),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{}",
            "Quiescence-search nodes:",
            with_commas(self.quiescence_nodes_searched),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{}",
            "Total nodes:",
            with_commas(self.total_nodes()),
            w = LABEL_WIDTH,
        )?;
        writeln!(f)?;

        // Alpha-beta cutoff rates
        writeln!(
            f,
            "{:<w$}{} ({:.1}% of main-search nodes)",
            "Alpha-beta cutoffs (main):",
            with_commas(self.alpha_beta_cutoffs),
            pct(self.alpha_beta_cutoffs, self.nodes_searched),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{} ({:.1}% of quiescence nodes)",
            "Alpha-beta cutoffs (quiescence):",
            with_commas(self.quiescence_alpha_beta_cutoffs),
            pct(
                self.quiescence_alpha_beta_cutoffs,
                self.quiescence_nodes_searched,
            ),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{} ({:.1}% of quiescence nodes)",
            "Stand-pat cutoffs (quiescence):",
            with_commas(self.quiescence_stand_pat_cutoffs),
            pct(
                self.quiescence_stand_pat_cutoffs,
                self.quiescence_nodes_searched,
            ),
            w = LABEL_WIDTH,
        )?;
        writeln!(f)?;

        // Transposition table health
        writeln!(
            f,
            "{:<w$}{}",
            "Transposition table entries found:",
            with_commas(self.transposition_table_entries_found),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{} ({:.1}% of entries found)",
            "Transposition table hits (usable):",
            with_commas(self.transposition_table_hits),
            pct(
                self.transposition_table_hits,
                self.transposition_table_entries_found,
            ),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{} ({:.1}% of hits)",
            "Transposition table cuts:",
            with_commas(self.transposition_table_cuts),
            pct(
                self.transposition_table_cuts,
                self.transposition_table_hits
            ),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{}/1000 (UCI per-mille fill)",
            "Transposition table hashfull:",
            self.transposition_table_hashfull,
            w = LABEL_WIDTH,
        )?;
        writeln!(f)?;

        // Principal variation search
        writeln!(
            f,
            "{:<w$}{}",
            "PVS scouts:",
            with_commas(self.pvs_scouts),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{} ({:.1}% of all scouts)",
            "PVS scouts (open window):",
            with_commas(self.pvs_open_window_scouts),
            pct(self.pvs_open_window_scouts, self.pvs_scouts),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{} ({:.1}% of open-window scouts)",
            "PVS re-searches:",
            with_commas(self.pvs_re_searches),
            pct(self.pvs_re_searches, self.pvs_open_window_scouts),
            w = LABEL_WIDTH,
        )?;
        writeln!(f)?;

        // Null move pruning
        writeln!(
            f,
            "{:<w$}{}",
            "Null move attempts:",
            with_commas(self.null_move_attempts),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{} ({:.1}% of attempts)",
            "Null move cutoffs:",
            with_commas(self.null_move_cutoffs),
            pct(self.null_move_cutoffs, self.null_move_attempts),
            w = LABEL_WIDTH,
        )?;
        writeln!(f)?;

        // Late move reductions
        writeln!(
            f,
            "{:<w$}{}",
            "LMR attempts:",
            with_commas(self.lmr_attempts),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{} ({:.1}% of attempts)",
            "LMR successes (reduction held):",
            with_commas(self.lmr_successes),
            pct(self.lmr_successes, self.lmr_attempts),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{} ({:.1}% of attempts)",
            "LMR full-depth re-searches:",
            with_commas(self.lmr_re_searches_depth),
            pct(self.lmr_re_searches_depth, self.lmr_attempts),
            w = LABEL_WIDTH,
        )?;
        writeln!(f)?;

        // Move ordering quality
        writeln!(
            f,
            "{:<w$}{}",
            "TT move tried:",
            with_commas(self.tt_move_tried),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{} ({:.1}% of tried)",
            "TT move cutoffs:",
            with_commas(self.tt_move_cutoffs),
            pct(self.tt_move_cutoffs, self.tt_move_tried),
            w = LABEL_WIDTH,
        )?;
        writeln!(f)?;
        writeln!(
            f,
            "{:<w$}{}",
            "Killer move tried:",
            with_commas(self.killer_move_tried),
            w = LABEL_WIDTH,
        )?;
        writeln!(
            f,
            "{:<w$}{} ({:.1}% of tried)",
            "Killer move cutoffs:",
            with_commas(self.killer_move_cutoffs),
            pct(self.killer_move_cutoffs, self.killer_move_tried),
            w = LABEL_WIDTH,
        )?;
        writeln!(f)?;
        writeln!(
            f,
            "{:<w$}{}",
            "History top-ranked tried:",
            with_commas(self.history_top_tried),
            w = LABEL_WIDTH,
        )?;
        write!(
            f,
            "{:<w$}{} ({:.1}% of tried)",
            "History top-ranked was best:",
            with_commas(self.history_top_best),
            pct(self.history_top_best, self.history_top_tried),
            w = LABEL_WIDTH,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::search::{SearchResult, Searcher};
    use crate::state::GameState;

    /// Profiling target: a single fixed-depth search from the kiwipete
    /// position with no time control and no reporting.
    #[test]
    #[ignore]
    fn profile_search_kiwipete_depth_10() {
        let mut state = GameState::from_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        )
        .unwrap();
        let mut searcher = Searcher::new();
        let result = searcher.search(&mut state, 10, None::<fn(SearchResult)>);
        println!("Game: {}", state);
        result.analytics.print();
    }

    /// Profiling target: a single fixed-depth search from the kiwipete
    /// position with no time control and no reporting.
    #[test]
    #[ignore]
    fn profile_search_startpos_depth_10() {
        let mut state = GameState::new();
        let mut searcher = Searcher::new();
        let result = searcher.search(&mut state, 10, None::<fn(SearchResult)>);
        result.analytics.print();
    }
}
