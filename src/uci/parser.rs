//! Parser for UCI (Universal Chess Interface) commands.
//!
//! This module turns raw text lines from a GUI or test harness into the
//! strongly typed [`UCICommand`] variants consumed by
//! [`crate::uci::adapter`]. Parsing is intentionally permissive: unknown
//! tokens inside otherwise well-formed commands are skipped rather than
//! rejected, and any unrecognized input becomes [`UCICommand::Unknown`]
//! so the runtime loop can decide how to respond. This is per the UCI spec.
//!
//! See the UCI protocol specification for the canonical command grammar.

use std::io::BufRead;
use std::time::Duration;

/// Parameters extracted from a UCI `go` command.
///
/// Each field corresponds to an optional sub-token of `go`. Missing
/// tokens leave their field as [`None`] (or `false` for [`Self::infinite`]),
/// so the search layer can pick a sensible default per field.
///
/// Durations originate from millisecond integers in the protocol and are
/// stored as [`Duration`] for use with [`crate::time_control`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoCommand {
    /// Fixed search depth in plies (`go depth N`).
    pub depth: Option<u8>,
    /// Hard cap on the time spent searching this move (`go movetime N`).
    pub movetime: Option<Duration>,
    /// White's remaining clock time (`go wtime N`).
    pub wtime: Option<Duration>,
    /// Black's remaining clock time (`go btime N`).
    pub btime: Option<Duration>,
    /// White's increment per move (`go winc N`).
    pub winc: Option<Duration>,
    /// Black's increment per move (`go binc N`).
    pub binc: Option<Duration>,
    /// Moves remaining until the next time control (`go movestogo N`).
    pub movestogo: Option<u32>,
    /// `true` when the GUI sent `go infinite`; search runs until `stop`.
    pub infinite: bool,
}

impl GoCommand {
    /// Returns a [`GoCommand`] with every field unset.
    fn new() -> Self {
        Self {
            depth: None,
            movetime: None,
            wtime: None,
            btime: None,
            winc: None,
            binc: None,
            movestogo: None,
            infinite: false,
        }
    }
}

/// A single parsed UCI command.
///
/// Variants map one-to-one to the commands a GUI may send the engine over
/// stdin. Inputs that do not match a known command (or that are
/// syntactically malformed) are surfaced as [`UCICommand::Unknown`] with
/// the offending text attached, so the caller can log or ignore them
/// without aborting the protocol loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UCICommand {
    /// `uci` — handshake request; engine should reply with `id`/`uciok`.
    Uci,
    /// `isready` — engine should reply `readyok` when idle.
    IsReady,
    /// `ucinewgame` — clear per-game state (e.g. transposition table).
    UciNewGame,
    /// `position [startpos | fen <fen>] [moves <m1> <m2> ...]`.
    ///
    /// `fen` is [`None`] when `startpos` was used; otherwise it holds the
    /// six space-separated FEN fields rejoined into a single string.
    /// `moves` are UCI long-algebraic strings to apply on top.
    Position {
        /// FEN string to load, or [`None`] for the standard start position.
        fen: Option<String>,
        /// UCI moves to apply after loading the position.
        moves: Vec<String>,
    },
    /// `go ...` — start searching with the parameters in [`GoCommand`].
    Go(GoCommand),
    /// `stop` — terminate the current search as soon as possible.
    Stop,
    /// `ponderhit` — the opponent played the predicted move while pondering.
    PonderHit,
    /// `setoption name <name> [value <value>]`.
    ///
    /// Multi-word names and values are preserved verbatim with single
    /// spaces between tokens.
    SetOption {
        /// Option name; may contain spaces.
        name: String,
        /// Option value, or [`None`] for value-less options.
        value: Option<String>,
    },
    /// `quit` — exit the engine.
    Quit,
    /// `display` — non-standard extension; print the current board.
    Display,
    /// `eval` — non-standard extension; print the static evaluation.
    Eval,
    /// `perft <depth>` — non-standard extension; print perft divide.
    Perft(u8),
    /// Any input that did not parse as a known command. The wrapped
    /// string is the offending line (or a short tag identifying which
    /// sub-parse failed).
    Unknown(String),
}

