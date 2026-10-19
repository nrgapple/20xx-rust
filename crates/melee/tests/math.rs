//! Bit-exact checks of the math layer (`msl`, `lb::trigf`, `lb::vector`,
//! `lb::lb_00ce`, `sdk`, `hsd::mtx`).
//!
//! 1. Differential tests: the original 1.02 machine code runs in
//!    `ppc_asm::interp` and must agree with the port, bit for bit, on many
//!    inputs (random bit patterns, gameplay-sized values, special values).
//!    The interpreter shares `melee::ppc`'s floating-point model and its
//!    provisional `frsqrte`/`fres`, so this checks the translation
//!    (operations, order, fusing, constants, control flow), not the model.
//!    Needs the DOL: `MELEE_DOL`, `originals/melee-1.02.dol`, or the DOL of
//!    `MELEE_ISO` / `originals/melee-ntsc-1.02.iso` /
//!    `originals/20xx-hp-5.0.2.iso` (20XX leaves these functions unchanged).
//!    Without one these tests print a note and pass.
//! 2. Constant tables in the port equal the DOL's bytes.
//! 3. Determinism: hashes of outputs over fixed inputs equal pinned values,
//!    on every target (native and wasm32).

use melee::lb::{lb_00ce, trigf, vector};
use melee::math::{Mtx, Quaternion, Vec3};
use melee::msl;
use melee::sdk::{mtx as sdk_mtx, vec as sdk_vec};
use ppc_asm::interp::{Cpu, Memory};
use std::path::PathBuf;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Inputs

/// xorshift64* — deterministic inputs on every platform.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    /// A mix of value classes: arbitrary bit patterns (including NaN,
    /// infinities, denormals), special values, and gameplay-sized ranges.
    fn mixed(&mut self) -> f32 {
        const SPECIAL: [f32; 16] = [
            0.0,
            -0.0,
            1.0,
            -1.0,
            0.5,
            std::f32::consts::FRAC_PI_2,
            -std::f32::consts::FRAC_PI_2,
            std::f32::consts::PI,
            -std::f32::consts::PI,
            std::f32::consts::TAU,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            f32::MIN_POSITIVE,
            1e-10,
            2.414_213_7,
        ];
        match self.next_u32() % 10 {
            0 => f32::from_bits(self.next_u32()),
            1 => SPECIAL[(self.next_u32() % 16) as usize],
            2 => self.range(-1.0, 1.0),
            3 | 4 => self.range(-10.0, 10.0),
            5 | 6 => self.range(-300.0, 300.0),
            7 => self.range(-1e5, 1e5),
            8 => self.range(-1e-3, 1e-3),
            _ => (self.next_u32() % 64) as f32 - 32.0,
        }
    }

    /// Finite values in gameplay-sized ranges only.
    fn finite(&mut self) -> f32 {
        match self.next_u32() % 4 {
            0 => self.range(-1.0, 1.0),
            1 => self.range(-10.0, 10.0),
            2 => self.range(-300.0, 300.0),
            _ => (self.next_u32() % 16) as f32 - 8.0,
        }
    }

    fn vec3(&mut self, finite: bool) -> Vec3 {
        let mut f = || if finite { self.finite() } else { self.mixed() };
        Vec3::new(f(), f(), f())
    }

    fn mtx(&mut self, finite: bool) -> Mtx {
        let mut m = [[0.0; 4]; 3];
        for row in &mut m {
            for v in row {
                *v = if finite { self.finite() } else { self.mixed() };
            }
        }
        m
    }
}

fn same(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

fn same_vec(a: &Vec3, b: &Vec3) -> bool {
    same(a.x, b.x) && same(a.y, b.y) && same(a.z, b.z)
}

fn same_mtx(a: &Mtx, b: &Mtx) -> bool {
    a.iter()
        .flatten()
        .zip(b.iter().flatten())
        .all(|(&x, &y)| same(x, y))
}

// ---------------------------------------------------------------------------
// The original code, interpreted

fn dol_bytes() -> Option<Vec<u8>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../originals");
    let dol = std::env::var_os("MELEE_DOL")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("melee-1.02.dol"));
    if let Ok(bytes) = std::fs::read(&dol) {
        return Some(bytes);
    }
    let isos = std::env::var_os("MELEE_ISO")
        .map(PathBuf::from)
        .into_iter()
        .chain([
            root.join("melee-ntsc-1.02.iso"),
            root.join("20xx-hp-5.0.2.iso"),
        ]);
    for iso in isos {
        let Ok(mut file) = std::fs::File::open(&iso) else {
            continue;
        };
        let disc = gc_disc::Disc::read(&mut file).ok()?;
        use std::io::{Read, Seek, SeekFrom};
        file.seek(SeekFrom::Start(disc.dol_offset)).ok()?;
        let mut bytes = vec![0; disc.dol_size as usize];
        file.read_exact(&mut bytes).ok()?;
        return Some(bytes);
    }
    None
}

