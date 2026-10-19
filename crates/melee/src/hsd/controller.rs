//! Port of HSD's controller library (`src/sysdolphin/baselib/controller.c`):
//! the raw-sample queue, clamping to the stick circle, the stick-direction
//! bits, conversion to floats, and the per-frame button edges.
//!
//! Every function names its decomp symbol and 1.02 address. All were audited
//! with `dol-inspect` against `melee-1.02.dol`; see `research/notes/input.md`.
//! The library's globals (`HSD_PadLibData`, `HSD_PadMasterStatus`,
//! `HSD_PadCopyStatus`, `HSD_PadGameStatus`, the queue) live in [`PadLib`],
//! which is part of the rollback state: `last_button`, `repeat_count`, and the
//! transfer-error path carry values from one frame to the next.
//!
//! Hardware side effects are not ported: `OSDisableInterrupts`, rumble
//! (`HSD_PadRumbleInterpret`), `PADReset` of unplugged ports, and the reset
//! button. `PADRead` itself, which turns the controller's report into a
//! [`PadStatus`] (origin subtraction, analog mode 3), belongs to the browser
//! shell that samples the Gamepad API.

use crate::ppc;

/// `HSD_PAD_DPADLEFT`, also `PAD_BUTTON_LEFT`.
pub const HSD_PAD_DPADLEFT: u32 = 1 << 0;
/// `HSD_PAD_DPADRIGHT`
pub const HSD_PAD_DPADRIGHT: u32 = 1 << 1;
/// `HSD_PAD_DPADDOWN`
pub const HSD_PAD_DPADDOWN: u32 = 1 << 2;
/// `HSD_PAD_DPADUP`
pub const HSD_PAD_DPADUP: u32 = 1 << 3;
/// `HSD_PAD_Z`
pub const HSD_PAD_Z: u32 = 1 << 4;
/// `HSD_PAD_R`: the digital (fully pressed) R click.
pub const HSD_PAD_R: u32 = 1 << 5;
/// `HSD_PAD_L`: the digital L click.
pub const HSD_PAD_L: u32 = 1 << 6;
/// `HSD_PAD_A`
pub const HSD_PAD_A: u32 = 1 << 8;
/// `HSD_PAD_B`
pub const HSD_PAD_B: u32 = 1 << 9;
/// `HSD_PAD_X`
pub const HSD_PAD_X: u32 = 1 << 10;
/// `HSD_PAD_Y`
pub const HSD_PAD_Y: u32 = 1 << 11;
/// `HSD_PAD_START`
pub const HSD_PAD_START: u32 = 1 << 12;
/// `HSD_PAD_XY`
pub const HSD_PAD_XY: u32 = HSD_PAD_X | HSD_PAD_Y;
/// `HSD_PAD_AB`
pub const HSD_PAD_AB: u32 = HSD_PAD_A | HSD_PAD_B;
/// `HSD_PAD_LR`: set by `Fighter_procInput` for any L/R input, digital or analog.
pub const HSD_PAD_LR: u32 = 1 << 31;
/// `PAD_STICK_UP` .. `PAD_SUBSTICK_RIGHT`: direction bits that
/// [`PadLib::renew_master_status`] adds to `button` (`HSD_PadADConvert`).
pub const PAD_STICK_UP: u32 = 1 << 16;
pub const PAD_STICK_DOWN: u32 = 1 << 17;
pub const PAD_STICK_LEFT: u32 = 1 << 18;
pub const PAD_STICK_RIGHT: u32 = 1 << 19;
pub const PAD_SUBSTICK_UP: u32 = 1 << 20;
pub const PAD_SUBSTICK_DOWN: u32 = 1 << 21;
pub const PAD_SUBSTICK_LEFT: u32 = 1 << 22;
pub const PAD_SUBSTICK_RIGHT: u32 = 1 << 23;

/// `PAD_ERR_NONE`
pub const PAD_ERR_NONE: i8 = 0;
/// `PAD_ERR_NO_CONTROLLER`
pub const PAD_ERR_NO_CONTROLLER: i8 = -1;
/// `PAD_ERR_NOT_READY`
pub const PAD_ERR_NOT_READY: i8 = -2;
/// `PAD_ERR_TRANSFER`: the HSD layer keeps the previous frame's values.
pub const PAD_ERR_TRANSFER: i8 = -3;

/// One controller sample: the eight values a standard GameCube controller
/// reports, in the form the SDK's `PADRead` returns them (sticks centered on
/// the calibrated origin, triggers minus their origin).
///
/// This is the simulation's input: one per port per frame, and what netplay
/// exchanges. [`PadInput::to_bytes`] gives the 8-byte wire form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PadInput {
    /// `PADStatus.button`: `PAD_BUTTON_*`/`PAD_TRIGGER_*` bits (`0x1F7F`
    /// at most; `PADRead` clears bit 7).
    pub button: u16,
    pub stick_x: i8,
    pub stick_y: i8,
    pub substick_x: i8,
    pub substick_y: i8,
    pub trigger_left: u8,
    pub trigger_right: u8,
}

impl PadInput {
    /// Wire form: big-endian `button`, then the six analog bytes in
    /// `PADStatus` order.
    pub fn to_bytes(self) -> [u8; 8] {
        let [hi, lo] = self.button.to_be_bytes();
        [
            hi,
            lo,
            self.stick_x as u8,
            self.stick_y as u8,
            self.substick_x as u8,
            self.substick_y as u8,
            self.trigger_left,
            self.trigger_right,
        ]
    }

    pub fn from_bytes(bytes: [u8; 8]) -> Self {
        Self {
            button: u16::from_be_bytes([bytes[0], bytes[1]]),
            stick_x: bytes[2] as i8,
            stick_y: bytes[3] as i8,
            substick_x: bytes[4] as i8,
            substick_y: bytes[5] as i8,
            trigger_left: bytes[6],
            trigger_right: bytes[7],
        }
    }
}

/// `PADStatus` (`dolphin/pad.h`), 12 bytes in the SDK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PadStatus {
    /// `+0`
    pub button: u16,
    /// `+2`
    pub stick_x: i8,
    /// `+3`
    pub stick_y: i8,
    /// `+4`
    pub substick_x: i8,
    /// `+5`
    pub substick_y: i8,
    /// `+6`
    pub trigger_left: u8,
    /// `+7`
    pub trigger_right: u8,
    /// `+8`: always 0 from a standard controller in the SDK's default
    /// analog mode 3 (`SPEC2_MakeStatus`); Melee never changes the mode.
    pub analog_a: u8,
    /// `+9`: as `analog_a`.
    pub analog_b: u8,
    /// `+A`: `PAD_ERR_*`.
    pub err: i8,
}

