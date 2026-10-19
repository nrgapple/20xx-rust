//! Offline decoding of tiled GX texture data. Layout/format IDs: original SDK
//! GXEnum.h, GXTexture.c and HSD_ImageDesc/HSD_TlutDesc in tobj.h.
//! No emulator implementation is used. Only mip level zero is exported.
use crate::{ensure, Result};
use hsd_dat::Archive;

pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
fn expand(v: u16, bits: u32) -> u8 {
    ((u32::from(v) * 255 + ((1 << bits) - 1) / 2) / ((1 << bits) - 1)) as u8
}
fn rgb565(v: u16) -> [u8; 4] {
    [
        expand(v >> 11, 5),
        expand((v >> 5) & 63, 6),
        expand(v & 31, 5),
        255,
    ]
}
fn rgb5a3(v: u16) -> [u8; 4] {
    if v & 0x8000 != 0 {
        [
            expand((v >> 10) & 31, 5),
            expand((v >> 5) & 31, 5),
            expand(v & 31, 5),
            255,
        ]
    } else {
        [
            expand((v >> 8) & 15, 4),
            expand((v >> 4) & 15, 4),
            expand(v & 15, 4),
            expand(v >> 12, 3),
        ]
    }
}
fn palette_color(v: u16, format: u32) -> Result<[u8; 4]> {
    Ok(match format {
        0 => [v as u8, v as u8, v as u8, (v >> 8) as u8],
        1 => rgb565(v),
        2 => rgb5a3(v),
        _ => return Err(format!("unsupported palette format {format}").into()),
    })
}

pub fn load(a: &Archive, image: u32, palette: Option<u32>) -> Result<Texture> {
    let width = u32::from(a.u16(image + 4)?);
    let height = u32::from(a.u16(image + 6)?);
    let format = a.u32(image + 8)?;
    let (bw, bh, bytes) = match format {
        0 | 8 | 14 => (8, 8, 32),
        1 | 2 | 9 => (8, 4, 32),
        3 | 4 | 5 | 10 => (4, 4, 32),
        6 => (4, 4, 64),
        _ => return Err(format!("unsupported texture format {format}").into()),
    };
    ensure(
        width > 0 && height > 0 && width <= 2048 && height <= 2048,
        "texture dimensions",
    )?;
    let size = width.div_ceil(bw) * height.div_ceil(bh) * bytes;
    let data = a.bytes(a.non_null_ptr(image)?, size)?;
    let mut colors = Vec::new();
    if matches!(format, 8..=10) {
        let palette = palette.ok_or("indexed texture missing palette")?;
        let address = a.non_null_ptr(palette)?;
        let fmt = a.u32(palette + 4)?;
        for i in 0..u32::from(a.u16(palette + 12)?) {
            colors.push(palette_color(a.u16(address + i * 2)?, fmt)?);
        }
    }
    decode(width, height, format, data, &colors)
}

