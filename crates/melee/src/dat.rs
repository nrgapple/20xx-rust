//! Typed, big-endian reads of game data from HSD archives.
//!
//! The game uses archive data in place as C structs. The port copies each
//! struct out field by field into native Rust values with identical bit
//! patterns, so later arithmetic sees exactly the values the game would.

use crate::math::{Vec2, Vec3};
use hsd_dat::{Archive, Result};

/// A value stored in an archive at a fixed size.
pub trait DatField: Sized {
    const SIZE: u32;
    fn read(archive: &Archive, offset: u32) -> Result<Self>;
}

macro_rules! scalar {
    ($($ty:ty => $method:ident),*) => {$(
        impl DatField for $ty {
            const SIZE: u32 = std::mem::size_of::<$ty>() as u32;
            fn read(archive: &Archive, offset: u32) -> Result<Self> {
                archive.$method(offset)
            }
        }
    )*};
}
scalar!(f32 => f32, i32 => i32, u32 => u32, u16 => u16, i16 => i16, u8 => u8);

impl DatField for i8 {
    const SIZE: u32 = 1;
    fn read(archive: &Archive, offset: u32) -> Result<Self> {
        archive.u8(offset).map(|b| b as i8)
    }
}

impl DatField for Vec2 {
    const SIZE: u32 = 8;
    fn read(archive: &Archive, offset: u32) -> Result<Self> {
        Ok(Vec2::new(archive.f32(offset)?, archive.f32(offset + 4)?))
    }
}

impl DatField for Vec3 {
    const SIZE: u32 = 12;
    fn read(archive: &Archive, offset: u32) -> Result<Self> {
        Ok(Vec3::new(
            archive.f32(offset)?,
            archive.f32(offset + 4)?,
            archive.f32(offset + 8)?,
        ))
    }
}

/// A pointer field, resolved to a data-block offset (`None` is NULL).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DatPtr(pub Option<u32>);

impl DatField for DatPtr {
    const SIZE: u32 = 4;
    fn read(archive: &Archive, offset: u32) -> Result<Self> {
        archive.ptr(offset).map(DatPtr)
    }
}

impl<T: DatField, const N: usize> DatField for [T; N] {
    const SIZE: u32 = T::SIZE * N as u32;
    fn read(archive: &Archive, offset: u32) -> Result<Self> {
        let mut values = Vec::with_capacity(N);
        for index in 0..N as u32 {
            values.push(T::read(archive, offset + index * T::SIZE)?);
        }
        Ok(values
            .try_into()
            .unwrap_or_else(|_| unreachable!("exactly N values read")))
    }
}

/// Declare a struct read from archive data. Each field lists its byte offset
/// within the struct, as annotated in the decomp; `size` is the C `sizeof`.
macro_rules! dat_struct {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            $($offset:literal $field:ident : $ty:ty,)*
        }
        size $size:literal
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Default)]
        pub struct $name {
            $(pub $field: $ty,)*
        }

        impl $name {
            /// `(name, offset, size)` of every field, for layout checks.
            #[allow(dead_code)]
            pub const FIELDS: &'static [(&'static str, u32, u32)] =
                &[$((stringify!($field), $offset, <$ty as DatField>::SIZE),)*];
        }

        impl DatField for $name {
            const SIZE: u32 = $size;
            fn read(archive: &hsd_dat::Archive, offset: u32) -> hsd_dat::Result<Self> {
                // Reject a struct that runs past the data block before reading fields.
                archive.bytes(offset, $size)?;
                Ok(Self {
                    $($field: <$ty as DatField>::read(archive, offset + $offset)?,)*
                })
            }
        }
    };
}
pub(crate) use dat_struct;

/// Check that `fields` are in order, do not overlap, and fit within `size`.
#[cfg(test)]
pub(crate) fn check_layout(fields: &[(&str, u32, u32)], size: u32) {
    let mut end = 0;
    for &(name, offset, len) in fields {
        assert!(
            offset >= end,
            "{name} at {offset:#x} overlaps previous field"
        );
        end = offset + len;
    }
    assert!(end <= size, "fields end at {end:#x}, past size {size:#x}");
}