impl From<PadInput> for PadStatus {
    /// The sample of a connected standard controller.
    fn from(input: PadInput) -> Self {
        Self {
            button: input.button,
            stick_x: input.stick_x,
            stick_y: input.stick_y,
            substick_x: input.substick_x,
            substick_y: input.substick_y,
            trigger_left: input.trigger_left,
            trigger_right: input.trigger_right,
            analog_a: 0,
            analog_b: 0,
            err: PAD_ERR_NONE,
        }
    }
}

impl PadStatus {
    /// An empty port (`PADRead` reports `PAD_ERR_NO_CONTROLLER` with zeros).
    pub const DISCONNECTED: Self = Self {
        button: 0,
        stick_x: 0,
        stick_y: 0,
        substick_x: 0,
        substick_y: 0,
        trigger_left: 0,
        trigger_right: 0,
        analog_a: 0,
        analog_b: 0,
        err: PAD_ERR_NO_CONTROLLER,
    };
}

/// `HSD_PadData`: one poll of all four ports.
pub type PadData = [PadStatus; 4];

/// `HSD_PadStatus`, 0x44 bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HsdPadStatus {
    /// `+0`: buttons plus the stick-direction bits (`PAD_STICK_*`).
    pub button: u32,
    /// `+4`
    pub last_button: u32,
    /// `+8`: buttons pressed this frame.
    pub trigger: u32,
    /// `+C`: `trigger`, then `button` again every `repeat_interval` frames.
    pub repeat: u32,
    /// `+10`: buttons released this frame.
    pub release: u32,
    /// `+14`
    pub repeat_count: i32,
    /// `+18`: clamped stick.
    pub stick_x: i8,
    /// `+19`
    pub stick_y: i8,
    /// `+1A`
    pub sub_stick_x: i8,
    /// `+1B`
    pub sub_stick_y: i8,
    /// `+1C`
    pub analog_l: u8,
    /// `+1D`
    pub analog_r: u8,
    /// `+1E`
    pub analog_a: u8,
    /// `+1F`
    pub analog_b: u8,
    /// `+20`: `stick_x / scale_stick`.
    pub nml_stick_x: f32,
    /// `+24`
    pub nml_stick_y: f32,
    /// `+28`
    pub nml_sub_stick_x: f32,
    /// `+2C`
    pub nml_sub_stick_y: f32,
    /// `+30`: `analog_l / scale_analog_lr`.
    pub nml_analog_l: f32,
    /// `+34`
    pub nml_analog_r: f32,
    /// `+38`
    pub nml_analog_a: f32,
    /// `+3C`
    pub nml_analog_b: f32,
    /// `+40`: state for `HSD_PadCrossDir` mode 3.
    pub cross_dir: u8,
    /// `+41`: `PAD_ERR_*` of the last sample.
    pub err: i8,
}

impl Default for HsdPadStatus {
    /// `default_status_data` (0x80406D68): zero except `cross_dir = 1`
    /// (checked against the 1.02 DOL bytes).
    fn default() -> Self {
        Self {
            button: 0,
            last_button: 0,
            trigger: 0,
            repeat: 0,
            release: 0,
            repeat_count: 0,
            stick_x: 0,
            stick_y: 0,
            sub_stick_x: 0,
            sub_stick_y: 0,
            analog_l: 0,
            analog_r: 0,
            analog_a: 0,
            analog_b: 0,
            nml_stick_x: 0.0,
            nml_stick_y: 0.0,
            nml_sub_stick_x: 0.0,
            nml_sub_stick_y: 0.0,
            nml_analog_l: 0.0,
            nml_analog_r: 0.0,
            nml_analog_a: 0.0,
            nml_analog_b: 0.0,
            cross_dir: 1,
            err: 0,
        }
    }
}

impl HsdPadStatus {
    /// `HSD_PadCopyStatusFields` (inline).
    fn copy_fields(&mut self, src: &HsdPadStatus) {
        self.button = src.button;
        self.stick_x = src.stick_x;
        self.stick_y = src.stick_y;
        self.sub_stick_x = src.sub_stick_x;
        self.sub_stick_y = src.sub_stick_y;
        self.analog_l = src.analog_l;
        self.analog_r = src.analog_r;
        self.analog_a = src.analog_a;
        self.analog_b = src.analog_b;
        self.nml_stick_x = src.nml_stick_x;
        self.nml_stick_y = src.nml_stick_y;
        self.nml_sub_stick_x = src.nml_sub_stick_x;
        self.nml_sub_stick_y = src.nml_sub_stick_y;
        self.nml_analog_l = src.nml_analog_l;
        self.nml_analog_r = src.nml_analog_r;
        self.nml_analog_a = src.nml_analog_a;
        self.nml_analog_b = src.nml_analog_b;
    }

    /// `HSD_PadClearStatusFields` (inline; also inlined in
    /// `HSD_PadRenewMasterStatus`).
    fn clear_fields(&mut self) {
        self.button = 0;
        self.sub_stick_y = 0;
        self.sub_stick_x = 0;
        self.stick_y = 0;
        self.stick_x = 0;
        self.analog_b = 0;
        self.analog_a = 0;
        self.analog_r = 0;
        self.analog_l = 0;
        self.nml_sub_stick_y = 0.0;
        self.nml_sub_stick_x = 0.0;
        self.nml_stick_y = 0.0;
        self.nml_stick_x = 0.0;
        self.nml_analog_b = 0.0;
        self.nml_analog_a = 0.0;
        self.nml_analog_r = 0.0;
        self.nml_analog_l = 0.0;
    }

    /// The edge and repeat tail shared by `HSD_PadRenewMasterStatus`,
    /// `HSD_PadRenewCopyStatus`, and `HSD_PadRenewGameStatus` (written out
    /// three times in the C).
    fn renew_edges(&mut self, repeat_start: i32, repeat_interval: i32) {
        self.trigger = self.button & (self.last_button ^ self.button);
        self.release = self.last_button & (self.last_button ^ self.button);
        if self.last_button ^ self.button != 0 {
            self.repeat = self.trigger;
            self.repeat_count = repeat_start;
        } else {
            let count = self.repeat_count.wrapping_sub(1);
            self.repeat_count = count;
            if count != 0 {
                self.repeat = 0;
            } else {
                self.repeat = self.button;
                self.repeat_count = repeat_interval;
            }
        }
    }
}

