//! A small Gekko interpreter for differential tests of ported code.
//!
//! It runs leaf math routines straight from the 1.02 `main.dol` so a Rust
//! port can be compared with the original machine code, bit for bit, over
//! many inputs. It covers the integer, branch, scalar floating-point, and
//! paired-single instructions those routines use, and stops with an error on
//! anything else.
//!
//! Floating-point semantics follow the same model as `melee::ppc`, so a test
//! that passes shows that the port performs the same operations in the same
//! order as the machine code. It does not prove the model matches hardware.
//! The model's assumptions:
//!
//! - Single-precision results (`fadds`, `fmadds`, paired-single lanes) are
//!   the `f64` operation (an `f64` fma for fused forms) rounded to `f32`.
//!   For `f32` operands, `+ - * /` computed in `f64` and rounded are
//!   correctly rounded (53 >= 2 * 24 + 2). Fused forms round twice; whether
//!   Gekko does the same in rare double-rounding cases is unverified.
//! - Single-precision arithmetic and `lfs` write both paired-single slots.
//!   (`PSVECCrossProduct` relies on `lfs` filling `ps1`.) Double-precision
//!   arithmetic, `lfd`, `fmr`, `fneg`, `fabs`, and `frsp` write `ps0` only.
//! - `frsqrte` and `fres` call the caller-supplied estimate functions.
//!   [`Cpu::non_single_operands`] counts single-precision operations given an
//!   operand that is not an `f32` value; results there depend on unverified
//!   hardware behavior (and on the estimate's low bits for `frsqrte`).
//! - The only quantization register used is `qr0`, as plain `f32`.
//! - `fnmadd`/`fnmsub` leave a NaN result's sign alone; `fneg` flips it.

use std::collections::HashSet;
use std::fmt;

/// One floating-point register: paired-single slots `ps0` (also the scalar
/// value) and `ps1`, both held as `f64`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Fpr {
    pub ps0: f64,
    pub ps1: f64,
}

/// Why execution stopped before returning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// An instruction this interpreter does not implement.
    Unsupported { pc: u32, word: u32 },
    /// A load, store, or fetch outside emulated memory.
    BadAddress { pc: u32, address: u32 },
    /// Execution reached an address registered with [`Cpu::stop_at`].
    Hook { pc: u32 },
    /// The step limit passed to [`Cpu::call`] ran out.
    StepLimit,
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stop::Unsupported { pc, word } => {
                write!(f, "unsupported {} at {pc:#010x}", crate::decode(*word, *pc))
            }
            Stop::BadAddress { pc, address } => {
                write!(f, "bad address {address:#010x} at {pc:#010x}")
            }
            Stop::Hook { pc } => write!(f, "reached stop address {pc:#010x}"),
            Stop::StepLimit => f.write_str("step limit"),
        }
    }
}

impl std::error::Error for Stop {}

/// Emulated main memory: `0x8000_0000..0x8000_0000 + len`, big-endian.
#[derive(Clone)]
pub struct Memory {
    bytes: Vec<u8>,
}

/// Base address of emulated memory.
pub const MEM_BASE: u32 = 0x8000_0000;
/// Size of emulated memory (24 MiB, the GameCube's main RAM).
pub const MEM_SIZE: u32 = 0x0180_0000;
/// Return address planted in `lr` by [`Cpu::call`]; reaching it ends the call.
pub const RETURN_ADDRESS: u32 = 0x0000_0100;

impl Memory {
    /// Zeroed memory.
    pub fn new() -> Self {
        Self {
            bytes: vec![0; MEM_SIZE as usize],
        }
    }

