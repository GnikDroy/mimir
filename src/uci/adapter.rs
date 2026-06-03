use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use crate::core::*;
use crate::search::{SearchResult, Searcher};
use crate::state::GameState;

use super::parser::{GoCommand, UCICommand};

pub struct UCIAdapter {
    state: GameState,
    search_generation: Arc<AtomicU64>,
    out: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
}

impl UCIAdapter {
    pub fn new() -> Self {
        Self {
            state: GameState::new(),
            search_generation: Arc::new(AtomicU64::new(0)),
            out: Arc::new(Mutex::new(Box::new(std::io::stdout()))),
        }
    }

    pub fn with_writer(writer: Box<dyn std::io::Write + Send>) -> Self {
        Self {
            state: GameState::new(),
            search_generation: Arc::new(AtomicU64::new(0)),
            out: Arc::new(Mutex::new(writer)),
        }
    }

    fn next_generation(&self) -> u64 {
        self.search_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn handle_info<W: std::io::Write>(
        info: SearchResult,
        writer: &mut W,
    ) -> std::io::Result<()> {
        let nodes = info.analytics.total_nodes();
        let nps = info.analytics.nodes_per_second();
        let time = info.analytics.elapsed.as_millis();
        let seldepth = info
            .analytics
            .max_quiescence_depth_reached
            .max(info.analytics.depth);

        write!(
            writer,
            "info depth {} seldepth {} time {} nodes {} nps {} tbhits 0",
            info.analytics.depth, seldepth, time, nodes, nps
        )?;

        match info.mate_in() {
            Some(ply) => {
                // UCI expects moves to mate, not plies until mate, therefore we divide by 2 and round up.
                let ply_to_mate = ((ply.abs() + 1) / 2) * ply.signum();
                write!(writer, " score mate {}", ply_to_mate)
            }
            None => write!(writer, " score cp {}", info.evaluation),
        }?;

        if let Some(best_move) = info.best_move {
            write!(writer, " pv {}", best_move.repr_string())?;
        }

        write!(writer, "\n")?;

        Ok(())
    }

    pub fn handle_command(&mut self, command: UCICommand) -> bool {
        match command {
            UCICommand::Uci => {
                let mut out = self.out.lock().unwrap();
                writeln!(out, "id name chess_engine").ok();
                writeln!(out, "id author gnikdroy").ok();
                writeln!(out, "uciok").ok();
            }
            UCICommand::IsReady => {
                let mut out = self.out.lock().unwrap();
                writeln!(out, "readyok").ok();
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
        let out = Arc::clone(&self.out);
        let mut state = self.state.clone();

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

            let depth = if go.infinite {
                32
            } else {
                go.depth.unwrap_or(32)
            };
            let result = searcher.search(
                &mut state,
                depth,
                Some(|info: SearchResult| {
                    if let Ok(mut writer) = out.lock() {
                        Self::handle_info(info, &mut *writer).unwrap();
                    }
                }),
            );

            if generation_token.load(Ordering::SeqCst) == generation {
                let msg = match result.best_move {
                    Some(best_move) => format!("bestmove {}", best_move.repr_string()),
                    None => "bestmove 0000".to_string(),
                };
                if let Ok(mut writer) = out.lock() {
                    writeln!(writer, "{}", msg).unwrap();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct SharedBuffer {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl SharedBuffer {
        fn new() -> (Self, Arc<Mutex<Vec<u8>>>) {
            let buffer = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    buffer: buffer.clone(),
                },
                buffer,
            )
        }
    }

    impl std::io::Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.buffer.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_handle_uci_command() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let should_continue = adapter.handle_command(UCICommand::Uci);
        assert!(should_continue);

        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert!(output_str.contains("id name chess_engine"));
        assert!(output_str.contains("id author gnikdroy"));
        assert!(output_str.contains("uciok"));
    }

    #[test]
    fn test_handle_isready_command() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let should_continue = adapter.handle_command(UCICommand::IsReady);
        assert!(should_continue);

        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert!(output_str.contains("readyok"));
    }

    #[test]
    fn test_handle_ucinewgame_command() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let should_continue = adapter.handle_command(UCICommand::UciNewGame);
        assert!(should_continue);

        // UciNewGame doesn't produce output
        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert_eq!(output_str.trim(), "");
    }

    #[test]
    fn test_handle_position_startpos() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let should_continue = adapter.handle_command(UCICommand::Position {
            fen: None,
            moves: vec![],
        });
        assert!(should_continue);

        // Position doesn't produce output
        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert_eq!(output_str.trim(), "");
    }

    #[test]
    fn test_handle_position_startpos_with_moves() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let should_continue = adapter.handle_command(UCICommand::Position {
            fen: None,
            moves: vec!["e2e4".to_string(), "e7e5".to_string()],
        });
        assert!(should_continue);

        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert_eq!(output_str.trim(), "");
    }