/// `HSD_FlushType`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushType {
    Merge,
    Throwaway,
    Leave1,
    Terminate,
}

/// `PadLibData` (`HSD_PadLibData`), minus the queue pointer (the queue is
/// [`PadLib::queue`]) and `rumble_info`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PadLibData {
    /// `+0`
    pub qnum: u8,
    /// `+1`
    pub qread: u8,
    /// `+2`
    pub qwrite: u8,
    /// `+3`
    pub qcount: u8,
    /// `+4`: 0 merges buttons into the next sample when the queue is full.
    pub qtype: u8,
    /// `+C`
    pub repeat_start: i32,
    /// `+10`
    pub repeat_interval: i32,
    /// `+14`
    pub adc_type: i8,
    /// `+15`: minimum stick radius for the direction bits.
    pub adc_th: i8,
    /// `+18`: overlap of neighboring direction sectors, in radians.
    pub adc_angle: f32,
    /// `+1C`
    pub clamp_stick_type: u8,
    /// `+1D`
    pub clamp_stick_shift: u8,
    /// `+1E`
    pub clamp_stick_max: i8,
    /// `+1F`
    pub clamp_stick_min: i8,
    /// `+20`
    pub clamp_analog_lr_shift: u8,
    /// `+21`
    pub clamp_analog_lr_max: u8,
    /// `+22`
    pub clamp_analog_lr_min: u8,
    /// `+23`
    pub clamp_analog_ab_shift: u8,
    /// `+24`
    pub clamp_analog_ab_max: u8,
    /// `+25`
    pub clamp_analog_ab_min: u8,
    /// `+26`
    pub scale_stick: i8,
    /// `+27`
    pub scale_analog_lr: u8,
    /// `+28`
    pub scale_analog_ab: u8,
    /// `+29`
    pub cross_dir: u8,
    /// `+2A`
    pub reset_switch_status: u8,
    /// `+2B`
    pub reset_switch: u8,
}

impl Default for PadLibData {
    /// `default_libinfo_data` (0x80406DAC), checked against the 1.02 DOL.
    fn default() -> Self {
        Self {
            qnum: 0,
            qread: 0,
            qwrite: 0,
            qcount: 0,
            qtype: 0,
            repeat_start: 0x2d,
            repeat_interval: 8,
            adc_type: 0,
            adc_th: 0x1e,
            adc_angle: 0.0,
            clamp_stick_type: 0,
            clamp_stick_shift: 0,
            clamp_stick_max: 0x7f,
            clamp_stick_min: 0,
            clamp_analog_lr_shift: 0,
            clamp_analog_lr_max: 0xff,
            clamp_analog_lr_min: 0,
            clamp_analog_ab_shift: 0,
            clamp_analog_ab_max: 0xff,
            clamp_analog_ab_min: 0,
            scale_stick: 0x7f,
            scale_analog_lr: 0xff,
            scale_analog_ab: 0xff,
            cross_dir: 0,
            reset_switch_status: 0,
            reset_switch: 0,
        }
    }
}

/// The HSD controller library's state: `HSD_PadLibData`, its sample queue,
/// and the three per-port status arrays.
#[derive(Debug, Clone, PartialEq)]
pub struct PadLib {
    pub lib: PadLibData,
    /// The `HSD_PadData[qnum]` queue passed to `HSD_PadInit`.
    pub queue: Vec<PadData>,
    /// `HSD_PadMasterStatus` (0x804C1FAC)
    pub master: [HsdPadStatus; 4],
    /// `HSD_PadCopyStatus` (0x804C20BC)
    pub copy: [HsdPadStatus; 4],
    /// `HSD_PadGameStatus` (0x804C21CC): what fighters read.
    pub game: [HsdPadStatus; 4],
}

/// `HSD_PadRawQueueShift` (static inline)
fn raw_queue_shift(qnum: u8, ptr: &mut u8) {
    *ptr = ((u32::from(*ptr) + 1) % u32::from(qnum)) as u8;
}

/// `HSD_PadRawMerge` (static inline): `dst.button = src1.button | src2.button`.
fn raw_merge(src1: &PadData, src2: &PadData, dst: &mut PadData) {
    for i in 0..4 {
        dst[i].button = src1[i].button | src2[i].button;
    }
}

impl PadLib {
    /// `HSD_PadInit` (0x80377D98) with a fresh `qnum`-entry queue. Rumble
    /// setup and `PADInit` are hardware and not ported.
    pub fn init(qnum: u8) -> Self {
        let lib = PadLibData {
            qnum,
            ..PadLibData::default()
        };
        Self {
            lib,
            queue: vec![[PadStatus::default(); 4]; usize::from(qnum)],
            master: [HsdPadStatus::default(); 4],
            copy: [HsdPadStatus::default(); 4],
            game: [HsdPadStatus::default(); 4],
        }
    }

    /// `gmMain_8015FD24` (0x8015FD24), the game's controller setup:
    /// `PADSetSpec(5)`, `HSD_PadInit(5, ...)`, then the clamp parameters
    /// (checked in the 1.02 machine code; identical in 20XX 5.0.2): stick
    /// circle of radius 80 with no inner deadzone, scaled by 80, and analog
    /// triggers clamped to 140 and scaled by 140.
    pub fn melee() -> Self {
        let mut pads = Self::init(5);
        let p = &mut pads.lib;
        p.clamp_stick_type = 0;
        p.clamp_stick_shift = 1;
        p.clamp_stick_max = 80;
        p.clamp_stick_min = 0;
        p.scale_stick = 80;
        p.clamp_analog_lr_shift = 1;
        p.clamp_analog_lr_max = 140;
        p.clamp_analog_lr_min = 0;
        p.scale_analog_lr = 140;
        pads
    }

    /// `HSD_PadGetRawQueueCount` (0x8037699C)
    pub fn raw_queue_count(&self) -> u8 {
        self.lib.qcount
    }

