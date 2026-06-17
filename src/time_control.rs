//! Time management for the iterative-deepening search.
//!
//! [`TimeControl`] tracks both the UCI clock state (per-side remaining
//! time, increments, `movestogo`, fixed `movetime`) and the live
//! search window. The search keeps two deadlines: a **soft** deadline
//! checked between iterative-deepening iterations, and a **hard**
//! deadline checked inside the tree every N nodes.
//!
//! - Don't *start* a new iteration past the soft deadline.
//! - Do abort mid-iteration once we cross the hard deadline.
//!
//! The base budget is `remaining / movestogo + inc / 2`, minus a
//! small safety buffer. Soft is `base / SOFT_DIVISOR`, hard is `base`.
//! `SOFT_DIVISOR` tracks the effective branching factor — see
//! [Chess Programming Wiki](https://www.chessprogramming.org/Time_Management).

use std::time::{Duration, Instant};

use crate::core::Color;

/// Soft-deadline divisor — soft fires at `base_budget / SOFT_DIVISOR`.
/// Picked to roughly match the effective branching factor.
const SOFT_DIVISOR: u32 = 3;

/// UCI clock state plus the current search deadlines.
///
/// Clock fields (`wtime`/`btime`/`winc`/`binc`/`movestogo`/`move_time`)
/// are refreshed per `go` command via
/// [`update_clock`](Self::update_clock).
/// Multiple timing fields may be present at once; the budget takes the
/// *strictest* (smallest) of the resulting candidates.
///
/// `search_start`, `soft_deadline`, and `hard_deadline` are managed by
/// [`set_search_deadline`](Self::set_search_deadline) and
/// [`clear_search_deadline`](Self::clear_search_deadline); they are
/// `None` outside an active search. `None` deadlines during an active
/// search mean no time bound (search runs to depth or until `stop`).
#[derive(Debug, Clone, Copy)]
pub struct TimeControl {
    /// White's remaining time on the live clock.
    pub wtime: Option<Duration>,
    /// Black's remaining time on the live clock.
    pub btime: Option<Duration>,
    /// White's per-move increment on the live clock.
    pub winc: Option<Duration>,
    /// Black's per-move increment on the live clock.
    pub binc: Option<Duration>,
    /// Moves remaining until the next time control, if specified.
    pub movestogo: Option<u32>,
    /// Fixed per-move time override (`go movetime`); when set, replaces
    /// the computed budget entirely and collapses soft and hard.
    pub move_time: Option<Duration>,
    /// Safety buffer subtracted from each candidate budget to cover UCI
    /// round-trip overhead.
    pub move_overhead: Duration,
    search_start: Option<Instant>,
    soft_deadline: Option<Instant>,
    hard_deadline: Option<Instant>,
}

impl TimeControl {
    /// Creates a [`TimeControl`] with a 10 ms default safety buffer.
    pub fn new() -> Self {
        Self::with_overhead(Duration::from_millis(10))
    }

    /// Creates a [`TimeControl`] whose move budget reserves `move_overhead`
    /// as a UCI round-trip safety buffer.
    pub fn with_overhead(move_overhead: Duration) -> Self {
        Self {
            wtime: None,
            btime: None,
            winc: None,
            binc: None,
            movestogo: None,
            move_time: None,
            move_overhead,
            search_start: None,
            soft_deadline: None,
            hard_deadline: None,
        }
    }

    /// Updates the safety buffer applied to subsequent move budgets.
    pub fn set_move_overhead(&mut self, move_overhead: Duration) {
        self.move_overhead = move_overhead;
    }

    /// Refreshes the live clock fields from a `go` command.
    pub fn update_clock(
        &mut self,
        wtime: Option<Duration>,
        btime: Option<Duration>,
        winc: Option<Duration>,
        binc: Option<Duration>,
        movestogo: Option<u32>,
        move_time: Option<Duration>,
    ) {
        self.wtime = wtime;
        self.btime = btime;
        self.winc = winc;
        self.binc = binc;
        self.movestogo = movestogo;
        self.move_time = move_time;
    }

    /// Computes the `(soft, hard)` budgets for this move.
    ///
    /// Hard is the strictest of `move_time` and the clock-derived budget
    /// (`remaining/movestogo + inc/2`), minus the safety buffer — the
    /// same value the old single-deadline scheme used.
    ///
    /// Soft is `hard / SOFT_DIVISOR` — the threshold for *starting* a
    /// new iterative-deepening iteration. When `move_time` is the
    /// binding candidate, soft collapses to hard so `go movetime`
    /// stays deterministic.
    ///
    /// Returns `None` when no timing field was supplied — the search
    /// then runs to depth or until `stop`.
    fn move_time_budget(&self, side_to_move: Color) -> Option<(Duration, Duration)> {
        let (remaining, increment) = match side_to_move {
            Color::White => (self.wtime, self.winc),
            Color::Black => (self.btime, self.binc),
        };

        let clock_budget = remaining.map(|rem| {
            let moves_left = self.movestogo.unwrap_or(20).max(1);
            let inc = increment.unwrap_or(Duration::ZERO);
            rem / moves_left + inc / 2
        });

        let hard = [self.move_time, clock_budget]
            .into_iter()
            .flatten()
            .min()?
            .saturating_sub(self.move_overhead);

        let soft = match self.move_time {
            Some(_) if self.move_time <= clock_budget || clock_budget.is_none() => hard,
            _ => hard / SOFT_DIVISOR,
        };

        Some((soft, hard))
    }

