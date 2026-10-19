# Native animation integration — 2026-10-18

The ROM-free runtime now evaluates original fighter animation in Rust/WASM and
skins the original meshes on the GPU. It still does not implement a playable
match, action transitions, collision response, combat, original menus, or audio.

## Integrated existing work

Claude's completed math branch (`fe701fa`, integrated as `177bf48`) was still
outside main. It provides MSL trig/sqrt, lb math and SDK matrix/vector ports,
with a developer-only DOL differential test evaluator. The evaluator is our
research code in `ppc-asm`, not part of the WASM game dependency graph; no
Dolphin/Slippi emulator code is used. Its floating-point model is shared with
the port and includes provisional reciprocal estimates: differential tests
verify translation against that model, not complete hardware equivalence.

## New core functions

- HSD_AObjReqAnim 0x8036410C, StopAnim 0x8036414C, InterpretAnim 0x80364190:
  first-play, frame rate, end/stop, loop remainder, rewind and no-update flags.
  Global end-callback counts are not global state; the caller can aggregate
  stopped/running animation flags. Object reference callbacks are not included.
- FObj callback suppression now also suppresses interpolation-state mutations,
  matching the early return in FObjUpdateAnim when its callback is NULL.
- HSD_MtxSRT 0x8037A250: original MSL sin/cos, double reciprocal followed by
  frsp, separate single products, fmsubs at 0x8037A388/3B8 and fmadds at
  0x8037A38C/3B4. 512 randomized SRT tests match DOL execution bit for bit.
- HSD_JObjMakeMatrix 0x8036F1F8 subset: Euler transforms, parent scale
  compensation, classical scaling and SDK PSMTXConcat ordering.
- JObjUpdateFunc 0x8036FDC0 subset: rotation, translation, scale (including
  the original positive 0.001 clamp), node and branch visibility.
- lbAnim_8001E6D8 0x8001E6D8 informs per-joint AObj setup and classical scaling.
  ftAnim_8006F4C8 informs node order. Current packaged clips have exactly one
  FigaTree node per original preorder joint; mismatches fail export instead of
  guessing a fighter-part remap.

Audit outputs: `/Volumes/External SSD/Developer/Builds/20xx-animation-audit/`.
Reference checkout remains pinned to the commit in `research/acquisition.json`.

## Content and renderer

`generated/content-v2` (ignored) contains the four fighters and stage. Version 2
metadata includes preorder skeletons, original inverse bind matrices and packed
FObj streams. Streams retain archive hash, symbol, motion index and data format.
The tracked manifest contains hashes; no original archive or converted asset
is committed. `tools/content-pack.py` still hydrates verified published files
for CI without a disc. Reproduce into a new directory using scene-export, then
record the manifest with `--record --source generated/content-v2`.

58 motion-table entries cover Wait1, three walk speeds, Dash, Run, Landing,
JumpF/JumpB, Fall, Attack11 and AttackAirN. Some entries legitimately share the
same animation. The web surface loops Wait1 for each fighter; this is display
playback, **not** the original random idle/action transition logic. No input is
misrepresented as controlling a match. Other clips are available to Rust tests
and the native content adapter for subsequent action-state integration.

Vertices retain original coordinates and up to four envelope influences.
Each joint supplies a direct world matrix and a world-times-inverse-bind matrix,
so single-weight joint-local and blended bind-space meshes use distinct palette
entries. The GPU receives the Rust matrices in a float texture. Existing static
buffers remain in this first pack for comparison and bounds inspection.

Limits: no IK, object/path callbacks, quaternion joints, animation blending,
dynamic fighter-part insertion/remapping, subaction-driven model visibility,
material/TEV animation, or exact normal transforms. The stage remains static.
Fighter body-part visibility, lighting and some materials remain visibly unlike
the game. The display clock uses fixed 60-frame stepping and pauses in a hidden
tab; full match timing/netplay is still unimplemented.

## Validation

- Existing game-math differential tests plus 512 SRT cases against DOL execution.
- AObj first frame, endpoint, loop overshoot, no-update and clone behavior.
- Every frame of every packaged clip produces finite matrices. Cloned skeleton
  state advances identically.
- `research/animation-goldens.json` pins native palette hashes at start, next,
  frame 30 where present, and endpoint. The Node integration test loads the same
  content in the production WASM and compares all hashes exactly.
- Actual browser inspection confirms the original idle poses render, with no
  console warnings or errors. These tests do not prove complete game fidelity.
