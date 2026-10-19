# 20XX Rust

Research and initial Rust tooling for **Super Smash Bros. Melee 20XX Hack Pack
5.0.2**, based on **Melee NTSC-U 1.02 / GALE01**. This is the Melee training mod.

The product target is a **website where friends can play faithful Melee online
with responsive controls**. The proposed direction is Rust compiled to WebAssembly,
deterministic simulation, and rollback netcode. Rendering, audio, browser input,
and networking should stay outside the simulation core.

Game logic is being hand-ported to Rust from the matching Melee decompilation,
function by function. The scope is tournament-style play: all characters and
stages, no items, online. Disc, archive, and executable tooling works. The
first fighter data and movement code is ported and tested, but nothing is
playable yet. See [research/porting-plan.md](research/porting-plan.md) for the
plan and fidelity rules, and [research/status.md](research/status.md) for
verified findings.

The verified game image is `originals/20xx-hp-5.0.2.iso`. Both the original Melee
image and patched 20XX image match the full-disc checksums published with the patch.

## Website

**Public HTTPS:** https://20xx-rust.champtoday.workers.dev

The public entrance opens directly to a full-window scene, with a compact menu
for lobby invitations, view selection, camera reset, and fullscreen. There is
no landing page, research navigation, numeric dashboard, or ROM upload.
Original Fox, Mario, Marth, Captain Falcon, and Final Destination assets are
converted by Rust and loaded from a prebuilt content pack.

**This is still an incomplete port, not playable Melee.** Characters now run original idle animations through the Rust/WASM track and
joint interpreter, with simplified rendering materials. Combat, complete match simulation,
faithful rendering/audio, UCF 0.84, and rollback remain unfinished. Four-seat
lobbies provide presence and invitation links; they do not run multiplayer combat.
The Rust physics fixture and experiment protocol remain available for automated
engine tests, but are not exposed in the player interface.

The research inspector sources remain in `web/inspector.html` and `web/app.js`
for developer use and are excluded from the public build.

`npm ci` installs pinned tooling. `make web` builds the site; `make web-check`
builds and tests it. `make serve` binds the local Worker to `0.0.0.0:8787` for
host verification, not remote access. `make deploy` updates the existing public
HTTPS site. No ROM or source archive is shipped.

### Deployment

`.github/workflows/ci.yml` runs `make check`, builds the site, and tests the lobby service for every
push to main and pull request. The `check` job is required before merging.
Auto-merge is enabled, and ready pull requests from branches in this repository
automatically opt in through `.github/workflows/auto-merge.yml`. External forks
and drafts are not automatically opted in. Pushes to `main` then deploy to Cloudflare through the
`production` GitHub environment. This needs:

- repository variable `CLOUDFLARE_ACCOUNT_ID` (already set);
- repository secret `CLOUDFLARE_API_TOKEN`: a Cloudflare API token created
  from the "Edit Cloudflare Workers" template, saved with
  `gh secret set CLOUDFLARE_API_TOKEN`.

## Layout

- `crates/gc-disc`: dependency-free reader for GameCube discs (header, DOL, FST files).
- `crates/hsd-dat`: reader for HAL HSD `.dat` archives (relocations, symbols, big-endian fields).
- `crates/ppc-asm`: partial Gekko disassembler for porting audits (floating-point ops in full).
- `crates/melee`: the Rust port, including original animation clocks/tracks/joints and audited game math. Module layout mirrors the decomp.
- `crates/melee-web`: WebAssembly exports for the ROM-free preview and optional inspector.
- `web`: the public lobby/preview and optional research inspector.
- `server`: Cloudflare Worker and four-seat Durable Object rooms.
- `tests`: ROM-free WASM, invitation protocol, and WebSocket integration tests.
- `tools/scene-export`: offline Rust conversion of original HSD meshes and textures.
- `tools/content-pack.py`: checksum-verified runtime graphics packaging (local or published pack).
- `tools/gc-inspect`: CLI to inspect ISO/GCM images, list files, and extract the DOL or files.
- `tools/dat-inspect`: validate every archive on a disc, or list one archive's symbols.
- `tools/dol-inspect`: disassemble functions by decomp symbol; diff two DOLs by symbol.
- `tools/gen_dat_structs.py`: generate offset-checked Rust data layouts from decomp C structs.
- `tools/gen_ext_attrs.sh`: regenerate every character's special-attribute layout (with type corrections).
- `tools/attr_access.py`: infer attribute field types from how 1.02 machine code loads them.
- `tools/build_20xx.py`: checksum-verified builder using the official xdelta patch.
- `research`: acquisition records, findings, and the next porting steps.
- `originals`: ignored disc images, patches, and extracted executables.
- `reference`: ignored checkouts of the 20XX source library and Melee decompilation.