    /// `HSD_PadRenewRawStatus` (0x803769FC): queue one poll (`now`, the
    /// result of `PADRead`). In the game this runs from an alarm at the
    /// polling rate; the deterministic simulation calls it once per frame.
    pub fn renew_raw_status(&mut self, now: &PadData, err_check: bool) {
        let mut now = *now;
        if err_check {
            let mut i = 0;
            while i < 4 {
                if now[i].err == 0 {
                    break;
                }
                i += 1;
            }
            if i == 4 {
                return;
            }
        }

        let p = &mut self.lib;
        let qwrite = usize::from(p.qwrite);
        if p.qcount == p.qnum {
            match p.qtype {
                0 => {
                    raw_queue_shift(p.qnum, &mut p.qread);
                    let qread = usize::from(p.qread);
                    if p.qnum != 1 {
                        let (src1, dst) = (self.queue[qwrite], &mut self.queue[qread]);
                        let src2 = *dst;
                        raw_merge(&src1, &src2, dst);
                    } else {
                        let src2 = self.queue[qread];
                        let src1 = now;
                        raw_merge(&src1, &src2, &mut now);
                    }
                }
                1 => raw_queue_shift(p.qnum, &mut p.qread),
                // `goto skip`: drop this poll.
                2 => return,
                _ => {}
            }
        } else {
            p.qcount += 1;
        }

        self.queue[qwrite] = now;
        raw_queue_shift(self.lib.qnum, &mut self.lib.qwrite);
        // skip: PADReset of ports reporting PAD_ERR_NO_CONTROLLER and the
        // reset-button latch are hardware and not ported.
    }

    /// `HSD_PadFlushQueue` (0x80376D04)
    pub fn flush_queue(&mut self, ftype: FlushType) {
        let p = &mut self.lib;
        match ftype {
            FlushType::Merge => {
                while p.qcount > 1 {
                    let src = usize::from(p.qread);
                    raw_queue_shift(p.qnum, &mut p.qread);
                    let dst = usize::from(p.qread);
                    let (src1, src2) = (self.queue[src], self.queue[dst]);
                    raw_merge(&src1, &src2, &mut self.queue[dst]);
                    p.qcount -= 1;
                }
            }
            FlushType::Throwaway => {
                p.qread = p.qwrite;
                p.qcount = 0;
            }
            FlushType::Leave1 => {
                if p.qcount > 1 {
                    p.qread = if p.qwrite != 0 {
                        p.qwrite - 1
                    } else {
                        p.qnum - 1
                    };
                    p.qcount = 1;
                }
            }
            FlushType::Terminate => {}
        }
    }

    /// `HSD_PadClamp` (inlined into `HSD_PadRenewMasterStatus`).
    fn clamp(&self, mp: &mut HsdPadStatus) {
        let p = &self.lib;
        if p.clamp_stick_type == 0 {
            clamp_check3(
                &mut mp.stick_x,
                &mut mp.stick_y,
                p.clamp_stick_shift,
                p.clamp_stick_min,
                p.clamp_stick_max,
            );
            clamp_check3(
                &mut mp.sub_stick_x,
                &mut mp.sub_stick_y,
                p.clamp_stick_shift,
                p.clamp_stick_min,
                p.clamp_stick_max,
            );
        }
        clamp_check1(
            &mut mp.analog_l,
            p.clamp_analog_lr_shift,
            p.clamp_analog_lr_min,
            p.clamp_analog_lr_max,
        );
        clamp_check1(
            &mut mp.analog_r,
            p.clamp_analog_lr_shift,
            p.clamp_analog_lr_min,
            p.clamp_analog_lr_max,
        );
        clamp_check1(
            &mut mp.analog_a,
            p.clamp_analog_ab_shift,
            p.clamp_analog_ab_min,
            p.clamp_analog_ab_max,
        );
        clamp_check1(
            &mut mp.analog_b,
            p.clamp_analog_ab_shift,
            p.clamp_analog_ab_min,
            p.clamp_analog_ab_max,
        );
    }

    /// `HSD_PadADConvertCheck1` (0x803771D4): set a direction bit for each
    /// 90° sector (widened by `adc_angle`) containing the stick, when its
    /// radius is at least `adc_th`.
    #[allow(clippy::too_many_arguments)]
    fn ad_convert_check1(
        &self,
        mp: &mut HsdPadStatus,
        x: i8,
        y: i8,
        up: u32,
        down: u32,
        left: u32,
        right: u32,
    ) {
        let p = &self.lib;
        let (xf, yf) = (f32::from(x), f32::from(y));
        // `r = sq(x)` is a dead store; `vec2DSqDist` compiles to
        // `fmadds(x, x, y * y)`.
        let r = sqrtf(ppc::fmadds(xf, xf, yf * yf));
        let a = if f64::from(x).abs() == 0.0 {
            // `fabs` of the double conversion, compared with 0.0 (fcmpu).
            if y >= 0 {
                ppc::frsp(FRAC_PI_2_F64)
            } else {
                ppc::frsp(-FRAC_PI_2_F64)
            }
        } else {
            atan2f(yf, xf)
        };
        let ha = 0.5f32 * p.adc_angle;
        if r < f32::from(p.adc_th) {
            return;
        }
        // The bounds are double constants plus or minus `ha` (fadd/fsub).
        let (a, ha) = (f64::from(a), f64::from(ha));
        if a < NEG_3PI_4 + ha {
            mp.button |= left;
        }
        if a >= NEG_3PI_4 - ha && a <= NEG_PI_4 + ha {
            mp.button |= down;
        }
        if a > NEG_PI_4 - ha && a < PI_4 + ha {
            mp.button |= right;
        }
        if a >= PI_4 - ha && a <= PI_3_4 + ha {
            mp.button |= up;
        }
        if a > PI_3_4 - ha {
            mp.button |= left;
        }
    }

    /// `HSD_PadADConvert` (inlined into `HSD_PadRenewMasterStatus`).
    fn ad_convert(&self, mp: &mut HsdPadStatus) {
        if self.lib.adc_type == 0 {
            let (x, y) = (mp.stick_x, mp.stick_y);
            self.ad_convert_check1(
                mp,
                x,
                y,
                PAD_STICK_UP,
                PAD_STICK_DOWN,
                PAD_STICK_LEFT,
                PAD_STICK_RIGHT,
            );
            let (x, y) = (mp.sub_stick_x, mp.sub_stick_y);
            self.ad_convert_check1(
                mp,
                x,
                y,
                PAD_SUBSTICK_UP,
                PAD_SUBSTICK_DOWN,
                PAD_SUBSTICK_LEFT,
                PAD_SUBSTICK_RIGHT,
            );
        }
    }

