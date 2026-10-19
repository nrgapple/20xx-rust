# Input stream: controller → fighter input state (2026-10-18)

Vanilla NTSC 1.02 behavior, ported to `crates/melee/src/hsd/controller.rs`
(HSD pad library) and `crates/melee/src/ft/input.rs` (`Fighter_procInput`).
UCF is not implemented; its hooks are mapped below.

Unless marked "proposed", statements are observations from the decomp at
`f0cdbb0a7046` and `dol-inspect` disassembly of `originals/melee-1.02.dol`
(and `originals/20xx-5.0.2.dol` where 20XX is mentioned). Nothing here was
checked in an emulator or on hardware.

## Pipeline (observed)

1. **`PADRead` (SDK, not ported).** `gmMain_8015FD24` calls `PADSetSpec(5)`.
   `SPEC2_MakeStatus` subtracts 128 and the calibrated origin from each stick
   axis and the trigger origins (`ClampS8`/`ClampU8`). Melee never calls
   `PADSetAnalogMode`, so the SDK default mode 3 applies and `analogA`/`analogB`
   are always 0. `PADRead` clears button bit 7, so `button <= 0x1F7F`. Melee
   never calls `PADClamp`; only the debug console does. Producing these values
   from the Gamepad API is the browser shell's job.
2. **Polling.** The alarm `fn_800195FC` calls `HSD_PadRenewRawStatus(0)` once
   per poll period. `lb_80019628` caps that period at 1/60 s of game time.
   Polls go into a 5-entry queue (`gmMain_8046B108`, `qtype` 0). When the
   queue is full, the oldest poll is dropped and its buttons are ORed into
   the next one (`HSD_PadRawMerge`; only buttons are merged).
3. **Scene loop `gm_801A4D34`.** For each queued poll it runs one game frame:
   - `lb_800198E0` → `HSD_PadRenewMasterStatus`: dequeue, clamp, compute
     the direction bits, scale, then button edges and repeat.
   - `lb_80019900` renews game status, then copy status, when the 60 Hz tick
     `lb_80019A30(0)` is set. At game speed 1.0 that is every frame.
   - `gm_EvaluateAllControllerInputs`, the scene's `on_frame`, then
     `HSD_GObj_RunProcs`, which runs fighter procs. `Fighter_Create`
     registers `Fighter_procInput` as proc 3: after `Fighter_procHitlag`
     (0), `Fighter_procAnim` (1), and `Fighter_procCpu` (2), and before
     `Fighter_procUpdate` (4, physics).
4. **`Fighter_procInput` (0x8006AD10)** reads `HSD_PadGameStatus[fp->pad_port]`
   (0x804C21CC + 0x44 × port; `mulli r0, r0, 68`).

The port models one poll per frame: `PadLib::renew_for_frame` is the poll,
then master, game, and copy renewal. The queue is ported faithfully
(`renew_raw_status`, `flush_queue`) for completeness and for scene starts
(`gm_801A4D34` calls `HSD_PadFlushQueue(LEAVE1)`).

