/// Simple NNUE incremental evaluation function
///
/// Reference from: https://github.com/jw1912/bullet/blob/main/examples/simple.rs
use crate::bitboard::BitBoardMethods;
use crate::core::*;
use crate::state::GameState;

const HIDDEN_SIZE: usize = 64;
pub const SCALE: i32 = 400;
const QA: i16 = 255;
const QB: i16 = 64;

/// Multiplier from centipawn-style PIECE_VALUES into this network's eval units.
pub const NNUE_PAWN_SCALE: i32 = SCALE / 100;

pub static NNUE: Network =
    unsafe { std::mem::transmute(*include_bytes!("../resources/beans.bin")) };

#[inline]
/// Square Clipped ReLU - Activation Function.
/// Note that this takes the i16s in the accumulator to i32s.
/// Range is 0.0 .. 1.0 (in other words, 0 to QA*QA quantized).
fn screlu(x: i16) -> i32 {
    let y = i32::from(x).clamp(0, i32::from(QA));
    y * y
}

/// This is the quantised format that bullet outputs.
#[repr(C)]
pub struct Network {
    /// Column-Major `HIDDEN_SIZE x 768` matrix.
    /// Values have quantization of QA.
    feature_weights: [Accumulator; 768],
    /// Vector with dimension `HIDDEN_SIZE`.
    /// Values have quantization of QA.
    feature_bias: Accumulator,
    /// Column-Major `1 x (2 * HIDDEN_SIZE)`
    /// matrix, we use it like this to make the
    /// code nicer in `Network::evaluate`.
    /// Values have quantization of QB.
    output_weights: [i16; 2 * HIDDEN_SIZE],
    /// Scalar output bias.
    /// Value has quantization of QA * QB.
    output_bias: i16,
}

impl Network {
    /// Calculates the output of the network, starting from the already
    /// calculated hidden layer (done efficiently during makemoves).
    pub fn evaluate(&self, us: &Accumulator, them: &Accumulator) -> i32 {
        // Initialise output.
        let mut output = 0;

        // Side-To-Move Accumulator -> Output.
        for (&input, &weight) in us.vals.iter().zip(&self.output_weights[..HIDDEN_SIZE]) {
            output += screlu(input) * i32::from(weight);
        }

        // Not-Side-To-Move Accumulator -> Output.
        for (&input, &weight) in them.vals.iter().zip(&self.output_weights[HIDDEN_SIZE..]) {
            output += screlu(input) * i32::from(weight);
        }

        // Reduce quantization from QA * QA * QB to QA * QB.
        output /= i32::from(QA);

        // Add bias.
        output += i32::from(self.output_bias);

        // Apply eval scale.
        output *= SCALE;

        // Remove quantisation altogether.
        output /= i32::from(QA) * i32::from(QB);

        output
    }

    /// Maps this codebase's [`Piece`] ordering to the bullet-trained
    /// network's piece index (`Pawn=0, Knight=1, Bishop=2, Rook=3, Queen=4, King=5`).
    #[inline(always)]
    const fn piece_index(piece: Piece) -> usize {
        match piece {
            Piece::Pawn => 0,
            Piece::Knight => 1,
            Piece::Bishop => 2,
            Piece::Rook => 3,
            Piece::Queen => 4,
            Piece::King => 5,
        }
    }

    /// Feature index in the 768-input `(perspective, piece_color, piece, square)`
    /// encoding. Own-side pieces occupy `[0, 384)`, opponent pieces
    /// `[384, 768)`. From black's perspective the square is mirrored
    /// vertically.
    #[inline(always)]
    fn feature_index(
        perspective: Color,
        piece_color: Color,
        piece: Piece,
        square: Square,
    ) -> usize {
        let color_offset = if perspective == piece_color { 0 } else { 384 };
        let piece_offset = Self::piece_index(piece) * 64;
        let sq = match perspective {
            Color::White => square as usize,
            Color::Black => square.flip_vertical() as usize,
        };
        color_offset + piece_offset + sq
    }
}

/// A column of the feature-weights matrix.
/// Note the `align(64)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct Accumulator {
    vals: [i16; HIDDEN_SIZE],
}

impl Accumulator {
    /// Initialised with bias so we can just efficiently
    /// operate on it afterwards.
    pub fn new(net: &Network) -> Self {
        net.feature_bias
    }

