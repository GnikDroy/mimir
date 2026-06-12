//! Time management for the iterative-deepening search.
//!
//! [`TimeControl`] tracks both the UCI clock state (per-side remaining
//! time, increments, `movestogo`, fixed `movetime`) and the live
//! search window (a start instant and a deadline). The search calls
//! [`TimeControl::set_search_deadline`] before launching, then polls
//! [`TimeControl::is_time_up`] between nodes to decide when to stop.
//!
//! The budget is a simple "split remaining time across `movestogo`
//! moves and add half the increment", with a small safety buffer to
//! avoid losing on time due to UCI round-trip overhead.
//!
//! This strategy is competitive according the
//! [Chess Programming Wiki](https://www.chessprogramming.org/Time_Management)

use std::time::{Duration, Instant};

use crate::core::Color;

/// UCI clock state plus the current search deadline.
///
/// `base_time` / `increment` are the *initial* values supplied at
/// construction and are used as fallbacks when the GUI reports zero
/// remaining time (some adapters do this for `movestogo`-style games).
/// The active values come from `wtime`/`btime`/`winc`/`binc`, refreshed
/// per `go` command via [`update_clock`](Self::update_clock).
///
/// `search_start` and `search_deadline` are managed by
/// [`set_search_deadline`](Self::set_search_deadline) and
/// [`clear_search_deadline`](Self::clear_search_deadline); they are
/// `None` outside an active search.
#[derive(Debug, Clone, Copy)]
pub struct TimeControl {
    /// Initial base time, kept as a fallback if the live clock is zero.
    pub base_time: Duration,
    /// Initial increment, kept as a fallback if the live increment is zero.
    pub increment: Duration,
    /// White's remaining time on the live clock.
    pub wtime: Duration,
    /// Black's remaining time on the live clock.
    pub btime: Duration,
    /// White's per-move increment on the live clock.
    pub winc: Duration,
    /// Black's per-move increment on the live clock.
    pub binc: Duration,
    /// Moves remaining until the next time control, if specified.
    pub movestogo: Option<u32>,
    /// Fixed per-move time override (`go movetime`); when set, replaces
    /// the computed budget entirely.
    pub move_time: Option<Duration>,
    search_start: Option<Instant>,
    search_deadline: Option<Instant>,
}

impl TimeControl {
    /// Creates a [`TimeControl`] seeded with a single base/increment
    /// pair for both sides and no active search window.
    pub fn new(base_time: Duration, increment: Duration) -> Self {
        Self {
            base_time,
            increment,
            wtime: base_time,
            btime: base_time,
            winc: increment,
            binc: increment,
            movestogo: None,
            move_time: None,
            search_start: None,
            search_deadline: None,
        }
    }

    /// Refreshes the live clock fields from a `go` command. The base
    /// time and increment configured at construction are left
    /// untouched so they remain available as fallbacks.
    pub fn update_clock(
        &mut self,
        wtime: Duration,
        btime: Duration,
        winc: Duration,
        binc: Duration,
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

    /// Computes how long to spend on this move.
    ///
    /// - If `move_time` is set, returns it verbatim.
    /// - Otherwise: split `remaining_time` over `movestogo` (defaulting
    ///   to 20 when absent), add half of the increment, then subtract a
    ///   50ms safety buffer for UCI overhead.
    ///
    /// Zero-valued remaining time or increment are replaced by the
    /// initial `base_time`/`increment` to handle adapters that don't
    /// report a live clock.
    fn move_time_budget(&self, side_to_move: Color) -> Duration {
        if let Some(move_time) = self.move_time {
            return move_time;
        }

        let (remaining_time, increment) = match side_to_move {
            Color::White => (self.wtime, self.winc),
            Color::Black => (self.btime, self.binc),
        };

        let fallback_remaining = if remaining_time.is_zero() {
            self.base_time
        } else {
            remaining_time
        };
        let fallback_increment = if increment.is_zero() {
            self.increment
        } else {
            increment
        };

        let moves_left = self.movestogo.unwrap_or(20).max(1) as u128;
        let base_ms = fallback_remaining.as_millis();
        let inc_ms = fallback_increment.as_millis();
        let budget_ms = (base_ms / moves_left) + (inc_ms / 2);
        let budget_ms = budget_ms.max(1).min(u128::from(u64::MAX));

        // give ourselves a little extra time buffer to avoid time forfeits due to overhead
        let buffer_ms = 50;
        let budget_ms = budget_ms.saturating_sub(buffer_ms);

        Duration::from_millis(budget_ms as u64)
    }

    /// Starts a new search window: stamps the start instant and computes
    /// a deadline from the current clock state. Must be called before
    /// the search begins for
    /// [`is_time_up`](Self::is_time_up) and
    /// [`get_elapsed`](Self::get_elapsed) to behave correctly.
    pub fn set_search_deadline(&mut self, side_to_move: Color) {
        self.search_start = Some(Instant::now());
        let budget = self.move_time_budget(side_to_move);
        self.search_deadline = Some(Instant::now() + budget);
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

    /// Ends the active search window. After this,
    /// [`is_time_up`](Self::is_time_up) returns `false` until a new
    /// deadline is set.
    pub fn clear_search_deadline(&mut self) {
        self.search_start = None;
        self.search_deadline = None;
    }

    /// Returns `true` when the wall clock has passed the search
    /// deadline, or `false` if no deadline is currently set.
    pub fn is_time_up(&self) -> bool {
        match self.search_deadline {
            Some(dl) => Instant::now() >= dl,
            None => false,
        }
    }
}