impl UCICommand {
    /// Reads one line from `reader` and parses it as a [`UCICommand`].
    ///
    /// Returns [`None`] on EOF, on I/O error, or when the line is empty
    /// after trimming. A non-empty line always parses to `Some`, falling
    /// back to [`UCICommand::Unknown`] if no variant matches.
    pub fn read<R: BufRead>(reader: &mut R) -> Option<Self> {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim();
        match line {
            "" => None,
            _ => Some(Self::parse(line)),
        }
    }

    /// Parses a single UCI command line.
    pub fn parse(line: &str) -> Self {
        let mut parts = line.split_whitespace();
        let Some(command) = parts.next() else {
            return Self::Unknown(String::new());
        };

        match command {
            "uci" => Self::Uci,
            "isready" => Self::IsReady,
            "ucinewgame" => Self::UciNewGame,
            "position" => Self::parse_position(parts.collect()),
            "go" => Self::parse_go(parts.collect()),
            "stop" => Self::Stop,
            "ponderhit" => Self::PonderHit,
            "setoption" => Self::parse_setoption(parts.collect()),
            "quit" => Self::Quit,
            "display" => Self::Display,
            "eval" => Self::Eval,
            "perft" => Self::parse_perft(parts.collect()),
            _ => Self::Unknown(line.to_string()),
        }
    }

    /// Parses the tail of a `position` command (everything after the
    /// `position` keyword).
    ///
    /// Accepts `startpos` or `fen <6 fields>`, optionally followed by
    /// `moves <m1> <m2> ...`. The six FEN fields are required when `fen`
    /// is used and are rejoined with single spaces. Any deviation from
    /// this grammar produces an [`UCICommand::Unknown`] tagged with the
    /// sub-parse that failed.
    fn parse_position(tokens: Vec<&str>) -> Self {
        let mut tokens = tokens.into_iter().peekable();
        let Some(mode) = tokens.next() else {
            return Self::Unknown("position".to_string());
        };

        let fen = match mode {
            "startpos" => None,
            "fen" => {
                let Some(fen_fields) = Self::take_exact(&mut tokens, 6) else {
                    return Self::Unknown("position fen".to_string());
                };
                Some(fen_fields.join(" "))
            }
            _ => return Self::Unknown(format!("position {}", mode)),
        };

        // Spec: unknown tokens are skipped, not rejected. Scan until we
        // find `moves` (which introduces the move list) or run out.
        while let Some(&token) = tokens.peek() {
            if token == "moves" {
                tokens.next();
                break;
            }
            tokens.next();
        }

        let moves = tokens.map(str::to_string).collect();
        Self::Position { fen, moves }
    }

    /// Parses the tail of a `go` command into a [`GoCommand`].
    ///
    /// Sub-tokens are scanned in order; unknown tokens and unparseable
    /// numeric values are silently ignored, leaving the corresponding
    /// field at its default. `depth` is parsed as [`u8`], `movestogo` as
    /// [`u32`], and all time-related fields as milliseconds converted to
    /// [`Duration`]. `infinite` is a bare flag with no argument.
    fn parse_go(tokens: Vec<&str>) -> Self {
        let mut go = GoCommand::new();
        let mut tokens = tokens.into_iter().peekable();

        while let Some(token) = tokens.next() {
            match token {
                "depth" => {
                    go.depth = Self::next_value(&mut tokens).and_then(|value| value.parse().ok())
                }
                "movetime" => {
                    go.movetime = Self::next_value(&mut tokens)
                        .and_then(|value| value.parse::<u64>().ok())
                        .map(Duration::from_millis);
                }
                "wtime" => {
                    go.wtime = Self::next_value(&mut tokens)
                        .and_then(|value| value.parse::<u64>().ok())
                        .map(Duration::from_millis);
                }
                "btime" => {
                    go.btime = Self::next_value(&mut tokens)
                        .and_then(|value| value.parse::<u64>().ok())
                        .map(Duration::from_millis);
                }
                "winc" => {
                    go.winc = Self::next_value(&mut tokens)
                        .and_then(|value| value.parse::<u64>().ok())
                        .map(Duration::from_millis);
                }
                "binc" => {
                    go.binc = Self::next_value(&mut tokens)
                        .and_then(|value| value.parse::<u64>().ok())
                        .map(Duration::from_millis);
                }
                "movestogo" => {
                    go.movestogo =
                        Self::next_value(&mut tokens).and_then(|value| value.parse::<u32>().ok());
                }
                "infinite" => {
                    go.infinite = true;
                }
                _ => {}
            }
        }

        Self::Go(go)
    }