    /// `HSD_PadScale` (inlined into `HSD_PadRenewMasterStatus`): each value
    /// over its scale, as a single-precision division (`fdivs`).
    fn scale(&self, mp: &mut HsdPadStatus) {
        let p = &self.lib;
        let stick = f32::from(p.scale_stick);
        let lr = f32::from(p.scale_analog_lr);
        let ab = f32::from(p.scale_analog_ab);
        mp.nml_stick_x = f32::from(mp.stick_x) / stick;
        mp.nml_stick_y = f32::from(mp.stick_y) / stick;
        mp.nml_sub_stick_x = f32::from(mp.sub_stick_x) / stick;
        mp.nml_sub_stick_y = f32::from(mp.sub_stick_y) / stick;
        mp.nml_analog_l = f32::from(mp.analog_l) / lr;
        mp.nml_analog_r = f32::from(mp.analog_r) / lr;
        mp.nml_analog_a = f32::from(mp.analog_a) / ab;
        mp.nml_analog_b = f32::from(mp.analog_b) / ab;
    }

    /// `HSD_PadCrossDir` (0x80377450): optionally drop one D-pad axis.
    /// Melee leaves `cross_dir` at 0 (no change).
    fn cross_dir(&self, mp: &mut HsdPadStatus) {
        const VERTICAL: u32 = HSD_PAD_DPADDOWN | HSD_PAD_DPADUP;
        const HORIZONTAL: u32 = HSD_PAD_DPADLEFT | HSD_PAD_DPADRIGHT;
        match self.lib.cross_dir {
            1 => {
                if mp.button & VERTICAL == 0 {
                    return;
                }
                mp.button &= !HORIZONTAL;
            }
            2 => {
                if mp.button & HORIZONTAL == 0 {
                    return;
                }
                mp.button &= !VERTICAL;
            }
            3 => {
                if mp.button & VERTICAL != 0 {
                    if mp.button & HORIZONTAL != 0 {
                        if mp.cross_dir == 1 {
                            mp.button &= !HORIZONTAL;
                            return;
                        }
                        mp.button &= !VERTICAL;
                    } else {
                        mp.cross_dir = 1;
                    }
                    return;
                }
                if mp.button & HORIZONTAL != 0 {
                    mp.cross_dir = 2;
                }
            }
            _ => {}
        }
    }

    /// `HSD_PadRenewMasterStatus` (0x8037750C): take the oldest queued poll
    /// and convert each port into `master`. Does nothing if the queue is
    /// empty.
    pub fn renew_master_status(&mut self) {
        if self.lib.qcount == 0 {
            return;
        }
        let qread = usize::from(self.lib.qread);
        raw_queue_shift(self.lib.qnum, &mut self.lib.qread);
        self.lib.qcount -= 1;
        let polled = self.queue[qread];

        for (i, q) in polled.iter().enumerate() {
            let mut mp = self.master[i];
            mp.last_button = mp.button;
            mp.err = q.err;
            if mp.err == 0 {
                mp.button = u32::from(q.button);
                mp.stick_x = q.stick_x;
                mp.stick_y = q.stick_y;
                mp.sub_stick_x = q.substick_x;
                mp.sub_stick_y = q.substick_y;
                mp.analog_l = q.trigger_left;
                mp.analog_r = q.trigger_right;
                mp.analog_a = q.analog_a;
                mp.analog_b = q.analog_b;
                self.clamp(&mut mp);
                self.ad_convert(&mut mp);
                self.scale(&mut mp);
                self.cross_dir(&mut mp);
            } else if mp.err == PAD_ERR_TRANSFER {
                // Keep last frame's values.
                mp.err = 0;
            } else {
                mp.clear_fields();
            }
            mp.renew_edges(self.lib.repeat_start, self.lib.repeat_interval);
            self.master[i] = mp;
        }
    }

    /// `HSD_PadRenewCopyStatus` (0x803779C0)
    pub fn renew_copy_status(&mut self) {
        for i in 0..4 {
            renew_from_master(&mut self.copy[i], &self.master[i], &self.lib);
        }
    }

    /// `HSD_PadRenewGameStatus` (0x80377B54)
    pub fn renew_game_status(&mut self) {
        for i in 0..4 {
            renew_from_master(&mut self.game[i], &self.master[i], &self.lib);
        }
    }

    /// `HSD_PadRenewStatus` (0x80377CE8), with `now` as the `PADRead` result.
    pub fn renew_status(&mut self, now: &PadData) {
        self.renew_raw_status(now, false);
        self.renew_master_status();
        self.renew_copy_status();
        self.renew_game_status();
    }

    /// One frame of the scene loop (`gm_801A4D34`) at normal game speed with
    /// one poll per frame: the poll (`fn_800195FC` →
    /// `HSD_PadRenewRawStatus(0)`), `lb_800198E0` (`HSD_PadRenewMasterStatus`),
    /// then `lb_80019900`, which renews game and copy status when the 60 Hz
    /// tick `lb_80019A30(0)` is set (every frame at speed 1.0).
    pub fn renew_for_frame(&mut self, now: &PadData) {
        self.renew_raw_status(now, false);
        self.renew_master_status();
        self.renew_game_status();
        self.renew_copy_status();
    }
}

/// Body of `HSD_PadRenewCopyStatus`/`HSD_PadRenewGameStatus` for one port.
fn renew_from_master(dst: &mut HsdPadStatus, mp: &HsdPadStatus, p: &PadLibData) {
    dst.last_button = dst.button;
    dst.err = mp.err;
    if dst.err == 0 {
        dst.copy_fields(mp);
    } else {
        dst.clear_fields();
    }
    dst.renew_edges(p.repeat_start, p.repeat_interval);
}

/// `HSD_PadClampCheck1` (0x80376E48): clamp an analog trigger byte to `max`
/// (zero below `min`), then subtract `min` when `shift == 1`.
fn clamp_check1(val: &mut u8, shift: u8, min: u8, max: u8) {
    if *val < min {
        *val = 0;
        return;
    }
    if *val > max {
        *val = max;
    }
    if shift != 1 {
        return;
    }
    *val = val.wrapping_sub(min);
}

/// `fctiwz` then `stb`: truncate toward zero, keep the low byte.
fn to_s8(value: f32) -> i8 {
    value as i32 as i8
}

