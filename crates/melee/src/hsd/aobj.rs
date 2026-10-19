//! Animation clock: HSD_AObjReqAnim 0x8036410C, StopAnim 0x8036414C,
//! InterpretAnim 0x80364190. Audited: no fused arithmetic. Global end-callback
//! counters become explicit return state, for a caller to aggregate per frame.
use super::fobj::{Error, Track};
pub const REWINDED: u32 = 1 << 26;
pub const FIRST_PLAY: u32 = 1 << 27;
pub const NO_UPDATE: u32 = 1 << 28;
pub const LOOP: u32 = 1 << 29;
pub const NO_ANIM: u32 = 1 << 30;
#[derive(Debug, Clone)]
pub struct Animation {
    pub flags: u32,
    pub frame: f32,
    pub rewind: f32,
    pub end: f32,
    pub rate: f32,
    pub tracks: Vec<Track>,
}
impl Animation {
    pub fn new(tracks: Vec<Track>, end: f32, flags: u32) -> Result<Self, Error> {
        if !end.is_finite() {
            return Err(Error::InvalidTime);
        }
        Ok(Self {
            flags: (flags & (LOOP | NO_UPDATE)) | NO_ANIM,
            frame: 0.,
            rewind: 0.,
            end,
            rate: 1.,
            tracks,
        })
    }
    pub fn request(&mut self, frame: f32) -> Result<(), Error> {
        if !frame.is_finite() {
            return Err(Error::InvalidTime);
        }
        self.frame = frame;
        self.flags = (self.flags & !NO_ANIM) | FIRST_PLAY;
        for t in &mut self.tracks {
            t.request(frame)?;
        }
        Ok(())
    }
    pub fn stop(&mut self) -> Result<Vec<(u8, f32)>, Error> {
        let mut out = Vec::new();
        for t in &mut self.tracks {
            for v in t.stop(self.rate)? {
                out.push((t.object_type, v));
            }
        }
        self.flags |= NO_ANIM;
        Ok(out)
    }
    pub fn advance(&mut self) -> Result<Vec<(u8, f32)>, Error> {
        if self.flags & NO_ANIM != 0 {
            return Ok(Vec::new());
        }
        if !self.rate.is_finite() || !self.rewind.is_finite() || !self.end.is_finite() {
            return Err(Error::InvalidTime);
        }
        let mut out = Vec::new();
        let mut rate = if self.flags & FIRST_PLAY != 0 {
            self.flags &= !FIRST_PLAY;
            0.
        } else {
            self.frame += self.rate;
            self.rate
        };
        if self.flags & LOOP != 0 && self.end <= self.frame {
            if self.rewind < self.end {
                for t in &mut self.tracks {
                    for v in t.stop(rate)? {
                        out.push((t.object_type, v));
                    }
                }
                self.frame = (self.frame - self.rewind) % (self.end - self.rewind) + self.rewind;
                for t in &mut self.tracks {
                    t.request(self.frame)?;
                }
            } else {
                self.frame = self.end;
            }
            rate = 0.;
            self.flags |= REWINDED;
        } else {
            self.flags &= !REWINDED;
        }
        for t in &mut self.tracks {
            for v in t.advance_with_callback(rate, self.flags & NO_UPDATE == 0)? {
                out.push((t.object_type, v));
            }
        }
        if self.flags & LOOP == 0 && self.end <= self.frame {
            out.extend(self.stop()?);
        }
        Ok(out)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn clock(flags: u32) -> Animation {
        let track = Track::new(&[0x12, 0, 10, 20, 10][..], 1, 0x80, 0, 0).unwrap();
        let mut a = Animation::new(vec![track], 10., flags).unwrap();
        a.request(0.).unwrap();
        a
    }
    #[test]
    fn first_frame_stop_and_loop() {
        let mut a = clock(0);
        a.rate = 5.;
        assert_eq!(a.advance().unwrap(), [(1, 0.)]);
        assert_eq!(a.advance().unwrap(), [(1, 10.)]);
        assert_eq!(a.advance().unwrap(), [(1, 20.)]);
        assert_ne!(a.flags & NO_ANIM, 0);
        assert!(a.advance().unwrap().is_empty());
        let mut b = clock(LOOP);
        b.rate = 6.;
        b.advance().unwrap();
        b.advance().unwrap();
        assert_eq!(b.advance().unwrap(), [(1, 4.)]);
        assert_eq!(b.frame, 2.);
        assert_ne!(b.flags & REWINDED, 0);
        let mut saved = b.clone();
        assert_eq!(b.advance().unwrap(), saved.advance().unwrap());
    }
    #[test]
    fn no_update_preserves_pending_interpolation() {
        let mut a = clock(NO_UPDATE);
        assert!(a.advance().unwrap().is_empty());
        a.flags &= !NO_UPDATE;
        assert_eq!(a.advance().unwrap(), [(1, 2.)]);
    }
}
