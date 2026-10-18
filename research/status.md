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

## Next research step

Map the remaining 20XX injection sites to named base-game functions and analyze
the extracted AI engine. Select one bounded subsystem for a Rust behavior port
and comparison with the reference game. No game simulation or multiplayer exists yet.
