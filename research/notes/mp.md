# Stream notes: stage collision queries (`mplib.c`) — 2026-10-18

Source: `src/melee/mp/mplib.c` at decomp `f0cdbb0a7046`. Port:
`crates/melee/src/mp/` (`lib.rs` loading and links, `geom.rs` segment
geometry, `query.rs` line queries, `check.rs` segment checks, `joint.rs`
joint bookkeeping, `psvec.rs` `PSVECNormalize`). Tests:
`crates/melee/tests/mp_queries.rs` (synthetic stages with hand-derived
expectations, plus Final Destination, Battlefield and four other stages from
the vanilla disc).

Every ported function was disassembled from the 1.02 DOL with `dol-inspect`
and compared operation by operation; the audit result is in each doc
comment and summarized below. r2 (`_SDA2_BASE_`) is 0x804DF9E0: `lfd
-31168(r2)` loads 4.0 at 0x804D8020, which the decomp's `< 4.0` uses.

## Ported functions

Fused = fused multiply-add instructions in the 1.02 machine code, each
mirrored with `crate::ppc`.

| Symbol | Address | Rust | Fused ops |
| --- | --- | --- | --- |
| `mpRemap2d` | 0x8004DC90 | `geom::remap_2d` | 6 `fmadd` (double) |
| `mpLib_8004DD90_Floor` | 0x8004DD90 | `MpLib::floor_offset` | 0 |
| `mpLib_8004E090_Ceiling` | 0x8004E090 | `MpLib::ceiling_offset` | 0 |
| `mpLib_8004E398_LeftWall` | 0x8004E398 | `MpLib::left_wall_offset` | 0 |
| `mpLib_8004E684_RightWall` | 0x8004E684 | `MpLib::right_wall_offset` | 0 |
| `mpLineIntersection` | 0x8004E97C | `geom::line_intersection` | 5 `fmsub` + 2 `fmadd` (double) |
| `mpLineIntersectionH` | 0x8004EBF8 | `geom::line_intersection_h` | 1 `fmadd` (double) |
| `mpLib_8004ED5C` | 0x8004ED5C | `MpLib::line_extended` | 6, all inside inlined `sqrtf` |
| `mpCheckFloor` | 0x8004F008 | `MpLib::check_floor` | 0 |
| `mpCheckFloorRemap` | 0x8004F400 | `MpLib::check_floor_remap` | 2 `fmadds` (direction test) |
| `mpCheckCeiling` | 0x8004F8A4 | `MpLib::check_ceiling` | 0 |
| `mpCheckCeilingRemap` | 0x8004FC2C | `MpLib::check_ceiling_remap` | 2 `fmadds` |
| `mpLineIntersectionV` | 0x80050068 | `geom::line_intersection_v` | 1 `fmadd` (double) |
| `mpCheckLeftWall` | 0x800501CC | `MpLib::check_left_wall` | 0 |
| `mpCheckLeftWallRemap` | 0x8005057C | `MpLib::check_left_wall_remap` | 2 `fmadds` |
| `mpCheckRightWall` | 0x800509B8 | `MpLib::check_right_wall` | 0 |
| `mpCheckRightWallRemap` | 0x80050D68 | `MpLib::check_right_wall_remap` | 2 `fmadds` |
| `mpLib_800511A4_RightWall` | 0x800511A4 | `MpLib::check_right_wall_vertices` | 4 `fmadds` |
| `mpLib_800515A0_LeftWall` | 0x800515A0 | `MpLib::check_left_wall_vertices` | 4 `fmadds` |
| `mpLib_8005199C_Floor` | 0x8005199C | `MpLib::floor_below` | 1 `fmadds` |
| `mpLib_80051BA8_Floor` | 0x80051BA8 | `MpLib::find_ledge` | 0 |
| `mpCheckMultiple` | 0x80051EC8 | `MpLib::check_multiple` | 0 |
| `mpCheckAllRemap` | 0x800524DC | `MpLib::check_all_remap` | 0 |
| `mpCheckAll` | 0x80052508 | `MpLib::check_all` | 0 |
| `mpLineNextNonFloor` … `mpLineNextNonRightWall` (8) | 0x80052534–0x800531C8 | `MpLib::line_{next,prev}_non_*` | 0 |
| `mpLib_80053394_Floor`, `_80053448_Floor`, `_8005389C_Ceiling`, `_80053950_Ceiling` | 0x80053394, 0x80053448, 0x8005389C, 0x80053950 | `line_{next,prev}0_non_{floor,ceiling}` | 0 |
| `mpLib_800534FC_Floor`, `_800536CC_Floor`, `_80053A04_Ceiling`, `_80053BD4_Ceiling` | 0x800534FC, 0x800536CC, 0x80053A04, 0x80053BD4 | `{floor,ceiling}_{next,prev}_via_alt` | 0 |
| `mpLib_80053DA4_Floor`, `mpLib_80053ECC_Floor` | 0x80053DA4, 0x80053ECC | `floor_next0_end`, `floor_prev0_end` | 0 |
| `mpFloorGetRight/Left`, `mpCeilingGetRight/Left`, `mpLeftWallGetTop/Bottom`, `mpRightWallGetTop/Bottom` | 0x80053FF4–0x800549B0 | same names, snake case | 0 |
| `mpLineGetV1Pos`, `mpLineGetV0Pos` | 0x80054B14, 0x80054BC0 | `line_v1_pos`, `line_v0_pos` | 0 |
| `mpLineGetKind`, `mpLineGetFlags` | 0x80054C6C, 0x80054CEC | `line_kind`, `line_flags` | 0 |
| `mpLib_80054D68` | 0x80054D68 | `line_set_flags_low` | 0 |
| `mpLineGetNormal` | 0x80054DFC | `line_normal` | 0 |
| `mpLib_80054ED8` | 0x80054ED8 | `line_active` | 0 |
| `mpLinesConnected` | 0x80054F68 | `lines_connected` | 0 |
| `mpJointHide`, `mpJointUnhide` | 0x8005541C, 0x800557D0 | `joint_hide`, `joint_unhide` | 0 |
| `mpJointUpdateDynamics` | 0x80055C5C | `joint_update_dynamics` | 0 |
| `mpLib_80055E24` | 0x80055E24 | `joint_update_dynamics_islands` | 0 |
| `mpJointUpdateBounding` | 0x800565DC | `joint_update_bounding` | 0 |
| `mpLib_8005667C` | 0x8005667C | `joint_islands` | 0 |
| `mpVtxGetPos`, `mpVtxSetPos`, `mpLineSetPos` | 0x800566D8–0x80056710 | `vtx_pos`, `vtx_set_pos`, `line_set_pos` | 0 |
| `mpLib_80056758` | 0x80056758 | `line_set_pos_from_base` | 0 |
| `mpGetSpeed` | 0x800567C0 | `line_speed` | 0 (in `mpRemap2d`) |
| `mpJointFromLine` | 0x80056B6C | `joint_from_line` | 0 |
| `mpLib_80056C54` | 0x80056C54 | `floor_walk` | 9: 6 in `sqrtf`, 3 `fmadds` |
| `mpLib_80057424` | 0x80057424 | `joint_save_prev_pos` | 0 |
| `mpLib_80057528`, `mpLib_800575B0` | 0x80057528, 0x800575B0 | `line_enable`, `line_disable` | 0 |
| `mpJointListAdd`, `mpJointListUnlink` | 0x80057638, 0x80057B4C | `joint_list_add`, `joint_list_unlink` | 0 |
| `mpLib_80057BC0` | 0x80057BC0 | `joint_list_remove` | 0 |
| `mpLib_80057FDC`, `mpLib_80058044` | 0x80057FDC, 0x80058044 | `joint_clear_b11`, `joint_set_b11` | 0 |
| `mpJointSetB10` | 0x800580AC | `joint_set_b10` | 0 |
| `mpLib_800581DC`, `mpLib_80058560` | 0x800581DC, 0x80058560 | `joint_connect`, `joint_connect_all` | 0 |
| `mpLib_80058614_Floor` | 0x80058614 | `update_floor_bounds` | 0 |
| `mpCheckedBounding`, `mpBoundingCheck`, `mpBoundingCheck2`, `mpBoundingCheck3`, `mpUncheckBounding` | 0x800588C8–0x80058AA0 | same names, snake case | 0 |
| `PSVECNormalize` (SDK) | 0x80342DB8 | `psvec::ps_vec_normalize` | `ps_madd` + `fnmsubs` |

