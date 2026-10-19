//! A partial Gekko (PowerPC 750CL) disassembler for porting audits.
//!
//! Floating-point, paired-single, and the common integer/branch instructions are
//! decoded fully, because those decide whether a Rust port reproduces the
//! original rounding (for example, whether MWCC fused `a * b + c` into
//! `fmadds`). Anything else is printed as `.word` with its primary opcode.

pub mod interp;

/// A decoded instruction: mnemonic plus operand text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insn {
    pub mnemonic: String,
    pub operands: String,
}

impl std::fmt::Display for Insn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.operands.is_empty() {
            f.write_str(&self.mnemonic)
        } else {
            write!(f, "{:<8} {}", self.mnemonic, self.operands)
        }
    }
}

fn insn(mnemonic: impl Into<String>, operands: impl Into<String>) -> Insn {
    Insn {
        mnemonic: mnemonic.into(),
        operands: operands.into(),
    }
}

fn bits(word: u32, shift: u32, width: u32) -> u32 {
    (word >> shift) & ((1 << width) - 1)
}

fn simm(word: u32) -> i32 {
    i32::from(word as u16 as i16)
}

fn dot(word: u32) -> &'static str {
    if word & 1 != 0 {
        "."
    } else {
        ""
    }
}

/// Floating-point A-form arithmetic (opcode 59 single, 63 double, 4 paired).
fn a_form(word: u32, ext: u32, suffix: &str) -> Option<Insn> {
    let (d, a, b, c) = (
        bits(word, 21, 5),
        bits(word, 16, 5),
        bits(word, 11, 5),
        bits(word, 6, 5),
    );
    // Gekko implements only `fres` (single, opcode 59) and `frsqrte` (double,
    // opcode 63) of the estimate instructions, and no `fsqrt`/`ps_sqrt`.
    let estimate = match (suffix, ext) {
        ("s", 24) => Some("fres"),
        ("", 26) => Some("frsqrte"),
        ("ps_", 24) => Some("ps_res"),
        ("ps_", 26) => Some("ps_rsqrte"),
        (_, 22 | 24 | 26) => return None,
        _ => None,
    };
    if let Some(name) = estimate {
        return Some(insn(format!("{name}{}", dot(word)), format!("f{d}, f{b}")));
    }
    let (name, ops) = match ext {
        18 => ("div", format!("f{d}, f{a}, f{b}")),
        20 => ("sub", format!("f{d}, f{a}, f{b}")),
        21 => ("add", format!("f{d}, f{a}, f{b}")),
        23 => ("sel", format!("f{d}, f{a}, f{c}, f{b}")),
        25 => ("mul", format!("f{d}, f{a}, f{c}")),
        28 => ("msub", format!("f{d}, f{a}, f{c}, f{b}")),
        29 => ("madd", format!("f{d}, f{a}, f{c}, f{b}")),
        30 => ("nmsub", format!("f{d}, f{a}, f{c}, f{b}")),
        31 => ("nmadd", format!("f{d}, f{a}, f{c}, f{b}")),
        _ => return None,
    };
    let mnemonic = if suffix == "ps_" {
        format!("ps_{name}{}", dot(word))
    } else {
        format!("f{name}{suffix}{}", dot(word))
    };
    Some(insn(mnemonic, ops))
}

fn op63_x(word: u32) -> Option<Insn> {
    let (d, a, b) = (bits(word, 21, 5), bits(word, 16, 5), bits(word, 11, 5));
    let r = dot(word);
    Some(match bits(word, 1, 10) {
        0 => insn("fcmpu", format!("cr{}, f{a}, f{b}", d >> 2)),
        32 => insn("fcmpo", format!("cr{}, f{a}, f{b}", d >> 2)),
        12 => insn(format!("frsp{r}"), format!("f{d}, f{b}")),
        14 => insn(format!("fctiw{r}"), format!("f{d}, f{b}")),
        15 => insn(format!("fctiwz{r}"), format!("f{d}, f{b}")),
        40 => insn(format!("fneg{r}"), format!("f{d}, f{b}")),
        72 => insn(format!("fmr{r}"), format!("f{d}, f{b}")),
        136 => insn(format!("fnabs{r}"), format!("f{d}, f{b}")),
        264 => insn(format!("fabs{r}"), format!("f{d}, f{b}")),
        38 => insn("mtfsb1", format!("{d}")),
        70 => insn("mtfsb0", format!("{d}")),
        583 => insn("mffs", format!("f{d}")),
        711 => insn("mtfsf", format!("{:#x}, f{b}", bits(word, 17, 8))),
        _ => return None,
    })
}

