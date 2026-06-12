use num_enum::UnsafeFromPrimitive;

/// Board file (column). `A` is the queenside file, `H` is the kingside.
///
/// Discriminant matches the algebraic file letter (`A = 0`, ..., `H = 7`).
#[repr(u8)]
#[derive(UnsafeFromPrimitive, Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum File {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
}

impl File {
    /// Number of files (always 8).
    pub const NUM: usize = std::mem::variant_count::<File>();

    /// Reconstructs a [`File`] from its discriminant.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `index` is not in `0..NUM`. Release
    /// builds skip the check; passing an out-of-range value is undefined
    /// behaviour.
    #[inline(always)]
    pub fn index(index: usize) -> Self {
        debug_assert!(index < Self::NUM);
        unsafe { File::unchecked_transmute_from(index as u8) }
    }

    /// Iterator over every file from `A` to `H`.
    #[inline(always)]
    pub fn all() -> impl Iterator<Item = File> {
        (0..File::NUM).map(Self::index)
    }
}