/// `HSD_PadClampCheck3` (0x80376E90): clamp a stick to the circle of radius
/// `max` (zero inside `min`), then pull it in by `min` when `shift == 1`.
/// All single precision except the inlined `sqrtf`.
fn clamp_check3(x: &mut i8, y: &mut i8, shift: u8, min: i8, max: i8) {
    let sq_len = |x: i8, y: i8| {
        let (xf, yf) = (f32::from(x), f32::from(y));
        xf * xf + yf * yf
    };
    let mut r = sqrtf(sq_len(*x, *y));

    if r < f32::from(min) {
        *y = 0;
        *x = 0;
        return;
    }
    if r > f32::from(max) {
        *x = to_s8((f32::from(*x) * f32::from(max)) / r);
        *y = to_s8((f32::from(*y) * f32::from(max)) / r);
        r = sqrtf(sq_len(*x, *y));
    }

    if shift == 1 && r > f32::from_bits(0x2edb_e6ff) {
        // 1.000000013351432e-10f
        *x = to_s8(f32::from(*x) - (f32::from(*x) * f32::from(min)) / r);
        *y = to_s8(f32::from(*y) - (f32::from(*y) * f32::from(min)) / r);
    }
}

/// `(float)` of the double `M_PI_2` constant at 0x804DE5E0.
const FRAC_PI_2_F64: f64 = f64::from_bits(0x3ff9_21fb_5444_2d18);
/// Double sector bounds at 0x804DE5F8..0x804DE610: -3π/4, -π/4, π/4, 3π/4.
const NEG_3PI_4: f64 = f64::from_bits(0xc002_d97c_7f33_21d2);
const NEG_PI_4: f64 = f64::from_bits(0xbfe9_21fb_5444_2d18);
const PI_4: f64 = f64::from_bits(0x3fe9_21fb_5444_2d18);
const PI_3_4: f64 = f64::from_bits(0x4002_d97c_7f33_21d2);

/// MSL's inline `sqrtf` (`src/MSL/math_ppc.h`) as MWCC compiled it into
/// `controller.c` and `lb_8000D148`: an `frsqrte` estimate, three Newton
/// steps in double where `3.0 - x * (g * g)` is one fused `fnmsub`, then
/// `x * g` rounded to single.
///
/// The estimate comes from [`ppc::frsqrte`], which is provisional (not the
/// hardware table). `tests/input.rs` checks that every value the stick
/// clamp and direction code can pass in rounds to the correctly rounded
/// square root even with the estimate perturbed by the documented error
/// bound, so these callers do not depend on the table.
pub(crate) fn sqrtf(x: f32) -> f32 {
    sqrtf_with(x, ppc::frsqrte)
}

/// [`sqrtf`] with a given reciprocal square root estimate.
pub(crate) fn sqrtf_with(x: f32, estimate: impl Fn(f64) -> f64) -> f32 {
    if x > 0.0 {
        let x = f64::from(x);
        let mut guess = estimate(x);
        for _ in 0..3 {
            let square = guess * guess;
            let half = 0.5 * guess;
            guess = half * ppc::fnmsub(x, square, 3.0);
        }
        ppc::frsp(x * guess)
    } else {
        x
    }
}

/// `atanf_lookup` (0x803B7300), 46 floats as stored in the 1.02 DOL.
const ATANF_LOOKUP: [u32; 46] = [
    0x3f80_0000,
    0xbeaa_aaaa,
    0x3e4c_cc81,
    0xbe12_3e7d,
    0x3de2_1f95,
    0xbdad_417c,
    0x3d41_186d,
    0x40da_826b,
    0x404f_5958,
    0x4000_0000,
    0x3fb9_25ab,
    0x3f95_f61a,
    0x3f85_1081,
    0x36ef_692f,
    0x355c_1df9,
    0x0000_0000,
    0x3529_1d45,
    0x0000_0000,
    0x0000_0000,
    0x0000_0000,
    0x3ec9_0eaa,
    0x3f16_cbe4,
    0x3f49_0fda,
    0x3f7b_53c5,
    0x3f96_cbe2,
    0x3faf_edd9,
    0x0000_0000,
    0x3718_5d99,
    0x32c5_9189,
    0x3387_4a9e,
    0x353c_fa83,
    0x3486_37bd,
    0x3554_1063,
    0x401a_8277,
    0x3fbf_90c7,
    0x3f80_0000,
    0x3f2b_0dc1,
    0x3ed4_13cd,
    0x3e4b_afaf,
    0x3516_dc59,
    0x0000_0000,
    0x0000_0000,
    0x0000_0000,
    0x0000_0000,
    0x0000_0000,
    0x0000_0000,
];

/// The atanf table and constants, for the disc test.
#[doc(hidden)]
pub const ATANF_LOOKUP_BITS: &[u32; 46] = &ATANF_LOOKUP;

fn atanf_lookup(index: i32) -> f32 {
    f32::from_bits(ATANF_LOOKUP[index as usize])
}

const PI_F32: f32 = f32::from_bits(0x4049_0fdb);
const FRAC_PI_2_F32: f32 = f32::from_bits(0x3fc9_0fdb);

/// `atan2f` (0x80022C30, `src/melee/lb/lbtrigf.c`), used by
/// `HSD_PadADConvertCheck1`. Single precision throughout.
///
/// Kept private to the controller port until a shared `lb::trigf` exists.
pub(crate) fn atan2f(y: f32, x: f32) -> f32 {
    const SIGN: u32 = 0x8000_0000;
    let sign_y = y.to_bits() & SIGN;
    let sign_x = x.to_bits() & SIGN;
    if sign_x == sign_y {
        if sign_x != 0 {
            return if x == -0.0 {
                -FRAC_PI_2_F32
            } else {
                atanf(y / x) - PI_F32
            };
        }
        return if x != 0.0 {
            atanf(y / x)
        } else {
            FRAC_PI_2_F32
        };
    }
    if x < 0.0 {
        return PI_F32 + atanf(y / x);
    }
    if x != 0.0 {
        return atanf(y / x);
    }
    f32::from_bits(sign_y.wrapping_add(FRAC_PI_2_F32.to_bits()))
}