fn op4(word: u32) -> Option<Insn> {
    let (d, a, b, c) = (
        bits(word, 21, 5),
        bits(word, 16, 5),
        bits(word, 11, 5),
        bits(word, 6, 5),
    );
    // Cache and indexed quantized load/store forms share opcode 4.
    if bits(word, 1, 10) == 1014 {
        return Some(insn("dcbz_l", format!("r{a}, r{b}")));
    }
    let indexed = match bits(word, 1, 6) {
        6 => Some("psq_lx"),
        7 => Some("psq_stx"),
        38 => Some("psq_lux"),
        39 => Some("psq_stux"),
        _ => None,
    };
    if let Some(name) = indexed {
        return Some(insn(
            name,
            format!(
                "f{d}, r{a}, r{b}, {}, qr{}",
                bits(word, 10, 1),
                bits(word, 7, 3)
            ),
        ));
    }
    if let Some(i) = a_form(word, bits(word, 1, 5), "ps_") {
        return Some(i);
    }
    let r = dot(word);
    let five = match bits(word, 1, 5) {
        10 => Some(("ps_sum0", format!("f{d}, f{a}, f{c}, f{b}"))),
        11 => Some(("ps_sum1", format!("f{d}, f{a}, f{c}, f{b}"))),
        12 => Some(("ps_muls0", format!("f{d}, f{a}, f{c}"))),
        13 => Some(("ps_muls1", format!("f{d}, f{a}, f{c}"))),
        14 => Some(("ps_madds0", format!("f{d}, f{a}, f{c}, f{b}"))),
        15 => Some(("ps_madds1", format!("f{d}, f{a}, f{c}, f{b}"))),
        _ => None,
    };
    if let Some((name, ops)) = five {
        return Some(insn(format!("{name}{r}"), ops));
    }
    let (name, ops) = match bits(word, 1, 10) {
        0 => ("ps_cmpu0", format!("cr{}, f{a}, f{b}", d >> 2)),
        32 => ("ps_cmpo0", format!("cr{}, f{a}, f{b}", d >> 2)),
        64 => ("ps_cmpu1", format!("cr{}, f{a}, f{b}", d >> 2)),
        96 => ("ps_cmpo1", format!("cr{}, f{a}, f{b}", d >> 2)),
        40 => ("ps_neg", format!("f{d}, f{b}")),
        72 => ("ps_mr", format!("f{d}, f{b}")),
        136 => ("ps_nabs", format!("f{d}, f{b}")),
        264 => ("ps_abs", format!("f{d}, f{b}")),
        528 => ("ps_merge00", format!("f{d}, f{a}, f{b}")),
        560 => ("ps_merge01", format!("f{d}, f{a}, f{b}")),
        592 => ("ps_merge10", format!("f{d}, f{a}, f{b}")),
        624 => ("ps_merge11", format!("f{d}, f{a}, f{b}")),
        _ => return None,
    };
    Some(insn(format!("{name}{r}"), ops))
}

fn branch_target(address: u32, offset: i32, absolute: bool) -> u32 {
    if absolute {
        offset as u32
    } else {
        address.wrapping_add(offset as u32)
    }
}

