# Porting plan — 2026-10-18

## Decisions (user, 2026-10-18)

- **Approach: hand-port to Rust.** Game logic is translated function by function
  from the matching decomp into `crates/melee`. Two alternatives were considered
  and not chosen: compiling the decomp's C to WebAssembly, and emulating the
  20XX DOL in the browser.
- **Scope: tournament-style play.** All characters and stages, no items, online
  multiplayer first. 20XX is the named target; see "20XX vs. vanilla" below.

- **UCF 0.84** for online play (user, 2026-10-18), matching Slippi.
- **No emulation and no emulator code** (user, 2026-10-18). Hardware details
  such as `frsqrte` estimates must be re-derived or shown not to matter.
- **Validation corpus:** agent's choice (user, 2026-10-18). Public Slippi
  replays (`.slp`), parsed from the published format, are the first frame-level
  oracle.
- **Asset delivery: open.** The user wants friends to play from a link with
  nothing to provide. That conflicts with not redistributing Nintendo's game
  data; see the discussion in the session log and research/status.md.

## Why this is tractable now (observed)

- The decomp at `f0cdbb0a7046` marks all 1,117 objects `Matching` in
  `configure.py`. Its C compiles to the original 1.02 machine code, so it is an
  exact reference for control flow, float types, and data layouts.
- 985 of 987 C files under `src/melee` and `src/sysdolphin` pass
  `clang -fsyntax-only` for a little-endian ILP32 target (the two failures are
  `sislib_font.c` and `hsd_3915.c`). Their structs and arithmetic are not
  tied to PowerPC beyond endianness. (Recorded for reference; this port does
  not compile the C.)
- Fighter data loads from the player's own disc with exact values: all 14
  `@datvalue` annotations on `ftCommonData` and published Fox values match
  (`crates/melee/tests/disc_data.rs`, both discs).

## Fidelity rules

The goal is frame-identical behavior with 1.02, which requires bit-identical
floating point. Rules (also in `crates/melee/src/lib.rs`):

1. Port from the decomp's C, keeping its control flow, operand order, and
   `float`/`double` types. Cite the decomp symbol and 1.02 address.
2. Before porting a function, run
   `dol-inspect originals/melee-1.02.dol reference/melee/config/GALE01/symbols.txt NAME`.
   Melee and HSD code was built with `-fp_contract on` (`cflags_base` in the
   decomp's `configure.py`), so MWCC may fuse `a * b + c`
   into `fmadds`. A fused op rounds once; port it as
   `((a as f64) * (b as f64) + (c as f64)) as f32`. That is exact for `f32`
   inputs because the product fits in an `f64`, and it matches Dolphin's
   `fmadds` (an `f64` fma, then rounding to single). Unfused ops port as plain
   `f32` ops, which match Gekko single-precision ops exactly. The SDK
   (`libs/dolphin`) was built with `-fp_contract off`.
3. Use `math::abs_c` for the C `ABS` macro and keep `!(a < b)` forms. Both
   differ from the obvious Rust when values are `-0.0` or NaN.
4. No global state: everything the game keeps in globals (RNG seed,
   `p_ftCommonData`, GObj lists) lives in explicit simulation state. Rollback
   saves and restores this state, and peers hash it to detect desyncs.
5. Game data is read from the player's disc (`gc-disc` → `hsd-dat` →
   `melee::dat`); none is committed or bundled.

Still to verify: the paired-single (`ps_*`) math in the HSD matrix library,
`frsp` placement where C mixes `double` constants into `float` math, and the
trig/sqrt implementations (`src/MSL/trigf.c`, `math_ppc.h`, `frsqrte`).
These need ports that are tested bit for bit against the original.

## Layout verification

Generated layouts (`tools/gen_dat_structs.py`, `tools/gen_ext_attrs.sh`) get
three independent checks:

1. Decomp `/* +OFFSET */` annotations agree with the computed layout. A lone
   typo is accepted, with a warning, only when the next annotation agrees.
2. Struct sizes match the machine code that copies them. For example, each
   `*_Init_LoadSpecialAttrs` is a copy loop whose iteration count gives the size.
3. Loaded disc values have no misalignment signatures: no integers holding
   float bit patterns and no denormal-sized floats.
   `tools/attr_access.py` then shows how 1.02 code actually loads each offset
   (`lfs` = float, `lwz` = int), which settles disputed types.

## Validation strategy (proposed)

