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

impl Disc {
    /// Inspect the disc header and DOL section table without loading the full image.
    pub fn read(input: &mut (impl Read + Seek)) -> io::Result<Self> {
        let image_size = input.seek(SeekFrom::End(0))?;
        input.seek(SeekFrom::Start(0))?;
        let mut header = [0u8; 0x440];
        input.read_exact(&mut header)?;
        if be32(&header, 0x1c) != 0xc2339f3d {
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
        let dol_offset = u64::from(be32(&header, 0x420));
        if dol_offset < 0x440 || dol_offset + 0x100 > image_size {
            return Err(invalid("DOL header outside disc image"));
        }
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
            revision: header[7],
            title,
            dol_offset,
            dol_size,
            entry_point: be32(&dol, 0xe0),
            sections,
        })
    }
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
    fn rejects_other_formats_and_truncated_headers() {
        assert!(Disc::read(&mut Cursor::new(vec![0; 0x440])).is_err());
        assert!(Disc::read(&mut Cursor::new(vec![0; 10])).is_err());
    }
}
