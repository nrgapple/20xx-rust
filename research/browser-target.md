# Browser multiplayer target

User direction, 2026-10-18: port Melee/20XX to a website where friends can play
online with extremely low latency.

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
- Keep game assets as local inputs during research. Website hosting and the game
  runtime are separate from distributing a disc image.

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

No website, signaling service, room system, or public exposure has been created.