    /// Parses the tail of a `setoption` command.
    ///
    /// The grammar is `name <name tokens...> [value <value tokens...>]`.
    /// Name and value may contain spaces; tokens are rejoined with a
    /// single space. A missing leading `name` keyword yields
    /// [`UCICommand::Unknown`]. If `value` is absent the value field is
    /// [`None`].
    fn parse_setoption(tokens: Vec<&str>) -> Self {
        let mut tokens = tokens.into_iter().peekable();
        if tokens.next() != Some("name") {
            return Self::Unknown("setoption".to_string());
        }

        let mut name_parts = Vec::new();
        while let Some(token) = tokens.peek() {
            if *token == "value" {
                break;
            }
            name_parts.push(tokens.next().unwrap());
        }

        let value = if tokens.next() == Some("value") {
            let value_parts: Vec<&str> = tokens.collect();
            Some(value_parts.join(" "))
        } else {
            None
        };

        Self::SetOption {
            name: name_parts.join(" "),
            value,
        }
    }

    /// Parses the tail of a `perft` command into a depth value.
    fn parse_perft(tokens: Vec<&str>) -> Self {
        match tokens.first().and_then(|t| t.parse::<u8>().ok()) {
            Some(depth) => Self::Perft(depth),
            None => Self::Unknown("perft".to_string()),
        }
    }

    /// Pops the next token, used to read the argument that follows a
    /// keyword like `depth` or `movetime` in a `go` command.
    fn next_value<'a, I>(tokens: &mut std::iter::Peekable<I>) -> Option<&'a str>
    where
        I: Iterator<Item = &'a str>,
    {
        tokens.next()
    }

