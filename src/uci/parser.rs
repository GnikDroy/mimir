use std::time::Duration;

use super::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoCommand {
    pub depth: Option<u8>,
    pub movetime: Option<Duration>,
    pub wtime: Option<Duration>,
    pub btime: Option<Duration>,
    pub winc: Option<Duration>,
    pub binc: Option<Duration>,
    pub movestogo: Option<u32>,
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
    pub fn read() -> Option<Self> {
        let line = io::read()?;
        Some(Self::parse(line.trim()))
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
