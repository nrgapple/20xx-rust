//! HSD animation tracks: `fobj.c`, Melee 1.02.
//!
//! `parseFloat` 0x8036AC10, `parsePackInfo` 0x8036ADDC,
//! `FObjUpdateAnim` 0x8036AE70, `HSD_FObjInterpretAnim` 0x8036B030.
//! The small state handlers in the decomp are inlined into InterpretAnim.
//! Audited with dol-inspect: parsing/stepping have no fused operations;
//! UpdateAnim has one fmadds, and splGetHelmite (0x80378A34) has three.
//! Byte streams are little-endian inside otherwise big-endian DAT archives.
//! Unlike the unchecked original, malformed streams return errors. Valid-track
//! state, operand order, u16 counters and single-rounding points are preserved.
use crate::ppc::fmadds;
use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Truncated,
    InvalidFraction(u8),
    InvalidOpcode(u8),
    InvalidCount,
    InvalidTime,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid HSD animation track: {self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

/// Immutable stream data is shared; every mutable field is cloned for rollback.
#[derive(Debug, Clone)]
pub struct Track {
    data: Arc<[u8]>,
    cursor: usize,
    pub object_type: u8,
    frac_value: u8,
    frac_slope: u8,
    start_frame: i16,
    flags: u8,
    op: u8,
    interpolation: u8,
    packed: u16,
    term: u16,
    time: f32,
    p0: f32,
    p1: f32,
    d0: f32,
    d1: f32,
}
impl Track {
    pub fn new(
        data: impl Into<Arc<[u8]>>,
        object_type: u8,
        frac_value: u8,
        frac_slope: u8,
        start_frame: i16,
    ) -> Result<Self> {
        for frac in [frac_value, frac_slope] {
            if frac != 0 && !matches!(frac & 0xe0, 0x20 | 0x40 | 0x60 | 0x80) {
                return Err(Error::InvalidFraction(frac));
            }
        }
        Ok(Self {
            data: data.into(),
            cursor: 0,
            object_type,
            frac_value,
            frac_slope,
            start_frame,
            flags: 0,
            op: 0,
            interpolation: 0,
            packed: 0,
            term: 0,
            time: 0.,
            p0: 0.,
            p1: 0.,
            d0: 0.,
            d1: 0.,
        })
    }
    fn state(&self) -> u8 {
        self.flags & 15
    }
    fn set_state(&mut self, state: u8) -> u8 {
        self.flags = (self.flags & 0xf0) | (state & 15);
        state
    }
    /// Inlined HSD_FObjReqAnim, called by HSD_FObjReqAnimAll.
    pub fn request(&mut self, frame: f32) -> Result<()> {
        if !frame.is_finite() {
            return Err(Error::InvalidTime);
        }
        self.cursor = 0;
        self.time = f32::from(self.start_frame) + frame;
        self.op = 0;
        self.interpolation = 0;
        self.flags &= !0x40;
        self.packed = 0;
        self.term = 0;
        self.p0 = 0.;
        self.p1 = 0.;
        self.d0 = 0.;
        self.d1 = 0.;
        self.set_state(1);
        Ok(())
    }
    fn byte(&mut self) -> Result<u8> {
        let b = *self.data.get(self.cursor).ok_or(Error::Truncated)?;
        self.cursor += 1;
        Ok(b)
    }
    fn number(&mut self, frac: u8) -> Result<f32> {
        if frac == 0 {
            return Ok(f32::from_le_bytes([
                self.byte()?,
                self.byte()?,
                self.byte()?,
                self.byte()?,
            ]));
        }
        let numerator = match frac & 0xe0 {
            0x20 => f32::from(i16::from_le_bytes([self.byte()?, self.byte()?])),
            0x40 => f32::from(u16::from_le_bytes([self.byte()?, self.byte()?])),
            0x60 => f32::from(self.byte()? as i8),
            0x80 => f32::from(self.byte()?),
            _ => return Err(Error::InvalidFraction(frac)),
        };
        // The C denominator is signed s32, including the 31-bit shift.
        Ok(numerator / ((1u32 << (frac & 31)) as i32 as f32))
    }
    fn packed_count(&mut self) -> Result<u16> {
        let mut d = self.byte()?;
        let mut count = u32::from((d >> 4) & 7) + 1;
        let mut shift = 3;
        while d & 128 != 0 {
            if shift >= 32 {
                return Err(Error::InvalidCount);
            }
            d = self.byte()?;
            count = count.wrapping_add(u32::from(d & 127) << shift);
            shift += 7;
        }
        // Original stores the parsed count in a u16. Reject zero, which would
        // underflow on decrement and turn a malformed stream into 65535 keys.
        let count = count as u16;
        if count == 0 {
            return Err(Error::InvalidCount);
        }
        Ok(count)
    }
    fn wait(&mut self) -> Result<u16> {
        let mut value = 0u32;
        for shift in (0..35).step_by(7) {
            let d = self.byte()?;
            if shift == 28 && d & 0xf0 != 0 {
                return Err(Error::InvalidCount);
            }
            value |= u32::from(d & 127) << shift;
            if d & 128 == 0 {
                return Ok(value as u16);
            }
        }
        Err(Error::InvalidCount)
    }
    fn launch_key(&mut self) {
        if self.flags & 0x40 != 0 {
            self.interpolation = self.op;
            self.flags = (self.flags & !0x40) | 0x80;
            self.p0 = self.p1;
        }
    }
    fn load_data(&mut self) -> Result<u8> {
        if self.cursor >= self.data.len() {
            return Ok(6);
        }
        self.interpolation = self.op;
        if self.packed == 0 {
            self.op = self.data[self.cursor] & 15;
            self.packed = self.packed_count()?;
        }
        self.packed -= 1;
        match self.op {
            1 | 2 => {
                self.p0 = self.p1;
                self.p1 = self.number(self.frac_value)?;
                if self.interpolation != 5 {
                    self.d0 = self.d1;
                    self.d1 = 0.;
                }
            }
            3 => {
                self.p0 = self.p1;
                self.d0 = self.d1;
                self.p1 = self.number(self.frac_value)?;
                self.d1 = 0.;
            }
            4 => {
                self.p0 = self.p1;
                self.p1 = self.number(self.frac_value)?;
                self.d0 = self.d1;
                self.d1 = self.number(self.frac_slope)?;
            }
            5 => {
                self.d0 = self.d1;
                self.d1 = self.number(self.frac_slope)?;
                return Ok(self.state());
            }
            6 => {
                self.launch_key();
                self.p1 = self.number(self.frac_value)?;
                self.flags |= 0x40;
            }
            op => return Err(Error::InvalidOpcode(op)),
        }
        Ok(self.set_state(if self.state() == 1 { 3 } else { 4 }))
    }
    fn update(&mut self, output: &mut Vec<f32>, enabled: bool) {
        if !enabled {
            return;
        }
        let value = match self.interpolation {
            6 => {
                if self.flags & 0x80 == 0 {
                    return;
                }
                self.flags &= !0x80;
                self.p0
            }
            1 => {
                if self.time >= f32::from(self.term) {
                    self.p1
                } else {
                    self.p0
                }
            }
            2 => {
                if self.flags & 0x20 != 0 {
                    self.flags &= !0x20;
                    if self.term != 0 {
                        self.d0 = (self.p1 - self.p0) / f32::from(self.term);
                    } else {
                        self.d0 = 0.;
                        self.p0 = self.p1;
                    }
                }
                fmadds(self.d0, self.time, self.p0)
            }
            3..=5 => {
                if self.term != 0 {
                    hermite(
                        (1.0f64 / f64::from(self.term)) as f32,
                        self.time,
                        self.p0,
                        self.p1,
                        self.d0,
                        self.d1,
                    )
                } else {
                    self.p1
                }
            }
            // Original invokes its callback with undefined stack data when no
            // interpolation mode exists. A valid animation never needs it.
            _ => return,
        };
        output.push(value);
    }
    /// Advance at the caller's rate and return every callback value in order.
    /// KEY tracks may emit multiple events in one frame; do not keep only last.
    /// `HSD_FObjInterpretAnim` 0x8036B030.
    pub fn advance(&mut self, rate: f32) -> Result<Vec<f32>> {
        self.advance_with_callback(rate, true)
    }
    /// A NULL original callback also suppresses interpolation-state mutations.
    pub fn advance_with_callback(&mut self, rate: f32, enabled: bool) -> Result<Vec<f32>> {
        if !rate.is_finite() {
            return Err(Error::InvalidTime);
        }
        let mut output = Vec::new();
        let mut term = 0.;
        let mut state = self.state();
        if state == 0 {
            return Ok(output);
        }
        self.time += rate;
        if self.time < 0. {
            return Ok(output);
        }
        loop {
            match state {
                6 => {
                    self.time += term;
                    self.launch_key();
                    self.update(&mut output, enabled);
                    return Ok(output);
                }
                1 | 2 => state = self.load_data()?,
                3 => {
                    if self.flags & 0x80 != 0 {
                        self.update(&mut output, enabled);
                    }
                    if self.cursor >= self.data.len() {
                        state = 6;
                    } else {
                        self.term = self.wait()?;
                        self.flags |= 0x20;
                        state = self.set_state(2);
                    }
                }
                4 => {
                    if f32::from(self.term) <= self.time {
                        term = f32::from(self.term);
                        self.time -= f32::from(self.term);
                        state = self.set_state(3);
                    } else {
                        self.update(&mut output, enabled);
                        self.set_state(5);
                        return Ok(output);
                    }
                }
                5 => state = self.set_state(4),
                0 => return Ok(output),
                _ => unreachable!(),
            }
        }
    }
    /// HSD_FObjStopAnim flushes pending KEY callbacks before disabling a track.
    pub fn stop(&mut self, rate: f32) -> Result<Vec<f32>> {
        let values = if self.interpolation == 6 {
            self.advance(rate)?
        } else {
            Vec::new()
        };
        self.set_state(0);
        Ok(values)
    }
}

/// `splGetHelmite` 0x80378A34. Operand order and three fmadds match the DOL.
pub fn hermite(recip_term: f32, time: f32, p0: f32, p1: f32, d0: f32, d1: f32) -> f32 {
    let time2 = time * time;
    let recip2 = recip_term * recip_term;
    let t2_over_t = time2 * recip_term;
    let t3_over_t2 = recip2 * (time2 * time);
    let two_t3_over_t3 = (2.0 * t3_over_t2) * recip_term;
    let three_t2_over_t2 = (3.0 * time2) * recip2;
    let diff = t3_over_t2 - t2_over_t;
    let a = p1 * (-two_t3_over_t3 + three_t2_over_t2);
    let b = fmadds(p0, 1.0 + (two_t3_over_t3 - three_t2_over_t2), a);
    let c = fmadds(d0, time + (diff - t2_over_t), b);
    fmadds(d1, diff, c)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track(bytes: &[u8]) -> Track {
        let mut t = Track::new(Arc::<[u8]>::from(bytes), 1, 0x80, 0x60, 0).unwrap();
        t.request(0.).unwrap();
        t
    }
    #[test]
    fn linear_constant_and_restore() {
        // Two linear keys (0 then 20) separated by 10 frames, final 10 wait.
        let mut t = track(&[0x12, 0, 10, 20, 10]);
        assert_eq!(t.advance(0.).unwrap(), [0.]);
        assert_eq!(t.advance(5.).unwrap(), [10.]);
        let saved = t.clone();
        assert_eq!(t.advance(2.).unwrap(), [14.]);
        assert_eq!(saved.clone().advance(2.).unwrap(), [14.]);
        let mut c = track(&[0x11, 5, 10, 20, 10]);
        assert_eq!(c.advance(0.).unwrap(), [5.]);
        assert_eq!(c.advance(9.).unwrap(), [5.]);
        assert_eq!(c.advance(1.).unwrap(), [20.]);
    }
    #[test]
    fn keys_emit_all_crossed_events_once() {
        let mut t = track(&[0x26, 2, 3, 4, 3, 6, 3]);
        assert_eq!(t.advance(0.).unwrap(), [2.]);
        assert_eq!(t.advance(7.).unwrap(), [4., 6.]);
        assert!(t.advance(1.).unwrap().is_empty());
        t.stop(0.).unwrap();
        assert!(t.advance(1.).unwrap().is_empty());
    }
    #[test]
    fn hermite_endpoints_slopes_and_midpoint() {
        assert_eq!(hermite(0.1, 0., 2., 8., 0., 0.), 2.);
        assert!((hermite(0.1, 10., 2., 8., 0., 0.) - 8.).abs() < 0.00001);
        assert!((hermite(0.1, 5., 2., 8., 0., 0.) - 5.).abs() < 0.00001);
        assert!((hermite(0.1, 5., 2., 8., 1., -1.) - 7.5).abs() < 0.00001);
    }
    #[test]
    fn little_endian_floats_signed_values_and_waits() {
        let mut t = track(&[0, 0, 0x80, 0x3f, 0xf0, 0xff, 0x81, 0x01]);
        assert_eq!(t.number(0).unwrap(), 1.);
        assert_eq!(t.number(0x24).unwrap(), -1.);
        assert_eq!(t.wait().unwrap(), 129);
        let mut t = track(&[0x92, 0x02]);
        assert_eq!(t.packed_count().unwrap(), 18);
    }
    #[test]
    fn delayed_start_and_invalid_streams() {
        let mut t = track(&[0x12, 0, 10, 20, 10]);
        t.request(-2.).unwrap();
        assert!(t.advance(1.).unwrap().is_empty());
        assert_eq!(t.advance(1.).unwrap(), [0.]);
        assert!(track(&[0x12]).advance(0.).is_err());
        assert!(track(&[0x07]).advance(0.).is_err());
        assert!(track(&[0x12, 0, 128]).advance(0.).is_err());
        assert!(t.advance(f32::NAN).is_err());
    }
}
