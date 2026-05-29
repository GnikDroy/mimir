use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use crate::core::*;
use crate::search::{SearchResult, Searcher};
use crate::state::GameState;

use super::io;
use super::parser::{GoCommand, UCICommand};

pub struct UCIAdapter {
    state: GameState,
    search_generation: Arc<AtomicU64>,
}

impl UCIAdapter {
    pub fn new() -> Self {
        Self {
            state: GameState::new(),
            search_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    fn next_generation(&self) -> u64 {
        self.search_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn handle_info(info: SearchResult) {
        let nodes = info.analytics.total_nodes();
        let nps = info.analytics.nodes_per_second();
        let time = info.analytics.elapsed.as_millis();
        match info.best_move {
            Some(best_move) => io::write(&format!(
                "info depth {} score cp {} time {} nodes {} nps {} pv {}",
                info.analytics.depth,
                info.evaluation,
                time,
                nodes,
                nps,
                best_move.repr_string()
            )),
            None => io::write(&format!(
                "info depth {} score cp {} time {} nodes {} nps {}",
                info.analytics.depth, info.evaluation, time, nodes, nps
            )),
        }
    }

    pub fn handle_command(&mut self, command: UCICommand) -> bool {
        match command {
            UCICommand::Uci => {
                io::write("id name chess_engine");
                io::write("id author gnikdroy");
                io::write("uciok");
            }
            UCICommand::IsReady => {
                io::write("readyok");
            }
            UCICommand::UciNewGame => {
                self.state = GameState::new();
                self.next_generation();
            }
            UCICommand::Position { fen, moves } => {
                self.next_generation();
                self.apply_position(fen, moves);
            }
            UCICommand::Go(go) => {
                self.launch_search(go);
            }
            UCICommand::Stop => {
                self.next_generation();
            }
            UCICommand::PonderHit => {}
            UCICommand::SetOption { .. } => {}
            UCICommand::Quit => {
                self.next_generation();
                return false;
            }
            UCICommand::Unknown(_) => {}
        }
        true
    }

    fn apply_position(&mut self, fen: Option<String>, moves: Vec<String>) {
        self.state = match fen {
            Some(fen) => GameState::from_fen(&fen).unwrap_or_else(|_| GameState::new()),
            None => GameState::new(),
        };

        for mv_text in moves {
            if let Some(mv) = self.parse_uci_move(&mv_text) {
                let _ = self.state.make_move(mv);
            }
        }
    }

    fn launch_search(&mut self, go: GoCommand) {
        let generation = self.next_generation();
        let generation_token = Arc::clone(&self.search_generation);
        let mut state = self.state;

        thread::spawn(move || {
            let mut searcher = Searcher::new();
            searcher.update_clock(
                go.wtime.unwrap_or(Duration::ZERO),
                go.btime.unwrap_or(Duration::ZERO),
                go.winc.unwrap_or(Duration::ZERO),
                go.binc.unwrap_or(Duration::ZERO),
                go.movestogo,
                go.movetime,
            );

            let depth = go.depth.unwrap_or(20);
            let result = searcher.search(&mut state, depth, Some(Self::handle_info));

            if generation_token.load(Ordering::SeqCst) == generation {
                match result.best_move {
                    Some(best_move) => io::write(&format!("bestmove {}", best_move.repr_string())),
                    None => io::write("bestmove 0000"),
                }
            }
        });
    }

    fn parse_uci_move(&mut self, uci: &str) -> Option<Move> {
        let mut legal_moves = Vec::with_capacity(256);
        self.state.generate_valid_moves(&mut legal_moves);
        legal_moves.into_iter().find(|mv| mv.repr_string() == uci)
    }
}
