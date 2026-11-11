//! Outcome of a search call.
//!
//! [`SearchStatus`] distinguishes a completed search from two abort
//! causes: an external `stop` and a time-control deadline.
//!
//! [`try_search!`] propagates the abort variants, standing in for the
//! `?` operator: implementing [`Try`](std::ops::Try) for a custom type
//! is still nightly-only

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

/// Unwraps a [`SearchStatus`]
///
/// What `?` does, spelled out. The enclosing function must
/// itself return a [`SearchStatus`]
#[macro_export]
macro_rules! try_search {
    ($status:expr) => {
        match $status {
            $crate::search_status::SearchStatus::Complete(value) => value,
            $crate::search_status::SearchStatus::Stopped => {
                return $crate::search_status::SearchStatus::Stopped;
            }
            $crate::search_status::SearchStatus::TimedOut => {
                return $crate::search_status::SearchStatus::TimedOut;
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_one(x: SearchStatus<i32>) -> SearchStatus<i32> {
        let v = crate::try_search!(x);
        SearchStatus::Complete(v + 1)
    }

    #[test]
    fn test_try_search_propagates_complete() {
        assert_eq!(
            add_one(SearchStatus::Complete(41)),
            SearchStatus::Complete(42)
        );
    }

    #[test]
    fn test_try_search_propagates_stopped() {
        assert_eq!(add_one(SearchStatus::Stopped), SearchStatus::Stopped);
    }

    #[test]
    fn test_try_search_propagates_timed_out() {
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
