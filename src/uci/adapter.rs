//! Imperative side of the UCI loop.
//!
//! [`UCIAdapter`] owns the engine's mutable runtime state — the current
//! [`GameState`], a shared output writer, and a `search_generation`
//! counter — and executes parsed [`UCICommand`] values.
//!
//! Searches run on a worker thread so the main thread can keep reading
//! stdin (this is what makes `stop`, `position`, and `ucinewgame`
//! responsive while a search is running). Every command that invalidates
//! the in-flight search (`stop`, `position`, `ucinewgame`, `quit`)
//! increments `search_generation`; the worker only emits its
//! `bestmove`/`info` lines if the counter is still on the generation it
//! captured at launch. Stale workers finish their work silently.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use crate::bitboard::{BitBoard, BitBoardMethods};
use crate::core::*;
use crate::evaluation::evaluate;
use crate::search::{SearchResult, Searcher};
use crate::state::GameState;

use super::parser::{GoCommand, UCICommand};

/// Engine-side UCI runtime: position, generation counter, and writer.
///
/// The writer is shared (`Arc<Mutex<…>>`) between the main thread and
/// any in-flight search worker so both can emit lines without
/// interleaving. The generation counter is also shared so workers can
/// check whether their results are still relevant before printing.
pub struct UCIAdapter {
    state: GameState,
    search_generation: Arc<AtomicU64>,
    out: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
}

impl Default for UCIAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl UCIAdapter {
    /// Creates an adapter at the standard start position, writing UCI
    /// output to stdout.
    pub fn new() -> Self {
        Self {
            state: GameState::new(),
            search_generation: Arc::new(AtomicU64::new(0)),
            out: Arc::new(Mutex::new(Box::new(std::io::stdout()))),
        }
    }

    /// Like [`new`](Self::new), but routes all UCI output to `writer`
    /// instead of stdout. Used by tests to capture and assert on the
    /// emitted protocol.
    pub fn with_writer(writer: Box<dyn std::io::Write + Send>) -> Self {
        Self {
            state: GameState::new(),
            search_generation: Arc::new(AtomicU64::new(0)),
            out: Arc::new(Mutex::new(writer)),
        }
    }

    /// Bumps the generation counter and returns the new value.
    /// Any worker that launched against an older generation will see
    /// the bump and skip emitting `bestmove` / late `info` lines.
    fn next_generation(&self) -> u64 {
        self.search_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Formats a [`SearchResult`] as a single UCI `info` line and
    /// writes it to `writer`.
    ///
    /// `seldepth` is reported as the max of the iterative-deepening
    /// depth and the deepest quiescence ply reached. Mate scores are
    /// converted from plies-to-mate to moves-to-mate (the UCI convention).
    /// If no principal variation is available, falls back to printing
    /// just the root best move under `pv`.
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
            "info depth {} seldepth {} time {} nodes {} nps {} hashfull {} tbhits 0",
            info.analytics.depth,
            seldepth,
            time,
            nodes,
            nps,
            info.analytics.transposition_table_hashfull
        )?;

        match info.mate_in() {
            Some(ply) => {
                // UCI expects moves to mate, not plies until mate, therefore we divide by 2 and round up.
                let ply_to_mate = ((ply.abs() + 1) / 2) * ply.signum();
                write!(writer, " score mate {}", ply_to_mate)
            }
            None => write!(writer, " score cp {}", info.evaluation),
        }?;

        if !info.pv.is_empty() {
            write!(writer, " pv")?;
            for mv in &*info.pv {
                write!(writer, " {}", mv.to_uci())?;
            }
        } else if let Some(best_move) = info.best_move {
            // Fall back to the root best move when no PV is available
            // (e.g. early termination before any iteration completed).
            write!(writer, " pv {}", best_move.to_uci())?;
        }

        writeln!(writer)?;

