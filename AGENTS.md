# 20XX Rust research workspace

- Target Super Smash Bros. Melee 20XX Hack Pack 5.0.2, based on NTSC-U Melee 1.02 (GALE01, revision byte 2).
- Product target: a browser website where friends play faithful Melee with low perceived input latency. Target Rust/WebAssembly and deterministic simulation with rollback; keep simulation independent of rendering, audio, browser APIs, and transport.
- Treat WebRTC peer transport and private friend-room signaling as proposed architecture until feasibility is measured. Do not promise latency targets without browser/device/network measurements.
- Keep original discs, extracted executables, patches, assets, and downloaded reference checkouts out of Git. Preserve originals; write derived files to new paths.
- Record acquisition URLs, upstream commits, hashes, and observed findings in research/. Distinguish observations from proposed port behavior.
- Reference checkouts are research inputs. Read their instructions before editing them; do not silently modify upstream checkouts.
- Start with binary inspection and a small, evidenced Rust subsystem. Do not claim that an emulator launch or scaffolding is a native Rust port.
- Run `make check` after Rust changes. Build outputs and downloads stay on the external SSD.
- No Mistakes is disabled by workspace instructions. Use normal review and relevant checks.