const STACK: u32 = 0x8170_0000;
const SCRATCH: u32 = 0x8170_0100;
const R2: u32 = 0x804D_F9E0;
const R13: u32 = 0x804D_B6A0;
const STEPS: u64 = 200_000;

fn fres_model(x: f64) -> f64 {
    f64::from(melee::ppc::fres(x))
}

/// A CPU with the DOL loaded and `__sinit_trigf_c` run, or `None` (with a
/// note) if no DOL is available.
fn base_cpu() -> Option<&'static Cpu> {
    static CPU: OnceLock<Option<Cpu>> = OnceLock::new();
    CPU.get_or_init(|| {
        let Some(bytes) = dol_bytes() else {
            eprintln!("skipping differential math tests: no 1.02 DOL or disc in originals/");
            return None;
        };
        let dol = gc_disc::Dol::parse(bytes).expect("valid DOL");
        let sections = dol.sections.iter().map(|s| {
            let start = s.file_offset as usize;
            (s.address, &dol.bytes[start..start + s.size as usize])
        });
        let mut cpu = Cpu::new(Memory::with_sections(sections));
        cpu.frsqrte = melee::ppc::frsqrte;
        cpu.fres = fres_model;
        let mut oracle = Oracle { cpu };
        oracle.call(0x8032_6578); // __sinit_trigf_c fills __four_over_pi_m1
        Some(oracle.cpu)
    })
    .as_ref()
}

struct Oracle {
    cpu: Cpu,
}

impl Oracle {
    fn new() -> Option<Self> {
        Some(Self {
            cpu: base_cpu()?.clone(),
        })
    }

    fn reset(&mut self) {
        let cpu = &mut self.cpu;
        cpu.gpr = [0; 32];
        cpu.gpr[1] = STACK;
        cpu.gpr[2] = R2;
        cpu.gpr[13] = R13;
        for f in &mut cpu.fpr {
            // Poison: a function reading a register it never set yields NaN.
            f.ps0 = f64::from_bits(0x7ff4_dead_0000_0001);
            f.ps1 = f.ps0;
        }
        cpu.cr = 0;
        cpu.ctr = 0;
    }

    /// Call `address`; `false` if it hit the step limit (e.g. `expf`'s
    /// endless loop on NaN).
    fn call(&mut self, address: u32) -> bool {
        match self.cpu.call(address, STEPS) {
            Ok(()) => true,
            Err(ppc_asm::interp::Stop::StepLimit) => false,
            Err(stop) => panic!("{address:#010x}: {stop}"),
        }
    }

    fn f1(&self) -> f32 {
        let v = self.cpu.fpr[1].ps0;
        assert!(f64::from(v as f32).to_bits() == v.to_bits() || v.is_nan());
        v as f32
    }

    fn call_f(&mut self, address: u32, args: &[f32]) -> Option<f32> {
        self.reset();
        for (i, &a) in args.iter().enumerate() {
            self.cpu.set_f(1 + i, f64::from(a));
        }
        self.call(address).then(|| self.f1())
    }

    fn put_vec(&mut self, slot: u32, v: &Vec3) -> u32 {
        let address = SCRATCH + slot * 0x40;
        for (i, x) in [v.x, v.y, v.z].into_iter().enumerate() {
            self.cpu.mem.write_f32(address + 4 * i as u32, x).unwrap();
        }
        address
    }

    fn get_vec(&self, address: u32) -> Vec3 {
        let f = |i: u32| self.cpu.mem.read_f32(address + 4 * i).unwrap();
        Vec3::new(f(0), f(1), f(2))
    }

    fn put_mtx(&mut self, slot: u32, m: &Mtx) -> u32 {
        let address = SCRATCH + slot * 0x40;
        for (i, &x) in m.iter().flatten().enumerate() {
            self.cpu.mem.write_f32(address + 4 * i as u32, x).unwrap();
        }
        address
    }

    fn get_mtx(&self, address: u32) -> Mtx {
        let mut m = [[0.0; 4]; 3];
        for (i, v) in m.iter_mut().flatten().enumerate() {
            *v = self.cpu.mem.read_f32(address + 4 * i as u32).unwrap();
        }
        m
    }

    fn put_quat(&mut self, slot: u32, q: &Quaternion) -> u32 {
        let address = SCRATCH + slot * 0x40;
        for (i, x) in [q.x, q.y, q.z, q.w].into_iter().enumerate() {
            self.cpu.mem.write_f32(address + 4 * i as u32, x).unwrap();
        }
        address
    }
}