    /// Starts a new search window: stamps the start instant and computes
    /// both deadlines from the current clock state. `None` deadlines
    /// mean no time bound was supplied; and the search runs to depth.
    pub fn set_search_deadline(&mut self, side_to_move: Color) {
        let now = Instant::now();
        self.search_start = Some(now);
        let (soft, hard) = match self.move_time_budget(side_to_move) {
            Some(b) => b,
            None => {
                self.soft_deadline = None;
                self.hard_deadline = None;
                return;
            }
        };
        self.soft_deadline = Some(now + soft);
        self.hard_deadline = Some(now + hard);
    }

    /// Time spent since the current search started, or one second as a
    /// neutral fallback if no search is active (used by UCI reporters
    /// to avoid divide-by-zero when computing nodes-per-second).
    pub fn get_elapsed(&self) -> Duration {
        match self.search_start {
            Some(start) => start.elapsed(),
            None => Duration::from_secs(1),
        }
    }

    /// Ends the active search window. After this, both time-up
    /// predicates return `false` until a new deadline is set.
    pub fn clear_search_deadline(&mut self) {
        self.search_start = None;
        self.soft_deadline = None;
        self.hard_deadline = None;
    }

    /// Returns `true` once the wall clock has passed the soft deadline.
    /// Use between iterative-deepening iterations to decide whether to
    /// start a new (likely-doomed) iteration.
    pub fn is_soft_time_up(&self) -> bool {
        match self.soft_deadline {
            Some(dl) => Instant::now() >= dl,
            None => false,
        }
    }

    /// Returns `true` once the wall clock has passed the hard deadline.
    /// Use inside the search tree to abort mid-iteration.
    pub fn is_hard_time_up(&self) -> bool {
        match self.hard_deadline {
            Some(dl) => Instant::now() >= dl,
            None => false,
        }
    }
}

impl Default for TimeControl {
    fn default() -> Self {
        TimeControl::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tc_with_wtime(ms: u64) -> TimeControl {
        let mut tc = TimeControl::with_overhead(Duration::ZERO);
        tc.update_clock(
            Some(Duration::from_millis(ms)),
            Some(Duration::from_millis(ms)),
            None,
            None,
            None,
            None,
        );
        tc
    }

    #[test]
    fn test_soft_is_third_of_hard() {
        let tc = tc_with_wtime(60_000);
        let (soft, hard) = tc.move_time_budget(Color::White).unwrap();
        // base = 60_000ms / 20 = 3000ms, soft = 3000 / SOFT_DIVISOR
        assert_eq!(hard, Duration::from_millis(3000));
        assert_eq!(soft, Duration::from_millis(3000) / SOFT_DIVISOR);
    }

    #[test]
    fn test_movetime_collapses_soft_and_hard() {
        let mut tc = TimeControl::with_overhead(Duration::from_millis(50));
        tc.update_clock(
            None,
            None,
            None,
            None,
            None,
            Some(Duration::from_millis(1000)),
        );
        let (soft, hard) = tc.move_time_budget(Color::White).unwrap();
        // Both should be movetime − overhead = 950ms.
        assert_eq!(hard, Duration::from_millis(950));
        assert_eq!(soft, Duration::from_millis(950));
    }

    #[test]
    fn test_movetime_with_clock_collapses_when_movetime_is_binding() {
        let mut tc = TimeControl::with_overhead(Duration::ZERO);
        tc.update_clock(
            Some(Duration::from_secs(600)),
            Some(Duration::from_secs(600)),
            None,
            None,
            None,
            Some(Duration::from_millis(500)),
        );
        // clock_budget = 600s/20 = 30s; movetime = 500ms is the strictest.
        let (soft, hard) = tc.move_time_budget(Color::White).unwrap();
        assert_eq!(hard, Duration::from_millis(500));
        assert_eq!(soft, Duration::from_millis(500));
    }

    #[test]
    fn test_no_clock_no_deadlines() {
        let mut tc = TimeControl::new();
        tc.set_search_deadline(Color::White);
        assert!(!tc.is_soft_time_up());
        assert!(!tc.is_hard_time_up());
    }

    #[test]
    fn test_overhead_saturating_sub() {
        // remaining < overhead -> saturating subtraction should produce
        // a zero-duration hard budget without panicking.
        let mut tc = TimeControl::with_overhead(Duration::from_millis(500));
        tc.update_clock(
            Some(Duration::from_millis(100)),
            Some(Duration::from_millis(100)),
            None,
            None,
            None,
            None,
        );
        let (soft, hard) = tc.move_time_budget(Color::White).unwrap();
        // base = 100/20 = 5ms; overhead 500ms saturates to 0.
        assert_eq!(hard, Duration::ZERO);
        assert_eq!(soft, Duration::ZERO);
    }

    #[test]
    fn test_increment_halved_into_budget() {
        let mut tc = TimeControl::with_overhead(Duration::ZERO);
        tc.update_clock(
            Some(Duration::from_secs(60)),
            Some(Duration::from_secs(60)),
            Some(Duration::from_secs(2)),
            Some(Duration::from_secs(2)),
            None,
            None,
        );
        let (_, hard) = tc.move_time_budget(Color::White).unwrap();
        // 60s/20 + 2s/2 = 3s + 1s = 4s.
        assert_eq!(hard, Duration::from_secs(4));
    }
}
