use std::time::{Duration, Instant};

use crate::core::Color;

#[derive(Debug, Clone, Copy)]
pub struct TimeControl {
    pub base_time: Duration,
    pub increment: Duration,
    pub wtime: Duration,
    pub btime: Duration,
    pub winc: Duration,
    pub binc: Duration,
    pub movestogo: Option<u32>,
    pub move_time: Option<Duration>,
    search_start: Option<Instant>,
    search_deadline: Option<Instant>,
}

impl TimeControl {
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
        let budget_ms = (base_ms / moves_left) + (inc_ms * 1 / 2);
        let budget_ms = budget_ms.max(1).min(u128::from(u64::MAX));

        // give ourselves a little extra time buffer to avoid time forfeits due to overhead
        let buffer_ms = 50;
        let budget_ms = budget_ms.saturating_sub(buffer_ms);

        Duration::from_millis(budget_ms as u64)
    }

    pub fn set_search_deadline(&mut self, side_to_move: Color) {
        self.search_start = Some(Instant::now());
        let budget = self.move_time_budget(side_to_move);
        self.search_deadline = Some(Instant::now() + budget);
    }

    pub fn get_elapsed(&self) -> Duration {
        match self.search_start {
            Some(start) => start.elapsed(),
            None => Duration::from_secs(1),
        }
    }

    pub fn clear_search_deadline(&mut self) {
        self.search_start = None;
        self.search_deadline = None;
    }

    pub fn is_time_up(&self) -> bool {
        match self.search_deadline {
            Some(dl) => Instant::now() >= dl,
            None => false,
        }
    }
}