`mpLibLoad` (already ported) now also builds the joint list and runs
`mpUncheckBounding`, as the C does. `mpLineGetNext`/`Prev` were re-audited:
unfused, matching the existing port.

The call graph of each check was compared with the machine code's `bl`
targets (for example, `mpCheckCeilingRemap` calls `mpLib_8004ED5C` and
`mpCheckFloorRemap` does not).

## Skipped, and why

- `mpLib_8004D164`, `mpGetGroundCollVtx/Line/Joint`: accessors for globals
  that are public fields of `MpLib`.
- `mpLib_800552B0`, `mpLib_80055E9C`: need `HSD_JObj` (joint tree walk;
  animated vertex transforms with 22 fused ops). Port with the HSD joint
  system.
- `mpJointSetCb1`, `mpJointClearCb1`, `mpJointGetCb1`, `mpJointSetCb2`,
  `mpJointGetCb2`, `mpLib_8005811C`: store and call `Ground` callbacks
  (`cb_0`/`cb_1`); need a decision on how stage callbacks are represented.
- `mpLib_800569EC`, `mpLib_80056A1C` … `mpLib_80056B34`: per-stage tables
  (`mpLib_803BF248`, indexed by `stage_info.grkind`) in DOL data; port with
  stage info.
- `mpLib_80058820`, `mpLib_800587FC`: create the GObj and its per-frame
  process (`mpLib_80058614_Floor` itself is ported).
