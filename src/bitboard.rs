use crate::core::*;

pub type BitBoard = u64;

pub trait BitBoardMethods {
    const EMPTY: Self;
    fn repr_string(&self) -> String;
    fn on(square: Square) -> BitBoard;
    fn flip_ranks(self) -> Self;
    fn flip_files(self) -> Self;
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
    const EMPTY: Self = 0;

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

    fn flip_ranks(self) -> Self {
        // https://www.chessprogramming.org/Flipping_Mirroring_and_Rotating#Vertical
        self.swap_bytes()
    }

    fn flip_files(self) -> Self {
        // https://www.chessprogramming.org/Flipping_Mirroring_and_Rotating#Horizontal
        const K1: u64 = 0x5555555555555555;
        const K2: u64 = 0x3333333333333333;
        const K4: u64 = 0x0F0F0F0F0F0F0F0F;
        let mut flipped = self;
        flipped = ((flipped >> 1) & K1) | ((flipped & K1) << 1);
        flipped = ((flipped >> 2) & K2) | ((flipped & K2) << 2);
        flipped = ((flipped >> 4) & K4) | ((flipped & K4) << 4);
        flipped
    }

    fn on(square: Square) -> BitBoard {
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

#[macro_export]
macro_rules! bitboard {
    (
        $a8:tt $b8:tt $c8:tt $d8:tt $e8:tt $f8:tt $g8:tt $h8:tt
        $a7:tt $b7:tt $c7:tt $d7:tt $e7:tt $f7:tt $g7:tt $h7:tt
        $a6:tt $b6:tt $c6:tt $d6:tt $e6:tt $f6:tt $g6:tt $h6:tt
        $a5:tt $b5:tt $c5:tt $d5:tt $e5:tt $f5:tt $g5:tt $h5:tt
        $a4:tt $b4:tt $c4:tt $d4:tt $e4:tt $f4:tt $g4:tt $h4:tt
        $a3:tt $b3:tt $c3:tt $d3:tt $e3:tt $f3:tt $g3:tt $h3:tt
        $a2:tt $b2:tt $c2:tt $d2:tt $e2:tt $f2:tt $g2:tt $h2:tt
        $a1:tt $b1:tt $c1:tt $d1:tt $e1:tt $f1:tt $g1:tt $h1:tt
    ) => {
        $crate::bitboard! { @__inner
            $a1 $b1 $c1 $d1 $e1 $f1 $g1 $h1
            $a2 $b2 $c2 $d2 $e2 $f2 $g2 $h2
            $a3 $b3 $c3 $d3 $e3 $f3 $g3 $h3
            $a4 $b4 $c4 $d4 $e4 $f4 $g4 $h4
            $a5 $b5 $c5 $d5 $e5 $f5 $g5 $h5
            $a6 $b6 $c6 $d6 $e6 $f6 $g6 $h6
            $a7 $b7 $c7 $d7 $e7 $f7 $g7 $h7
            $a8 $b8 $c8 $d8 $e8 $f8 $g8 $h8
        }
    };
    (@__inner $($occupied:tt)*) => {{
        const BITBOARD: $crate::BitBoard = {
            let mut index = 0;
            let mut bitboard = $crate::BitBoard::EMPTY;
            $(
                if $crate::bitboard!(@__square $occupied) {
                    bitboard |= 1 << index;
                }
                index += 1;
            )*
            let _ = index;
            bitboard
        };
        BITBOARD
    }};
    (@__square X) => { true };
    (@__square .) => { false };
    (@__square $token:tt) => {
        compile_error!(
            concat!(
                "Expected only `X` or `.` tokens, found `",
                stringify!($token),
                "`"
            )
        )
    };
    ($($token:tt)*) => {
        compile_error!("Expected 64 squares")
    };
}