The normal entry point is `~/Projects/20xx-rust`; files are stored on the external SSD.
The public repository is hosted at https://github.com/nrgapple/20xx-rust.

## Claude Remote Control

The remote environment is named `20xx-rust` and runs in the detached tmux session
`20xx-remote`, rooted in this repository. Open it from Claude Code on the web or
the Claude mobile app. The tmux session survives terminal disconnects; the host
and Claude process must remain running.

```sh
tmux attach -t 20xx-remote
# If the session is no longer running:
tmux new-session -d -s 20xx-remote -c "$HOME/Projects/20xx-rust" 'exec /opt/homebrew/bin/claude remote-control --name 20xx-rust --spawn=same-dir'
```

`CLAUDE.md` imports this repository's `AGENTS.md` and points to the current research state.

## Commands

Rust is installed at `~/.cargo/bin`; the Makefile uses it directly. Add that
directory to `PATH` to run the Cargo commands below.

```sh
make build
make check  # formatting, Clippy, parser tests
make patch  # requires the base ISO, downloaded patch, Python 3, and xdelta3
make inspect DISC=originals/20xx-hp-5.0.2.iso
cargo run -p gc-inspect -- originals/melee-ntsc-1.02.iso --extract-dol originals/melee-1.02.dol
cargo run -p gc-inspect -- originals/20xx-hp-5.0.2.iso --extract-dol originals/20xx-5.0.2.dol
cargo run -p gc-inspect -- originals/20xx-hp-5.0.2.iso --list
cargo run -p dat-inspect -- originals/melee-ntsc-1.02.iso PlFx.dat
cargo run -p dol-inspect -- originals/melee-1.02.dol reference/melee/config/GALE01/symbols.txt ftCommon_Fall
cargo run -p dol-inspect -- --diff originals/melee-1.02.dol originals/20xx-5.0.2.dol reference/melee/config/GALE01/symbols.txt
cargo run -p melee --example fighter_attrs -- originals/melee-ntsc-1.02.iso PlFx.dat ftDataFox
cargo run -p melee --example ext_attrs -- originals/melee-ntsc-1.02.iso PlPr.dat ftDataPurin
tools/attr_access.py ftPr_            # how Jigglypuff's code reads its attributes
```

`make check` also runs `crates/melee/tests/disc_data.rs`, which loads real data
from `originals/20xx-hp-5.0.2.iso` (or `MELEE_ISO`). Without a disc those tests
print a note and pass.

The inspector supports uncompressed GameCube ISO/GCM files, checks section bounds,
and refuses to overwrite an extraction. It does not run the game or convert RVZ,
CISO, NKit, or other compressed disc formats.

## Source references

- [Official 20XX source library](https://github.com/DRGN-DRC/20XX-HACK-PACK)
- [Official 5.0.2 patch release](https://github.com/DRGN-DRC/20XX-HACK-PACK/releases/tag/v5.0.2)
- [Existing Melee decompilation](https://github.com/doldecomp/melee)

Pinned upstream commits and locally computed download hashes are recorded in
`research/acquisition.json`. The existing Melee decompilation provides a major
reference for the base game; 20XX-specific assembly and runtime-loaded code still
need analysis. A matching PowerPC rebuild and a native Rust port are separate goals.

Graphics and animation details, measured coverage, and known limitations: [research/notes/graphics-and-animation.md](research/notes/graphics-and-animation.md).