fn decode(
    width: u32,
    height: u32,
    format: u32,
    data: &[u8],
    palette: &[[u8; 4]],
) -> Result<Texture> {
    let (bw, bh, bytes): (u32, u32, usize) = match format {
        0 | 8 | 14 => (8, 8, 32),
        1 | 2 | 9 => (8, 4, 32),
        3 | 4 | 5 | 10 => (4, 4, 32),
        6 => (4, 4, 64),
        _ => return Err("unsupported texture format".into()),
    };
    ensure(
        data.len() == (width.div_ceil(bw) * height.div_ceil(bh)) as usize * bytes,
        "texture byte count",
    )?;
    let mut rgba = vec![0; (width * height * 4) as usize];
    let mut tile = 0;
    for ty in (0..height).step_by(bh as usize) {
        for tx in (0..width).step_by(bw as usize) {
            let d = &data[tile..tile + bytes];
            for y in 0..bh {
                for x in 0..bw {
                    let p = (y * bw + x) as usize;
                    let word = || u16::from_be_bytes([d[p * 2], d[p * 2 + 1]]);
                    let color = match format {
                        0 => {
                            let v =
                                ((d[p / 2] >> (if p.is_multiple_of(2) { 4 } else { 0 })) & 15) * 17;
                            [v, v, v, v]
                        }
                        1 => [d[p], d[p], d[p], d[p]],
                        2 => {
                            let v = (d[p] & 15) * 17;
                            [v, v, v, (d[p] >> 4) * 17]
                        }
                        3 => [d[p * 2 + 1], d[p * 2 + 1], d[p * 2 + 1], d[p * 2]],
                        4 => rgb565(word()),
                        5 => rgb5a3(word()),
                        6 => [d[p * 2 + 1], d[32 + p * 2], d[33 + p * 2], d[p * 2]],
                        8..=10 => {
                            let index = match format {
                                8 => usize::from(
                                    (d[p / 2] >> if p.is_multiple_of(2) { 4 } else { 0 }) & 15,
                                ),
                                9 => usize::from(d[p]),
                                _ => usize::from(word() & 0x3fff),
                            };
                            *palette.get(index).ok_or("palette index out of range")?
                        }
                        14 => {
                            // Four 4x4 subblocks per 8x8 tile, each 2 endpoints and 32 index bits.
                            let start = (((y / 4) * 2 + x / 4) * 8) as usize;
                            let block = &d[start..start + 8];
                            let c0 = u16::from_be_bytes([block[0], block[1]]);
                            let c1 = u16::from_be_bytes([block[2], block[3]]);
                            let mut c = [rgb565(c0), rgb565(c1), [0; 4], [0; 4]];
                            for channel in [0, 1, 2] {
                                let a = u16::from(c[0][channel]);
                                let b = u16::from(c[1][channel]);
                                if c0 > c1 {
                                    c[2][channel] = ((2 * a + b) / 3) as u8;
                                    c[3][channel] = ((a + 2 * b) / 3) as u8;
                                } else {
                                    c[2][channel] = ((a + b) / 2) as u8;
                                    c[3][channel] = c[2][channel];
                                }
                            }
                            c[2][3] = 255;
                            c[3][3] = if c0 > c1 { 255 } else { 0 };
                            c[((block[4 + (y % 4) as usize] >> (6 - (x % 4) * 2)) & 3) as usize]
                        }
                        _ => unreachable!(),
                    };
                    if tx + x < width && ty + y < height {
                        let dst = (((ty + y) * width + tx + x) * 4) as usize;
                        rgba[dst..dst + 4].copy_from_slice(&color);
                    }
                }
            }
            tile += bytes;
        }
    }
    Ok(Texture {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rgba_planes_and_partial_tile() {
        let mut bytes = [0; 64];
        bytes[0..2].copy_from_slice(&[128, 10]);
        bytes[32..34].copy_from_slice(&[20, 30]);
        assert_eq!(
            decode(1, 1, 6, &bytes, &[]).unwrap().rgba,
            [10, 20, 30, 128]
        );
        assert!(decode(4, 4, 6, &bytes[..63], &[]).is_err());
    }
    #[test]
    fn cmpr_subblocks_and_transparency() {
        let mut bytes = [0; 32];
        bytes[0..4].copy_from_slice(&[0xf8, 0, 0x07, 0xe0]); // red/green
        bytes[4] = 0b00_01_10_11;
        bytes[8..12].copy_from_slice(&[0, 0, 0xff, 0xff]);
        bytes[12] = 0xff;
        let tex = decode(8, 8, 14, &bytes, &[]).unwrap();
        assert_eq!(
            &tex.rgba[0..16],
            &[255, 0, 0, 255, 0, 255, 0, 255, 170, 85, 0, 255, 85, 170, 0, 255]
        );
        assert_eq!(tex.rgba[4 * 4 + 3], 0);
    }
    #[test]
    fn indexed_palette_bounds_and_intensity_tiles() {
        assert!(decode(8, 4, 9, &[1; 32], &[[0; 4]]).is_err());
        let tex = decode(9, 1, 0, &[0xf0; 64], &[]).unwrap();
        assert_eq!(&tex.rgba[0..8], &[255, 255, 255, 255, 0, 0, 0, 0]);
        assert_eq!(&tex.rgba[32..36], &[255; 4]);
        assert_eq!(rgb5a3(0xffff), [255; 4]);
    }
}