/// Run `check` on `count` inputs, failing with the first few mismatches.
fn differential(
    name: &str,
    count: usize,
    mut check: impl FnMut(&mut Oracle, &mut Rng) -> Option<String>,
) {
    let Some(mut oracle) = Oracle::new() else {
        return;
    };
    let mut rng = Rng::new(
        name.bytes()
            .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b))),
    );
    let mut failures = Vec::new();
    for _ in 0..count {
        if let Some(message) = check(&mut oracle, &mut rng) {
            failures.push(message);
            if failures.len() >= 5 {
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{name}: port differs from DOL:\n{}",
        failures.join("\n")
    );
}

fn scalar1(name: &str, address: u32, count: usize, gen: fn(&mut Rng) -> f32, port: fn(f32) -> f32) {
    differential(name, count, |o, rng| {
        let x = gen(rng);
        let expected = o.call_f(address, &[x])?;
        let actual = port(x);
        (!same(expected, actual)).then(|| {
            format!("  {name}({x:e} = {:#010x}): DOL {expected:e} ({:#010x}), port {actual:e} ({:#010x})",
                x.to_bits(), expected.to_bits(), actual.to_bits())
        })
    });
}

fn scalar2(
    name: &str,
    address: u32,
    count: usize,
    gen: fn(&mut Rng) -> f32,
    port: fn(f32, f32) -> f32,
) {
    differential(name, count, |o, rng| {
        let (a, b) = (gen(rng), gen(rng));
        let expected = o.call_f(address, &[a, b])?;
        let actual = port(a, b);
        (!same(expected, actual)).then(|| {
            format!(
                "  {name}({a:e}, {b:e}): DOL {expected:e} ({:#010x}), port {actual:e} ({:#010x})",
                expected.to_bits(),
                actual.to_bits()
            )
        })
    });
}

const N: usize = 20_000;

/// Guards against a vacuous harness: near-miss ports (libm's `sinf`, an
/// unfused `lbVector_Mirror`) must be caught.
#[test]
fn harness_catches_near_misses() {
    let Some(mut o) = Oracle::new() else {
        return;
    };
    let mut rng = Rng::new(7);
    let (mut sin_differs, mut mirror_differs) = (0, 0);
    for _ in 0..2000 {
        let x = rng.range(-10.0, 10.0);
        let expected = o.call_f(0x8032_63D4, &[x]).unwrap();
        sin_differs += usize::from(!same(expected, x.sin()));

        let (a, b, pa, _) = vec_case(&mut o, &mut rng, true);
        o.call(0x8000_DC6C);
        let f = (b.x * a.x + b.y * a.y) * -2.0;
        let unfused = Vec3::new(a.x + b.x * f, a.y + b.y * f, a.z);
        mirror_differs += usize::from(!same_vec(&o.get_vec(pa), &unfused));
    }
    assert!(sin_differs > 100, "{sin_differs}");
    assert!(mirror_differs > 100, "{mirror_differs}");
}

// ---------------------------------------------------------------------------
// MSL

#[test]
fn sqrtf_matches_dol() {
    scalar1("sqrtf__Ff", 0x8000_D5BC, N, Rng::mixed, msl::sqrtf);
    scalar1(
        "sqrtf__Ff/positive",
        0x8000_D5BC,
        N,
        |r| f32::from_bits(r.next_u32() >> 1),
        msl::sqrtf,
    );
    scalar1(
        "lbVector_sqrtf_accurate",
        0x8000_E19C,
        N,
        Rng::mixed,
        msl::sqrtf_accurate,
    );
}

#[test]
fn sin_cos_tan_match_dol() {
    scalar1("sinf", 0x8032_63D4, N, Rng::mixed, msl::sinf);
    scalar1("cosf", 0x8032_6240, N, Rng::mixed, msl::cosf);
    scalar1("tanf", 0x8032_61BC, N, Rng::mixed, msl::tanf);
    let wide = |r: &mut Rng| r.range(-1e9, 1e9);
    scalar1("sinf/wide", 0x8032_63D4, N, wide, msl::sinf);
    scalar1("cosf/wide", 0x8032_6240, N, wide, msl::cosf);
}

// ---------------------------------------------------------------------------
// lbtrigf

#[test]
fn lbtrigf_matches_dol() {
    scalar1("atanf", 0x8002_2E68, N, Rng::mixed, trigf::atanf);
    scalar1(
        "atanf/intervals",
        0x8002_2E68,
        N,
        |r| r.range(-3.0, 3.0),
        trigf::atanf,
    );
    scalar2("atan2f", 0x8002_2C30, N, Rng::mixed, trigf::atan2f);
    scalar1("acosf", 0x8002_2D1C, N, Rng::mixed, trigf::acosf);
    scalar1(
        "acosf/domain",
        0x8002_2D1C,
        N,
        |r| r.range(-1.0, 1.0),
        trigf::acosf,
    );
    scalar1("asinf", 0x8002_2DBC, N, Rng::mixed, trigf::asinf);
    scalar1(
        "asinf/domain",
        0x8002_2DBC,
        N,
        |r| r.range(-1.0, 1.0),
        trigf::asinf,
    );
    scalar1("lb_sqrtf", 0x8002_2DF8, N, Rng::mixed, trigf::lb_sqrtf);
}

// ---------------------------------------------------------------------------
// lb_00CE

#[test]
fn lb_00ce_matches_dol() {
    scalar1(
        "expf",
        0x8000_CE50,
        N,
        |r| r.range(-80.0, 80.0),
        lb_00ce::expf,
    );
    scalar2(
        "powf",
        0x8000_CEE0,
        N / 4,
        |r| r.range(0.0, 8.0),
        lb_00ce::powf,
    );
    scalar2(
        "lb_8000D008",
        0x8000_D008,
        N,
        Rng::mixed,
        lb_00ce::lb_8000d008,
    );
    scalar2(
        "lb_8000D008/small",
        0x8000_D008,
        N,
        |r| r.range(-3e-5, 3e-5),
        lb_00ce::lb_8000d008,
    );
    differential("powi", N, |o, rng| {
        let (base, exp) = (rng.next_u32() as i32 % 20, rng.next_u32() as i32 % 40 - 5);
        o.reset();
        o.cpu.gpr[3] = base as u32;
        o.cpu.gpr[4] = exp as u32;
        o.call(0x8000_CF74);
        let actual = lb_00ce::powi(base, exp);
        (o.cpu.gpr[3] != actual as u32).then(|| format!("  powi({base}, {exp})"))
    });
    differential("lb_8000D148", N, |o, rng| {
        let gen = |r: &mut Rng| {
            if r.next_u32().is_multiple_of(4) {
                r.mixed()
            } else {
                r.range(-20.0, 20.0)
            }
        };
        let mut args: Vec<f32> = (0..6).map(|_| gen(rng)).collect();
        args.push(rng.range(0.0, 10.0));
        o.reset();
        for (i, &a) in args.iter().enumerate() {
            o.cpu.set_f(1 + i, f64::from(a));
        }
        o.call(0x8000_D148);
        let actual = lb_00ce::lb_8000d148(
            args[0], args[1], args[2], args[3], args[4], args[5], args[6],
        );
        (o.cpu.gpr[3] != u32::from(actual)).then(|| format!("  lb_8000D148({args:?})"))
    });
}

// ---------------------------------------------------------------------------
// lbvector

fn vec_case(o: &mut Oracle, rng: &mut Rng, finite: bool) -> (Vec3, Vec3, u32, u32) {
    let (a, b) = (rng.vec3(finite), rng.vec3(finite));
    o.reset();
    let pa = o.put_vec(0, &a);
    let pb = o.put_vec(1, &b);
    o.cpu.gpr[3] = pa;
    o.cpu.gpr[4] = pb;
    (a, b, pa, pb)
}

#[test]
fn lbvector_in_place_ops_match_dol() {
    type InPlace = fn(&mut Vec3, &Vec3);
    let ops: [(&str, u32, InPlace); 4] = [
        ("lbVector_Add", 0x8000_D46C, vector::add),
        ("lbVector_Add_xy", 0x8000_D4A0, vector::add_xy),
        ("lbVector_Sub", 0x8000_D4C4, vector::sub),
        ("lbVector_Mirror", 0x8000_DC6C, vector::mirror),
    ];
    for (name, address, port) in ops {
        differential(name, N / 2, |o, rng| {
            let (mut a, b, pa, _) = vec_case(o, rng, false);
            o.call(address);
            let expected = o.get_vec(pa);
            port(&mut a, &b);
            (!same_vec(&expected, &a)).then(|| format!("  {name}: DOL {expected:?}, port {a:?}"))
        });
    }
    type Normalize = fn(&mut Vec3) -> f32;
    let ops: [(&str, u32, Normalize); 2] = [
        ("lbVector_Normalize", 0x8000_D2EC, vector::normalize),
        ("lbVector_NormalizeXY", 0x8000_D3B0, vector::normalize_xy),
    ];
    for (name, address, port) in ops {
        differential(name, N / 2, |o, rng| {
            let (mut a, _, pa, _) = {
                let finite = rng.next_u32().is_multiple_of(2);
                vec_case(o, rng, finite)
            };
            o.call(address);
            let (expected, len) = (o.get_vec(pa), o.f1());
            let actual_len = port(&mut a);
            (!same_vec(&expected, &a) || !same(len, actual_len))
                .then(|| format!("  {name}: DOL {expected:?} {len}, port {a:?} {actual_len}"))
        });
    }
}

#[test]
fn lbvector_rotations_match_dol() {
    for (axis, code) in [
        (vector::Axis::X, 1),
        (vector::Axis::Y, 2),
        (vector::Axis::Z, 4),
    ] {
        differential("lbVector_Rotate", N / 3, |o, rng| {
            let (mut v, _, pv, _) = vec_case(o, rng, true);
            let angle = rng.mixed();
            o.cpu.gpr[4] = code;
            o.cpu.set_f(1, f64::from(angle));
            o.call(0x8000_DB00);
            let expected = o.get_vec(pv);
            vector::rotate(&mut v, axis, angle);
            (!same_vec(&expected, &v))
                .then(|| format!("  Rotate {code} {angle}: DOL {expected:?}, port {v:?}"))
        });
    }
    differential("lbVector_RotateAboutUnitAxis", N, |o, rng| {
        let (mut v, axis, pv, _) = vec_case(o, rng, true);
        let axis = if rng.next_u32().is_multiple_of(2) {
            let mut n = axis;
            vector::normalize(&mut n);
            n
        } else {
            axis
        };
        o.put_vec(1, &axis);
        let angle = rng.mixed();
        o.cpu.set_f(1, f64::from(angle));
        o.call(0x8000_D8F4);
        let expected = o.get_vec(pv);
        vector::rotate_about_unit_axis(&mut v, &axis, angle);
        (!same_vec(&expected, &v)).then(|| {
            format!("  RotateAboutUnitAxis {axis:?} {angle}: DOL {expected:?}, port {v:?}")
        })
    });
    differential("lbVector_ApplyEulerRotation", N / 2, |o, rng| {
        let (mut v, angles, pv, _) = vec_case(o, rng, true);
        o.call(0x8000_E138);
        let expected = o.get_vec(pv);
        vector::apply_euler_rotation(&mut v, &angles);
        (!same_vec(&expected, &v))
            .then(|| format!("  ApplyEulerRotation: DOL {expected:?}, port {v:?}"))
    });
    differential("lbVector_CreateEulerMatrix", N / 2, |o, rng| {
        let finite = rng.next_u32().is_multiple_of(2);
        let angles = rng.vec3(finite);
        o.reset();
        let pa = o.put_vec(1, &angles);
        o.cpu.gpr[3] = SCRATCH + 0x200;
        o.cpu.gpr[4] = pa;
        o.call(0x8000_E530);
        let expected = o.get_mtx(SCRATCH + 0x200);
        let actual = vector::create_euler_matrix(&angles);
        (!same_mtx(&expected, &actual)).then(|| format!("  CreateEulerMatrix {angles:?}"))
    });
}

#[test]
fn lbvector_measures_match_dol() {
    type Measure = fn(&Vec3, &Vec3) -> f32;
    let ops: [(&str, u32, Measure); 3] = [
        ("lbVector_Angle", 0x8000_D620, vector::angle),
        ("lbVector_AngleXY", 0x8000_D790, vector::angle_xy),
        ("lbVector_CosAngle", 0x8000_DCA8, vector::cos_angle),
    ];
    for (name, address, port) in ops {
        differential(name, N, |o, rng| {
            let (a, b, _, _) = {
                let finite = !rng.next_u32().is_multiple_of(4);
                vec_case(o, rng, finite)
            };
            o.call(address);
            let (expected, actual) = (o.f1(), port(&a, &b));
            (!same(expected, actual))
                .then(|| format!("  {name}({a:?}, {b:?}): DOL {expected:e}, port {actual:e}"))
        });
    }
    type Binary = fn(&Vec3, &Vec3) -> Vec3;
    let ops: [(&str, u32, Binary); 2] = [
        ("lbVector_Diff", 0x8000_D4F8, vector::diff),
        (
            "lbVector_CrossprodNormalized",
            0x8000_D530,
            vector::crossprod_normalized,
        ),
    ];
    for (name, address, port) in ops {
        differential(name, N / 2, |o, rng| {
            let (a, b, _, _) = {
                let finite = rng.next_u32().is_multiple_of(2);
                vec_case(o, rng, finite)
            };
            o.cpu.gpr[5] = SCRATCH + 0x200;
            o.call(address);
            let (expected, actual) = (o.get_vec(SCRATCH + 0x200), port(&a, &b));
            (!same_vec(&expected, &actual))
                .then(|| format!("  {name}: DOL {expected:?}, port {actual:?}"))
        });
    }
    differential("lbVector_Lerp", N / 2, |o, rng| {
        let (a, b, _, _) = vec_case(o, rng, false);
        let f = rng.mixed();
        o.cpu.gpr[5] = SCRATCH + 0x200;
        o.cpu.set_f(1, f64::from(f));
        o.call(0x8000_DDAC);
        let (expected, actual) = (o.get_vec(SCRATCH + 0x200), vector::lerp(&a, &b, f));
        (!same_vec(&expected, &actual))
            .then(|| format!("  Lerp: DOL {expected:?}, port {actual:?}"))
    });
    differential("lbVector_8000DE38", N / 2, |o, rng| {
        let points = [rng.vec3(false), rng.vec3(false), rng.vec3(false)];
        let c = rng.mixed();
        o.reset();
        let pp = o.put_vec(0, &points[0]);
        for (i, p) in points.iter().enumerate() {
            for (j, x) in [p.x, p.y, p.z].into_iter().enumerate() {
                o.cpu
                    .mem
                    .write_f32(pp + 12 * i as u32 + 4 * j as u32, x)
                    .unwrap();
            }
        }
        o.cpu.gpr[3] = pp;
        o.cpu.gpr[4] = SCRATCH + 0x200;
        o.cpu.set_f(1, f64::from(c));
        o.call(0x8000_DE38);
        let (expected, actual) = (
            o.get_vec(SCRATCH + 0x200),
            vector::lbvector_8000de38(&points, c),
        );
        (!same_vec(&expected, &actual))
            .then(|| format!("  8000DE38 {c}: DOL {expected:?}, port {actual:?}"))
    });
    differential("lbVector_EulerAnglesFromONB", N / 2, |o, rng| {
        let (a, b, c) = (rng.vec3(true), rng.vec3(true), rng.vec3(true));
        let b = match rng.next_u32() % 4 {
            0 => Vec3::new(b.x, b.y, 1.0),
            1 => Vec3::new(b.x, b.y, -1.0),
            _ => Vec3::new(b.x, b.y, rng.range(-1.0, 1.0)),
        };
        o.reset();
        o.cpu.gpr[3] = SCRATCH + 0x200;
        o.cpu.gpr[4] = o.put_vec(0, &a);
        o.cpu.gpr[5] = o.put_vec(1, &b);
        o.cpu.gpr[6] = o.put_vec(2, &c);
        o.call(0x8000_DF0C);
        let (expected, actual) = (
            o.get_vec(SCRATCH + 0x200),
            vector::euler_angles_from_onb(&a, &b, &c),
        );
        (!same_vec(&expected, &actual))
            .then(|| format!("  EulerAnglesFromONB: DOL {expected:?}, port {actual:?}"))
    });
    differential("lbVector_EulerAnglesFromPartialONB", N / 2, |o, rng| {
        let (a, c) = (rng.vec3(true), rng.vec3(true));
        o.reset();
        o.cpu.gpr[3] = SCRATCH + 0x200;
        o.cpu.gpr[4] = o.put_vec(0, &a);
        o.cpu.gpr[5] = o.put_vec(1, &c);
        o.call(0x8000_DFF4);
        let (expected, actual) = (
            o.get_vec(SCRATCH + 0x200),
            vector::euler_angles_from_partial_onb(&a, &c),
        );
        (!same_vec(&expected, &actual))
            .then(|| format!("  EulerAnglesFromPartialONB: DOL {expected:?}, port {actual:?}"))
    });
    differential("lbVector_8000E838", N, |o, rng| {
        let finite = rng.next_u32().is_multiple_of(2);
        let (a, b, c) = (rng.vec3(finite), rng.vec3(finite), rng.vec3(finite));
        let b = if rng.next_u32().is_multiple_of(4) {
            a
        } else {
            b
        };
        o.reset();
        o.cpu.gpr[3] = o.put_vec(0, &a);
        o.cpu.gpr[4] = o.put_vec(1, &b);
        o.cpu.gpr[5] = o.put_vec(2, &c);
        o.cpu.gpr[6] = SCRATCH + 0x200;
        o.call(0x8000_E838);
        let (d, len) = (o.get_vec(SCRATCH + 0x200), o.f1());
        let (pd, plen) = vector::lbvector_8000e838(&a, &b, &c);
        (!same_vec(&d, &pd) || !same(len, plen))
            .then(|| format!("  8000E838: DOL {d:?} {len}, port {pd:?} {plen}"))
    });
}

// ---------------------------------------------------------------------------
// SDK vector and matrix routines

#[test]
fn psvec_matches_dol() {
    type Binary = fn(&Vec3, &Vec3) -> Vec3;
    let ops: [(&str, u32, Binary); 3] = [
        ("PSVECAdd", 0x8034_2D54, sdk_vec::ps_vec_add),
        ("PSVECSubtract", 0x8034_2D78, sdk_vec::ps_vec_subtract),
        (
            "PSVECCrossProduct",
            0x8034_2E58,
            sdk_vec::ps_vec_cross_product,
        ),
    ];
    for (name, address, port) in ops {
        differential(name, N / 2, |o, rng| {
            let (a, b, _, _) = vec_case(o, rng, false);
            o.cpu.gpr[5] = SCRATCH + 0x200;
            o.call(address);
            let (expected, actual) = (o.get_vec(SCRATCH + 0x200), port(&a, &b));
            (!same_vec(&expected, &actual))
                .then(|| format!("  {name}: DOL {expected:?}, port {actual:?}"))
        });
    }
    differential("PSVECDotProduct", N, |o, rng| {
        let (a, b, _, _) = vec_case(o, rng, false);
        o.call(0x8034_2E38);
        let (expected, actual) = (o.f1(), sdk_vec::ps_vec_dot_product(&a, &b));
        (!same(expected, actual))
            .then(|| format!("  PSVECDotProduct: DOL {expected}, port {actual}"))
    });
    differential("PSVECScale", N, |o, rng| {
        let (a, _, _, _) = vec_case(o, rng, false);
        let s = rng.mixed();
        o.cpu.gpr[4] = SCRATCH + 0x200;
        o.cpu.set_f(1, f64::from(s));
        o.call(0x8034_2D9C);
        let (expected, actual) = (o.get_vec(SCRATCH + 0x200), sdk_vec::ps_vec_scale(&a, s));
        (!same_vec(&expected, &actual))
            .then(|| format!("  PSVECScale: DOL {expected:?}, port {actual:?}"))
    });
    differential("PSVECNormalize", N, |o, rng| {
        let (a, _, _, _) = {
            let finite = rng.next_u32().is_multiple_of(2);
            vec_case(o, rng, finite)
        };
        o.cpu.gpr[4] = SCRATCH + 0x200;
        o.call(0x8034_2DB8);
        let (expected, actual) = (o.get_vec(SCRATCH + 0x200), sdk_vec::ps_vec_normalize(&a));
        (!same_vec(&expected, &actual))
            .then(|| format!("  PSVECNormalize {a:?}: DOL {expected:?}, port {actual:?}"))
    });
    differential("PSVECMag", N, |o, rng| {
        let (a, _, _, _) = {
            let finite = rng.next_u32().is_multiple_of(2);
            vec_case(o, rng, finite)
        };
        o.call(0x8034_2DFC);
        let (expected, actual) = (o.f1(), sdk_vec::ps_vec_mag(&a));
        (!same(expected, actual))
            .then(|| format!("  PSVECMag {a:?}: DOL {expected}, port {actual}"))
    });
}

fn mtx_case(o: &mut Oracle, rng: &mut Rng, finite: bool) -> (Mtx, Mtx) {
    let (a, b) = (rng.mtx(finite), rng.mtx(finite));
    o.reset();
    o.cpu.gpr[3] = o.put_mtx(0, &a);
    o.cpu.gpr[4] = o.put_mtx(1, &b);
    o.cpu.gpr[5] = SCRATCH + 0x200;
    (a, b)
}

#[test]
fn psmtx_matches_dol() {
    differential("PSMTXConcat", N / 2, |o, rng| {
        let (a, b) = {
            let finite = rng.next_u32().is_multiple_of(2);
            mtx_case(o, rng, finite)
        };
        o.call(0x8034_2204);
        let (expected, actual) = (o.get_mtx(SCRATCH + 0x200), sdk_mtx::ps_mtx_concat(&a, &b));
        (!same_mtx(&expected, &actual))
            .then(|| format!("  PSMTXConcat: DOL {expected:?}, port {actual:?}"))
    });
    differential("PSMTXConcat/aliased", N / 4, |o, rng| {
        let (a, b) = mtx_case(o, rng, true);
        o.cpu.gpr[5] = o.cpu.gpr[4]; // MTXConcat(a, b, b)
        o.call(0x8034_2204);
        let (expected, actual) = (o.get_mtx(o.cpu.gpr[4]), sdk_mtx::ps_mtx_concat(&a, &b));
        (!same_mtx(&expected, &actual)).then(|| "  PSMTXConcat aliased".to_string())
    });
    differential("PSMTXTranspose", N / 4, |o, rng| {
        let (a, _) = mtx_case(o, rng, false);
        o.cpu.gpr[4] = o.cpu.gpr[3]; // in place
        o.call(0x8034_22D0);
        let (expected, actual) = (o.get_mtx(o.cpu.gpr[3]), sdk_mtx::ps_mtx_transpose(&a));
        (!same_mtx(&expected, &actual)).then(|| "  PSMTXTranspose".to_string())
    });
    differential("PSMTXInverse", N / 2, |o, rng| {
        let (a, _) = {
            let finite = rng.next_u32().is_multiple_of(2);
            mtx_case(o, rng, finite)
        };
        let a = if rng.next_u32().is_multiple_of(8) {
            let mut s = a;
            s[1] = s[0]; // singular
            o.put_mtx(0, &s);
            s
        } else {
            a
        };
        o.cpu.gpr[4] = SCRATCH + 0x200;
        o.put_mtx(8, &[[7.0; 4]; 3]);
        o.call(0x8034_2320);
        let ok = o.cpu.gpr[3];
        let expected = o.get_mtx(SCRATCH + 0x200);
        match sdk_mtx::ps_mtx_inverse(&a) {
            Some(m) if ok == 1 && same_mtx(&expected, &m) => None,
            None if ok == 0 => None,
            other => Some(format!(
                "  PSMTXInverse {a:?}: DOL {ok} {expected:?}, port {other:?}"
            )),
        }
    });
    differential("PSMTXMultVec", N / 2, |o, rng| {
        let m = rng.mtx(false);
        let v = rng.vec3(false);
        o.reset();
        o.cpu.gpr[3] = o.put_mtx(0, &m);
        o.cpu.gpr[4] = o.put_vec(4, &v);
        o.cpu.gpr[5] = SCRATCH + 0x200;
        o.call(0x8034_2AA8);
        let (e1, a1) = (o.get_vec(SCRATCH + 0x200), sdk_mtx::ps_mtx_mult_vec(&m, &v));
        o.call(0x8034_2AFC);
        let (e2, a2) = (
            o.get_vec(SCRATCH + 0x200),
            sdk_mtx::ps_mtx_mult_vec_sr(&m, &v),
        );
        (!same_vec(&e1, &a1) || !same_vec(&e2, &a2))
            .then(|| format!("  MultVec: {e1:?} {a1:?} SR {e2:?} {a2:?}"))
    });
    differential("PSMTXQuat", N / 2, |o, rng| {
        let q = Quaternion::new(rng.finite(), rng.finite(), rng.finite(), rng.finite());
        o.reset();
        o.cpu.gpr[3] = SCRATCH + 0x200;
        o.cpu.gpr[4] = o.put_quat(0, &q);
        o.call(0x8034_2690);
        let (expected, actual) = (o.get_mtx(SCRATCH + 0x200), sdk_mtx::ps_mtx_quat(&q));
        (!same_mtx(&expected, &actual))
            .then(|| format!("  PSMTXQuat {q:?}: DOL {expected:?}, port {actual:?}"))
    });
    differential("PSMTXRotAxisRad", N / 2, |o, rng| {
        let axis = rng.vec3(true);
        let rad = rng.mixed();
        o.reset();
        o.cpu.gpr[3] = SCRATCH + 0x200;
        o.cpu.gpr[4] = o.put_vec(0, &axis);
        o.cpu.set_f(1, f64::from(rad));
        o.call(0x8034_2530);
        let (expected, actual) = (
            o.get_mtx(SCRATCH + 0x200),
            sdk_mtx::ps_mtx_rot_axis_rad(&axis, rad),
        );
        (!same_mtx(&expected, &actual))
            .then(|| format!("  PSMTXRotAxisRad {axis:?} {rad}: DOL {expected:?}, port {actual:?}"))
    });
    differential("MTXRotRad", N / 2, |o, rng| {
        let axis = b"xyzXYZw"[(rng.next_u32() % 7) as usize];
        let rad = rng.mixed();
        o.reset();
        let sentinel = [[9.0; 4]; 3];
        o.cpu.gpr[3] = o.put_mtx(8, &sentinel);
        o.cpu.gpr[4] = u32::from(axis);
        o.cpu.set_f(1, f64::from(rad));
        o.call(0x8034_2418);
        let expected = o.get_mtx(SCRATCH + 8 * 0x40);
        let actual = sdk_mtx::mtx_rot_rad(axis, rad).unwrap_or(sentinel);
        (!same_mtx(&expected, &actual)).then(|| format!("  MTXRotRad {axis} {rad}"))
    });
    differential("PSMTXTrans/Scale/Identity", N / 8, |o, rng| {
        let (x, y, z) = (rng.mixed(), rng.mixed(), rng.mixed());
        let mut bad = false;
        for (address, port) in [
            (0x8034_2634, sdk_mtx::ps_mtx_trans(x, y, z)),
            (0x8034_2668, sdk_mtx::ps_mtx_scale(x, y, z)),
            (0x8034_21A4, sdk_mtx::ps_mtx_identity()),
        ] {
            o.reset();
            o.cpu.gpr[3] = o.put_mtx(8, &[[9.0; 4]; 3]);
            for (i, v) in [x, y, z].into_iter().enumerate() {
                o.cpu.set_f(1 + i, f64::from(v));
            }
            o.call(address);
            bad |= !same_mtx(&o.get_mtx(SCRATCH + 8 * 0x40), &port);
        }
        bad.then(|| format!("  Trans/Scale/Identity {x} {y} {z}"))
    });
    differential("C_MTXLookAt", N / 2, |o, rng| {
        let (p, u, t) = (rng.vec3(true), rng.vec3(true), rng.vec3(true));
        o.reset();
        o.cpu.gpr[3] = SCRATCH + 0x200;
        o.cpu.gpr[4] = o.put_vec(0, &p);
        o.cpu.gpr[5] = o.put_vec(1, &u);
        o.cpu.gpr[6] = o.put_vec(2, &t);
        o.call(0x8034_2734);
        let (expected, actual) = (
            o.get_mtx(SCRATCH + 0x200),
            sdk_mtx::c_mtx_look_at(&p, &u, &t),
        );
        (!same_mtx(&expected, &actual)).then(|| "  C_MTXLookAt".to_string())
    });
}