- `mpIsland_8005B334` (in `mpisland.c`) is not ported. The functions that
  end by calling it return its arguments as `IslandUpdate`.
- Drawing (`mpLib_SetupDraw`, `mpLib_DrawEcbs`, …): rendering, not
  simulation.

## Observations (evidence: machine code, DOL data, tests)

- None of the ported functions differ between the vanilla and 20XX DOLs:
  the only `mp*` symbol in `research/dol-symbol-diff.txt` is
  `mpLib_8005A2DC` (debug drawing). The constants at 0x804D8000–0x804D8160
  and `PSVECNormalize` are also unchanged.
- Double precision is pervasive. The 0.1/0.0001 tolerances, `4.0`, and
  `2.0` are `double` constants; the single differences are compared in
  double. `mpLineIntersection*` and `mpRemap2d` compute in double. The
  `ABS(area) > 0.0001F` in `mpLineIntersection` loads `(double)0.0001f`
  (0x3F1A36E2E0000000).
- `mpJointUpdateDynamics` compares a single `fdivs` slope with double TAN60
  `0x3FFBB67AF037935A` and TAN30 `0x3FE279A753D75617`. A slope of
  1.7320509f (the first f32 above the double TAN60) is a left wall here,
  but would be a floor with a single-precision compare, because TAN60
  rounds to that same f32. Test: `dynamic_lines_are_classified_by_slope`.
- MWCC fuses `a*b + c*d` as `fmadds a, b, c*d` (left product fused) where it
  fuses at all. The same C expression `SQ(x) + SQ(y)` is fused in
  `mpLib_800511A4_RightWall` (the 0.001 test) but not in its distance on the
  next lines, so each site needs the audit.
