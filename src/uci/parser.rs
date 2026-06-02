use std::io::BufRead;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoCommand {
    pub depth: Option<u8>,
    pub movetime: Option<Duration>,
    pub wtime: Option<Duration>,
    pub btime: Option<Duration>,
    pub winc: Option<Duration>,
    pub binc: Option<Duration>,
    pub movestogo: Option<u32>,
    pub infinite: bool,
}

impl GoCommand {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UCICommand {
    Uci,
    IsReady,
    UciNewGame,
    Position {
        fen: Option<String>,
        moves: Vec<String>,
    },
    Go(GoCommand),
    Stop,
    PonderHit,
    SetOption {
        name: String,
        value: Option<String>,
    },
    Quit,
    Unknown(String),
}

impl UCICommand {
    pub fn read<R: BufRead>(reader: &mut R) -> Option<Self> {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim();
        match line {
            "" => None,
            _ => Some(Self::parse(line)),
        }
    }

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
            _ => Self::Unknown(line.to_string()),
        }
    }

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

        match tokens.peek().copied() {
            None => {}
            Some("moves") => {
                tokens.next();
            }
            _ => return Self::Unknown("position".to_string()),
        }

        let moves = tokens.map(str::to_string).collect();
        Self::Position { fen, moves }
    }

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

    fn next_value<'a, I>(tokens: &mut std::iter::Peekable<I>) -> Option<&'a str>
    where
        I: Iterator<Item = &'a str>,
    {
        tokens.next()
    }

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