fn op31(word: u32) -> Option<Insn> {
    let (d, a, b) = (bits(word, 21, 5), bits(word, 16, 5), bits(word, 11, 5));
    let r = dot(word);
    let spr = bits(word, 16, 5) | (bits(word, 11, 5) << 5);
    let three = |name: &str| insn(format!("{name}{r}"), format!("r{d}, r{a}, r{b}"));
    // Logical ops write rA from rS (the d field).
    let logical = |name: &str| insn(format!("{name}{r}"), format!("r{a}, r{d}, r{b}"));
    Some(match bits(word, 1, 10) {
        0 => insn("cmpw", format!("cr{}, r{a}, r{b}", d >> 2)),
        32 => insn("cmplw", format!("cr{}, r{a}, r{b}", d >> 2)),
        8 => three("subfc"),
        10 => three("addc"),
        40 => three("subf"),
        104 => insn(format!("neg{r}"), format!("r{d}, r{a}")),
        136 => three("subfe"),
        138 => three("adde"),
        202 => insn(format!("addze{r}"), format!("r{d}, r{a}")),
        235 => three("mullw"),
        75 => three("mulhw"),
        11 => three("mulhwu"),
        266 => three("add"),
        459 => three("divwu"),
        491 => three("divw"),
        28 => logical("and"),
        60 => logical("andc"),
        124 => logical("nor"),
        316 => logical("xor"),
        444 => logical("or"),
        24 => logical("slw"),
        536 => logical("srw"),
        792 => logical("sraw"),
        824 => insn(format!("srawi{r}"), format!("r{a}, r{d}, {b}")),
        26 => insn(format!("cntlzw{r}"), format!("r{a}, r{d}")),
        922 => insn(format!("extsh{r}"), format!("r{a}, r{d}")),
        954 => insn(format!("extsb{r}"), format!("r{a}, r{d}")),
        23 => insn("lwzx", format!("r{d}, r{a}, r{b}")),
        151 => insn("stwx", format!("r{d}, r{a}, r{b}")),
        87 => insn("lbzx", format!("r{d}, r{a}, r{b}")),
        215 => insn("stbx", format!("r{d}, r{a}, r{b}")),
        279 => insn("lhzx", format!("r{d}, r{a}, r{b}")),
        343 => insn("lhax", format!("r{d}, r{a}, r{b}")),
        407 => insn("sthx", format!("r{d}, r{a}, r{b}")),
        535 => insn("lfsx", format!("f{d}, r{a}, r{b}")),
        599 => insn("lfdx", format!("f{d}, r{a}, r{b}")),
        663 => insn("stfsx", format!("f{d}, r{a}, r{b}")),
        727 => insn("stfdx", format!("f{d}, r{a}, r{b}")),
        339 => match spr {
            8 => insn("mflr", format!("r{d}")),
            9 => insn("mfctr", format!("r{d}")),
            _ => insn("mfspr", format!("r{d}, {spr}")),
        },
        467 => match spr {
            8 => insn("mtlr", format!("r{d}")),
            9 => insn("mtctr", format!("r{d}")),
            _ => insn("mtspr", format!("{spr}, r{d}")),
        },
        19 => insn("mfcr", format!("r{d}")),
        144 => insn("mtcrf", format!("{:#x}, r{d}", bits(word, 12, 8))),
        200 => insn(format!("subfze{r}"), format!("r{d}, r{a}")),
        232 => insn(format!("subfme{r}"), format!("r{d}, r{a}")),
        234 => insn(format!("addme{r}"), format!("r{d}, r{a}")),
        284 => logical("eqv"),
        412 => logical("orc"),
        476 => logical("nand"),
        983 => insn("stfiwx", format!("f{d}, r{a}, r{b}")),
        55 => insn("lwzux", format!("r{d}, r{a}, r{b}")),
        183 => insn("stwux", format!("r{d}, r{a}, r{b}")),
        567 => insn("lfsux", format!("f{d}, r{a}, r{b}")),
        695 => insn("stfsux", format!("f{d}, r{a}, r{b}")),
        918 => insn("sthbrx", format!("r{d}, r{a}, r{b}")),
        790 => insn("lhbrx", format!("r{d}, r{a}, r{b}")),
        534 => insn("lwbrx", format!("r{d}, r{a}, r{b}")),
        662 => insn("stwbrx", format!("r{d}, r{a}, r{b}")),
        371 => insn("mftb", format!("r{d}, {spr}")),
        83 => insn("mfmsr", format!("r{d}")),
        146 => insn("mtmsr", format!("r{d}")),
        595 => insn("mfsr", format!("r{d}, {}", a & 0xf)),
        210 => insn("mtsr", format!("{}, r{d}", a & 0xf)),
        512 => insn("mcrxr", format!("cr{}", d >> 2)),
        598 => insn("sync", ""),
        854 => insn("eieio", ""),
        54 => insn("dcbst", format!("r{a}, r{b}")),
        86 => insn("dcbf", format!("r{a}, r{b}")),
        278 => insn("dcbt", format!("r{a}, r{b}")),
        470 => insn("dcbi", format!("r{a}, r{b}")),
        982 => insn("icbi", format!("r{a}, r{b}")),
        1014 => insn("dcbz", format!("r{a}, r{b}")),
        _ => return None,
    })
}