        Ok(())
    }

    /// Dispatches one parsed UCI command and returns `true` if the
    /// runtime loop should keep going, or `false` on `quit`.
    ///
    /// Commands that invalidate any in-flight search
    /// (`stop`, `position`, `ucinewgame`, `quit`) bump the generation
    /// counter so the worker's eventual output is discarded.
    /// `go` spawns a search on a worker thread and returns immediately.
    /// `setoption`, `ponderhit`, and `Unknown` are accepted but ignored.
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
            UCICommand::Display => {
                let mut out = self.out.lock().unwrap();
                Self::write_display(&self.state, &mut *out).ok();
            }
            UCICommand::Eval => {
                let side = match self.state.side_to_move {
                    Color::White => "white",
                    Color::Black => "black",
                };
                let mut out = self.out.lock().unwrap();
                writeln!(out, "{} cp ({} to move)", evaluate(&self.state), side).ok();
            }
            UCICommand::Perft(depth) => {
                let mut out = self.out.lock().unwrap();
                Self::write_perft_divide(&mut self.state, depth, &mut *out).ok();
            }
            UCICommand::Unknown(_) => {}
        }
        true
    }

    /// Renders the position to `writer` as an ASCII board plus FEN and Zobrist key.
    fn write_display<W: std::io::Write>(state: &GameState, writer: &mut W) -> std::io::Result<()> {
        const SEPARATOR: &str = " +---+---+---+---+---+---+---+---+";

        for rank in Rank::all().rev() {
            writeln!(writer, "{}", SEPARATOR)?;
            write!(writer, " |")?;
            for file in File::all() {
                let square = Square::index((rank as u8) * 8u8 + file as u8);
                let ch = Self::piece_char_at(state, square).unwrap_or(' ');
                write!(writer, " {} |", ch)?;
            }
            writeln!(writer, " {}", rank as u8 + 1)?;
        }
        writeln!(writer, "{}", SEPARATOR)?;
        writeln!(writer, "   a   b   c   d   e   f   g   h")?;
        writeln!(writer)?;
        writeln!(writer, "Fen: {}", state.to_fen())?;
        writeln!(writer, "Key: {:016X}", state.zobrist_hash)?;
        Ok(())
    }

    /// Returns the FEN-style character for the piece occupying `square`,
    /// or `None` when the square is empty.
    fn piece_char_at(state: &GameState, square: Square) -> Option<char> {
        let mask = BitBoard::on(square);
        for color in Color::all() {
            for piece in Piece::all() {
                if state.pieces[color as usize][piece as usize] & mask != 0 {
                    let mut piece_char = match piece {
                        Piece::King => 'K',
                        Piece::Queen => 'Q',
                        Piece::Rook => 'R',
                        Piece::Bishop => 'B',
                        Piece::Knight => 'N',
                        Piece::Pawn => 'P',
                    };
                    if color == Color::Black {
                        piece_char = piece_char.to_ascii_lowercase();
                    }
                    return Some(piece_char);
                }
            }
        }
        None
    }

    /// Runs perft to `depth` from the current position and writes a
    /// Stockfish-style divided node count followed by the total.
    fn write_perft_divide<W: std::io::Write>(
        state: &mut GameState,
        depth: u8,
        writer: &mut W,
    ) -> std::io::Result<()> {
        let mut total = 0u64;
        if depth > 0 {
            for (mv, count) in state.perft_divide(depth) {
                if count == 0 {
                    continue;
                }
                writeln!(writer, "{}: {}", mv.to_uci(), count)?;
                total += count;
            }
        }
        writeln!(writer)?;
        writeln!(writer, "Nodes searched: {}", total)?;
        Ok(())
    }

    /// Resets the position to `fen` (or the standard start when
    /// `fen` is `None`), then applies each UCI move in `moves` in
    /// order. Invalid FEN falls back to the start position; moves
    /// that don't match a legal move are silently skipped.
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

    /// Spawns a search worker for `go` and returns immediately.
    ///
    /// The worker captures the bumped generation token at launch and
    /// only emits `bestmove` if the counter still matches when the
    /// search finishes; `info` callbacks always print (interleaving is
    /// prevented by the shared writer mutex). `go infinite` is mapped
    /// to a fixed depth of 32 since the engine relies on the time
    /// control / `stop` command to terminate iterative deepening.
    fn launch_search(&mut self, go: GoCommand) {
        let generation = self.next_generation();
        let generation_token = Arc::clone(&self.search_generation);
        let out = Arc::clone(&self.out);
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
                    Some(best_move) => format!("bestmove {}", best_move.to_uci()),
                    None => "bestmove 0000".to_string(),
                };
                if let Ok(mut writer) = out.lock() {
                    writeln!(writer, "{}", msg).unwrap();
                }
            }
        });
    }

    /// Resolves a UCI move string against the current position's legal
    /// moves and returns the matching [`Move`], or [`None`] if no legal
    /// move has that UCI encoding.
    fn parse_uci_move(&mut self, uci: &str) -> Option<Move> {
        let mut legal_moves = MoveList::default();
        self.state.generate_moves(&mut legal_moves);
        legal_moves.into_iter().find(|mv| mv.to_uci() == uci)
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
    fn test_handle_display_command() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let should_continue = adapter.handle_command(UCICommand::Display);
        assert!(should_continue);

        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert!(
            output_str.contains("Fen: rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
        );
        assert!(output_str.contains("Key:"));
        assert!(output_str.contains("a   b   c   d   e   f   g   h"));
    }

    #[test]
    fn test_handle_eval_command() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let should_continue = adapter.handle_command(UCICommand::Eval);
        assert!(should_continue);

        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert!(output_str.contains("cp"));
        assert!(output_str.contains("white to move"));
    }

    #[test]
    fn test_handle_perft_command() {
        let (writer, output) = SharedBuffer::new();
        let mut adapter = UCIAdapter::with_writer(Box::new(writer));

        let should_continue = adapter.handle_command(UCICommand::Perft(1));
        assert!(should_continue);

        let guard = output.lock().unwrap();
        let output_str = String::from_utf8_lossy(&guard);
        assert!(output_str.contains("Nodes searched: 20"));
    }

    #[test]
    fn test_handle_info_with_best_move() {
        let mut output = Vec::new();

        let mv: Move = Move::from_quiet(Square::E2, Square::E4, Piece::Pawn);

        let info = SearchResult {
            best_move: Some(mv),
            evaluation: 42,
            analytics: crate::search::SearchAnalytics::default(),
            pv: crate::search::PvList::default(),
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