- Quirks kept from the C (each has a test):
  - `mpLib_8004DD90_Floor` never sets its direction when walking right, so
    a walk can go right, then left, then clamp.
  - `mpLib_8004E398_LeftWall` does not clamp `y` after turning around (the
    right-wall version does).
  - `mpLib_8004ED5C` extends the far end using the already-extended near
    end: Final Destination's main floor (x = ±75) becomes −76 to
    75 + 151/150 = 76.006668 for `mpCheckFloor`/`mpCheckCeiling`.
  - `mpCheckFloor` uses the extended floor; `mpCheckFloorRemap` uses raw
    vertices; `mpCheckCeilingRemap` keeps the extension.
  - Flat surfaces (`|Δ| <= 0.0001`) take the `H`/`V` intersection paths,
    which only accept one crossing direction and return constant normals
    ((0, ±1, 0), (±1, 0, 0)) with no `PSVECNormalize`.
  - `mpCheckMultiple` keeps the earlier check on equal distance (floor,
    ceiling, left wall, right wall).
  - `mpLib_8005199C_Floor` returns the first floor under the point in joint
    and line order, not the nearest: on Battlefield at (40, 30) it returns
    the main floor (line 1), not the platform at y = 27.2.
  - The `goto` into the dynamic-line loop body makes any nonzero dynamic
    count (even negative) visit at least one line.
  - `mpRemap2d`'s degenerate branch adds `b1 - a0`, not `b1 - a1`.
  - `mpLib_8004DD90_Floor`'s +0.0001 bias carries into `mpLib_80056C54`: a
    walk starts 0.0001 above the floor.
- `mpGetSpeed` asserts on speeds above 10000 only when
  `DbLevel >= DbLKind_DebugRom` (3). `DbLevel` (0x804D4A08) is 1 in both
  DOLs' data; whether 20XX raises it at runtime was not checked.
- Disc results (vanilla 1.02): Final Destination's floor normal from
  `mpCheckFloor` is exactly (0, 1, 0); the side walls are hit at
  x = ±85.5657 with normals (±1, 0, 0); the underside is line 4. On six
  tournament stages, a downward `mpCheckFloor` at x = 0 and
  `mpLib_8004DD90_Floor` at the hit agree to within the 0.0001 bias.

## Open questions and decisions needed

- **`frsqrte`.** `PSVECNormalize` does one Newton step from the estimate,
  so every normalized normal depends on the exact hardware table. Tests
  compare these within 4 ulps. Also to confirm with the `frsqrte` work: how
  `fmuls` rounds when an operand is the double-precision estimate
  (`psvec.rs`, `fmuls_estimate`).
- **`sqrtf`.** `mpLib_8004ED5C` and `mpLib_80056C54` inline the MSL
  `sqrtf`: `frsqrte`, then three steps
  `(0.5 * g) * fnmsub(x, g * g, 3.0)` in double, then `frsp(x * g)`. The port
  calls the provisional `crate::math::sqrtf`; it must become exact.
- **Uninitialized `total_dist`** in `mpLib_80056C54`: read only if the walk
  starts on a wall line. Both callers start on a floor, so the port uses
  0.0. Documented divergence.
- **Islands.** `IslandUpdate` values must be applied with
  `mpIsland_8005B334` once `mpisland.c` is ported.
- **Callback shape.** `mpCheckFloor`'s fighter callback is
  `&mut dyn FnMut(&MpLib, i32) -> bool` (the fighter is captured). The
  `mpcoll.c` port should confirm that this fits its callers.
- `floor_bounds` (`mpLib_80458868[1]`) starts at zero; the game sets it to
  ±10000 in `mpLib_80058820` (not ported).

## Verification

`make check` passes (with `CARGO_TARGET_DIR` set to the stream's build
directory) with `MELEE_ISO` and `MELEE_VANILLA_ISO` set. The disc tests ran
against the vanilla disc and printed no skip notes. `cargo check -p melee
--target wasm32-unknown-unknown` passes.