/// Decode one big-endian instruction word located at `address`.
pub fn decode(word: u32, address: u32) -> Insn {
    let (d, a) = (bits(word, 21, 5), bits(word, 16, 5));
    let imm = simm(word);
    let uimm = word & 0xffff;
    let mem = |name: &str, reg: char| insn(name, format!("{reg}{d}, {imm}(r{a})"));
    let decoded = match word >> 26 {
        4 => op4(word),
        7 => Some(insn("mulli", format!("r{d}, r{a}, {imm}"))),
        8 => Some(insn("subfic", format!("r{d}, r{a}, {imm}"))),
        10 => Some(insn("cmplwi", format!("cr{}, r{a}, {uimm}", d >> 2))),
        11 => Some(insn("cmpwi", format!("cr{}, r{a}, {imm}", d >> 2))),
        12 => Some(insn("addic", format!("r{d}, r{a}, {imm}"))),
        13 => Some(insn("addic.", format!("r{d}, r{a}, {imm}"))),
        14 if a == 0 => Some(insn("li", format!("r{d}, {imm}"))),
        14 => Some(insn("addi", format!("r{d}, r{a}, {imm}"))),
        15 if a == 0 => Some(insn("lis", format!("r{d}, {uimm:#x}"))),
        15 => Some(insn("addis", format!("r{d}, r{a}, {uimm:#x}"))),
        16 => {
            let target = branch_target(address, simm(word & !3), word & 2 != 0);
            let link = if word & 1 != 0 { "l" } else { "" };
            Some(insn(
                format!("bc{link}"),
                format!("{d}, {}, {target:#010x}", bits(word, 16, 5)),
            ))
        }
        18 => {
            let offset = ((word & 0x03ff_fffc) as i32) << 6 >> 6;
            let target = branch_target(address, offset, word & 2 != 0);
            let link = if word & 1 != 0 { "l" } else { "" };
            Some(insn(format!("b{link}"), format!("{target:#010x}")))
        }
        19 => match bits(word, 1, 10) {
            16 if d == 20 => Some(insn(if word & 1 != 0 { "blrl" } else { "blr" }, "")),
            16 => Some(insn("bclr", format!("{d}, {a}"))),
            528 if d == 20 => Some(insn(if word & 1 != 0 { "bctrl" } else { "bctr" }, "")),
            150 => Some(insn("isync", "")),
            50 => Some(insn("rfi", "")),
            33 | 129 | 193 | 225 | 257 | 289 | 417 | 449 => {
                let name = match bits(word, 1, 10) {
                    33 => "crnor",
                    129 => "crandc",
                    193 => "crxor",
                    225 => "crnand",
                    257 => "crand",
                    289 => "creqv",
                    417 => "crorc",
                    _ => "cror",
                };
                Some(insn(name, format!("{d}, {a}, {}", bits(word, 11, 5))))
            }
            _ => None,
        },
        17 if word & 2 != 0 => Some(insn("sc", "")),
        20 | 21 => {
            let name = if word >> 26 == 20 { "rlwimi" } else { "rlwinm" };
            Some(insn(
                format!("{name}{}", dot(word)),
                format!(
                    "r{a}, r{d}, {}, {}, {}",
                    bits(word, 11, 5),
                    bits(word, 6, 5),
                    bits(word, 1, 5)
                ),
            ))
        }
        23 => Some(insn(
            format!("rlwnm{}", dot(word)),
            format!(
                "r{a}, r{d}, r{}, {}, {}",
                bits(word, 11, 5),
                bits(word, 6, 5),
                bits(word, 1, 5)
            ),
        )),
        24 if word == 0x6000_0000 => Some(insn("nop", "")),
        24 => Some(insn("ori", format!("r{a}, r{d}, {uimm:#x}"))),
        25 => Some(insn("oris", format!("r{a}, r{d}, {uimm:#x}"))),
        26 => Some(insn("xori", format!("r{a}, r{d}, {uimm:#x}"))),
        // `xoris rX, rY, 0x8000` is half of MWCC's int-to-float idiom.
        27 => Some(insn("xoris", format!("r{a}, r{d}, {uimm:#x}"))),
        28 => Some(insn("andi.", format!("r{a}, r{d}, {uimm:#x}"))),
        29 => Some(insn("andis.", format!("r{a}, r{d}, {uimm:#x}"))),
        31 => op31(word),
        32 => Some(mem("lwz", 'r')),
        33 => Some(mem("lwzu", 'r')),
        34 => Some(mem("lbz", 'r')),
        35 => Some(mem("lbzu", 'r')),
        36 => Some(mem("stw", 'r')),
        37 => Some(mem("stwu", 'r')),
        38 => Some(mem("stb", 'r')),
        39 => Some(mem("stbu", 'r')),
        40 => Some(mem("lhz", 'r')),
        41 => Some(mem("lhzu", 'r')),
        42 => Some(mem("lha", 'r')),
        43 => Some(mem("lhau", 'r')),
        44 => Some(mem("sth", 'r')),
        45 => Some(mem("sthu", 'r')),
        46 => Some(mem("lmw", 'r')),
        47 => Some(mem("stmw", 'r')),
        48 => Some(mem("lfs", 'f')),
        49 => Some(mem("lfsu", 'f')),
        50 => Some(mem("lfd", 'f')),
        51 => Some(mem("lfdu", 'f')),
        52 => Some(mem("stfs", 'f')),
        53 => Some(mem("stfsu", 'f')),
        54 => Some(mem("stfd", 'f')),
        55 => Some(mem("stfdu", 'f')),
        56 | 57 | 60 | 61 => {
            let name = ["psq_l", "psq_lu", "", "", "psq_st", "psq_stu"][(word >> 26) as usize - 56];
            let offset = ((word & 0xfff) as i32) << 20 >> 20;
            Some(insn(
                name,
                format!(
                    "f{d}, {offset}(r{a}), {}, qr{}",
                    bits(word, 15, 1),
                    bits(word, 12, 3)
                ),
            ))
        }
        59 => a_form(word, bits(word, 1, 5), "s"),
        63 => a_form(word, bits(word, 1, 5), "").or_else(|| op63_x(word)),
        _ => None,
    };
    decoded.unwrap_or_else(|| insn(".word", format!("{word:#010x} (op {})", word >> 26)))
}