/// `atanf` (0x80022E68, `src/melee/lb/lbtrigf.c`): range reduction through
/// `atanf_lookup`, then an odd polynomial. Fused where the 1.02 code fuses
/// (two `fnmsubs` in the reduction, the `fmadds` Horner chain).
pub(crate) fn atanf(x: f32) -> f32 {
    const SIGN: u32 = 0x8000_0000;
    let silver_ratio = f32::from_bits(0x401a_827a);
    let silver_ratio_conjugate = f32::from_bits(0x3ed4_13cd);

    let mut lookup_index: i32 = -1;
    let mut x_ge_ratio = false;
    let sign_bit_x = x.to_bits() & SIGN;
    let x = f32::from_bits(x.to_bits() & !SIGN);

    let mut result;
    if x >= silver_ratio {
        x_ge_ratio = true;
        result = 1.0 / x;
    } else if silver_ratio_conjugate < x {
        lookup_index = 0;
        let bits = x.to_bits() as i32;
        match x.to_bits() & 0x7f80_0000 {
            0x3f00_0000 => {
                if !(bits < 0x3f08_d5b9) {
                    lookup_index = 1;
                }
                if !(bits < 0x3f52_1801) {
                    lookup_index += 1;
                }
            }
            0x3f80_0000 => {
                lookup_index = 2;
                if !(bits < 0x3f9b_f7ec) {
                    lookup_index = 3;
                }
                if !(bits < 0x3fef_789e) {
                    lookup_index += 1;
                }
            }
            0x4000_0000 => lookup_index = 4,
            _ => {}
        }
        let offset_39 = atanf_lookup(lookup_index + 39);
        let offset_33 = atanf_lookup(lookup_index + 33);
        result = 1.0 / (offset_33 + (x + offset_39));
        result = ppc::fnmsubs(result, atanf_lookup(lookup_index + 7), offset_33)
            + ppc::fnmsubs(result, atanf_lookup(lookup_index + 13), offset_39);
    } else {
        result = x;
    }

    let result_squared = result * result;
    let poly = ppc::fmadds(
        result_squared,
        ppc::fmadds(
            result_squared,
            ppc::fmadds(
                result_squared,
                ppc::fmadds(
                    result_squared,
                    ppc::fmadds(result_squared, atanf_lookup(6), atanf_lookup(5)),
                    atanf_lookup(4),
                ),
                atanf_lookup(3),
            ),
            atanf_lookup(2),
        ),
        atanf_lookup(1),
    );
    result = ppc::fmadds(result * result_squared, poly, result);
    result += atanf_lookup(lookup_index + 27);
    result += atanf_lookup(lookup_index + 20);

    if x_ge_ratio {
        result -= FRAC_PI_2_F32;
        return if sign_bit_x != 0 { result } else { -result };
    }
    f32::from_bits(result.to_bits() | sign_bit_x)
}

