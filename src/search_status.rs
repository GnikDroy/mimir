//! Outcome of a search call.
//!
//! [`SearchStatus`] distinguishes a search that returned a value from
//! two abort causes: an external `stop` request and a time-control
//! deadline. The variants are kept separate so callers (UCI layer,
//! iterative deepening) can report and react appropriately.
//!
//! The `?` operator is supported via [`Try`] so search
//! internals can propagate abort reasons without boilerplate.
use std::convert::Infallible;
use std::ops::{ControlFlow, FromResidual, Residual, Try};

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchStatus<T> {
    /// The call ran to a usable result.
    Complete(T),
    /// An external `stop` signal aborted the search.
    Stopped,
    /// The hard time-control deadline was hit.
    TimedOut,
}

impl<T> SearchStatus<T> {
    /// Transforms the carried value, leaving abort variants untouched.
    #[inline]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> SearchStatus<U> {
        match self {
            SearchStatus::Complete(t) => SearchStatus::Complete(f(t)),
            SearchStatus::Stopped => SearchStatus::Stopped,
            SearchStatus::TimedOut => SearchStatus::TimedOut,
        }
    }
}

impl<T> Try for SearchStatus<T> {
    type Output = T;
    type Residual = SearchStatus<Infallible>;

    #[inline]
    fn from_output(output: T) -> Self {
        SearchStatus::Complete(output)
    }

    #[inline]
    fn branch(self) -> ControlFlow<Self::Residual, T> {
        match self {
            SearchStatus::Complete(t) => ControlFlow::Continue(t),
            SearchStatus::Stopped => ControlFlow::Break(SearchStatus::Stopped),
            SearchStatus::TimedOut => ControlFlow::Break(SearchStatus::TimedOut),
        }
    }
}

impl<T> FromResidual<SearchStatus<Infallible>> for SearchStatus<T> {
    #[inline]
    fn from_residual(residual: SearchStatus<Infallible>) -> Self {
        match residual {
            SearchStatus::Stopped => SearchStatus::Stopped,
            SearchStatus::TimedOut => SearchStatus::TimedOut,
            // `Infallible` has no inhabitants — `Complete` is unreachable.
            SearchStatus::Complete(never) => match never {},
        }
    }
}

impl<T> Residual<T> for SearchStatus<Infallible> {
    type TryType = SearchStatus<T>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_one(x: SearchStatus<i32>) -> SearchStatus<i32> {
        let v = x?;
        SearchStatus::Complete(v + 1)
    }

    #[test]
    fn test_try_propagates_complete() {
        assert_eq!(
            add_one(SearchStatus::Complete(41)),
            SearchStatus::Complete(42)
        );
    }

    #[test]
    fn test_try_propagates_stopped() {
        assert_eq!(add_one(SearchStatus::Stopped), SearchStatus::Stopped);
    }

    #[test]
    fn test_try_propagates_timed_out() {
        assert_eq!(add_one(SearchStatus::TimedOut), SearchStatus::TimedOut);
    }

    #[test]
    fn test_map_complete() {
        let s: SearchStatus<i32> = SearchStatus::Complete(5);
        assert_eq!(s.map(|v| v * 2), SearchStatus::Complete(10));
    }

    #[test]
    fn test_map_leaves_aborts_alone() {
        let s: SearchStatus<i32> = SearchStatus::Stopped;
        assert_eq!(s.map(|v| v * 2), SearchStatus::Stopped);
        let s: SearchStatus<i32> = SearchStatus::TimedOut;
        assert_eq!(s.map(|v| v * 2), SearchStatus::TimedOut);
    }
}