    #[test]
    fn test_handle_position_fen() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
        let should_continue = adapter.handle_command(UCICommand::Position {
            fen: Some(fen.to_string()),
            moves: vec![],
        });
        assert!(should_continue);

        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert_eq!(output_str.trim(), "");
    }

    #[test]
    fn test_handle_stop_command() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let should_continue = adapter.handle_command(UCICommand::Stop);
        assert!(should_continue);

        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert_eq!(output_str.trim(), "");
    }

    #[test]
    fn test_handle_ponderhit_command() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let should_continue = adapter.handle_command(UCICommand::PonderHit);
        assert!(should_continue);

        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert_eq!(output_str.trim(), "");
    }

    #[test]
    fn test_handle_setoption_command() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let should_continue = adapter.handle_command(UCICommand::SetOption {
            name: "Hash".to_string(),
            value: Some("256".to_string()),
        });
        assert!(should_continue);

        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert_eq!(output_str.trim(), "");
    }

    #[test]
    fn test_handle_unknown_command() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let should_continue = adapter.handle_command(UCICommand::Unknown("foo bar".to_string()));
        assert!(should_continue);

        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert_eq!(output_str.trim(), "");
    }

    #[test]
    fn test_handle_quit_command() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let should_continue = adapter.handle_command(UCICommand::Quit);
        assert!(!should_continue);

        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert_eq!(output_str.trim(), "");
    }

    #[test]
    fn test_next_generation() {
        let (writer, _) = SharedBuffer::new();
        let adapter = UCIAdapter::with_writer(Box::new(writer));

        let gen1 = adapter.next_generation();
        let gen2 = adapter.next_generation();
        let gen3 = adapter.next_generation();

        assert_eq!(gen1, 1);
        assert_eq!(gen2, 2);
        assert_eq!(gen3, 3);
    }

    #[test]
    fn test_command_sequence() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        // UCI handshake
        let cont = adapter.handle_command(UCICommand::Uci);
        assert!(cont);

        // IsReady
        let cont = adapter.handle_command(UCICommand::IsReady);
        assert!(cont);

        // New game
        let cont = adapter.handle_command(UCICommand::UciNewGame);
        assert!(cont);

        // Position
        let cont = adapter.handle_command(UCICommand::Position {
            fen: None,
            moves: vec![],
        });
        assert!(cont);

        // Quit
        let cont = adapter.handle_command(UCICommand::Quit);
        assert!(!cont);

        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert!(output_str.contains("id name chess_engine"));
        assert!(output_str.contains("id author gnikdroy"));
        assert!(output_str.contains("uciok"));
        assert!(output_str.contains("readyok"));
    }

    #[test]
    fn test_handle_info_with_best_move() {
        let mut output = Vec::new();

        let mv: Move = Move::from_quiet(Square::E2, Square::E4, Piece::Pawn);

        let info = SearchResult {
            best_move: Some(mv),
            evaluation: 42,
            analytics: crate::search::SearchAnalytics::default(),
        };

        let result = UCIAdapter::handle_info(info, &mut output);
        assert!(result.is_ok());

        let output_str = String::from_utf8_lossy(&output);
        println!("Output: {}", output_str);
        assert!(output_str.contains("info depth"));
        assert!(output_str.contains("pv e2e4"));
        assert!(output_str.contains("score cp 42"));
    }
}
