# Setup status — 2026-10-18

## Target

20XX Hack Pack 5.0.2 on Melee NTSC-U 1.02, with a future Rust/WebAssembly website
for friends to play online using rollback. See [browser-target.md](browser-target.md).

## Acquisition

- Official 5.0.2 Creator archive downloaded and extracted successfully.
- Upstream 20XX source library and Melee decompilation cloned locally.
- Acquisition URLs, pinned commits, and local hashes: [acquisition.json](acquisition.json).
- Melee NTSC-U revision 2 archive downloaded from Internet Archive; archive MD5
  matched its source metadata, and extraction passed ZIP CRC verification.
- Converted lossless RVZ to ISO using `nodtool` v2.0.0-alpha.12 (upstream release
  binary SHA-256 verified). Full-disc MD5 matches the official vanilla requirement.
- Applied the official patch using `make patch`. The resulting
  `originals/20xx-hp-5.0.2.iso` matches the official 5.0.2 MD5 exactly.
- Extracted both main executables using the Rust inspector. Vanilla DOL SHA-1
  matches the Melee decompilation's expected value exactly.
- Extracted `originals/20xx-5.0.2-AI_Engine.bin` from the patched disc filesystem.

## Observed binary findings

- Both main DOLs are 4,425,184 bytes, with the same section layout and entry point
  `0x8000522c`. There are 77,585 differing byte positions.
- The base disc has 1,209 filesystem files; the patched disc has 1,741.
- The patched disc contains `AI_Engine.bin`, 20,804 bytes, at disc offset 4,596,356.
- At runtime address `0x8016e91c` (DOL offset `0x16b4fc`), the original instruction
  bytes `80010024` are replaced by `4802611c`, consistent with the documented
  loader hook. Address `0x8006b008` is unchanged on disc; the source describes
  patching it at runtime after loading the AI engine.
- Full file inventories: [disc-inventory.json](disc-inventory.json).
  Initial executable comparison: [dol-comparison.json](dol-comparison.json).

These are static binary observations. The game has not been booted or checked
interactively in an emulator this session.

## Observed source findings

- The official patch requires vanilla NTSC 1.02, full-disc MD5
  `0e63d4223b01d9aba596259dc155a174`. Expected patched 5.0.2 MD5:
  `d926ba5b39551f5245fd655bc1dfeb3f`.
- The Melee decompilation README identifies the original `main.dol` by SHA-1
  `08e0bf20134dfcb260699671004527b2d6bb1a45`.
- The 20XX source includes assembly, code-injection definitions, notes, and `GALE01.map`.
- In v5, `AI_Engine.bin` is loaded from the disc filesystem during matches. The
  loader source names a hook at `0x8016e91c` within `StartMelee` and patches the
  AI hook at `0x8006b008`. These are runtime addresses, not ISO file offsets.
  Inspect both the executable and this external binary when analyzing training AI.

Source locations:

- `reference/20xx-hack-pack/Installer Package Files/ReadMe and MD5s.txt`
- `reference/20xx-hack-pack/20XXHP 5.0 Code Library/Core Engines.txt`
- `reference/20xx-hack-pack/Notes & Source Codes/Source Codes/20XX AI Engine Loader.asm`
- `reference/melee/README.md`
- `reference/melee/libs/dolphin/include/dolphin/dolformat.h`

## Verified tooling

- `make check`: Rust formatting, Clippy with warnings denied, and three parser tests pass.
- `cargo check -p gc-disc --target wasm32-unknown-unknown`: passes.
- Synthetic CLI integration: extraction bytes match the fixture exactly;
  repeated extraction refuses to overwrite it.
- Patch builder rejects an incorrect base checksum before producing an output.
- Installed native macOS `xdelta3` through Homebrew (`xdelta` 3.2.2).
- Disc images, extracted binaries, downloads, and reference checkouts are ignored by Git.

## Port start — 2026-10-18 (later session)

User decisions: hand-port game logic to Rust; scope is tournament-style play
(all characters and stages, no items, online). Plan, fidelity rules, and
20XX-vs-vanilla findings: [porting-plan.md](porting-plan.md).

Observed this session:

- The decomp marks all 1,117 objects `Matching`; 985 of 987 of its game and
  HSD C files parse for a little-endian ILP32 target.
