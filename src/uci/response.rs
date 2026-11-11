//! Rendering of UCI responses.

use std::io::Write;

use crate::core::*;
use crate::nnue::Cp;
use crate::notation::fen::piece_char;
use crate::search::SearchResult;
use crate::state::GameState;

/// Text printed by the `help` command.
pub const HELP: &str = "\
mimir is a UCI chess engine. Commands:

  uci                 identify and list options
  isready             reply readyok when idle
  ucinewgame          clear state for a new game
  setoption name <n> [value <v>]
  position [startpos | fen <fen>] [moves <m>...]
  go [depth N] [nodes N] [movetime ms] [wtime ms] [btime ms]
     [winc ms] [binc ms] [movestogo N] [infinite]
  go perft <depth>    count leaf nodes per root move
  stop                end the search, report bestmove
  quit                exit

Non-standard:
  display             board, FEN and Zobrist key
  eval                static evaluation
  flip                mirror the position onto the other colour
  perft <depth>       same as `go perft`
  bench [depth]       fixed-workload node count
  license             licensing information
  help                this message
";

/// Formats a [`SearchResult`] as a single UCI `info` line.
///
/// Mate scores are converted from plies to moves (UCI convention).
pub fn write_info<W: Write>(info: SearchResult, writer: &mut W) -> std::io::Result<()> {
    let nodes = info.analytics.total_nodes();
    let nps = info.analytics.nodes_per_second();
    let time = info.analytics.elapsed.as_millis();
    let seldepth = info.analytics.max_ply_reached.max(info.analytics.depth);

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
        None => write!(writer, " score cp {}", Cp::from_internal(info.evaluation)),
    }?;

    if !info.pv.is_empty() {
        write!(writer, " pv")?;
        for mv in &*info.pv {
            write!(writer, " {}", mv.to_uci())?;
        }
    }

    writeln!(writer)?;

    Ok(())
}

/// Renders the position as an ASCII board plus FEN and Zobrist key.
pub fn write_display<W: Write>(state: &GameState, writer: &mut W) -> std::io::Result<()> {
    const SEPARATOR: &str = " +---+---+---+---+---+---+---+---+";

    for rank in Rank::all().rev() {
        writeln!(writer, "{}", SEPARATOR)?;
        write!(writer, " |")?;
        for file in File::all() {
            let square = Square::index((rank as u8) * 8u8 + file as u8);
            let ch = match state.piece_at(square) {
                Some((color, piece)) => piece_char(color, piece),
                None => ' ',
            };
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

/// Runs perft to `depth` from `state` and writes a Stockfish-style
/// divided node count followed by the total.
pub fn write_perft_divide<W: Write>(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_info_with_pv() {
        let mut output = Vec::new();

        let mv: Move = Move::from_quiet(Square::E2, Square::E4, Piece::Pawn);
        let mut pv = crate::search::PvList::default();
        pv.push(mv);

        let info = SearchResult {
            evaluation: Cp(150).to_internal(),
            analytics: crate::analytics::SearchAnalytics::default(),
            pv,
        };

        write_info(info, &mut output).unwrap();

        let output_str = String::from_utf8_lossy(&output);
        assert!(output_str.contains("info depth"));
        assert!(output_str.contains("pv e2e4"));
        assert!(output_str.contains("score cp 150"));
    }

    #[test]
    fn test_write_display_board_agrees_with_fen() {
        const FEN: &str = "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 3 3";

        let state = GameState::from_fen(FEN).unwrap();
        let mut output = Vec::new();
        write_display(&state, &mut output).unwrap();
        let output_str = String::from_utf8_lossy(&output);

        assert!(output_str.contains(&format!("Fen: {}", FEN)));

        let rows: Vec<String> = output_str
            .lines()
            .filter(|line| line.starts_with(" |"))
            .map(|line| {
                line.split('|')
                    .skip(1)
                    .take(8)
                    .map(|cell| cell.trim())
                    .map(|cell| cell.chars().next().unwrap_or('1'))
                    .collect()
            })
            .collect();

        assert_eq!(rows.len(), 8);
        let expected: Vec<String> = FEN
            .split(' ')
            .next()
            .unwrap()
            .split('/')
            .map(|row| {
                row.chars()
                    .flat_map(|ch| match ch.to_digit(10) {
                        Some(n) => vec!['1'; n as usize],
                        None => vec![ch],
                    })
                    .collect()
            })
            .collect();
        assert_eq!(rows, expected);
    }

    #[test]
    fn test_write_perft_divide_startpos() {
        let mut state = GameState::new();
        let mut output = Vec::new();
        write_perft_divide(&mut state, 1, &mut output).unwrap();

        let output_str = String::from_utf8_lossy(&output);
        assert!(output_str.contains("Nodes searched: 20"));
        assert!(output_str.contains("e2e4: 1"));
    }

    #[test]
    fn test_help_lists_every_documented_command() {
        for command in ["uci", "isready", "position", "go perft", "display", "flip"] {
            assert!(HELP.contains(command), "help omits `{}`", command);
        }
    }
}
