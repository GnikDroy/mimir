use crate::state::*;

#[derive(Debug, Clone)]
pub struct EPDInfo {
    pub state: GameState,
    pub id: Option<String>,
    pub best_moves: Vec<String>,
}

impl EPDInfo {
    pub fn from_epd(epd: &str) -> Result<Self, String> {
        let trimmed = epd.trim();

        // The first four whitespace-separated tokens are the position fields
        // (board, side-to-move, castling, en-passant). The remainder is a
        // sequence of operations.
        let mut iter = trimmed.splitn(5, char::is_whitespace);
        let board = iter.next().ok_or("Invalid EPD: missing board")?;
        let side = iter.next().ok_or("Invalid EPD: missing side to move")?;
        let castling = iter.next().ok_or("Invalid EPD: missing castling rights")?;
        let ep = iter.next().ok_or("Invalid EPD: missing en passant")?;
        let rest = iter.next().unwrap_or("");

        let ops = split_operations(rest)?;

        // EPD does not carry halfmove clock or fullmove number in the position
        // fields. They default to 0 and 1, and are overridden by hmvc / fmvn.
        let mut halfmove_clock: u8 = 0;
        let mut fullmove_number: u16 = 1;
        let mut id: Option<String> = None;
        let mut best_moves: Vec<String> = Vec::new();

        for op in &ops {
            let mut parts = op.splitn(2, char::is_whitespace);
            let opcode = parts.next().unwrap();
            let operands = parts.next().unwrap_or("").trim();

            match opcode {
                "hmvc" => {
                    halfmove_clock = operands
                        .parse()
                        .map_err(|_| format!("Invalid hmvc operand: {}", operands))?;
                }
                "fmvn" => {
                    fullmove_number = operands
                        .parse()
                        .map_err(|_| format!("Invalid fmvn operand: {}", operands))?;
                }
                "id" => {
                    id = Some(strip_quotes(operands).to_string());
                }
                "bm" => {
                    best_moves = operands
                        .split_whitespace()
                        .map(|m| m.trim_end_matches(['+', '#']).to_string())
                        .collect();
                }
                _ => {} // ignore unknown / unsupported opcodes
            }
        }

        let fen = format!("{} {} {} {} {} {}", board, side, castling, ep, halfmove_clock, fullmove_number);
        let state = GameState::from_fen(&fen)?;

        Ok(EPDInfo {
            state,
            id,
            best_moves,
        })
    }
}

/// Split an EPD operations string on `;`, respecting quoted strings so
/// semicolons inside `"..."` or `'...'` do not terminate an operation.
fn split_operations(s: &str) -> Result<Vec<String>, String> {
    let mut ops = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for c in s.chars() {
        match quote {
            Some(q) => {
                current.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    current.push(c);
                }
                ';' => {
                    let op = current.trim();
                    if !op.is_empty() {
                        ops.push(op.to_string());
                    }
                    current.clear();
                }
                _ => current.push(c),
            },
        }
    }

    if quote.is_some() {
        return Err("Invalid EPD: unterminated quoted string".to_string());
    }
    if !current.trim().is_empty() {
        return Err(format!("Invalid EPD: trailing operation without ';': {}", current.trim()));
    }

    Ok(ops)
}

fn strip_quotes(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &s[1..s.len() - 1];
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_epd_basic_wac() {
        let epd = "5rk1/1ppb3p/p1pb4/6q1/3P1p1r/2P1R2P/PP1BQ1P1/5RKN w - - bm Rg3; id \"WAC.003\";";
        let info = EPDInfo::from_epd(epd).unwrap();
        assert_eq!(info.id.as_deref(), Some("WAC.003"));
        assert_eq!(info.best_moves, vec!["Rg3".to_string()]);
        assert_eq!(info.state.halfmove_clock, 0);
        assert_eq!(info.state.fullmove_number, 1);
    }

    #[test]
    fn test_parse_epd_multiple_best_moves() {
        let epd = "5rk1/1ppb3p/p1pb4/6q1/3P1p1r/2P1R2P/PP1BQ1P1/5RKN w - - bm Rg3 Qxg2+; id \"X\";";
        let info = EPDInfo::from_epd(epd).unwrap();
        assert_eq!(info.best_moves, vec!["Rg3".to_string(), "Qxg2".to_string()]);
    }

    #[test]
    fn test_parse_epd_hmvc_fmvn() {
        let epd = "5rk1/1ppb3p/p1pb4/6q1/3P1p1r/2P1R2P/PP1BQ1P1/5RKN w - - hmvc 7; fmvn 42; id \"Z\";";
        let info = EPDInfo::from_epd(epd).unwrap();
        assert_eq!(info.state.halfmove_clock, 7);
        assert_eq!(info.state.fullmove_number, 42);
    }

    #[test]
    fn test_parse_epd_ignores_unknown_operations() {
        let epd = "5rk1/1ppb3p/p1pb4/6q1/3P1p1r/2P1R2P/PP1BQ1P1/5RKN w - - bm Rg3; ce 100; acd 12; id \"WAC.003\";";
        let info = EPDInfo::from_epd(epd).unwrap();
        assert_eq!(info.id.as_deref(), Some("WAC.003"));
        assert_eq!(info.best_moves, vec!["Rg3".to_string()]);
    }

    #[test]
    fn test_parse_epd_semicolon_inside_quotes() {
        let epd = "5rk1/1ppb3p/p1pb4/6q1/3P1p1r/2P1R2P/PP1BQ1P1/5RKN w - - id \"tricky; id\"; bm Rg3;";
        let info = EPDInfo::from_epd(epd).unwrap();
        assert_eq!(info.id.as_deref(), Some("tricky; id"));
        assert_eq!(info.best_moves, vec!["Rg3".to_string()]);
    }

    #[test]
    fn test_parse_epd_strips_check_markers_from_bm() {
        let epd = "r1bq2rk/pp3pbp/2p1p1pQ/7P/3P4/2PB1N2/PP3PPR/2KR4 w - - bm Qxh7+; id \"WAC.004\";";
        let info = EPDInfo::from_epd(epd).unwrap();
        assert_eq!(info.best_moves, vec!["Qxh7".to_string()]);
    }

    #[test]
    fn test_parse_epd_strips_check_mate_markers_from_bm() {
        let epd = "r1bq2rk/pp3pbp/2p1p1pQ/7P/3P4/2PB1N2/PP3PPR/2KR4 w - - bm Qxh7#; id \"WAC.004\";";
        let info = EPDInfo::from_epd(epd).unwrap();
        assert_eq!(info.best_moves, vec!["Qxh7".to_string()]);
    }

    #[test]
    fn test_parse_epd_no_operations() {
        let epd = "5rk1/1ppb3p/p1pb4/6q1/3P1p1r/2P1R2P/PP1BQ1P1/5RKN w - -";
        let info = EPDInfo::from_epd(epd).unwrap();
        assert!(info.id.is_none());
        assert!(info.best_moves.is_empty());
    }

    #[test]
    fn test_parse_epd_unterminated_quote_errors() {
        let epd = "5rk1/1ppb3p/p1pb4/6q1/3P1p1r/2P1R2P/PP1BQ1P1/5RKN w - - id \"oops;";
        assert!(EPDInfo::from_epd(epd).is_err());
    }

    #[test]
    fn test_parse_epd_missing_terminator_errors() {
        let epd = "5rk1/1ppb3p/p1pb4/6q1/3P1p1r/2P1R2P/PP1BQ1P1/5RKN w - - bm Rg3";
        assert!(EPDInfo::from_epd(epd).is_err());
    }
}
