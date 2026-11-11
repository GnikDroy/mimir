//! A search detached from the adapter.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::core::*;
use crate::search::{SearchLimits, SearchResult, Searcher};
use crate::state::GameState;
use crate::zobrist::ZobristHash;

use super::parser::GoCommand;
use super::response;

pub(super) struct SearchJob {
    searcher: Arc<Mutex<Searcher>>,
    out: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    generation_token: Arc<AtomicU64>,
    generation: u64,
    stop_flag: Arc<AtomicBool>,
    state: GameState,
    history: Vec<ZobristHash>,
    go: GoCommand,
}

impl SearchJob {
    /// Builds a fresh SearchJob
    pub(super) fn new(
        searcher: Arc<Mutex<Searcher>>,
        out: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
        generation_token: Arc<AtomicU64>,
        stop_flag: Arc<AtomicBool>,
        state: GameState,
        history: Vec<ZobristHash>,
        go: GoCommand,
    ) -> Self {
        let generation = generation_token.fetch_add(1, Ordering::SeqCst) + 1;
        stop_flag.store(false, Ordering::SeqCst);

        Self {
            searcher,
            out,
            generation_token,
            generation,
            stop_flag,
            state,
            history,
            go,
        }
    }

    /// Whether this was a `go infinite`, which runs until `stop`.
    pub(super) fn is_infinite(&self) -> bool {
        self.go.infinite
    }

    /// Whether this search is still the one the caller wants.
    pub(super) fn is_current(&self) -> bool {
        self.generation_token.load(Ordering::SeqCst) == self.generation
    }

    /// Whether we are awaiting a stop command (usually on go infinite)
    pub(super) fn awaiting_stop(&self) -> bool {
        !self.stop_flag.load(Ordering::SeqCst) && self.is_current()
    }

    /// Searches, emitting an `info` line per completed iteration.
    pub(super) fn search(&mut self) -> SearchResult {
        let mut searcher = self.searcher.lock().unwrap();

        let limits = SearchLimits {
            depth: if self.go.infinite {
                None
            } else {
                self.go.depth
            },
            nodes: self.go.nodes,
            wtime: self.go.wtime,
            btime: self.go.btime,
            winc: self.go.winc,
            binc: self.go.binc,
            movestogo: self.go.movestogo,
            movetime: self.go.movetime,
        };

        let out = Arc::clone(&self.out);
        searcher.search(
            &mut self.state,
            limits,
            &self.history,
            Some(move |info: SearchResult| {
                if let Ok(mut writer) = out.lock() {
                    // You can't get rid of ok() here.
                    // There channel we used to report errors just died ;)
                    response::write_info(info, &mut *writer).ok();
                }
            }),
        )
    }

    /// Emits `bestmove`, unless a later command superseded this search.
    pub(super) fn emit_bestmove(&self, result: &SearchResult) {
        if !self.is_current() {
            return;
        }

        let msg = match result.best_move() {
            Some(best_move) => format!("bestmove {}", best_move.to_uci()),
            None => "bestmove 0000".to_string(),
        };
        if let Ok(mut writer) = self.out.lock() {
            // You can't get rid of ok() here.
            // There channel we used to report errors just died ;)
            writeln!(writer, "{}", msg).ok();
        }
    }
}
