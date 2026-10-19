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

The public entrance requires **no ROM, ISO, original disc, emulator, or upload**.
It displays original Fox, Mario, Marth, Captain Falcon and Final Destination
model assets converted by Rust. The viewer shows static stored poses with
simplified materials; animation and faithful rendering are unfinished. It also
runs the ported air physics using a small numeric test fixture. Create a lobby, copy the invitation, and have a friend join. Up to four
players can join and ready up to compare their chosen inputs; both browsers receive the
same experiments. Copy an experiment link to replay those inputs later
without a server-side recording.

**This is an engine preview, not playable Melee.** There is no complete match
simulation, combat, faithful renderer, audio, UCF 0.84 integration, or rollback yet.
The fixture is not a complete fighter or game content pack. See
[the delivery plan](research/browser-target.md) for remaining work.

The optional [research inspector](https://20xx-rust.champtoday.workers.dev/inspector.html)
still reads a developer's local disc with `File.slice`; nothing is uploaded.
It is separate from the ROM-free player entrance.

Sources are in `web/`, `server/`, and `crates/melee-web`. `npm ci` installs pinned
web tooling. `make web` builds `web/dist`; `make serve` runs the Worker and lobby
service on the host at port 8787, bound to `0.0.0.0`. That is **host verification**,
not remote access. `make web-check` builds and tests the site and real WebSocket
flows against a local Worker. `make deploy` updates the existing public HTTPS
site using Wrangler. Only derived meshes/textures enter the graphics pack; no ROM or source archive is shipped.

### Deployment

`.github/workflows/ci.yml` runs `make check`, builds the site, and tests the lobby service for every
push and pull request. Pushes to `main` then deploy to Cloudflare through the
`production` GitHub environment. This needs:

- repository variable `CLOUDFLARE_ACCOUNT_ID` (already set);
- repository secret `CLOUDFLARE_API_TOKEN`: a Cloudflare API token created
  from the "Edit Cloudflare Workers" template, saved with
  `gh secret set CLOUDFLARE_API_TOKEN`.

## Layout

- `crates/gc-disc`: dependency-free reader for GameCube discs (header, DOL, FST files).
- `crates/hsd-dat`: reader for HAL HSD `.dat` archives (relocations, symbols, big-endian fields).
- `crates/ppc-asm`: partial Gekko disassembler for porting audits (floating-point ops in full).
- `crates/melee`: the Rust port. Module layout mirrors the decomp.
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
The repository is hosted privately at https://github.com/nrgapple/20xx-rust.

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
