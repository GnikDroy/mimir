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
//! avoid losing on time due to UCI overhead.
//!
//! This strategy is competitive:
//! [Chess Programming Wiki](https://www.chessprogramming.org/Time_Management)

use std::time::{Duration, Instant};

use crate::core::Color;

/// UCI clock state plus the current search deadline.
///
/// Clock fields (`wtime`/`btime`/`winc`/`binc`/`movestogo`/`move_time`)
/// are refreshed per `go` command via
/// [`update_clock`](Self::update_clock). 
/// Multiple timing fields may be present at once; the search deadline
/// takes the *strictest* of the resulting candidate budgets.
///
/// `search_start` and `search_deadline` are managed by
/// [`set_search_deadline`](Self::set_search_deadline) and
/// [`clear_search_deadline`](Self::clear_search_deadline); they are
/// `None` outside an active search. A `None` `search_deadline` during
/// an active search means no time bound (search runs to depth or until
/// `stop`).
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
    /// the computed budget entirely.
    pub move_time: Option<Duration>,
    search_start: Option<Instant>,
    search_deadline: Option<Instant>,
}

impl TimeControl {
    /// Creates a [`TimeControl`]
    pub fn new() -> Self {
        Self {
            wtime: None,
            btime: None,
            winc: None,
            binc: None,
            movestogo: None,
            move_time: None,
            search_start: None,
            search_deadline: None,
        }
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

    /// Computes how long to spend on this move, adhering to every timing
    /// field present on the `go` command and returning the strictest
    /// (smallest) of them. Returns `None` when no timing field was set
    /// — it runs to depth or until `stop`.
    ///
    /// Candidate budgets:
    /// - `move_time` (from `go movetime`) — used verbatim.
    /// - clock budget (from `wtime`/`btime` for the side to move)
    ///
    /// A safety buffer is subtracted from the chosen budget to
    /// cover UCI round-trip overhead.
    fn move_time_budget(&self, side_to_move: Color) -> Option<Duration> {
        const SAFETY_BUFFER: Duration = Duration::from_millis(25);

        let (remaining, increment) = match side_to_move {
            Color::White => (self.wtime, self.winc),
            Color::Black => (self.btime, self.binc),
        };

        let clock_budget = remaining.map(|rem| {
            let moves_left = self.movestogo.unwrap_or(20).max(1);
            let inc = increment.unwrap_or(Duration::ZERO);
            rem / moves_left + inc / 2
        });

        [self.move_time, clock_budget]
            .into_iter()
            .flatten()
            .min()
            .map(|b| b.saturating_sub(SAFETY_BUFFER))
    }

    /// Starts a new search window: stamps the start instant and computes
    /// a deadline from the current clock state. A `None` deadline means
    /// no time bound was supplied; [`is_time_up`](Self::is_time_up) will
    /// keep returning `false` and the search runs to depth.
    pub fn set_search_deadline(&mut self, side_to_move: Color) {
        let now = Instant::now();
        self.search_start = Some(now);
        self.search_deadline = self.move_time_budget(side_to_move).map(|b| now + b);
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

impl Default for TimeControl {
    fn default() -> Self {
        TimeControl::new()
    }
}
