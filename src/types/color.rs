use num_enum::UnsafeFromPrimitive;

/// Side to move / piece colour.
#[repr(u8)]
#[derive(UnsafeFromPrimitive, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    White,
    Black,
}

impl Color {
    /// Number of variants; usable as the size of per-colour lookup arrays.
    pub const NUM: usize = std::mem::variant_count::<Color>();

    /// Reconstructs a [`Color`] from its discriminant.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `index` is not in `0..NUM`. Release
    /// builds skip the check; passing an out-of-range value is undefined
    /// behaviour.
    #[inline(always)]
    pub fn index(index: usize) -> Self {
        debug_assert!(index < Self::NUM);
        unsafe { Color::unchecked_transmute_from(index as u8) }
    }

    /// Iterator over every colour in discriminant order.
    #[inline(always)]
    pub fn all() -> impl Iterator<Item = Color> {
        (0..Color::NUM).map(Self::index)
    }

    /// Returns the opposing colour.
    #[inline(always)]
    pub fn opposite(&self) -> Self {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}