/// Whether the instruction rounds a fused multiply-add (scalar or paired).
pub fn is_fused(insn: &Insn) -> bool {
    let m = insn.mnemonic.trim_end_matches('.');
    matches!(
        m,
        "fmadd" | "fmsub" | "fnmadd" | "fnmsub" | "fmadds" | "fmsubs" | "fnmadds" | "fnmsubs"
    ) || m.starts_with("ps_madd")
        || m.starts_with("ps_msub")
        || m.starts_with("ps_nmadd")
        || m.starts_with("ps_nmsub")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(word: u32, address: u32) -> String {
        decode(word, address).to_string()
    }

    #[test]
    fn decodes_float_arithmetic() {
        // fmadds f1, f2, f3, f4 ; fsubs f1, f1, f2 ; frsp f1, f1 ; fcmpo cr0, f1, f2
        assert_eq!(text(0xec22_20fa, 0), "fmadds   f1, f2, f3, f4");
        assert!(is_fused(&decode(0xec22_20fa, 0)));
        assert_eq!(text(0xec21_1028, 0), "fsubs    f1, f1, f2");
        assert!(!is_fused(&decode(0xec21_1028, 0)));
        assert_eq!(text(0xfc20_0818, 0), "frsp     f1, f1");
        assert_eq!(text(0xfc01_1040, 0), "fcmpo    cr0, f1, f2");
        assert_eq!(text(0xc023_0014, 0), "lfs      f1, 20(r3)");
        assert_eq!(text(0xd03f_fff8, 0), "stfs     f1, -8(r31)");
    }

    #[test]
    fn decodes_integer_and_branches() {
        assert_eq!(text(0x4e80_0020, 0), "blr");
        assert_eq!(text(0x4bff_fff1, 0x8000_0010), "bl       0x80000000");
        assert_eq!(text(0x3860_ffff, 0), "li       r3, -1");
        assert_eq!(text(0x7c08_02a6, 0), "mflr     r0");
        assert_eq!(text(0x6000_0000, 0), "nop");
        // `<=` on floats: fcmpo then cror eq, lt, eq.
        assert_eq!(text(0x4c40_1382, 0), "cror     2, 0, 2");
        assert_eq!(text(0x0000_0000, 0), ".word    0x00000000 (op 0)");
    }

    #[test]
    fn decodes_estimates_and_gekko_specific_forms() {
        // From the 1.02 DOL: sqrtf__Ff, PSMTXInverse, PSVECMag, and a
        // `dcbz_l` that must not decode as a (nonexistent) `ps_sqrt`.
        assert_eq!(text(0xfc40_0834, 0), "frsqrte  f2, f1");
        assert_eq!(text(0xec00_3830, 0), "fres     f0, f7");
        assert_eq!(text(0xfc00_082e, 0), "fsel     f0, f0, f0, f1");
        assert_eq!(text(0x1006_1fec, 0), "dcbz_l   r6, r3");
        assert_eq!(text(0x1007_3040, 0), "ps_cmpo0 cr0, f7, f6");
        // Int-to-float idiom in sinf: xoris r0, r0, 0x8000.
        assert_eq!(text(0x6c00_8000, 0), "xoris    r0, r0, 0x8000");
        assert_eq!(text(0x7c00_07ae, 0), "stfiwx   f0, r0, r0");
        assert_eq!(text(0x8c03_0001, 0), "lbzu     r0, 1(r3)");
    }
}