- **Unit level:** per-function tests, and `dol-inspect` audits recorded in doc comments.
- **Data level:** typed loads checked against decomp annotations and
  published values on both discs.
- **Frame level:** Slippi replays (`.slp`) record every frame's controller
  inputs and resulting fighter state: position, action state, damage, facing,
  and more. Feeding replay inputs into the port and comparing each frame's
  state gives a large ground-truth corpus of real tournament matches. Slippi
  applies UCF and other codes, so the port must model the exact Slippi code
  set to match these replays. Not yet attempted.
- **Determinism:** the same inputs must give the same state hashes on native
  and `wasm32` builds, across browsers.

## Phases

1. **Done:** disc/FST/DOL/HSD-archive readers, the PowerPC audit tool, the
   20XX DOL change map, generated data layouts, the RNG, 30 ported
   `ftcommon.c` / `ft_081B.c` movement functions, and every tournament
   character's special attributes, checked against machine code and disc data.
2. **Data foundation:** stage collision data (`map_coll`); hurtbox and hitbox
   descriptors; subaction (`ftAction`) scripts; and animation banks (`Pl*AJ.dat`
   slices; `hsd_dat::Archive::parse` accepts a byte slice, so this is untested
   on AJ banks rather than unsupported).
3. **Engine core:** HSD JObj/AObj/FObj animation and matrices (bones drive
   ECBs and hitboxes), GObj process order (the frame update sequence),
   `mpLib` environment collision, the subaction interpreter, `ftColl`
   hit/hurt resolution, the `ftCo_*` action-state machine, and damage and
   knockback.
4. **Headless vertical slice:** two fighters on Final Destination with no
   rendering, driven by recorded inputs and checked frame by frame against
   reference traces. Then all characters, then all stages.
5. **Browser shell:** WebGPU/WebGL2 renderer for HSD models with GX TEV
   emulation, audio, Gamepad API input, and frame pacing at 59.94 Hz. Kept
   outside the simulation. Input-to-photon latency is measured on real devices.
6. **Netplay:** state save/restore (state is plain Rust data), input exchange
   with prediction and rollback, private friend rooms, and WebRTC data
   channels with TURN fallback. Fairness work: symmetric input delay
   negotiated from RTT, a capped rollback window, and desync detection by
   state hash. All of this is proposed until measured.
7. **20XX layer:** UCF and any 20XX gameplay codes chosen for online play,
   then training features. Each is ported from the hooks in
   `research/dol-symbol-diff.txt` and the 20XX code library.

## 20XX vs. vanilla (observed)

- 20XX changes 410 functions and 613 data objects in `main.dol` (77,566
  bytes in loaded sections). Most large changes overwrite unused code, such as
  the THP video decoder and the debug-menu functions around `0x8019xxxx`,
  to make room for 20XX code. Gameplay functions mostly show 4-byte changes
  consistent with injection hooks, for example in `ftCo_Dash_Phys`,
  `ftCo_KneeBend_*`, `ftCo_Walk_IASA`, `ftCo_Turn_IASA`, and
  `Fighter_procInput`. Full map: `dol-symbol-diff.txt`.
- 20XX includes **UCF v0.8**, modified for 20XX and toggled by the debug flag
  at `0x803FA210` (`Mechanics & HUD/Gameplay.txt`). Online play today
  (Slippi) uses UCF 0.84. Which UCF version to ship is an open decision.
- 33 of 34 `Pl??.dat` fighter archives, including `PlCo.dat`, are byte-identical
  between vanilla and 20XX. `PlCa.dat` is 146 bytes larger in 20XX; its
  `ftCo_DatAttrs` and symbol tables are unchanged, and the cause has not been
  identified.
- 20XX replaces many stage files with alternates. Examples: `GrNLa.dat`
  (Final Destination) exists only as `GrNLa.0at`–`GrNLa.hat`, `GrPs.dat` is
  absent (`GrP0.usd`–`GrPn.usd` exist), and `GrNBa.dat` grew from 452,171
  to 1,160,491 bytes. Tournament-faithful stages should be checked against the
  vanilla files; how 20XX selects alternates has not been analyzed.

## Licensing note

The decomp has no repository license file, and `tools/dat-cli` is AGPL-3.0;
no decomp code is copied into this repository. The port is a translation
derived from studying the decomp, and Nintendo owns the game. Assess the legal
position before any public distribution. Game assets always come from the
player's own disc.