    /// Pops exactly `count` tokens from the iterator.
    ///
    /// Returns [`None`] if the iterator is exhausted before `count`
    /// tokens are collected, which `parse_position` uses to detect a
    /// truncated FEN.
    fn take_exact<'a, I>(tokens: &mut std::iter::Peekable<I>, count: usize) -> Option<Vec<&'a str>>
    where
        I: Iterator<Item = &'a str>,
    {
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(tokens.next()?);
        }
        Some(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_parse_uci() {
        let cmd = UCICommand::parse("uci");
        assert_eq!(cmd, UCICommand::Uci);
    }

    #[test]
    fn test_parse_isready() {
        let cmd = UCICommand::parse("isready");
        assert_eq!(cmd, UCICommand::IsReady);
    }

    #[test]
    fn test_parse_ucinewgame() {
        let cmd = UCICommand::parse("ucinewgame");
        assert_eq!(cmd, UCICommand::UciNewGame);
    }

    #[test]
    fn test_parse_stop() {
        let cmd = UCICommand::parse("stop");
        assert_eq!(cmd, UCICommand::Stop);
    }

    #[test]
    fn test_parse_ponderhit() {
        let cmd = UCICommand::parse("ponderhit");
        assert_eq!(cmd, UCICommand::PonderHit);
    }

    #[test]
    fn test_parse_quit() {
        let cmd = UCICommand::parse("quit");
        assert_eq!(cmd, UCICommand::Quit);
    }

    #[test]
    fn test_parse_position_startpos() {
        let cmd = UCICommand::parse("position startpos");
        assert_eq!(
            cmd,
            UCICommand::Position {
                fen: None,
                moves: vec![]
            }
        );
    }

    #[test]
    fn test_parse_position_startpos_with_moves() {
        let cmd = UCICommand::parse("position startpos moves e2e4 e7e5");
        assert_eq!(
            cmd,
            UCICommand::Position {
                fen: None,
                moves: vec!["e2e4".to_string(), "e7e5".to_string()]
            }
        );
    }

    #[test]
    fn test_parse_position_fen() {
        let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        let cmd = UCICommand::parse(&format!("position fen {}", fen));
        assert_eq!(
            cmd,
            UCICommand::Position {
                fen: Some(fen.to_string()),
                moves: vec![]
            }
        );
    }

    #[test]
    fn test_parse_position_skips_unknown_tokens_between_startpos_and_moves() {
        // Spec: unknown tokens should be ignored, not rejected.
        let cmd = UCICommand::parse("position startpos foo bar moves e2e4");
        assert_eq!(
            cmd,
            UCICommand::Position {
                fen: None,
                moves: vec!["e2e4".to_string()]
            }
        );
    }

    #[test]
    fn test_parse_position_skips_unknown_tokens_between_fen_and_moves() {
        let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        let cmd = UCICommand::parse(&format!("position fen {} junk moves e2e4", fen));
        assert_eq!(
            cmd,
            UCICommand::Position {
                fen: Some(fen.to_string()),
                moves: vec!["e2e4".to_string()]
            }
        );
    }

    #[test]
    fn test_parse_position_trailing_garbage_without_moves_is_ignored() {
        let cmd = UCICommand::parse("position startpos junk more junk");
        assert_eq!(
            cmd,
            UCICommand::Position {
                fen: None,
                moves: vec![]
            }
        );
    }

    #[test]
    fn test_parse_position_fen_with_moves() {
        let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        let cmd = UCICommand::parse(&format!("position fen {} moves e2e4", fen));
        assert_eq!(
            cmd,
            UCICommand::Position {
                fen: Some(fen.to_string()),
                moves: vec!["e2e4".to_string()]
            }
        );
    }

    #[test]
    fn test_parse_go_depth() {
        let cmd = UCICommand::parse("go depth 20");
        assert_eq!(
            cmd,
            UCICommand::Go(GoCommand {
                depth: Some(20),
                movetime: None,
                wtime: None,
                btime: None,
                winc: None,
                binc: None,
                movestogo: None,
                infinite: false,
            })
        );
    }

    #[test]
    fn test_parse_go_movetime() {
        let cmd = UCICommand::parse("go movetime 5000");
        assert_eq!(
            cmd,
            UCICommand::Go(GoCommand {
                depth: None,
                movetime: Some(Duration::from_millis(5000)),
                wtime: None,
                btime: None,
                winc: None,
                binc: None,
                movestogo: None,
                infinite: false,
            })
        );
    }

    #[test]
    fn test_parse_go_wtime_btime() {
        let cmd = UCICommand::parse("go wtime 300000 btime 300000");
        assert_eq!(
            cmd,
            UCICommand::Go(GoCommand {
                depth: None,
                movetime: None,
                wtime: Some(Duration::from_millis(300000)),
                btime: Some(Duration::from_millis(300000)),
                winc: None,
                binc: None,
                movestogo: None,
                infinite: false,
            })
        );
    }

    #[test]
    fn test_parse_go_with_increments() {
        let cmd = UCICommand::parse("go wtime 300000 btime 300000 winc 5000 binc 5000");
        assert_eq!(
            cmd,
            UCICommand::Go(GoCommand {
                depth: None,
                movetime: None,
                wtime: Some(Duration::from_millis(300000)),
                btime: Some(Duration::from_millis(300000)),
                winc: Some(Duration::from_millis(5000)),
                binc: Some(Duration::from_millis(5000)),
                movestogo: None,
                infinite: false,
            })
        );
    }

    #[test]
    fn test_parse_go_movestogo() {
        let cmd = UCICommand::parse("go wtime 300000 btime 300000 movestogo 40");
        assert_eq!(
            cmd,
            UCICommand::Go(GoCommand {
                depth: None,
                movetime: None,
                wtime: Some(Duration::from_millis(300000)),
                btime: Some(Duration::from_millis(300000)),
                winc: None,
                binc: None,
                movestogo: Some(40),
                infinite: false,
            })
        );
    }

    #[test]
    fn test_parse_setoption_with_value() {
        let cmd = UCICommand::parse("setoption name Hash value 256");
        assert_eq!(
            cmd,
            UCICommand::SetOption {
                name: "Hash".to_string(),
                value: Some("256".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_setoption_without_value() {
        let cmd = UCICommand::parse("setoption name Ponder");
        assert_eq!(
            cmd,
            UCICommand::SetOption {
                name: "Ponder".to_string(),
                value: None,
            }
        );
    }

    #[test]
    fn test_parse_setoption_multiword_name() {
        let cmd = UCICommand::parse("setoption name Some Option Name value some value");
        assert_eq!(
            cmd,
            UCICommand::SetOption {
                name: "Some Option Name".to_string(),
                value: Some("some value".to_string()),
            }
        );
    }

    #[test]
    fn test_parse_display() {
        let cmd = UCICommand::parse("display");
        assert_eq!(cmd, UCICommand::Display);
    }

    #[test]
    fn test_parse_eval() {
        let cmd = UCICommand::parse("eval");
        assert_eq!(cmd, UCICommand::Eval);
    }

    #[test]
    fn test_parse_perft() {
        let cmd = UCICommand::parse("perft 5");
        assert_eq!(cmd, UCICommand::Perft(5));
    }

    #[test]
    fn test_parse_perft_missing_depth() {
        let cmd = UCICommand::parse("perft");
        assert_eq!(cmd, UCICommand::Unknown("perft".to_string()));
    }

    #[test]
    fn test_parse_perft_invalid_depth() {
        let cmd = UCICommand::parse("perft abc");
        assert_eq!(cmd, UCICommand::Unknown("perft".to_string()));
    }

    #[test]
    fn test_parse_unknown_command() {
        let cmd = UCICommand::parse("unknown command");
        assert_eq!(cmd, UCICommand::Unknown("unknown command".to_string()));
    }

    #[test]
    fn test_read_from_cursor() {
        let input = "uci\n";
        let mut cursor = Cursor::new(input);
        let cmd = UCICommand::read(&mut cursor);
        assert_eq!(cmd, Some(UCICommand::Uci));
    }

    #[test]
    fn test_read_position_from_cursor() {
        let input = "position startpos moves e2e4\n";
        let mut cursor = Cursor::new(input);
        let cmd = UCICommand::read(&mut cursor);
        assert_eq!(
            cmd,
            Some(UCICommand::Position {
                fen: None,
                moves: vec!["e2e4".to_string()]
            })
        );
    }

    #[test]
    fn test_read_go_from_cursor() {
        let input = "go depth 15 movetime 3000\n";
        let mut cursor = Cursor::new(input);
        let cmd = UCICommand::read(&mut cursor);
        assert_eq!(
            cmd,
            Some(UCICommand::Go(GoCommand {
                depth: Some(15),
                movetime: Some(Duration::from_millis(3000)),
                wtime: None,
                btime: None,
                winc: None,
                binc: None,
                movestogo: None,
                infinite: false,
            }))
        );
    }

    #[test]
    fn test_read_multiple_commands() {
        let input = "uci\nisready\nquit\n";
        let mut cursor = Cursor::new(input);

        let cmd1 = UCICommand::read(&mut cursor);
        assert_eq!(cmd1, Some(UCICommand::Uci));

        let cmd2 = UCICommand::read(&mut cursor);
        assert_eq!(cmd2, Some(UCICommand::IsReady));

        let cmd3 = UCICommand::read(&mut cursor);
        assert_eq!(cmd3, Some(UCICommand::Quit));

        let cmd4 = UCICommand::read(&mut cursor);
        assert_eq!(cmd4, None);
    }

    #[test]
    fn test_read_empty_input() {
        let input = "";
        let mut cursor = Cursor::new(input);
        let cmd = UCICommand::read(&mut cursor);
        assert_eq!(cmd, None);
    }
}
