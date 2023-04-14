use crate::core::*;

pub type BitBoard = u64;

pub trait BitBoardMethods {
    fn repr_string(&self) -> String;
    fn on_square(square: Square) -> BitBoard;
    fn shift_north(&self) -> Self;
    fn shift_south(&self) -> Self;
    fn shift_east(&self) -> Self;
    fn shift_west(&self) -> Self;
    fn shift_north_east(&self) -> Self;
    fn shift_north_west(&self) -> Self;
    fn shift_south_east(&self) -> Self;
    fn shift_south_west(&self) -> Self;
}

impl BitBoardMethods for BitBoard {
    fn repr_string(&self) -> String {
        let mut repr = String::new();
        for rank in 0..NUM_RANKS {
            for file in 0..NUM_FILES {
                let idx = (NUM_RANKS - rank - 1) * NUM_FILES + file;
                let piece = (self >> idx) & 1u64;
                repr.push(if piece == 0 { '_' } else { '*' });
            }
            repr.push('\n');
        }
        repr
    }

    fn on_square(square: Square) -> BitBoard {
        1 << square as u64
    }

    fn shift_north(&self) -> BitBoard {
        self << NUM_FILES
    }
    fn shift_south(&self) -> BitBoard {
        self >> NUM_FILES
    }
    fn shift_east(&self) -> BitBoard {
        self << 1
    }

    fn shift_west(&self) -> BitBoard {
        self >> 1
    }

    fn shift_north_east(&self) -> BitBoard {
        self << (NUM_FILES + 1)
    }

    fn shift_north_west(&self) -> BitBoard {
        self << (NUM_FILES - 1)
    }

    fn shift_south_east(&self) -> BitBoard {
        self >> (NUM_FILES - 1)
    }

    fn shift_south_west(&self) -> BitBoard {
        self >> (NUM_FILES + 1)
    }
}