- `gc-disc` reads the disc FST: 1,741 files on the 20XX disc, with
  `AI_Engine.bin` at the offset recorded above. 933 of 935 non-animation
  archives across both discs parse with `hsd-dat`. The 33 `Pl*AJ.dat` failures
  are expected: these are banks of concatenated archives, loaded in slices.
  `TyMnInfo.dat`/`.usd` (trophy data) have unaligned relocations, which
  the strict parser rejects.
- The vanilla→20XX DOL change map by decomp symbol is in
  [dol-symbol-diff.txt](dol-symbol-diff.txt): 410 functions, 613 objects, and
  616 ranges outside any symbol, totaling 77,566 bytes in loaded sections.
  The other 19 of the earlier 77,585 are unused DOL header bytes `0xE4`–`0xF7`.
- 20XX includes UCF v0.8 (`Mechanics & HUD/Gameplay.txt`).
- Fighter archives are identical between discs except `PlCa.dat`, and many
  tournament stage files are replaced by 20XX alternates. Details in the plan.
- `ftCommonData` and Fox's `ftCo_DatAttrs` load with exact values from both
  discs. All 14 decomp `@datvalue` annotations and published Fox values match.
- The decomp annotates `ftCommonData.x808` as `+804`; its name and layout
  give `+808`.
- Ported and unit-tested: `HSD_Rand`/`Randf`/`Randi` and 28 movement helpers
  from `ftcommon.c`. Their machine code has no fused multiply-adds (audited
  with `dol-inspect`). The port crates build for `wasm32-unknown-unknown`.

Nothing renders or plays yet; there is no simulation loop and no multiplayer.

## Character data, web inspector, CI — 2026-10-18 (later)

Observed:

- Every tournament character's special attributes (`ftData::ext_attr`, 20
  layouts covering 26 characters plus Nana) are generated. Each size matches
  the copy loop in that character's `*_Init_LoadSpecialAttrs` in the 1.02
  DOL. Clones call their original's copy function.
- A plausibility scan of the loaded values found five decomp header type
  errors, now corrected by generator overrides with recorded evidence.
  `ftPurinAttributes` begins with the multi-jump table (`Fighter_x2D0_t`):
  `x0` is an int (12) and `x14`–`x24` are floats (1.65, 1.59, 1.47, 1.36, 1.25).
  Loads through `fp->x2D0` in `ftCo_800D74A4`/`ftCo_800D730C` confirm this.
  Unused fields were typed by their contents: Falcon `x68` (int 2),
  Ice Climbers `x1C` (1.5f), Zelda `x28`/`x30` (2.0f, 0.6f).
- `ftPe_DatAttrs.speciallw_item_table` is annotated `+1C` in the decomp; the
  layout and the following annotation give `+18`.
- **Sound affects gameplay RNG.** `ft_PlaySFX` calls `HSD_Randi(200)` for
  sound IDs 332–370 (after a per-fighter remap in `ft_80087D0C`), and
  `lbaudio_ax.c` calls `HSD_Randi` elsewhere. Sound dispatch must run inside
  the deterministic simulation, which emits events for the browser to play.
  Until the remap is ported, the port records sound requests without
  touching the RNG.
- Ported `ftCommon_CheckFallFast` and `ft_80084DB0`, the shared airborne
  physics callback (fast-fall check, gravity, drift). In `Fighter_procUpdate`,
  `self_vel += self_accel` is a `PSVECAdd`, whose per-lane single rounding
  equals `f32` adds.
- Browser inspector deployed at https://20xx-rust.champtoday.workers.dev
  (Cloudflare Workers static assets). It reads the chosen ISO locally with
  `File.slice`, identifies vanilla vs 20XX by `AI_Engine.bin`, shows every
  fighter's attributes, and plots the ported Fall physics. Verified in a
  browser against both local discs. The WebAssembly module is 573 KB
  (100 KB gzipped).
- GitHub: private repo `nrgapple/20xx-rust`. CI runs `make check` (disc tests
  skip without an ISO) and builds the site. Deploys from `main` need the
  `CLOUDFLARE_API_TOKEN` secret.

## Next step

Load stage collision data (`map_coll`) and port the `mpLib` line queries.
Then start the HSD animation and joint system, which ECBs and hitboxes depend
on. See phase 2 of the porting plan.
