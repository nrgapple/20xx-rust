# Graphics conversion and animation tracks — 2026-10-18

## User scope

Four simultaneous human players, original graphics/content, hand-ported Rust
logic, and no ROM supplied by a player. This does not change the no-emulation
rule. Original source archives and converted binary content stay out of Git.

## Original graphics: observed, implemented, limitations

`scene-export` is a new **offline Rust converter**, not a game engine. It reads
local NTSC-U 1.02 archives via the existing Rust disc and HSD parsers and writes
little-endian triangle buffers plus decoded RGBA textures. Original archive
SHA-256, output hashes, and all mesh ranges are recorded in metadata. The tracked
`research/content-manifest.json` contains hashes and filenames, not asset bytes.

Inputs from the verified vanilla disc:

| Asset | Archive | Joints | Mesh batches | Texture descriptors | Output bytes |
|---|---|---:|---:|---:|---:|
| Fox | PlFxNr.dat | 73 | 91 | 77 | 4,434,368 |
| Mario | PlMrNr.dat | 61 | 68 | 58 | 2,966,048 |
| Marth | PlMsNr.dat | 90 | 105 | 85 | 3,304,608 |
| Captain Falcon | PlCaNr.dat | 63 | 98 | 76 | 5,329,888 |
| Final Destination | GrNLa.dat | 95 | 37 | 14 | 1,329,696 |

Texture descriptors can refer to overlapping source images; these counts are
not distinct artwork counts. All converted meshes total 34,817 triangles.

References: pinned `reference/melee` commit in acquisition.json:

- `src/sysdolphin/baselib/{jobj,dobj,pobj,mobj,tobj}.h`: descriptor layouts.
- `HSD_JObjLoadJoint` 0x80370E44, `HSD_DObjLoadDesc` 0x8035E1B4,
  `HSD_PObjLoadDesc` 0x8036BE64: descriptor loading/link structure.
- `HSD_MtxSRT` 0x8037A250: scale/rotation/translation and parent scale
  compensation. Audited with dol-inspect; it has fused operations.
- `SetupEnvelopeModelMtx` 0x8036E4C4: single-weight joint-local coordinates
  versus blended bind-space inverse matrices. Audited with dol-inspect.
- Original SDK `GXEnum.h`: vertex modes, component formats, primitives, texture
  format IDs. No emulator implementation was incorporated.
- `src/melee/gr/types.h`: map_head -> UnkStageDat_x8_t model roots.

The converter uses host f64/trig for **display geometry only**; it is not a
bit-identical port of HSD matrix evaluation and must not feed gameplay collision
or rollback. The WebGL 2 presenter uses the first UV texture, diffuse color and
simple lighting. It omits additional texture layers/TEV, animated visibility,
material animation, reflection, particles, fog, shadows and audio. It displays
stored bind poses, not character action states. Final Destination's roots 1–3
are shown; its sky/particle roots require their animation/effect passes. A
bright vertical element in the stage's static geometry remains visible; its
intended appearance has not been verified against a reference frame. This is explicitly labeled on the site
as a static model viewer, not a faithful rendered match.

Supported conversion: joint hierarchies, scale compensation, rigid/shared and
weighted skin matrices, indexed/direct vertex attributes, triangle/strip/fan/
quad display lists, and I4/I8/IA4/IA8/RGB565/RGB5A3/RGBA8/C4/C8/C14X2/CMPR
texture formats. Shape animation, quaternion joints and NBT3 descriptors fail
explicitly. Mesh/texture bounds, pointer checks and cycle guards are enforced.

Reproduce into a **new** output directory (existing files are never overwritten):

```
cargo run -p scene-export -- originals/melee-ntsc-1.02.iso PlFxNr.dat generated/content-v1 fox
# Repeat for the other archive/asset pairs above.
python3 tools/content-pack.py --record --source generated/content-v1
make web-check
```

`tools/content-pack.py` verifies all hashes and copies the derived content into
`web/dist/content`. When local converted files are absent, it downloads the
exact recorded files from the existing public deployment. Thus CI can rebuild
without original discs or reference checkouts. A missing or mismatching file
fails the build. Hashed asset filenames avoid mixing cached generations. This
is a single currently published pack, not a permanent historical asset store.

## HSD animation interpreter: actual Rust engine work

`crates/melee/src/hsd/fobj.rs` implements the animation stream interpreter,
separate from the offline viewer. Mutable state is explicit and cloneable;
immutable stream bytes are shared with Arc. It supports constant, linear,
spline, slope and discrete KEY tracks, little-endian packed values, packed
counts/waits, request/advance/stop, and ordered multiple callbacks per frame.

DOL audit:

- `parseFloat` 0x8036AC10 and `parsePackInfo` 0x8036ADDC.
- `HSD_FObjInterpretAnim` 0x8036B030 contains the inlined state handlers;
  no fused multiply-adds. Start frames are signed i16, packed/wait fields u16.
- `FObjUpdateAnim` 0x8036AE70: linear evaluation has `fmadds` at 0x8036AF98.
  Hermite's reciprocal is computed in double and rounded to single.
- `splGetHelmite` 0x80378A34: three nested `fmadds` at 0x80378A84/88/8C.
  The Rust port preserves every intermediate rounding/operand order.

Unlike the original unchecked parser, malformed streams return an error. The
original undefined-value callback for an absent interpolation opcode is not
emitted. These differences are documented in code. The track interpreter is
not yet wired to fighter joint updates or a complete AObj animation lifecycle.

Validation:

- Focused tests: linear/constant endpoints, Hermite endpoints and slopes,
  packed and signed parsing, KEY event order, delayed start, malformed data,
  and clone/restore.
- Real-data scan of all 27 tournament fighter motion tables visited 1,075,289
  tracks (including repeated motion references). Each was evaluated at start,
  then advanced by up to 120 frames, and replayed after restoring a clone.
  All outputs were finite and restored outputs matched bitwise.
- This corpus scan is **not a reference-game fidelity oracle**. It does not
  prove skeletal poses, complete animation events, or frame-level gameplay.

## Four-seat lobby

Capacity is four; experiments begin when at least two players are connected and
all currently connected players are ready. The server assigns seats, overrides
client-supplied seat claims, and includes seats in experiment/replay payloads.
Gaps remain correctly labeled (for example players 1, 3, 4). A fifth client is
rejected; a disconnected seat can be reclaimed. This remains a lobby/input
experiment protocol, not realtime four-player combat or rollback.

## Next dependencies

The original user request is not complete. Required work includes exact HSD
joint/AObj evaluation, script execution, collision queries, action states,
hit/hurt resolution, damage/knockback, the full match update order, all content,
UCF 0.84, rendering/material fidelity, audio, and rollback. Next: connect the
ported FObj tracks to audited joint transforms and verify poses against a
reference before using those transforms for hitboxes/ECBs.