    /// Adds the feature for a piece of `piece_color` on `square` to this
    /// accumulator. `perspective` is the side from whose viewpoint this
    /// accumulator is maintained.
    #[inline]
    pub fn add_piece(
        &mut self,
        net: &Network,
        perspective: Color,
        piece_color: Color,
        piece: Piece,
        square: Square,
    ) {
        self.add_feature(
            Network::feature_index(perspective, piece_color, piece, square),
            net,
        );
    }

    /// Removes the feature for a piece of `piece_color` on `square` from
    /// this accumulator.
    #[inline]
    pub fn remove_piece(
        &mut self,
        net: &Network,
        perspective: Color,
        piece_color: Color,
        piece: Piece,
        square: Square,
    ) {
        self.remove_feature(
            Network::feature_index(perspective, piece_color, piece, square),
            net,
        );
    }

    /// Add a feature by raw input index.
    #[inline]
    fn add_feature(&mut self, feature_idx: usize, net: &Network) {
        for (i, d) in self
            .vals
            .iter_mut()
            .zip(&net.feature_weights[feature_idx].vals)
        {
            *i += *d
        }
    }

    /// Remove a feature by raw input index.
    #[inline]
    fn remove_feature(&mut self, feature_idx: usize, net: &Network) {
        for (i, d) in self
            .vals
            .iter_mut()
            .zip(&net.feature_weights[feature_idx].vals)
        {
            *i -= *d
        }
    }
}

/// A bias-initialised pair of accumulators, one per perspective
/// (`[Color::White as usize]` = white-to-move, `[Color::Black as usize]` = black).
/// Used as the starting point before any piece features are added.
#[inline]
pub fn empty_accumulators() -> [Accumulator; Color::NUM] {
    [Accumulator::new(&NNUE); Color::NUM]
}

/// Adds a piece-on-square feature to both perspective accumulators.
#[inline(always)]
pub fn add_piece(
    accumulators: &mut [Accumulator; Color::NUM],
    color: Color,
    piece: Piece,
    square: Square,
) {
    accumulators[Color::White as usize].add_piece(&NNUE, Color::White, color, piece, square);
    accumulators[Color::Black as usize].add_piece(&NNUE, Color::Black, color, piece, square);
}

/// Removes a piece-on-square feature from both perspective accumulators.
#[inline(always)]
pub fn remove_piece(
    accumulators: &mut [Accumulator; Color::NUM],
    color: Color,
    piece: Piece,
    square: Square,
) {
    accumulators[Color::White as usize].remove_piece(&NNUE, Color::White, color, piece, square);
    accumulators[Color::Black as usize].remove_piece(&NNUE, Color::Black, color, piece, square);
}

/// Rebuilds both perspective accumulators from scratch by walking the
/// piece bitboards. Used by [`GameState::empty`] / [`GameState::from_fen`]
/// to seed the running accumulators before any moves are applied.
pub fn refresh_from_state(state: &GameState) -> [Accumulator; Color::NUM] {
    let mut acc = empty_accumulators();
    for color in Color::all() {
        for piece in Piece::all() {
            for square in state.pieces[color as usize][piece as usize].iter() {
                add_piece(&mut acc, color, piece, square);
            }
        }
    }
    acc
}

/// Static NNUE evaluation of `state` from the side-to-move's perspective.
///
/// Reads the perspective accumulators kept up-to-date incrementally by
/// `make_move`/`unmake_move` and runs the network's output layer.
#[inline]
pub fn evaluate(state: &GameState) -> i32 {
    let stm = state.side_to_move;
    NNUE.evaluate(
        &state.accumulators[stm as usize],
        &state.accumulators[stm.opposite() as usize],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::MoveList;

    /// Walks a small game tree applying make_move/unmake_move and at every
    /// reached node asserts the incrementally maintained accumulators match
    /// a from-scratch refresh. Catches feature-index drift between bitboard
    /// updates and NNUE updates.
    fn perft_nnue(state: &mut GameState, depth: u8) {
        assert_eq!(state.accumulators, refresh_from_state(state));
        if depth == 0 {
            return;
        }
        let mut moves = MoveList::default();
        state.generate_moves(&mut moves);
        for mv in moves.iter() {
            let undo = state.make_move(*mv);
            perft_nnue(state, depth - 1);
            state.unmake_move(*mv, &undo);
            assert_eq!(state.accumulators, refresh_from_state(state));
        }
    }

    #[test]
    fn test_incremental_accumulator_matches_from_scratch_start() {
        let mut state = GameState::new();
        perft_nnue(&mut state, 4);
    }

    #[test]
    fn test_incremental_accumulator_matches_from_scratch_kiwipete() {
        let mut state = GameState::from_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        )
        .unwrap();
        perft_nnue(&mut state, 4);
    }
}