**Proposed for netplay:** exchange one 8-byte `PadInput` per port per frame
(big-endian `button`, then stick x/y, C-stick x/y, L, R as in `PADStatus`).
With `err == 0`, the HSD values a fighter reads (`button`, `nml_*`) depend
only on the current sample. Still, `PadLib` (all three status arrays, the
queue, and its indices) belongs in the rollback state, because
`last_button`, `repeat_count`, and the transfer-error path (`err == -3`
keeps last frame's values) carry over between frames.

## Parameters

`PadLibData` comes from `default_libinfo_data` (0x80406DAC). `HSD_PadInit`
then sets `qnum = 5`, and `gmMain_8015FD24` overrides some fields
(immediates and `stb` offsets read from its machine code; identical in 20XX):

| Field | Default | Melee | Effect |
| --- | --- | --- | --- |
| `clamp_stickType` | 0 | 0 | circle clamp (`HSD_PadClampCheck3`) |
| `clamp_stickShift` | 0 | 1 | subtract `min` after clamping (`min` is 0: no effect) |
| `clamp_stickMax` | 127 | 80 | stick radius 80 |
| `clamp_stickMin` | 0 | 0 | no inner deadzone at this layer |
| `scale_stick` | 127 | 80 | `nml = n / 80`, n ∈ [-80, 80] |
| `clamp_analogLRShift`/`Max`/`Min` | 0/255/0 | 1/140/0 | triggers clamp to 140 |
| `scale_analogLR` | 255 | 140 | `nml = n / 140` |
| `adc_th` / `adc_angle` | 30 / 0.0 | same | direction bits need radius ≥ 30; four exclusive 90° sectors |
| `repeat_start` / `repeat_interval` | 45 / 8 | same | HSD `repeat` |
| `cross_dir` | 0 | 0 | D-pad unchanged |
| `qtype` | 0 | 0 | merge buttons when the queue is full |

`default_status_data` (0x80406D68) is zero except `cross_dir = 1`.

`ftCommonData` thresholds used here, read from `PlCo.dat` on both discs
(`tests/input.rs::common_data_thresholds_match_disc`):

- stick deadzone 0.28 (both axes)
- smash deadzone 0.25 (both axes)
- shoulder deadzone 0.3; Z analog value 0.35
- shield press threshold 0.25
- tap jump: threshold 0.6625, window 4 frames
- fast fall: `x88` 0.6625, `x8c` 4 frames
- special-move stick thresholds: `x218` 0.6, `x21C` 0.55
- joystick statistic: `x7B8` 0.5, `x7BC` 0.5, `x7C0` 4.0
- `x6FC` 16 (int; passed as a float to `ftCommon_8007FFD8`)

## Findings (observed)

- **Stick values are n/80.** After the 0.28 deadzone, |n| ≤ 22 reads as 0
  and 23/80 = 0.2875 is the smallest nonzero value. The smash deadzone
  (0.25) is below the deadzone, so `active_timer` resets on the first frame
  the stick leaves the deadzone.
- **Circle clamp truncates.** Each axis becomes `fctiwz(v * 80 / r)`, so a
  raw diagonal (127, 127) reads as (56, 56) = (0.7, 0.7). (60, 80) gives
  exactly (48, 64).
- **Triggers.** A raw value of 42 gives 42/140, the same float as the 0.3
  deadzone, so it reads as 0. 43 is the first value that sets `HSD_PAD_LR`.
  A digital L/R click forces `triggers[0] = 1.0`. Z adds `HSD_PAD_LR |
  HSD_PAD_A` with trigger 0.35.
- **Stick direction bits reach the fighter.** `HSD_PadADConvert` ORs
  `PAD_STICK_*`/`PAD_SUBSTICK_*` (bits 16–23) into `button`, and
  `Fighter_procInput` copies `button` into `held_buttons`, so these bits
  appear in held, pressed, and released buttons. Sector bounds are double
  constants (±π/4, ±3π/4 at 0x804DE5F8). `atan2f` (lbtrigf) returns
  f32(π/4) = 0x3F490FDB for a diagonal, which lies above π/4 in double
  precision. So up-right reads as up, up-left as left, down-left as left,
  and down-right as down. `tests/input.rs::stick_direction_bits` checks
  every in-circle point.
- **The C-stick is ignored in single-player modes.** Fighters read it only if
  `DbLevel < DbLKind_DebugRom && !gm_IsCurrently1PMode_inline()`. That
  function is true for major scenes 3, 4, 5, 15, 28, 32–38, and 43, which
  include Training mode. 20XX forces it false twice: "Normal C-Stick
  Functionality in 1P Mode" and the Debug Mode code at 0x8006AE00/0x8006AE98.
- **Hitlag buffers presses.** While `x2219_b5` is set (it is set after
  `pre_hitlag_cb`), pressed and released buttons accumulate. The
  post-processing (smash charge, statistics, counters, `input_cb`) does not run.
- **Input timers.** They increment with `addi`/`stb`, so 255 wraps to 0
  before the clamp to 254. All resets stamp 254, so this does not happen in
  practice. `active_duration` counts every frame, even at neutral. A
  direction held through `input_disabled` frames is not a fresh smash when
  input resumes.
- **The single-button rule (`gm_8016B0FC`)** zeroes the trigger and masks
  buttons to A (humans) or A|X|Y (CPUs). 20XX replaces it with Turbo Mode.

## Ported functions and audit

All functions were disassembled with `dol-inspect` (melee-1.02.dol). The
counts below are fused multiply-adds.

| Function | 1.02 address | Fused | Notes |
| --- | --- | --- | --- |
| `HSD_PadRenewRawStatus` | 0x803769FC | 0 | Queue only. Rumble, `PADReset`, and the reset switch are omitted (hardware). |
| `HSD_PadFlushQueue` | 0x80376D04 | 0 | |
| `HSD_PadClampCheck1` | 0x80376E48 | 0 | u8 compares (`cmplw`) |
| `HSD_PadClampCheck3` | 0x80376E90 | 6 | Two inlined `sqrtf`, each with three `fnmsub`. Otherwise single precision: `fmuls`, `fdivs`, `fsubs`, `fctiwz` + `stb`. |
| `HSD_PadADConvertCheck1` | 0x803771D4 | 4 | `x*x + y*y` is one `fmadds`; `sqrtf` uses three `fnmsub`. The x == 0 test and the sector compares are in double. |
| `HSD_PadCrossDir` | 0x80377450 | 0 | |
| `HSD_PadRenewMasterStatus` | 0x8037750C | 0 | Scale is int→float (0x43300000 trick, `fsubs`) then `fdivs`. Calls ClampCheck1/3, ADConvertCheck1, and CrossDir. |
| `HSD_PadRenewCopyStatus`, `HSD_PadRenewGameStatus` | 0x803779C0, 0x80377B54 | 0 | |
| `HSD_PadRenewStatus`, `HSD_PadInit`, `HSD_PadGetRawQueueCount` | 0x80377CE8, 0x80377D98, 0x8037699C | 0 | Init without rumble or `PADInit` |
| `gmMain_8015FD24` | 0x8015FD24 | 0 | `PadLib::melee()` |
| `atan2f` | 0x80022C30 | 0 | `src/melee/lb/lbtrigf.c` |
| `atanf` | 0x80022E68 | 8 | Two `fnmsubs` in the reduction plus the `fmadds` Horner chain. Table bits read from 0x803B7300. |
| `Fighter_procInput` | 0x8006AD10 | 0 | Hooks below |
| `Fighter_UnkInitLoad_80068914_Inner1` | inlined | 0 | `clear_input` |
| `Fighter_ResetInputData_80068854` | 0x80068854 | 0 | |
| input part of `Fighter_UnkInitLoad_80068914` | 0x80068914 | — | `init_input` |
| `Fighter_UnkIncrementCounters_8006ABEC` | 0x8006ABEC | 0 | `x685`–`x68B` |
| `ftCo_Jump_GetInput` | 0x800CAE80 | 0 | `cmpw` (signed) against `tap_jump_window` |
| `ftCo_800D6928`, `ftCo_800D688C`, `ftCo_SpecialS_HasInput`, `ftCo_800D67C4` | 0x800D6928, 0x800D688C, 0x800964FC, 0x800D67C4 | 0 | |
| `ftCommon_8008031C` | 0x8008031C | 0 | u8 → float via the unsigned 0x43300000 trick |
| `lb_8000D148` | 0x8000D148 | 6 | `fmsubs` for the cross product; `fmadds` for `dist_01` and for `dx*p2x + dy*p2y`; three `fnmsub` in `sqrtf` |

Constants embedded in the port are checked against both DOLs by
`tests/input.rs::embedded_constants_match_the_dol`. The checks cover the
pad defaults, `gmMain_8015FD24`'s `li`/`stb` sequence, `atanf_lookup`, and
the direction-bound doubles. Controller float constants (0.5, 3.0, 1e-10f,
0x4330000080000000, ±π/2, and the four bounds) were read at r2-relative
0x804DE5B0..0x804DE617. `Fighter_procInput` uses 0.0f and 1.0f at
0x804D8250. `lb_8000D148` uses 1e-5f at 0x804D7AA0.

**`sqrtf`.** All three callers (ClampCheck3, ADConvertCheck1, `lb_8000D148`)
inline MSL's `sqrtf` as `g' = (0.5 * g) * fnmsub(x, g * g, 3.0)`, three
times, then `frsp(x * g)`. The shared `crate::math::sqrtf` computes
`3.0 - g*g*x` unfused, so it is not bit-identical to this sequence. The
port uses its own `hsd::controller::sqrtf`. Two exhaustive tests show that
the result does not depend on the `frsqrte` table: for every integer
0..=32768 (all `x² + y²` of s8 sticks), and for all 354,024 `dist_01`
values that 595 distinct human stick deltas can produce, the result equals
the correctly rounded `sqrtf`. That holds with the exact estimate and with
estimates off by ±1/4096. So these input paths are exact even though
`ppc::frsqrte` is provisional.

**Hooks (`ProcInputHooks`).** These calls go into unported systems:

- `ftCommon_8007FFD8` (item mash, only when `fp->x1980` is set)
- `ftCo_800DF0D0` (smash-attack charge state machine)
- `Player_UpdateJoystickCountByIndex` (statistics)
- `fp->input_cb` (the action state's interrupts)

CPU input is `ProcInputEnv::cpu` (the `ftCo_GetCpu*` results). Game-mode
flags are `ProcInputEnv` fields: `db_level`, `one_player_mode`,
`single_button`, `db_pause_flags`.

## Verification

- `make check` passes. New tests:
  - `hsd::controller`: 12 unit tests, including exhaustive `sqrtf` table
    independence, `atanf` accuracy across every table row, clamp and queue
    behavior, and repeat timing (repeats on frames 0, 45, 53, 61, ...).
  - `ft::input`: 3 unit tests, including exhaustive `lb_8000D148` `sqrtf`
    independence.
  - `tests/input.rs`: 14 tests. They cover deadzone and clamp boundaries,
    triggers, the LR and Z macro, button edges and counters, hitlag
    accumulation, timer saturation, tap-jump windows, and the fast-fall
    frame sequence (a down flick on frame N fast-falls on frame N; a slow
    press reaching -0.6625 four frames after -0.25 does not). They also
    cover disabled input, hooks, direction bits, and the disc-backed
    constant and threshold checks.
- `tools/build-web.sh` builds (`melee_web.wasm` is 599 KB). `simulate_fall`
  now drives the ported pipeline with synthetic pad samples. Its API is
  unchanged.

Not verified:

- No comparison against emulator or hardware traces.
- The wasm32 build's results were not compared with native for these
  functions. They use `f64::mul_add` and `f32` division, which are
  deterministic there.
- `atanf` was checked by structure, by an accuracy sweep (worst about
  7.4 × 2^-23 relative, from cancellation in HAL's reduction), and by exact
  values at the diagonals. Its full output was not compared against hardware.

## UCF hook map (UCF not implemented; user decision pending)

The source is `20XXHP 5.0 Code Library/Mechanics & HUD/Gameplay.txt`, entry
"Universal Controller Fix (UCF), v0.8.tm" [Dan Salvato, taukhan, UnclePunch],
"slightly modified ... for compatibility with Training Mode and 20XX
replays". Its text-section DOL offsets map to RAM as offset + 0x80003420.
Each RAM address below matches a changed word in `research/dol-symbol-diff.txt`.

| Code library offset | RAM address | Vanilla symbol | Replaced word | UCF behavior |
| --- | --- | --- | --- | --- |
| 0x6459C | 0x800679BC | `Fighter_800679B0`+0xC | `li r4, 0x23EC` → `0x23FC` | Fighter allocation grows by 16 bytes, giving UCF state at fp+0x23EC.. |
| 0x67A6C | 0x8006AE8C | `Fighter_procInput`+0x17C | `stfs f0, 0x624(r31)` (human `lstick[0].y`) | Shifts a 3-frame history of the clamped raw stick X, `HSD_PadGameStatus[port].stickX` (`lbz 0x18(r3)`): fp+0x23EC is now, 0x23ED is 1 frame ago, 0x23EE is 2 frames ago. |
| 0x679D4 | 0x8006ADF4 | `Fighter_procInput`+0xE4 | `stfs f1, 0x624(r31)` (CPU path) | Same history for CPUs, from `cpu.lstick.x` (fp+0x1A8C). |
| 0xC6624 | 0x800C9A44 | `ftCo_Turn_IASA`+0x4C | `stfs f0, 0x2C(r31)` (facing) | Dashback. All of these must hold: `cur_anim_frame` (fp+0x894) == 2.0; \|lstick.x\| ≥ `dash_smash_stick_threshold` (0.8); `active_timer.lstick.x` < 2; not the sub-fighter; (x_now − x_2ago)² > 3600 (raw X moved more than 60 in 2 frames). Then it sets `mv.co.turn.has_turned` (fp+0x2340) and `just_turned` (fp+0x2358) to 1. For Popo (kind 10) it also sets Nana's CPU stick X to 127 or -128 (0x7F/0x80) by facing. |
| 0x96484 | 0x800998A4 | `ftCo_80099894`+0x10 (spot-dodge entry, `ftCo_Escape.c`) | `lwz r4, 0x2C(r3)` | Shield drop. It rebuilds raw stick magnitudes from `lstick` (×80, `fctiwz`) and reads `cstick[0].y`, `ftCommonData+0x314`, `active_timer.lstick.x`, and constants including −0.8. When the input falls in the shield-drop zone, it returns past the caller and skips the spot dodge. |
| 0x8D4D4 | 0x800908F4 | `ftCo_DamageFall_IASA`+0xCC | `cmpw r3, r0` | Tumble wiggle. If the previous frame's \|lstick.x\| (`lstick[1].x`) was below `ftCommonData+0x210`, the wiggle also needs (x_now − x_2ago)² > 3600. |
| 0x3F7210 (data) | 0x803FA210 | `un_803FA128`+0xE8 | 5 → 1 | Control flag; every UCF branch tests it first (0 = off). |

**Where a UCF layer plugs into this port (proposed):**

1. Add `Fighter` fields for fp+0x23EC..0x23EE: three `i8` raw-X samples.
   They lie past the vanilla struct end (0x23EC), so they are a pure extension.
2. In `ft::input::proc_input`, right after `input.lstick[0]` is set, shift
   the history. Human players use `env.pads[pad_port].stick_x` (the clamped
   `HsdPadStatus.stick_x`); CPUs use the CPU raw stick. A hook method on
   `ProcInputHooks` (or a `ucf` option on `ProcInputEnv`) at that point
   matches 20XX's two injection sites exactly.
3. The consumers are action-state code owned by other streams:
   `ftCo_Turn_IASA`, the spot-dodge entry `ftCo_80099894`, and
   `ftCo_DamageFall_IASA`. Each gets a UCF branch at the address above,
   guarded by a match setting in place of the 0x803FA210 flag.
4. The HSD layer (`PadLib`) is unaffected by UCF v0.8.

## Other 20XX changes to this pipeline (observed, not ported)

`Fighter_procInput` (49 bytes changed):

- 0x8006AD18 (entry) and 0x8006B820 (exit): "Replay System - Save/Load
  System Buttons" (Core Engines)
- 0x8006AF10, just before the deadzone step: "Replay System Engine"
  (Core Engines)
- 0x8006AE00 and 0x8006AE98: "Normal C-Stick in Develop Mode (and 1P Mode
  kind of)" (Debug Mode)
- 0x8006B02C (`bl gm_8016B0FC` → `li r3, 0`), 0x8006B798, and 0x8006B7F8:
  "Turbo Mode (v1.3)", which replaces Single Button Mode (Game Modes)
- 0x8006B6A0: "Match Start - D-Pad Up to Reload Music" (Audio)
- 0x8006B80C: "20XXTE Input Display - New Loc" (HUD)
- 0x8006B810: "Big Head Mode" (Game Modes)

`HSD_PadRenewMasterStatus` (8 bytes changed):

- 0x803775BC: "Start Button Hold Counter" (Debug Mode)
- 0x8037793C: "Music Volume Reduction on Idle" (Audio)

Other functions:

- `gm_IsCurrently1PMode_inline` 0x8016B480: "Normal C-Stick Functionality in
  1P Mode" (Gameplay)
- `gm_801A4D34` (scene loop): 0x801A4D98/0x801A5050 are tauKhan's "Half frame
  video lag reduction + poll drift fix", offered at boot when progressive
  scan is on. 0x801A4F84/0x801A5070 nop the USB-screenshot calls.
  0x801A4FA4 is "Save When Transitioning from Debug Menu to CSS". These
  change poll timing and presentation, not the per-frame input values.

## Open questions

- **UCF version (user decision).** The options are 20XX's v0.8.tm (mapped
  above) or Slippi's 0.84. Comparing them needs 0.84's source and license;
  nothing from Slippi (GPL) has been read or copied. Matching Slippi replays
  (porting plan, validation) needs the exact Slippi code set.
- The mapping from Slippi `.slp` per-frame fields to `PadInput` is
  unchecked. In particular, it is not known whether raw stick Y and raw
  trigger bytes are recorded.
- `HSD_PadADConvertCheck1` and `lb_8000D148` share an inlined `sqrtf` whose
  fused form differs from `crate::math::sqrtf`. The math stream should adopt
  the `fnmsub` form or confirm per call site. `atan2f`/`atanf` should move
  to a shared `lb::trigf` module once one exists; they are `pub(crate)` in
  `hsd::controller` for now.
- Game speeds other than 1.0 skip game/copy renewal on some frames
  (`lb_80019A30`). This is not modelled.
- The CPU input path is plumbed through, but no CPU AI is ported.
