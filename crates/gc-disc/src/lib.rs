//! Read-only inspection of uncompressed GameCube ISO/GCM images.

use std::io::{self, Read, Seek, SeekFrom};

#[derive(Debug)]
pub struct Disc {
    pub game_id: String,
    pub revision: u8,
    pub title: String,
    pub dol_offset: u64,
    pub dol_size: u64,
    pub entry_point: u32,
    pub sections: Vec<Section>,
    pub fst_offset: u64,
    pub fst_size: u64,
}

/// A file in the disc filesystem (FST). Directories are folded into `path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Path relative to the disc root, using `/` separators.
    pub path: String,
    pub offset: u64,
    pub size: u64,
}

#[derive(Debug)]
pub struct Section {
    pub kind: &'static str,
    pub file_offset: u32,
    pub address: u32,
    pub size: u32,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn be32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// The fixed 0x440-byte disc header ("boot.bin" + "bi2.bin" prefix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub game_id: String,
    pub revision: u8,
    pub title: String,
    pub dol_offset: u64,
    pub fst_offset: u64,
    pub fst_size: u64,
}

impl Header {
    pub const SIZE: usize = 0x440;

    /// Parse the header and check that the DOL header and FST lie within an
    /// image of `image_size` bytes. Usable without the rest of the image.
    pub fn parse(header: &[u8], image_size: u64) -> io::Result<Self> {
        if header.len() < Self::SIZE {
            return Err(invalid("disc header truncated"));
        }
        if be32(header, 0x1c) != 0xc2339f3d {
            return Err(invalid("not an uncompressed GameCube ISO/GCM image"));
        }
        let game_id = std::str::from_utf8(&header[..6])
            .map_err(|_| invalid("invalid game ID"))?
            .to_owned();
        let title = String::from_utf8_lossy(&header[0x20..0x400])
            .split('\0')
            .next()
            .unwrap_or_default()
            .to_owned();
        let dol_offset = u64::from(be32(header, 0x420));
        if dol_offset < 0x440 || dol_offset + 0x100 > image_size {
            return Err(invalid("DOL header outside disc image"));
        }
        let fst_offset = u64::from(be32(header, 0x424));
        let fst_size = u64::from(be32(header, 0x428));
        if fst_offset + fst_size > image_size {
            return Err(invalid("FST outside disc image"));
        }
        Ok(Self {
            game_id,
            revision: header[7],
            title,
            dol_offset,
            fst_offset,
            fst_size,
        })
    }
}

impl Disc {
    /// Inspect the disc header and DOL section table without loading the full image.
    pub fn read(input: &mut (impl Read + Seek)) -> io::Result<Self> {
        let image_size = input.seek(SeekFrom::End(0))?;
        input.seek(SeekFrom::Start(0))?;
        let mut header = [0u8; Header::SIZE];
        input.read_exact(&mut header)?;
        let Header {
            game_id,
            revision,
            title,
            dol_offset,
            fst_offset,
            fst_size,
        } = Header::parse(&header, image_size)?;
        input.seek(SeekFrom::Start(dol_offset))?;
        let mut dol = [0u8; 0x100];
        input.read_exact(&mut dol)?;
        let mut sections = Vec::new();
        let mut dol_size = 0x100;
        for index in 0..18 {
            let file_offset = be32(&dol, index * 4);
            let address = be32(&dol, 0x48 + index * 4);
            let size = be32(&dol, 0x90 + index * 4);
            if size == 0 {
                continue;
            }
            let end = u64::from(file_offset) + u64::from(size);
            if file_offset < 0x100 || dol_offset + end > image_size {
                return Err(invalid("DOL section outside disc image"));
            }
            dol_size = dol_size.max(end);
            sections.push(Section {
                kind: if index < 7 { "text" } else { "data" },
                file_offset,
                address,
                size,
            });
        }
        if sections.is_empty() {
            return Err(invalid("DOL has no nonempty sections"));
        }
        Ok(Self {
            game_id,
            revision,
            title,
            dol_offset,
            dol_size,
            entry_point: be32(&dol, 0xe0),
            sections,
            fst_offset,
            fst_size,
        })
    }

