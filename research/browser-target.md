# Browser multiplayer target

User direction, 2026-10-18: port Melee/20XX to a website where friends can play
online with extremely low latency.

## Decided (user, 2026-10-18)

- Hand-port the game logic to Rust from the matching decomp (not recompiled C,
  not emulation). Scope is tournament-style play: all characters and stages,
  no items, online multiplayer. See [porting-plan.md](porting-plan.md).

## Proposed direction

- Compile a platform-independent Rust simulation to WebAssembly. Keep gamepad sampling,
  rendering, sound, and browser lifecycle integration outside that core.
- Run the simulation at the original game cadence. Retain original numerical behavior,
  update order, RNG, inputs, and state transitions; validate these against the reference
  game before calling the simulation faithful.
- Make complete simulation state serializable and restorable. Validate deterministic
  replay and cross-build state hashes before adding online rollback.
- Exchange frame-numbered inputs, predict missing remote inputs, and roll back and
  replay when predictions differ. This reduces the need to delay local controls;
  network round-trip time, jitter, packet loss, controller polling, audio, and display
  presentation still affect the experience.
- Investigate WebRTC data channels for peer input exchange, private room signaling,
  and TURN fallback for friends whose networks cannot connect directly. These are
  candidate choices, not an implemented or measured network stack.
- Keep original game assets as local inputs during research. The final player
  runtime must consume prebuilt versioned content with no player-side extraction.
  The complete content pipeline is not implemented yet.

## Runnable outcomes to pursue next

1. Trace one small training/gameplay subsystem using the base decompilation, the
   20XX assembly library, and verified executables; reproduce it in Rust with
   recorded input/output cases. Choose the subsystem after the binary inventory.
2. Make that subsystem interactive in a browser and compare its behavior with the
   reference. Expand to a local two-player match slice; confirm restore/replay.
3. Run the same match between two browser peers through a private friend room.
   Exercise artificial delay, jitter, loss, and resimulation; measure input-to-display
   latency and frame time on the actual target devices.

These outcomes guide incremental work. They are not a claim that the full game,
rendering engine, or multiplayer can be delivered by translating a few functions.

## Relevant existing projects

- [Melee matching decompilation](https://github.com/doldecomp/melee): base-game code
  and symbol references, already downloaded and pinned locally.
- [20XX Hack Pack](https://github.com/DRGN-DRC/20XX-HACK-PACK): mod source library,
  notes, symbol map, and official patch, already downloaded locally.
- [Slippi Dolphin](https://github.com/project-slippi/Ishiiruka): reference to inspect
  for existing Melee netplay integration. Not cloned or incorporated here.
- [OpenSmash Melee PC](https://github.com/turtlesoupy/opensmash-melee-pc): an existing
  native-port project worth evaluating before committing to a complete rewrite.
  Its published claims have not been locally validated here.

## Player delivery decision — 2026-10-18

Opening a website and sharing a lobby/match link must be enough. Players must
not supply any original ROM, ISO, executable, or local extraction. Research
binaries remain developer-only inputs. Continue the hand port: no emulation
or recompilation of the original C.

### Implemented entrance

The site at https://20xx-rust.champtoday.workers.dev now has a ROM-free physics
preview and four-seat friend lobbies. The optional disc inspector is at
`/inspector.html`. The preview initializes the eight movement values and input
thresholds already exercised by the Rust tests. A separate graphics viewer now
loads converted original models/textures for four fighters and Final Destination;
see `notes/graphics-and-animation.md` for its limitations.

Each invitation has a cryptographically random 128-bit room ID. Anyone holding
the link can join; there are no accounts or public directory. A same-origin
WebSocket connects to a Cloudflare Durable Object. Seats, readiness, and
per-connection rate counters survive hibernation via WebSocket attachments.
Rooms accept only a versioned, bounded physics fixture configuration, never
arbitrary game data. At least two connected seats, all ready, cause a shared experiment message, then
readiness resets. Disconnecting cancels the remaining player's readiness.
Heartbeat detects stale client connections. Rejoining is explicit; a refreshed
browser claims an available seat, with no reservation or identity recovery.

Experiment links encode one to four fixture configurations in the URL fragment;
they replay locally without keeping room history. They are **not match replays**.
The lobby transport is not a measured gameplay transport. WebRTC, TURN, and
rollback remain proposed.

Implementation follows Cloudflare's [hibernation API](https://developers.cloudflare.com/durable-objects/examples/websocket-hibernation-server/)
and [static asset routing](https://developers.cloudflare.com/workers/static-assets/binding/).

### Remaining delivery gates

1. **Headless match:** finish collision queries, animation/joints, script
   execution, action states, hit/hurt resolution, damage, and complete update
   order. Validate two fighters against reference traces; then expand coverage.
2. **Runtime content:** define a versioned manifest and convert/author all
   required fighter/stage parameters, animations, meshes, materials, and audio
   into build artifacts. Record provenance, hashes, and distribution status.
   A decompiled executable alone does not supply these assets. Keep source
   archives and generated full-game asset packs out of Git.
3. **Browser match:** render and play audio from that content, handle controllers,
   and validate an entire local match without reading any disc or archive.
4. **Online match:** snapshot/restore complete simulation state, negotiate exact
   engine/content/UCF versions, exchange frame inputs, detect desyncs, implement
   rollback, and measure two real devices on different networks. Bind playable
   round links to actual match IDs only when a match lifecycle exists.

Do not describe the numeric preview as completed ROM-free Melee, or the lobby
experiment protocol as combat or rollback. Web CI runs without originals or
reference checkouts; integration tests cover the public entrance and WASM,
room capacity, isolation, shared inputs, disconnect/rejoin, origin rejection,
invalid messages, and rate limits.
