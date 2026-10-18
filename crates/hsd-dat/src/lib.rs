//! Read-only access to HAL sysdolphin (HSD) `.dat` archives.
//!
//! Layout follows the decomp's `HSD_ArchiveParse` (`src/sysdolphin/baselib/archive.c`):
//! a 0x20-byte header, the data block, relocation offsets, public symbols,
//! extern symbols, then a string table. The game relocates pointers in place
//! by adding the data block's address; here the data stays untouched and
//! big-endian, and pointer fields are resolved to data-block offsets on read.

use std::fmt;

const HEADER_SIZE: usize = 0x20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The header's file size does not match the bytes provided.
    SizeMismatch {
        header: u32,
        actual: usize,
    },
    /// A table or string lies outside the file.
    Truncated(&'static str),
    /// A read or pointer target lies outside the data block.
    OutOfBounds {
        offset: u32,
        len: u32,
    },
    /// A relocation entry points outside the data block or is misaligned.
    BadRelocation(u32),
    /// A field read as a pointer is nonzero but has no relocation entry.
    UnrelocatedPointer {
        offset: u32,
        value: u32,
    },
    MissingSymbol(String),
    /// A pointer the caller requires is NULL.
    NullPointer(u32),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::SizeMismatch { header, actual } => {
                write!(f, "header file size {header:#x} != actual {actual:#x}")
            }
            Error::Truncated(what) => write!(f, "{what} extends past end of file"),
            Error::OutOfBounds { offset, len } => {
                write!(
                    f,
                    "read of {len:#x} bytes at {offset:#x} outside data block"
                )
            }
            Error::BadRelocation(offset) => write!(f, "invalid relocation at {offset:#x}"),
            Error::UnrelocatedPointer { offset, value } => {
                write!(
                    f,
                    "pointer at {offset:#x} holds {value:#x} without a relocation"
                )
            }
            Error::MissingSymbol(name) => write!(f, "public symbol {name} not in archive"),
            Error::NullPointer(offset) => write!(f, "required pointer at {offset:#x} is NULL"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// A parsed archive. Offsets passed to readers are relative to the data block,
/// which is how the archive's own pointers are stored before relocation.
#[derive(Debug, Clone)]
pub struct Archive {
    data: Vec<u8>,
    /// Sorted data offsets of pointer fields.
    relocations: Vec<u32>,
    publics: Vec<Symbol>,
    externs: Vec<Symbol>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub offset: u32,
}

fn be32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

impl Archive {
    pub fn parse(file: &[u8]) -> Result<Self> {
        if file.len() < HEADER_SIZE {
            return Err(Error::Truncated("header"));
        }
        let file_size = be32(file, 0x00);
        if file_size as usize != file.len() {
            return Err(Error::SizeMismatch {
                header: file_size,
                actual: file.len(),
            });
        }
        let data_size = be32(file, 0x04) as usize;
        let counts = [be32(file, 0x08), be32(file, 0x0c), be32(file, 0x10)].map(|n| n as usize);
        let [nb_reloc, nb_public, nb_extern] = counts;

        let mut offset = HEADER_SIZE;
        let mut take = |len: Option<usize>, what: &'static str| -> Result<std::ops::Range<usize>> {
            let end = len
                .and_then(|len| offset.checked_add(len))
                .filter(|&end| end <= file.len())
                .ok_or(Error::Truncated(what))?;
            let range = offset..end;
            offset = end;
            Ok(range)
        };
        let data = take(Some(data_size), "data block")?;
        let relocs = take(nb_reloc.checked_mul(4), "relocation table")?;
        let publics = take(nb_public.checked_mul(8), "public symbol table")?;
        let externs = take(nb_extern.checked_mul(8), "extern symbol table")?;
        let strings = &file[offset..];

        let data = file[data].to_vec();
        let mut relocations: Vec<u32> = file[relocs]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_be_bytes(*c))
            .collect();
        relocations.sort_unstable();
        relocations.dedup();
        for &reloc in &relocations {
            let field = reloc as usize;
            if !field.is_multiple_of(4)
                || field + 4 > data.len()
                || be32(&data, field) as usize > data.len()
            {
                return Err(Error::BadRelocation(reloc));
            }
        }
        let symbols = |table: &[u8]| -> Result<Vec<Symbol>> {
            table
                .as_chunks::<8>()
                .0
                .iter()
                .map(|entry| {
                    let tail = strings
                        .get(be32(entry, 4) as usize..)
                        .ok_or(Error::Truncated("symbol name"))?;
                    let end = tail
                        .iter()
                        .position(|&b| b == 0)
                        .ok_or(Error::Truncated("symbol name"))?;
                    Ok(Symbol {
                        name: String::from_utf8_lossy(&tail[..end]).into_owned(),
                        offset: be32(entry, 0),
                    })
                })
                .collect()
        };
        let publics = symbols(&file[publics])?;
        let externs = symbols(&file[externs])?;
        if let Some(bad) = publics.iter().find(|s| s.offset as usize > data.len()) {
            return Err(Error::OutOfBounds {
                offset: bad.offset,
                len: 0,
            });
        }
        Ok(Self {
            data,
            relocations,
            publics,
            externs,
        })
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn publics(&self) -> &[Symbol] {
        &self.publics
    }

    pub fn externs(&self) -> &[Symbol] {
        &self.externs
    }

    pub fn relocations(&self) -> &[u32] {
        &self.relocations
    }

    /// Data offset of a public symbol, like `HSD_ArchiveGetPublicAddress`
    /// (first match wins).
    pub fn public(&self, name: &str) -> Result<u32> {
        self.publics
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.offset)
            .ok_or_else(|| Error::MissingSymbol(name.to_owned()))
    }

    pub fn is_relocated(&self, offset: u32) -> bool {
        self.relocations.binary_search(&offset).is_ok()
    }

    pub fn bytes(&self, offset: u32, len: u32) -> Result<&[u8]> {
        let start = offset as usize;
        start
            .checked_add(len as usize)
            .and_then(|end| self.data.get(start..end))
            .ok_or(Error::OutOfBounds { offset, len })
    }

    fn array<const N: usize>(&self, offset: u32) -> Result<[u8; N]> {
        Ok(self.bytes(offset, N as u32)?.try_into().unwrap())
    }

    pub fn u8(&self, offset: u32) -> Result<u8> {
        Ok(self.array::<1>(offset)?[0])
    }

    pub fn u16(&self, offset: u32) -> Result<u16> {
        self.array(offset).map(u16::from_be_bytes)
    }

    pub fn u32(&self, offset: u32) -> Result<u32> {
        self.array(offset).map(u32::from_be_bytes)
    }

    pub fn i16(&self, offset: u32) -> Result<i16> {
        self.array(offset).map(i16::from_be_bytes)
    }

    pub fn i32(&self, offset: u32) -> Result<i32> {
        self.array(offset).map(i32::from_be_bytes)
    }

    /// Bit-exact IEEE single, as stored on disc.
    pub fn f32(&self, offset: u32) -> Result<f32> {
        self.u32(offset).map(f32::from_bits)
    }

    /// Resolve a pointer field. `None` is a NULL pointer: zero and not relocated.
    /// A relocated field always points into the data block, even when it is 0.
    pub fn ptr(&self, offset: u32) -> Result<Option<u32>> {
        let value = self.u32(offset)?;
        if self.is_relocated(offset) {
            Ok(Some(value))
        } else if value == 0 {
            Ok(None)
        } else {
            Err(Error::UnrelocatedPointer { offset, value })
        }
    }

    /// Resolve a pointer field that must not be NULL.
    pub fn non_null_ptr(&self, offset: u32) -> Result<u32> {
        self.ptr(offset)?.ok_or(Error::NullPointer(offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an archive whose data is `[ptr -> 8, 0, 1.5f32, 0]` with symbol `root` at 0.
    fn sample() -> Vec<u8> {
        let mut data = Vec::new();
        for word in [8u32, 0, 1.5f32.to_bits(), 0] {
            data.extend(word.to_be_bytes());
        }
        let mut tables = Vec::new();
        tables.extend(0u32.to_be_bytes()); // relocation: field at 0
        tables.extend(0u32.to_be_bytes()); // public: offset 0
        tables.extend(0u32.to_be_bytes()); // public: name at 0
        tables.extend(b"root\0");
        let size = (HEADER_SIZE + data.len() + tables.len()) as u32;
        let mut file = Vec::new();
        for word in [size, data.len() as u32, 1, 1, 0, 0, 0, 0] {
            file.extend(word.to_be_bytes());
        }
        file.extend(data);
        file.extend(tables);
        file
    }

    #[test]
    fn parses_symbols_and_pointers() {
        let archive = Archive::parse(&sample()).unwrap();
        let root = archive.public("root").unwrap();
        assert_eq!(root, 0);
        let target = archive.ptr(root).unwrap().unwrap();
        assert_eq!(archive.f32(target).unwrap(), 1.5);
        assert_eq!(archive.ptr(4).unwrap(), None);
        assert!(archive.public("other").is_err());
        assert!(archive.u32(16).is_err());
    }

    #[test]
    fn rejects_inconsistent_archives() {
        let mut file = sample();
        file.push(0);
        assert!(matches!(
            Archive::parse(&file),
            Err(Error::SizeMismatch { .. })
        ));

        let mut file = sample();
        file[HEADER_SIZE + 16..HEADER_SIZE + 20].copy_from_slice(&2u32.to_be_bytes());
        assert_eq!(Archive::parse(&file).unwrap_err(), Error::BadRelocation(2));

        let mut file = sample();
        file[0x0c..0x10].copy_from_slice(&1000u32.to_be_bytes());
        assert!(matches!(Archive::parse(&file), Err(Error::Truncated(_))));

        let mut file = sample();
        file[HEADER_SIZE + 12..HEADER_SIZE + 16].copy_from_slice(&7u32.to_be_bytes());
        let archive = Archive::parse(&file).unwrap();
        assert_eq!(
            archive.ptr(12).unwrap_err(),
            Error::UnrelocatedPointer {
                offset: 12,
                value: 7
            }
        );
    }
}
