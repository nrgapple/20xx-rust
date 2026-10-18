# 20XX Rust

Research and initial Rust tooling for **Super Smash Bros. Melee 20XX Hack Pack
5.0.2**, based on **Melee NTSC-U 1.02 / GALE01**. This is the Melee training mod.

The product target is a **website where friends can play faithful Melee online
with responsive controls**. The proposed direction is Rust compiled to WebAssembly,
deterministic simulation, and rollback netcode. Rendering, audio, browser input,
and networking should stay outside the simulation core.

The workspace starts with a disc inspector and executable extractor. A native
Rust game port has not been implemented yet. See [research/status.md](research/status.md)
for verified acquisition and binary findings.

The verified game image is `originals/20xx-hp-5.0.2.iso`. Both the original Melee
image and patched 20XX image match the full-disc checksums published with the patch.

## Layout

- `crates/gc-disc`: dependency-free Rust reader for GameCube disc and DOL headers.
- `tools/gc-inspect`: CLI to inspect ISO/GCM images and extract their main DOL.
- `tools/build_20xx.py`: checksum-verified builder using the official xdelta patch.
- `research`: acquisition records, findings, and the next porting steps.
- `originals`: ignored disc images, patches, and extracted executables.
- `reference`: ignored checkouts of the 20XX source library and Melee decompilation.

The normal entry point is `~/Projects/20xx-rust`; files are stored on the external SSD.
No remote repository or hosted service is configured.

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
```

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