    /// Read the disc filesystem table and return every file with its full path.
    pub fn files(&self, input: &mut (impl Read + Seek)) -> io::Result<Vec<FileEntry>> {
        let image_size = input.seek(SeekFrom::End(0))?;
        let fst_len = usize::try_from(self.fst_size).map_err(|_| invalid("FST too large"))?;
        input.seek(SeekFrom::Start(self.fst_offset))?;
        let mut fst = vec![0u8; fst_len];
        input.read_exact(&mut fst)?;
        parse_fst(&fst, image_size)
    }

    /// Find a file by its path relative to the disc root.
    pub fn find_file(&self, input: &mut (impl Read + Seek), path: &str) -> io::Result<FileEntry> {
        let path = path.trim_start_matches('/');
        self.files(input)?
            .into_iter()
            .find(|file| file.path == path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{path} not on disc")))
    }

    /// Read a whole file from the disc filesystem.
    pub fn read_file(&self, input: &mut (impl Read + Seek), path: &str) -> io::Result<Vec<u8>> {
        let file = self.find_file(input, path)?;
        let mut bytes =
            vec![0u8; usize::try_from(file.size).map_err(|_| invalid("file too large"))?];
        input.seek(SeekFrom::Start(file.offset))?;
        input.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}

/// A main executable loaded from bytes, for reading code and data by address.
#[derive(Debug)]
pub struct Dol {
    pub bytes: Vec<u8>,
    pub entry_point: u32,
    pub sections: Vec<Section>,
}

impl Dol {
    pub fn parse(bytes: Vec<u8>) -> io::Result<Self> {
        if bytes.len() < 0x100 {
            return Err(invalid("DOL header truncated"));
        }
        let mut sections = Vec::new();
        for index in 0..18 {
            let (file_offset, address, size) = (
                be32(&bytes, index * 4),
                be32(&bytes, 0x48 + index * 4),
                be32(&bytes, 0x90 + index * 4),
            );
            if size == 0 {
                continue;
            }
            if file_offset as usize + size as usize > bytes.len() {
                return Err(invalid("DOL section outside file"));
            }
            sections.push(Section {
                kind: if index < 7 { "text" } else { "data" },
                file_offset,
                address,
                size,
            });
        }
        Ok(Self {
            entry_point: be32(&bytes, 0xe0),
            bytes,
            sections,
        })
    }

    /// Bytes loaded at `address..address + len`, if one section contains them.
    pub fn at(&self, address: u32, len: u32) -> Option<&[u8]> {
        self.sections.iter().find_map(|section| {
            let start = address.checked_sub(section.address)?;
            let end = start.checked_add(len)?;
            (end <= section.size).then(|| {
                let file = section.file_offset as usize;
                &self.bytes[file + start as usize..file + end as usize]
            })
        })
    }
}

/// Parse a raw FST (as read from `Header::fst_offset`): 12-byte entries (the
/// root's size field is the entry count), then NUL-terminated names.
pub fn parse_fst(fst: &[u8], image_size: u64) -> io::Result<Vec<FileEntry>> {
    if fst.len() < 12 {
        return Err(invalid("FST too small"));
    }
    let count = be32(fst, 8) as usize;
    let strings = count
        .checked_mul(12)
        .filter(|&end| end <= fst.len() && count > 0)
        .ok_or_else(|| invalid("FST entry table outside FST"))?;
    let name = |entry: usize| -> io::Result<&str> {
        let start = strings + (be32(fst, entry * 12) & 0x00ff_ffff) as usize;
        let tail = fst
            .get(start..)
            .ok_or_else(|| invalid("FST name outside FST"))?;
        let end = tail
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| invalid("unterminated FST name"))?;
        std::str::from_utf8(&tail[..end]).map_err(|_| invalid("FST name is not UTF-8"))
    };
    let mut files = Vec::new();
    // Open directories as (index one past their last entry, path prefix).
    let mut dirs: Vec<(usize, String)> = Vec::new();
    for entry in 1..count {
        while dirs.last().is_some_and(|&(end, _)| entry >= end) {
            dirs.pop();
        }
        let prefix = dirs.last().map_or("", |(_, path)| path.as_str());
        let path = if prefix.is_empty() {
            name(entry)?.to_owned()
        } else {
            format!("{prefix}/{}", name(entry)?)
        };
        let field1 = u64::from(be32(fst, entry * 12 + 4));
        let field2 = u64::from(be32(fst, entry * 12 + 8));
        if fst[entry * 12] != 0 {
            let end = field2 as usize;
            if end <= entry || end > count {
                return Err(invalid("FST directory end out of range"));
            }
            dirs.push((end, path));
        } else {
            if field1 + field2 > image_size {
                return Err(invalid("FST file outside disc image"));
            }
            files.push(FileEntry {
                path,
                offset: field1,
                size: field2,
            });
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn image() -> Vec<u8> {
        let mut bytes = vec![0; 0x1500];
        bytes[..6].copy_from_slice(b"GALE01");
        bytes[7] = 2;
        bytes[0x1c..0x20].copy_from_slice(&0xc2339f3du32.to_be_bytes());
        bytes[0x420..0x424].copy_from_slice(&0x1000u32.to_be_bytes());
        bytes[0x1000..0x1004].copy_from_slice(&0x100u32.to_be_bytes());
        bytes[0x1048..0x104c].copy_from_slice(&0x80003100u32.to_be_bytes());
        bytes[0x1090..0x1094].copy_from_slice(&0x400u32.to_be_bytes());
        bytes
    }

    #[test]
    fn inspects_synthetic_disc() {
        let disc = Disc::read(&mut Cursor::new(image())).unwrap();
        assert_eq!(disc.game_id, "GALE01");
        assert_eq!(disc.revision, 2);
        assert_eq!(disc.dol_size, 0x500);
        assert_eq!(disc.sections[0].address, 0x80003100);
    }

    #[test]
    fn rejects_truncated_section() {
        let mut bytes = image();
        bytes.pop();
        assert!(Disc::read(&mut Cursor::new(bytes)).is_err());
    }

    #[test]
    fn lists_nested_fst_files() {
        // root, "a.dat", dir "d" (entries 2..4), "d/b.dat"
        let names = b"a.dat\0d\0b.dat\0";
        let mut fst = Vec::new();
        for (flags_name, f1, f2) in [
            (0x0100_0000u32, 0u32, 4u32),
            (0, 0x1100, 0x10),
            (0x0100_0000 | 6, 0, 4),
            (8, 0x1200, 0x20),
        ] {
            fst.extend(flags_name.to_be_bytes());
            fst.extend(f1.to_be_bytes());
            fst.extend(f2.to_be_bytes());
        }
        fst.extend(names);
        let mut bytes = image();
        bytes[0x424..0x428].copy_from_slice(&0x1500u32.to_be_bytes());
        bytes[0x428..0x42c].copy_from_slice(&(fst.len() as u32).to_be_bytes());
        bytes.extend(&fst);
        bytes[0x1200..0x1204].copy_from_slice(b"HSD!");
        let mut cursor = Cursor::new(bytes);
        let disc = Disc::read(&mut cursor).unwrap();
        let files = disc.files(&mut cursor).unwrap();
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["a.dat", "d/b.dat"]);
        assert_eq!(
            &disc.read_file(&mut cursor, "/d/b.dat").unwrap()[..4],
            b"HSD!"
        );
        assert!(disc.read_file(&mut cursor, "missing.dat").is_err());
    }

    #[test]
    fn reads_dol_by_address() {
        let dol = Dol::parse(image()[0x1000..].to_vec()).unwrap();
        assert_eq!(dol.at(0x80003100, 4), Some(&[0u8; 4][..]));
        assert_eq!(dol.at(0x800034fc, 4).map(<[u8]>::len), Some(4));
        assert_eq!(dol.at(0x800034fd, 4), None);
        assert_eq!(dol.at(0x80003000, 4), None);
    }

    #[test]
    fn rejects_other_formats_and_truncated_headers() {
        assert!(Disc::read(&mut Cursor::new(vec![0; 0x440])).is_err());
        assert!(Disc::read(&mut Cursor::new(vec![0; 10])).is_err());
        assert!(parse_fst(&[0; 11], 0).is_err());
    }
}