    /// Memory with each `(address, bytes)` section copied in (e.g. from a
    /// DOL); everything else, including `.bss`, is zero.
    pub fn with_sections<'a>(sections: impl IntoIterator<Item = (u32, &'a [u8])>) -> Self {
        let mut memory = Self::new();
        for (address, bytes) in sections {
            memory
                .slice_mut(address, bytes.len() as u32)
                .expect("section inside emulated memory")
                .copy_from_slice(bytes);
        }
        memory
    }

    fn range(address: u32, len: u32) -> Option<std::ops::Range<usize>> {
        let start = address.checked_sub(MEM_BASE)?;
        let end = start.checked_add(len)?;
        (end <= MEM_SIZE).then_some(start as usize..end as usize)
    }

    pub fn slice(&self, address: u32, len: u32) -> Option<&[u8]> {
        Some(&self.bytes[Self::range(address, len)?])
    }

    pub fn slice_mut(&mut self, address: u32, len: u32) -> Option<&mut [u8]> {
        Some(&mut self.bytes[Self::range(address, len)?])
    }

    pub fn read_u32(&self, address: u32) -> Option<u32> {
        Some(u32::from_be_bytes(self.slice(address, 4)?.try_into().ok()?))
    }

    pub fn write_u32(&mut self, address: u32, value: u32) -> Option<()> {
        self.slice_mut(address, 4)?
            .copy_from_slice(&value.to_be_bytes());
        Some(())
    }

    pub fn read_f32(&self, address: u32) -> Option<f32> {
        self.read_u32(address).map(f32::from_bits)
    }

    pub fn write_f32(&mut self, address: u32, value: f32) -> Option<()> {
        self.write_u32(address, value.to_bits())
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

/// `lfs`: widen single bits to double exactly, keeping NaN payloads.
pub fn widen(bits: u32) -> f64 {
    if bits & 0x7f80_0000 == 0x7f80_0000 {
        let sign = u64::from(bits >> 31) << 63;
        let mantissa = u64::from(bits & 0x007f_ffff) << 29;
        f64::from_bits(sign | 0x7ff0_0000_0000_0000 | mantissa)
    } else {
        f64::from(f32::from_bits(bits))
    }
}

fn is_single(x: f64) -> bool {
    x.is_nan() || f64::from(x as f32) == x
}

/// `stfs`: narrow double to single. Exact for `f32` values (and keeps NaN
/// payloads); otherwise the architecture's bit truncation.
pub fn narrow(x: f64) -> u32 {
    let bits = x.to_bits();
    let sign = ((bits >> 63) as u32) << 31;
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let mantissa = bits & 0x000f_ffff_ffff_ffff;
    if exponent == 0x7ff {
        return sign | 0x7f80_0000 | (mantissa >> 29) as u32;
    }
    if is_single(x) {
        return (x as f32).to_bits();
    }
    if exponent > 896 {
        // Normal single range (or overflow, which the architecture leaves
        // undefined; this keeps the low exponent bits like hardware does).
        let e = ((exponent - 896) as u32) & 0xff;
        sign | (e << 23) | (mantissa >> 29) as u32
    } else if exponent >= 874 {
        // Denormalize: shift the implicit-1 mantissa right, truncating.
        let full = (mantissa | 1 << 52) >> 29;
        sign | (full >> (897 - exponent)) as u32
    } else {
        sign
    }
}

fn round_single(x: f64) -> f64 {
    f64::from(x as f32)
}

fn fma(a: f64, c: f64, b: f64) -> f64 {
    a.mul_add(c, b)
}

/// Negate a fused result as `fnmadd`/`fnmsub` do: NaNs keep their sign.
fn negate_fused(x: f64) -> f64 {
    if x.is_nan() {
        x
    } else {
        -x
    }
}

/// `fctiwz`/`fctiw` saturation: NaN and too-small values give `i32::MIN`.
fn to_int(x: f64) -> i32 {
    if x.is_nan() {
        i32::MIN
    } else {
        x as i32
    }
}

fn bits(word: u32, shift: u32, width: u32) -> u32 {
    (word >> shift) & ((1 << width) - 1)
}

fn simm(word: u32) -> u32 {
    i32::from(word as u16 as i16) as u32
}

fn rotate_mask(mb: u32, me: u32) -> u32 {
    let begin = u32::MAX >> mb;
    let end = u32::MAX << (31 - me);
    if mb <= me {
        begin & end
    } else {
        begin | end
    }
}

/// Estimate function type for `frsqrte`/`fres`.
pub type Estimate = fn(f64) -> f64;

fn unset_estimate(_: f64) -> f64 {
    f64::NAN
}

/// Architectural state plus memory.
#[derive(Clone)]
pub struct Cpu {
    pub gpr: [u32; 32],
    pub fpr: [Fpr; 32],
    /// Condition register; field 0 is the most significant nibble.
    pub cr: u32,
    pub lr: u32,
    pub ctr: u32,
    /// Only the carry bit (`XER[CA]`) is modeled.
    pub carry: bool,
    pub pc: u32,
    pub mem: Memory,
    /// `frsqrte` estimate (input and output are doubles).
    pub frsqrte: Estimate,
    /// `fres` estimate; the result is rounded to single.
    pub fres: Estimate,
    /// Single-precision operations that received a non-`f32` operand.
    pub non_single_operands: u64,
    /// `stfs` stores of a value that was not an `f32` (bit-truncated).
    pub inexact_stores: u64,
    stops: HashSet<u32>,
}

impl Cpu {
    pub fn new(mem: Memory) -> Self {
        Self {
            gpr: [0; 32],
            fpr: [Fpr::default(); 32],
            cr: 0,
            lr: 0,
            ctr: 0,
            carry: false,
            pc: 0,
            mem,
            frsqrte: unset_estimate,
            fres: unset_estimate,
            non_single_operands: 0,
            inexact_stores: 0,
            stops: HashSet::new(),
        }
    }

    /// Stop with [`Stop::Hook`] if execution reaches `address` (e.g. an
    /// assertion handler).
    pub fn stop_at(&mut self, address: u32) {
        self.stops.insert(address);
    }

    /// Set a scalar float argument/result register (both slots).
    pub fn set_f(&mut self, index: usize, value: f64) {
        self.fpr[index] = Fpr {
            ps0: value,
            ps1: value,
        };
    }

    /// Call the function at `address` with `lr` set to [`RETURN_ADDRESS`];
    /// runs until it returns or `max_steps` instructions have executed.
    pub fn call(&mut self, address: u32, max_steps: u64) -> Result<(), Stop> {
        self.lr = RETURN_ADDRESS;
        self.pc = address;
        for _ in 0..max_steps {
            if self.pc == RETURN_ADDRESS {
                return Ok(());
            }
            if self.stops.contains(&self.pc) {
                return Err(Stop::Hook { pc: self.pc });
            }
            self.step()?;
        }
        Err(Stop::StepLimit)
    }

    fn cr_bit(&self, bit: u32) -> bool {
        self.cr >> (31 - bit) & 1 != 0
    }

    fn set_cr_bit(&mut self, bit: u32, value: bool) {
        let mask = 1 << (31 - bit);
        self.cr = if value {
            self.cr | mask
        } else {
            self.cr & !mask
        };
    }

    fn set_cr_field(&mut self, field: u32, nibble: u32) {
        let shift = 28 - 4 * field;
        self.cr = (self.cr & !(0xf << shift)) | (nibble << shift);
    }

    fn record(&mut self, value: u32) {
        let v = value as i32;
        let nibble = if v < 0 {
            8
        } else if v > 0 {
            4
        } else {
            2
        };
        self.set_cr_field(0, nibble);
    }

    fn compare_f(&mut self, field: u32, a: f64, b: f64) {
        let nibble = if a.is_nan() || b.is_nan() {
            1
        } else if a < b {
            8
        } else if a > b {
            4
        } else {
            2
        };
        self.set_cr_field(field, nibble);
    }

    fn bad(&self, address: u32) -> Stop {
        Stop::BadAddress {
            pc: self.pc,
            address,
        }
    }

    fn load32(&self, address: u32) -> Result<u32, Stop> {
        self.mem.read_u32(address).ok_or_else(|| self.bad(address))
    }

    fn store32(&mut self, address: u32, value: u32) -> Result<(), Stop> {
        let err = self.bad(address);
        self.mem.write_u32(address, value).ok_or(err)
    }

    fn load_bytes<const N: usize>(&self, address: u32) -> Result<[u8; N], Stop> {
        let slice = self
            .mem
            .slice(address, N as u32)
            .ok_or_else(|| self.bad(address))?;
        Ok(slice.try_into().unwrap())
    }

    fn store_bytes(&mut self, address: u32, bytes: &[u8]) -> Result<(), Stop> {
        let err = self.bad(address);
        self.mem
            .slice_mut(address, bytes.len() as u32)
            .ok_or(err)?
            .copy_from_slice(bytes);
        Ok(())
    }

    fn check_single(&mut self, values: &[f64]) {
        if values.iter().any(|&v| !is_single(v)) {
            self.non_single_operands += 1;
        }
    }

    fn store_single(&mut self, address: u32, value: f64) -> Result<(), Stop> {
        if !is_single(value) {
            self.inexact_stores += 1;
        }
        self.store32(address, narrow(value))
    }

    fn unsupported(&self, word: u32) -> Stop {
        Stop::Unsupported { pc: self.pc, word }
    }

    fn branch_condition(&mut self, bo: u32, bi: u32) -> bool {
        if bo & 0x04 == 0 {
            self.ctr = self.ctr.wrapping_sub(1);
        }
        let ctr_ok = bo & 0x04 != 0 || ((self.ctr != 0) != (bo & 0x02 != 0));
        let cond_ok = bo & 0x10 != 0 || self.cr_bit(bi) == (bo & 0x08 != 0);
        ctr_ok && cond_ok
    }

    /// Execute one instruction.
    pub fn step(&mut self) -> Result<(), Stop> {
        let word = self.load32(self.pc)?;
        let mut next = self.pc.wrapping_add(4);
        let (d, a, b) = (
            bits(word, 21, 5) as usize,
            bits(word, 16, 5) as usize,
            bits(word, 11, 5) as usize,
        );
        let c = bits(word, 6, 5) as usize;
        let rc = word & 1 != 0;
        let imm = simm(word);
        let uimm = word & 0xffff;
        let ra0 = |cpu: &Self| if a == 0 { 0 } else { cpu.gpr[a] };
        match word >> 26 {
            4 => self.paired(word, d, a, b, c)?,
            7 => self.gpr[d] = self.gpr[a].wrapping_mul(imm),
            8 => {
                let (r, carry) = imm.overflowing_sub(self.gpr[a]);
                self.gpr[d] = r;
                self.carry = !carry;
            }
            10 => {
                let (x, y) = (self.gpr[a], uimm);
                let nibble = if x < y {
                    8
                } else if x > y {
                    4
                } else {
                    2
                };
                self.set_cr_field(d as u32 >> 2, nibble);
            }
            11 => {
                let (x, y) = (self.gpr[a] as i32, imm as i32);
                let nibble = if x < y {
                    8
                } else if x > y {
                    4
                } else {
                    2
                };
                self.set_cr_field(d as u32 >> 2, nibble);
            }
            12 | 13 => {
                let (r, carry) = self.gpr[a].overflowing_add(imm);
                self.gpr[d] = r;
                self.carry = carry;
                if word >> 26 == 13 {
                    self.record(r);
                }
            }
            14 => self.gpr[d] = ra0(self).wrapping_add(imm),
            15 => self.gpr[d] = ra0(self).wrapping_add(uimm << 16),
            16 => {
                if self.branch_condition(d as u32, a as u32) {
                    let offset = simm(word & !3);
                    let target = if word & 2 != 0 {
                        offset
                    } else {
                        self.pc.wrapping_add(offset)
                    };
                    if word & 1 != 0 {
                        self.lr = next;
                    }
                    next = target;
                }
            }
            18 => {
                let offset = (((word & 0x03ff_fffc) as i32) << 6 >> 6) as u32;
                let target = if word & 2 != 0 {
                    offset
                } else {
                    self.pc.wrapping_add(offset)
                };
                if word & 1 != 0 {
                    self.lr = next;
                }
                next = target;
            }
            19 => match bits(word, 1, 10) {
                16 | 528 => {
                    let to_ctr = bits(word, 1, 10) == 528;
                    let cond = if to_ctr {
                        // bcctr never decrements CTR.
                        d as u32 & 0x10 != 0 || self.cr_bit(a as u32) == (d as u32 & 0x08 != 0)
                    } else {
                        self.branch_condition(d as u32, a as u32)
                    };
                    if cond {
                        let target = (if to_ctr { self.ctr } else { self.lr }) & !3;
                        if word & 1 != 0 {
                            self.lr = next;
                        }
                        next = target;
                    }
                }
                150 => {}
                op @ (33 | 129 | 193 | 225 | 257 | 289 | 417 | 449) => {
                    let (x, y) = (self.cr_bit(a as u32), self.cr_bit(b as u32));
                    let r = match op {
                        33 => !(x | y),
                        129 => x & !y,
                        193 => x ^ y,
                        225 => !(x & y),
                        257 => x & y,
                        289 => x == y,
                        417 => x | !y,
                        _ => x | y,
                    };
                    self.set_cr_bit(d as u32, r);
                }
                _ => return Err(self.unsupported(word)),
            },
            20 => {
                let rotated = self.gpr[d].rotate_left(bits(word, 11, 5));
                let mask = rotate_mask(bits(word, 6, 5), bits(word, 1, 5));
                let r = (rotated & mask) | (self.gpr[a] & !mask);
                self.gpr[a] = r;
                if rc {
                    self.record(r);
                }
            }
            21 | 23 => {
                let amount = if word >> 26 == 21 {
                    bits(word, 11, 5)
                } else {
                    self.gpr[b] & 31
                };
                let mask = rotate_mask(bits(word, 6, 5), bits(word, 1, 5));
                let r = self.gpr[d].rotate_left(amount) & mask;
                self.gpr[a] = r;
                if rc {
                    self.record(r);
                }
            }
            24 => self.gpr[a] = self.gpr[d] | uimm,
            25 => self.gpr[a] = self.gpr[d] | uimm << 16,
            26 => self.gpr[a] = self.gpr[d] ^ uimm,
            27 => self.gpr[a] = self.gpr[d] ^ uimm << 16,
            28 | 29 => {
                let mask = if word >> 26 == 28 { uimm } else { uimm << 16 };
                let r = self.gpr[d] & mask;
                self.gpr[a] = r;
                self.record(r);
            }
            31 => self.op31(word, d, a, b)?,
            op @ 32..=47 => {
                let update = op & 1 == 1 && op != 47 && op != 46;
                let ea = if update {
                    self.gpr[a].wrapping_add(imm)
                } else {
                    ra0(self).wrapping_add(imm)
                };
                match op & !1 {
                    32 => self.gpr[d] = self.load32(ea)?,
                    34 => self.gpr[d] = u32::from(self.load_bytes::<1>(ea)?[0]),
                    36 => self.store32(ea, self.gpr[d])?,
                    38 => self.store_bytes(ea, &[self.gpr[d] as u8])?,
                    40 => self.gpr[d] = u32::from(u16::from_be_bytes(self.load_bytes(ea)?)),
                    42 => self.gpr[d] = i32::from(i16::from_be_bytes(self.load_bytes(ea)?)) as u32,
                    44 => self.store_bytes(ea, &(self.gpr[d] as u16).to_be_bytes())?,
                    46 => {
                        for (i, r) in (d..32).enumerate() {
                            let addr = ea.wrapping_add(4 * i as u32);
                            if op == 46 {
                                self.gpr[r] = self.load32(addr)?;
                            } else {
                                self.store32(addr, self.gpr[r])?;
                            }
                        }
                    }
                    _ => unreachable!(),
                }
                if update {
                    self.gpr[a] = ea;
                }
            }
            op @ 48..=55 => {
                let update = op & 1 == 1;
                let ea = if update {
                    self.gpr[a].wrapping_add(imm)
                } else {
                    ra0(self).wrapping_add(imm)
                };
                self.float_mem(op & !1, d, ea)?;
                if update {
                    self.gpr[a] = ea;
                }
            }
            op @ (56 | 57 | 60 | 61) => {
                if bits(word, 12, 3) != 0 {
                    return Err(self.unsupported(word));
                }
                let offset = (((word & 0xfff) as i32) << 20 >> 20) as u32;
                let update = op & 1 == 1;
                let ea = if update {
                    self.gpr[a].wrapping_add(offset)
                } else {
                    ra0(self).wrapping_add(offset)
                };
                let single = bits(word, 15, 1) != 0;
                if op < 60 {
                    let ps0 = widen(self.load32(ea)?);
                    let ps1 = if single {
                        1.0
                    } else {
                        widen(self.load32(ea.wrapping_add(4))?)
                    };
                    self.fpr[d] = Fpr { ps0, ps1 };
                } else {
                    let Fpr { ps0, ps1 } = self.fpr[d];
                    self.store_single(ea, ps0)?;
                    if !single {
                        self.store_single(ea.wrapping_add(4), ps1)?;
                    }
                }
                if update {
                    self.gpr[a] = ea;
                }
            }
            59 => self.single(word, d, a, b, c)?,
            63 => self.double(word, d, a, b, c)?,
            _ => return Err(self.unsupported(word)),
        }
        self.pc = next;
        Ok(())
    }

    fn float_mem(&mut self, op: u32, d: usize, ea: u32) -> Result<(), Stop> {
        match op {
            48 => {
                let v = widen(self.load32(ea)?);
                self.fpr[d] = Fpr { ps0: v, ps1: v };
            }
            50 => self.fpr[d].ps0 = f64::from_bits(u64::from_be_bytes(self.load_bytes(ea)?)),
            52 => self.store_single(ea, self.fpr[d].ps0)?,
            54 => self.store_bytes(ea, &self.fpr[d].ps0.to_bits().to_be_bytes())?,
            _ => unreachable!(),
        }
        Ok(())
    }

    fn op31(&mut self, word: u32, d: usize, a: usize, b: usize) -> Result<(), Stop> {
        let rc = word & 1 != 0;
        let ra0 = if a == 0 { 0 } else { self.gpr[a] };
        let (rd, ra, rb) = (self.gpr[d], self.gpr[a], self.gpr[b]);
        // Arithmetic writes rD; logical/shift forms write rA from rS (= d).
        let mut arith: Option<u32> = None;
        let mut logical: Option<u32> = None;
        match bits(word, 1, 10) {
            0 | 32 => {
                let nibble = if bits(word, 1, 10) == 0 {
                    let (x, y) = (ra as i32, rb as i32);
                    if x < y {
                        8
                    } else if x > y {
                        4
                    } else {
                        2
                    }
                } else if ra < rb {
                    8
                } else if ra > rb {
                    4
                } else {
                    2
                };
                self.set_cr_field(d as u32 >> 2, nibble);
            }
            8 => {
                let (r, borrow) = rb.overflowing_sub(ra);
                self.carry = !borrow;
                arith = Some(r);
            }
            10 => {
                let (r, carry) = ra.overflowing_add(rb);
                self.carry = carry;
                arith = Some(r);
            }
            40 => arith = Some(rb.wrapping_sub(ra)),
            104 => arith = Some(ra.wrapping_neg()),
            136 => {
                let wide = u64::from(!ra) + u64::from(rb) + u64::from(self.carry);
                self.carry = wide >> 32 != 0;
                arith = Some(wide as u32);
            }
            138 => {
                let wide = u64::from(ra) + u64::from(rb) + u64::from(self.carry);
                self.carry = wide >> 32 != 0;
                arith = Some(wide as u32);
            }
            200 => {
                let wide = u64::from(!ra) + u64::from(self.carry);
                self.carry = wide >> 32 != 0;
                arith = Some(wide as u32);
            }
            202 => {
                let wide = u64::from(ra) + u64::from(self.carry);
                self.carry = wide >> 32 != 0;
                arith = Some(wide as u32);
            }
            235 => arith = Some(ra.wrapping_mul(rb)),
            75 => arith = Some(((i64::from(ra as i32) * i64::from(rb as i32)) >> 32) as u32),
            11 => arith = Some(((u64::from(ra) * u64::from(rb)) >> 32) as u32),
            266 => arith = Some(ra.wrapping_add(rb)),
            459 => arith = Some(ra.checked_div(rb).unwrap_or(0)),
            491 => arith = Some((ra as i32).checked_div(rb as i32).unwrap_or(0) as u32),
            28 => logical = Some(rd & rb),
            60 => logical = Some(rd & !rb),
            124 => logical = Some(!(rd | rb)),
            284 => logical = Some(!(rd ^ rb)),
            316 => logical = Some(rd ^ rb),
            412 => logical = Some(rd | !rb),
            444 => logical = Some(rd | rb),
            476 => logical = Some(!(rd & rb)),
            24 => logical = Some(if rb & 0x20 != 0 { 0 } else { rd << (rb & 31) }),
            536 => logical = Some(if rb & 0x20 != 0 { 0 } else { rd >> (rb & 31) }),
            792 => {
                let shift = rb & 0x3f;
                let value = rd as i32;
                let r = if shift > 31 {
                    value >> 31
                } else {
                    value >> shift
                };
                let lost = if shift > 31 {
                    rd != 0
                } else {
                    shift != 0 && rd << (32 - shift) != 0
                };
                self.carry = value < 0 && lost;
                logical = Some(r as u32);
            }
            824 => {
                let shift = b as u32;
                let value = rd as i32;
                self.carry = value < 0 && shift != 0 && rd << (32 - shift) != 0;
                logical = Some((value >> shift) as u32);
            }
            26 => logical = Some(rd.leading_zeros()),
            922 => logical = Some(rd as u16 as i16 as i32 as u32),
            954 => logical = Some(rd as u8 as i8 as i32 as u32),
            23 => self.gpr[d] = self.load32(ra0.wrapping_add(rb))?,
            151 => self.store32(ra0.wrapping_add(rb), rd)?,
            87 => self.gpr[d] = u32::from(self.load_bytes::<1>(ra0.wrapping_add(rb))?[0]),
            215 => self.store_bytes(ra0.wrapping_add(rb), &[rd as u8])?,
            279 => {
                self.gpr[d] = u32::from(u16::from_be_bytes(self.load_bytes(ra0.wrapping_add(rb))?))
            }
            343 => {
                self.gpr[d] =
                    i32::from(i16::from_be_bytes(self.load_bytes(ra0.wrapping_add(rb))?)) as u32
            }
            407 => self.store_bytes(ra0.wrapping_add(rb), &(rd as u16).to_be_bytes())?,
            55 | 183 => {
                let ea = ra.wrapping_add(rb);
                if bits(word, 1, 10) == 55 {
                    self.gpr[d] = self.load32(ea)?;
                } else {
                    self.store32(ea, rd)?;
                }
                self.gpr[a] = ea;
            }
            535 => self.float_mem(48, d, ra0.wrapping_add(rb))?,
            599 => self.float_mem(50, d, ra0.wrapping_add(rb))?,
            663 => self.float_mem(52, d, ra0.wrapping_add(rb))?,
            727 => self.float_mem(54, d, ra0.wrapping_add(rb))?,
            983 => self.store32(ra0.wrapping_add(rb), self.fpr[d].ps0.to_bits() as u32)?,
            339 => {
                let spr = bits(word, 16, 5) | bits(word, 11, 5) << 5;
                self.gpr[d] = match spr {
                    8 => self.lr,
                    9 => self.ctr,
                    _ => return Err(self.unsupported(word)),
                };
            }
            467 => {
                let spr = bits(word, 16, 5) | bits(word, 11, 5) << 5;
                match spr {
                    8 => self.lr = rd,
                    9 => self.ctr = rd,
                    _ => return Err(self.unsupported(word)),
                }
            }
            19 => self.gpr[d] = self.cr,
            144 => {
                let fxm = bits(word, 12, 8);
                for field in 0..8 {
                    if fxm & (0x80 >> field) != 0 {
                        self.set_cr_field(field, rd >> (28 - 4 * field) & 0xf);
                    }
                }
            }
            _ => return Err(self.unsupported(word)),
        }
        if let Some(r) = arith {
            self.gpr[d] = r;
            if rc {
                self.record(r);
            }
        }
        if let Some(r) = logical {
            self.gpr[a] = r;
            if rc {
                self.record(r);
            }
        }
        Ok(())
    }

    fn single(&mut self, word: u32, d: usize, a: usize, b: usize, c: usize) -> Result<(), Stop> {
        let (fa, fb, fc) = (self.fpr[a].ps0, self.fpr[b].ps0, self.fpr[c].ps0);
        let r = match bits(word, 1, 5) {
            18 => {
                self.check_single(&[fa, fb]);
                round_single(fa / fb)
            }
            20 => {
                self.check_single(&[fa, fb]);
                round_single(fa - fb)
            }
            21 => {
                self.check_single(&[fa, fb]);
                round_single(fa + fb)
            }
            24 => round_single((self.fres)(fb)),
            25 => {
                self.check_single(&[fa, fc]);
                round_single(fa * fc)
            }
            28 => {
                self.check_single(&[fa, fb, fc]);
                round_single(fma(fa, fc, -fb))
            }
            29 => {
                self.check_single(&[fa, fb, fc]);
                round_single(fma(fa, fc, fb))
            }
            30 => {
                self.check_single(&[fa, fb, fc]);
                negate_fused(round_single(fma(fa, fc, -fb)))
            }
            31 => {
                self.check_single(&[fa, fb, fc]);
                negate_fused(round_single(fma(fa, fc, fb)))
            }
            _ => return Err(self.unsupported(word)),
        };
        if word & 1 != 0 {
            return Err(self.unsupported(word));
        }
        self.fpr[d] = Fpr { ps0: r, ps1: r };
        Ok(())
    }

    fn double(&mut self, word: u32, d: usize, a: usize, b: usize, c: usize) -> Result<(), Stop> {
        if word & 1 != 0 {
            return Err(self.unsupported(word));
        }
        let (fa, fb, fc) = (self.fpr[a].ps0, self.fpr[b].ps0, self.fpr[c].ps0);
        let r = match bits(word, 1, 5) {
            18 => fa / fb,
            20 => fa - fb,
            21 => fa + fb,
            23 => {
                if fa >= 0.0 {
                    fc
                } else {
                    fb
                }
            }
            25 => fa * fc,
            26 => (self.frsqrte)(fb),
            28 => fma(fa, fc, -fb),
            29 => fma(fa, fc, fb),
            30 => negate_fused(fma(fa, fc, -fb)),
            31 => negate_fused(fma(fa, fc, fb)),
            _ => match bits(word, 1, 10) {
                0 | 32 => {
                    self.compare_f(d as u32 >> 2, fa, fb);
                    return Ok(());
                }
                12 => round_single(fb),
                14 | 15 => {
                    let v = if bits(word, 1, 10) == 15 {
                        to_int(fb)
                    } else {
                        to_int(fb.round_ties_even())
                    };
                    f64::from_bits(0xfff8_0000_0000_0000 | u64::from(v as u32))
                }
                40 => -fb,
                72 => fb,
                136 => -fb.abs(),
                264 => fb.abs(),
                _ => return Err(self.unsupported(word)),
            },
        };
        self.fpr[d].ps0 = r;
        Ok(())
    }

    fn paired(&mut self, word: u32, d: usize, a: usize, b: usize, c: usize) -> Result<(), Stop> {
        if word & 1 != 0 {
            return Err(self.unsupported(word));
        }
        let (fa, fb, fc) = (self.fpr[a], self.fpr[b], self.fpr[c]);
        let lanes = |f: &dyn Fn(f64, f64, f64) -> f64| Fpr {
            ps0: round_single(f(fa.ps0, fb.ps0, fc.ps0)),
            ps1: round_single(f(fa.ps1, fb.ps1, fc.ps1)),
        };
        let used: &[f64] = match bits(word, 1, 5) {
            18 | 20 | 21 => &[fa.ps0, fa.ps1, fb.ps0, fb.ps1],
            25 => &[fa.ps0, fa.ps1, fc.ps0, fc.ps1],
            28..=31 => &[fa.ps0, fa.ps1, fb.ps0, fb.ps1, fc.ps0, fc.ps1],
            10 => &[fa.ps0, fb.ps1, fc.ps1],
            11 => &[fa.ps0, fb.ps1, fc.ps0],
            12 => &[fa.ps0, fa.ps1, fc.ps0],
            13 => &[fa.ps0, fa.ps1, fc.ps1],
            14 => &[fa.ps0, fa.ps1, fb.ps0, fb.ps1, fc.ps0],
            15 => &[fa.ps0, fa.ps1, fb.ps0, fb.ps1, fc.ps1],
            _ => &[],
        };
        let used = used.to_vec();
        let r = match bits(word, 1, 5) {
            18 => lanes(&|a, b, _| a / b),
            20 => lanes(&|a, b, _| a - b),
            21 => lanes(&|a, b, _| a + b),
            25 => lanes(&|a, _, c| a * c),
            28 => lanes(&|a, b, c| fma(a, c, -b)),
            29 => lanes(&|a, b, c| fma(a, c, b)),
            30 => lanes(&|a, b, c| negate_fused(fma(a, c, -b))),
            31 => lanes(&|a, b, c| negate_fused(fma(a, c, b))),
            10 => Fpr {
                ps0: round_single(fa.ps0 + fb.ps1),
                ps1: round_single(fc.ps1),
            },
            11 => Fpr {
                ps0: round_single(fc.ps0),
                ps1: round_single(fa.ps0 + fb.ps1),
            },
            12 => Fpr {
                ps0: round_single(fa.ps0 * fc.ps0),
                ps1: round_single(fa.ps1 * fc.ps0),
            },
            13 => Fpr {
                ps0: round_single(fa.ps0 * fc.ps1),
                ps1: round_single(fa.ps1 * fc.ps1),
            },
            14 => Fpr {
                ps0: round_single(fma(fa.ps0, fc.ps0, fb.ps0)),
                ps1: round_single(fma(fa.ps1, fc.ps0, fb.ps1)),
            },
            15 => Fpr {
                ps0: round_single(fma(fa.ps0, fc.ps1, fb.ps0)),
                ps1: round_single(fma(fa.ps1, fc.ps1, fb.ps1)),
            },
            _ => {
                let r = match bits(word, 1, 10) {
                    op @ (0 | 32 | 64 | 96) => {
                        let (x, y) = if op < 64 {
                            (fa.ps0, fb.ps0)
                        } else {
                            (fa.ps1, fb.ps1)
                        };
                        self.compare_f(d as u32 >> 2, x, y);
                        return Ok(());
                    }
                    40 => Fpr {
                        ps0: -fb.ps0,
                        ps1: -fb.ps1,
                    },
                    72 => fb,
                    136 => Fpr {
                        ps0: -fb.ps0.abs(),
                        ps1: -fb.ps1.abs(),
                    },
                    264 => Fpr {
                        ps0: fb.ps0.abs(),
                        ps1: fb.ps1.abs(),
                    },
                    528 => Fpr {
                        ps0: fa.ps0,
                        ps1: fb.ps0,
                    },
                    560 => Fpr {
                        ps0: fa.ps0,
                        ps1: fb.ps1,
                    },
                    592 => Fpr {
                        ps0: fa.ps1,
                        ps1: fb.ps0,
                    },
                    624 => Fpr {
                        ps0: fa.ps1,
                        ps1: fb.ps1,
                    },
                    _ => return Err(self.unsupported(word)),
                };
                self.fpr[d] = r;
                return Ok(());
            }
        };
        self.check_single(&used);
        self.fpr[d] = r;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(code: &[u32], setup: impl FnOnce(&mut Cpu)) -> Cpu {
        let bytes: Vec<u8> = code.iter().flat_map(|w| w.to_be_bytes()).collect();
        let mut cpu = Cpu::new(Memory::with_sections([(0x8000_3000, &bytes[..])]));
        cpu.gpr[1] = 0x8100_0000;
        setup(&mut cpu);
        cpu.call(0x8000_3000, 1000).unwrap();
        cpu
    }

    #[test]
    fn runs_fused_single_ops() {
        // fmadds f1, f2, f3, f4 ; blr
        let cpu = run(&[0xec22_20fa, 0x4e80_0020], |cpu| {
            cpu.set_f(2, f64::from(1.0 + f32::EPSILON));
            cpu.set_f(3, f64::from(1.0 - f32::EPSILON));
            cpu.set_f(4, -1.0);
        });
        assert_eq!(cpu.fpr[1].ps0, -f64::from(f32::EPSILON * f32::EPSILON));
        assert_eq!(cpu.fpr[1].ps1, cpu.fpr[1].ps0);
    }

    #[test]
    fn runs_branches_and_compares() {
        // fcmpo cr0, f1, f2 ; bc 4, 0, +8 (skip if !lt) ; fmr f1, f2 ; blr
        let code = [0xfc01_1040, 0x4080_0008, 0xfc20_1090, 0x4e80_0020];
        let cpu = run(&code, |cpu| {
            cpu.set_f(1, 1.0);
            cpu.set_f(2, 2.0);
        });
        assert_eq!(cpu.fpr[1].ps0, 2.0);
        let cpu = run(&code, |cpu| {
            cpu.set_f(1, 3.0);
            cpu.set_f(2, 2.0);
        });
        assert_eq!(cpu.fpr[1].ps0, 3.0);
    }

    #[test]
    fn fctiwz_saturates_and_maps_nan() {
        // fctiwz f0, f1 ; stfiwx f0, 0, r3 ; blr
        for (input, expected) in [
            (-2.75, -2),
            (3e9, i32::MAX),
            (-3e9, i32::MIN),
            (f64::NAN, i32::MIN),
        ] {
            let cpu = run(&[0xfc00_081e, 0x7c00_1fae, 0x4e80_0020], |cpu| {
                cpu.set_f(1, input);
                cpu.gpr[3] = 0x8001_0000;
            });
            assert_eq!(cpu.mem.read_u32(0x8001_0000), Some(expected as u32));
        }
    }

    #[test]
    fn narrow_truncates_non_single_values() {
        assert_eq!(narrow(1.5), 1.5f32.to_bits());
        let above_one = 1.0 + f64::EPSILON;
        assert_eq!(narrow(above_one), 1.0f32.to_bits());
        assert_eq!(narrow(-(1.0 - f64::EPSILON / 2.0)), 0xbf7f_ffff);
        assert_eq!(widen(0x7fa0_0001).to_bits() >> 29 & 0x3f_ffff, 0x20_0001);
    }
}