/// The double constants `HSD_PadADConvertCheck1` compares against, for the
/// disc test: `M_PI_2`, -3π/4, -π/4, π/4, 3π/4.
#[doc(hidden)]
pub const AD_CONVERT_CONSTANTS: [f64; 5] = [FRAC_PI_2_F64, NEG_3PI_4, NEG_PI_4, PI_4, PI_3_4];

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(stick_x: i8, stick_y: i8) -> PadData {
        let mut poll = [PadStatus::DISCONNECTED; 4];
        poll[0] = PadInput {
            stick_x,
            stick_y,
            ..PadInput::default()
        }
        .into();
        poll
    }

    #[test]
    fn constants_match_their_decimal_sources() {
        assert_eq!(NEG_3PI_4, -2.356194490192345);
        assert_eq!(NEG_PI_4, -std::f64::consts::FRAC_PI_4);
        assert_eq!(PI_4, std::f64::consts::FRAC_PI_4);
        assert_eq!(PI_3_4, 2.356194490192345);
        assert_eq!(FRAC_PI_2_F64, std::f64::consts::FRAC_PI_2);
        // The decomp writes these as double literals converted to float.
        let decomp = |literal: f64| literal as f32;
        assert_eq!(atanf_lookup(22), decomp(0.7853981256484985));
        assert_eq!(atanf_lookup(29), decomp(6.30000016599297e-08));
        assert_eq!(atanf_lookup(33), decomp(2.414212942123413));
        assert_eq!(f32::from_bits(0x2edb_e6ff), decomp(1.000000013351432e-10));
    }

    #[test]
    fn atanf_of_one_is_pi_over_four_rounded_up() {
        // 0x3F490FDB is f32(π/4); the table's 0x3F490FDA plus 6.3e-8 rounds up.
        assert_eq!(atanf(1.0).to_bits(), 0x3f49_0fdb);
        assert_eq!(atanf(-1.0).to_bits(), 0xbf49_0fdb);
        assert_eq!(atanf(0.0), 0.0);
        assert!(atanf(-0.0).is_sign_negative());
    }

    #[test]
    fn atanf_is_close_in_every_lookup_branch() {
        // Sweep [2^-10, 2^10] (all five table rows, the reciprocal branch,
        // and the small-argument branch) and both signs. HAL's reduction
        // `T[33] - T[7] / (x + T[33])` cancels, so errors reach ~7.4 × 2^-23
        // relative (x = 0.432); a wrong row or constant would be ~1e-2.
        let mut worst = (0f64, 0f32);
        let mut bits = 0x3a80_0000u32;
        while bits < 0x4480_0000 {
            for x in [f32::from_bits(bits), -f32::from_bits(bits)] {
                let got = f64::from(atanf(x));
                let want = f64::from(x).atan();
                let ulp = f64::from(f32::EPSILON) * want.abs().max(f64::MIN_POSITIVE);
                let err = (got - want).abs() / ulp;
                if err > worst.0 {
                    worst = (err, x);
                }
            }
            bits += 997;
        }
        assert!(worst.0 < 8.0, "worst error {worst:?} × 2^-23");
    }

    #[test]
    fn atan2f_is_close_to_the_true_angle() {
        for y in -80i8..=80 {
            for x in -80i8..=80 {
                if x == 0 && y == 0 {
                    continue;
                }
                let got = f64::from(atan2f(f32::from(y), f32::from(x)));
                let want = f64::from(y).atan2(f64::from(x));
                assert!(
                    (got - want).abs() < 1e-6,
                    "atan2f({y}, {x}) = {got}, want {want}"
                );
            }
        }
        assert_eq!(atan2f(1.0, 0.0), FRAC_PI_2_F32);
        assert_eq!(atan2f(-1.0, 0.0), -FRAC_PI_2_F32);
        assert_eq!(atan2f(0.0, -1.0), PI_F32);
    }

    /// Estimates at the edges of `frsqrte`'s documented error (1/4096).
    pub(crate) fn assert_sqrtf_is_table_independent(x: f32) {
        let exact = |v: f64| 1.0 / v.sqrt();
        let low = |v: f64| exact(v) * (1.0 - 1.0 / 4096.0);
        let high = |v: f64| exact(v) * (1.0 + 1.0 / 4096.0);
        let want = x.sqrt();
        assert_eq!(sqrtf(x), want, "sqrtf({x})");
        assert_eq!(sqrtf_with(x, exact), want, "sqrtf({x}), exact estimate");
        assert_eq!(sqrtf_with(x, low), want, "sqrtf({x}), low estimate");
        assert_eq!(sqrtf_with(x, high), want, "sqrtf({x}), high estimate");
    }

    #[test]
    fn sqrtf_of_stick_lengths_does_not_depend_on_the_estimate() {
        // Every x² + y² of two s8 values (the clamp and direction code's
        // inputs) is an integer in 0..=32768.
        for n in 0..=32768u16 {
            assert_sqrtf_is_table_independent(f32::from(n));
        }
    }

    #[test]
    fn trigger_clamp_and_shift() {
        let mut v = 200;
        clamp_check1(&mut v, 1, 0, 140);
        assert_eq!(v, 140);
        let mut v = 30;
        clamp_check1(&mut v, 1, 10, 140);
        assert_eq!(v, 20);
        let mut v = 5;
        clamp_check1(&mut v, 1, 10, 140);
        assert_eq!(v, 0);
        let mut v = 200;
        clamp_check1(&mut v, 0, 10, 140);
        assert_eq!(v, 140);
    }

    #[test]
    fn stick_clamps_to_the_radius_80_circle() {
        let clamp = |x: i8, y: i8| {
            let (mut x, mut y) = (x, y);
            clamp_check3(&mut x, &mut y, 1, 0, 80);
            (x, y)
        };
        assert_eq!(clamp(80, 0), (80, 0));
        assert_eq!(clamp(81, 0), (80, 0));
        assert_eq!(clamp(-128, 0), (-80, 0));
        assert_eq!(clamp(0, 127), (0, 80));
        // A 3-4-5 triangle scales exactly: 60 * 80 / 100 = 48.
        assert_eq!(clamp(60, 80), (48, 64));
        // sqrt(127² * 2) = 179.6; 127 * 80 / 179.6 = 56.57, truncated.
        assert_eq!(clamp(127, 127), (56, 56));
        assert_eq!(clamp(-127, 127), (-56, 56));
        assert_eq!(clamp(57, 57), (56, 56));
        assert_eq!(clamp(56, 56), (56, 56));
        assert_eq!(clamp(0, 0), (0, 0));
        // An inner `min` zeroes small values and pulls the rest in.
        let (mut x, mut y) = (5i8, 0i8);
        clamp_check3(&mut x, &mut y, 1, 10, 80);
        assert_eq!((x, y), (0, 0));
        let (mut x, mut y) = (50i8, 0i8);
        clamp_check3(&mut x, &mut y, 1, 10, 80);
        assert_eq!((x, y), (40, 0));
    }

    #[test]
    fn master_status_scales_and_detects_edges() {
        let mut pads = PadLib::melee();
        let mut poll = sample(0, -80);
        poll[0].button = HSD_PAD_A as u16;
        poll[0].trigger_left = 70;
        pads.renew_for_frame(&poll);
        let gs = pads.game[0];
        assert_eq!(gs.nml_stick_x, 0.0);
        assert_eq!(gs.nml_stick_y, -1.0);
        assert_eq!(gs.nml_analog_l, 0.5);
        assert_eq!(gs.trigger & HSD_PAD_A, HSD_PAD_A);
        assert_eq!(gs.button & PAD_STICK_DOWN, PAD_STICK_DOWN);
        assert_eq!(gs.repeat_count, 45);
        // Unplugged ports are cleared.
        assert_eq!(pads.game[1].err, PAD_ERR_NO_CONTROLLER);
        assert_eq!(pads.game[1].button, 0);

        poll[0].button = 0;
        pads.renew_for_frame(&poll);
        assert_eq!(pads.game[0].release & HSD_PAD_A, HSD_PAD_A);
        assert_eq!(pads.game[0].trigger, 0);
    }

    #[test]
    fn held_buttons_repeat_after_45_then_every_8_frames() {
        let mut pads = PadLib::melee();
        let mut poll = sample(0, 0);
        poll[0].button = HSD_PAD_B as u16;
        let mut repeats = Vec::new();
        for frame in 0..70 {
            pads.renew_for_frame(&poll);
            if pads.master[0].repeat != 0 {
                repeats.push(frame);
            }
        }
        assert_eq!(repeats, [0, 45, 53, 61, 69]);
    }

    #[test]
    fn transfer_error_keeps_the_previous_frame() {
        let mut pads = PadLib::melee();
        pads.renew_for_frame(&sample(80, 0));
        let mut poll = sample(0, 0);
        poll[0].err = PAD_ERR_TRANSFER;
        pads.renew_for_frame(&poll);
        assert_eq!(pads.game[0].err, 0);
        assert_eq!(pads.game[0].nml_stick_x, 1.0);
    }

    #[test]
    fn full_queue_merges_buttons_into_the_next_poll() {
        let mut pads = PadLib::melee();
        let mut polls = Vec::new();
        for i in 0..6u16 {
            let mut poll = sample(0, 0);
            poll[0].button = 1 << (8 + i);
            polls.push(poll);
            pads.renew_raw_status(&poll, false);
        }
        assert_eq!(pads.raw_queue_count(), 5);
        // The sixth poll replaced the oldest; its button was merged into the
        // new oldest entry.
        pads.renew_master_status();
        assert_eq!(
            pads.master[0].button,
            u32::from(polls[0][0].button | polls[1][0].button)
        );
        pads.flush_queue(FlushType::Leave1);
        assert_eq!(pads.raw_queue_count(), 1);
        pads.renew_master_status();
        assert_eq!(pads.master[0].button, u32::from(polls[5][0].button));
    }

    #[test]
    fn wire_format_round_trips() {
        let input = PadInput {
            button: 0x1f7f,
            stick_x: -80,
            stick_y: 127,
            substick_x: -128,
            substick_y: 3,
            trigger_left: 140,
            trigger_right: 255,
        };
        assert_eq!(PadInput::from_bytes(input.to_bytes()), input);
        assert_eq!(input.to_bytes()[..2], [0x1f, 0x7f]);
    }
}
