import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import WebSocket from "ws";
import { PROFILE, parseTrials, trialLink, validTrial } from "../web/protocol.js";

const trial = { profile: PROFILE, stick: 1, flick: 20, frames: 60 };
const base = process.env.TEST_BASE_URL || "http://127.0.0.1:8791";
let server, logs = "";
const sockets = [];
before(async () => {
  if (process.env.TEST_BASE_URL) return;
  server = spawn("node_modules/.bin/wrangler", ["dev", "--ip", "127.0.0.1", "--port", "8791", "--inspector-port", "0"], {
    env: { ...process.env, WRANGLER_SEND_METRICS: "false", CI: "true" }, stdio: ["ignore", "pipe", "pipe"],
  });
  server.stdout.on("data", data => { logs += data; });
  server.stderr.on("data", data => { logs += data; });
  for (let i = 0; i < 100; i++) {
    try { if ((await fetch(`${base}/api/health`)).ok) return; } catch {}
    if (server.exitCode !== null) throw new Error(logs);
    await new Promise(resolve => setTimeout(resolve, 200));
  }
  throw new Error(`Worker did not start: ${logs}`);
});
after(async () => {
  for (const socket of sockets) socket.terminate();
  if (server && server.exitCode === null) { server.kill("SIGTERM"); await once(server, "exit"); }
});
function client(id, origin = base) {
  const ws = new WebSocket(`${base.replace(/^http/, "ws")}/api/rooms/${id}`, { origin });
  sockets.push(ws);
  const queue = [], waiters = [];
  ws.on("message", raw => {
    const message = raw.toString() === "pong" ? "pong" : JSON.parse(raw.toString());
    const index = waiters.findIndex(w => w.type === (message.type ?? message));
    if (index >= 0) { const [w] = waiters.splice(index, 1); clearTimeout(w.timer); w.resolve(message); }
    else queue.push(message);
  });
  ws.next = type => {
    const index = queue.findIndex(m => (m.type ?? m) === type);
    if (index >= 0) return Promise.resolve(queue.splice(index, 1)[0]);
    return new Promise((resolve, reject) => {
      const waiter = { type, resolve, timer: setTimeout(() => reject(new Error(`Timed out: ${type}`)), 5000) };
      waiters.push(waiter);
    });
  };
  return ws;
}
function room() { return crypto.randomUUID().replaceAll("-", ""); }
async function rejectConnection(id, origin, status) {
  const ws = client(id, origin);
  // ws emits an error after an unsuccessful upgrade unless handled.
  ws.on("error", () => {});
  const [, response] = await once(ws, "unexpected-response");
  assert.equal(response.statusCode, status);
  ws.terminate();
}

test("the public entrance and WASM load with no disc", async () => {
  const response = await fetch(base);
  assert.equal(response.status, 200);
  const html = await response.text();
  assert.match(html, /<canvas id="scene"/);
  assert.match(html, /<dialog id="menu"/);
  assert.doesNotMatch(html, /<header|<footer|id="plot"|Research inspector|The route to/);
  assert.doesNotMatch(html, /type="file"/);
  const wasm = await fetch(`${base}/melee_web.wasm`);
  assert.equal(wasm.headers.get("content-type"), "application/wasm");
  const { instance } = await WebAssembly.instantiate(await wasm.arrayBuffer());
  const engine = instance.exports;
  const output = () => JSON.parse(new TextDecoder().decode(new Uint8Array(engine.memory.buffer, engine.output_ptr(), engine.output_len())));
  assert.equal(engine.load_fixture(), 0);
  assert.equal(engine.simulate_fall(1, 20, 60), 0);
  const first = output();
  assert.equal(first.vy[19], -2.8);
  assert.equal(first.vy[20], -3.4);
  assert.equal(first.fast[19], 0);
  assert.equal(first.fast[20], 1);
  assert.ok(first.x[59] > 0);
  assert.equal(engine.simulate_fall(1, 20, 60), 0);
  assert.deepEqual(output(), first, "the same inputs replay exactly");
  assert.equal(engine.simulate_fall(-1, -1, 60), 0);
  assert.ok(output().x[59] < 0);
  assert.equal((await fetch(`${base}/inspector.html`)).status, 404);
  assert.equal((await fetch(`${base}/app.js`)).status, 404);
});

test("experiment links round-trip and reject malformed or unsupported inputs", () => {
  const link = new URL(trialLink([trial, { ...trial, stick: -1 }], `${base}/?room=${room()}`));
  assert.equal(link.search, "");
  assert.deepEqual(parseTrials(link.hash), [trial, { ...trial, stick: -1 }]);
  assert.equal(parseTrials(""), null);
  assert.throws(() => parseTrials("#trials=oops"));
  assert.throws(() => parseTrials("#trials=[]"));
  for (const bad of [{ frames: 601 }, { frames: 1.5 }, { stick: Infinity }, { profile: "unknown" }, { flick: -2 }]) {
    assert.equal(Boolean(validTrial({ ...trial, ...bad })), false);
  }
});

test("two browsers share a room and identical trials; leaving frees the seat", async () => {
  const id = room();
  const a = client(id);
  assert.equal((await a.next("welcome")).seat, 1);
  await a.next("room");
  a.send(JSON.stringify({ type: "ready", trial }));
  assert.equal((await a.next("room")).players[0].ready, true);
  const b = client(id);
  assert.equal((await b.next("welcome")).seat, 2);
  assert.equal((await a.next("room")).players.length, 2);
  await b.next("room");
  const remoteTrial = { ...trial, stick: -1 };
  b.send(JSON.stringify({ type: "ready", trial: remoteTrial }));
  const [first, second] = await Promise.all([a.next("trial"), b.next("trial")]);
  assert.deepEqual(first, second);
  assert.deepEqual(first.trials, [{ ...trial, seat: 1 }, { ...remoteTrial, seat: 2 }]);
  assert.ok(first.id);
  assert.ok((await a.next("room")).players.every(p => !p.ready));
  await b.next("room");
  a.send("ping"); assert.equal(await a.next("pong"), "pong");
  a.send(JSON.stringify({ type: "ready", trial }));
  await a.next("room"); await b.next("room");
  b.close(); await once(b, "close");
  const left = await a.next("room");
  assert.deepEqual(left.players, [{ seat: 1, ready: false }]);
  const replacement = client(id);
  assert.equal((await replacement.next("welcome")).seat, 2);
  const isolated = client(room());
  assert.equal((await isolated.next("room")).players.length, 1);
  a.close(); replacement.close(); isolated.close();
});

test("origin, room IDs, and payload limits are enforced", async () => {
  await rejectConnection(room(), "https://unrelated.example", 403);
  await rejectConnection("invalid", base, 404);
  for (const [payload, code] of [["{", 1008], [JSON.stringify({ type: "ready", trial: { ...trial, frames: 1e9 } }), 1008], ["x".repeat(1025), 1009]]) {
    const ws = client(room()); await ws.next("welcome");
    const closed = once(ws, "close"); ws.send(payload);
    assert.equal((await closed)[0], code);
  }
});

test("readiness can be cancelled and excessive messages close the connection", async () => {
  const ws = client(room()); await ws.next("welcome"); await ws.next("room");
  ws.send(JSON.stringify({ type: "ready", trial })); await ws.next("room");
  ws.send(JSON.stringify({ type: "unready" }));
  assert.equal((await ws.next("room")).players[0].ready, false);
  const closed = once(ws, "close");
  for (let i = 0; i < 20; i++) ws.send(JSON.stringify({ type: "unready" }));
  assert.equal((await closed)[0], 1008);
});



test("four players share one experiment, a fifth is rejected, and seat gaps retain identity", async () => {
  const id = room(), players = [];
  for (let i = 0; i < 4; i++) {
    const ws = client(id); players.push(ws);
    const welcome = await ws.next("welcome");
    assert.equal(welcome.seat, i + 1); assert.equal(welcome.capacity, 4);
  }
  async function stateMatching(ws, predicate) {
    for (let i = 0; i < 30; i++) { const state = await ws.next("room"); if (predicate(state)) return state; }
    throw new Error("Room did not reach expected state");
  }
  for (const ws of players) await stateMatching(ws, s => s.players.length === 4);
  await rejectConnection(id, base, 409);
  for (let i = 0; i < 4; i++) players[i].send(JSON.stringify({ type: "ready", trial: { ...trial, stick: i / 4, seat: 4 } }));
  const results = await Promise.all(players.map(ws => ws.next("trial")));
  for (const result of results) assert.deepEqual(result, results[0]);
  assert.deepEqual(results[0].trials.map(t => t.seat), [1, 2, 3, 4]);
  assert.deepEqual(results[0].trials.map(t => t.stick), [0, .25, .5, .75]);
  const replay = trialLink(results[0].trials, base);
  assert.deepEqual(parseTrials(new URL(replay).hash), results[0].trials);
  players[1].close(); await once(players[1], "close");
  await stateMatching(players[0], s => s.players.length === 3);
  for (const ws of [players[0], players[2], players[3]]) ws.send(JSON.stringify({ type: "ready", trial }));
  const gap = await players[0].next("trial"); assert.deepEqual(gap.trials.map(t => t.seat), [1, 3, 4]);
  const replacement = client(id); assert.equal((await replacement.next("welcome")).seat, 2);
  for (const ws of [...players, replacement]) ws.close();
});

test("derived graphics content is available without originals and matches recorded hashes", async () => {
  const { createHash } = await import("node:crypto");
  const response = await fetch(`${base}/content/manifest.json`); assert.equal(response.status, 200);
  const manifest = await response.json(); assert.equal(manifest.version, 1);
  assert.deepEqual(Object.keys(manifest.assets).sort(), ["falcon", "final-destination", "fox", "mario", "marth"]);
  for (const asset of Object.values(manifest.assets)) {
    const metadataResponse = await fetch(`${base}/content/${asset.metadata}`); assert.equal(metadataResponse.status, 200);
    const metadataBytes = Buffer.from(await metadataResponse.arrayBuffer());
    assert.equal(createHash("sha256").update(metadataBytes).digest("hex"), asset.metadata_sha256);
    const metadata = JSON.parse(metadataBytes); assert.equal(metadata.version, 2); assert.ok(metadata.meshes.length > 0);
    const binaryResponse = await fetch(`${base}/content/${asset.binary}`); assert.equal(binaryResponse.status, 200);
    const bytes = Buffer.from(await binaryResponse.arrayBuffer());
    assert.equal(createHash("sha256").update(bytes).digest("hex"), asset.binary_sha256);
    for (const mesh of metadata.meshes) {
      assert.equal(mesh.count % 3, 0); assert.ok(mesh.offset + mesh.count * 32 <= bytes.length);
      for (let i = 0; i < mesh.count * 8; i++) assert.ok(Number.isFinite(bytes.readFloatLE(mesh.offset + i * 4)));
    }
    for (const tex of metadata.textures) assert.ok(tex.offset + tex.width * tex.height * 4 <= bytes.length);
  }
});

test("Rust/WASM skeletons match native pose hashes for every packaged clip", async () => {
  const { readFile } = await import("node:fs/promises");
  const { createHash } = await import("node:crypto");
  const expected = JSON.parse(await readFile(new URL("../research/animation-goldens.json", import.meta.url)));
  const { instance } = await WebAssembly.instantiate(await (await fetch(`${base}/melee_web.wasm`)).arrayBuffer());
  const e = instance.exports;
  const check = code => assert.equal(code, 0, new TextDecoder().decode(new Uint8Array(e.memory.buffer, e.output_ptr(), e.output_len())));
  const manifest = await (await fetch(`${base}/content/manifest.json`)).json();
  let slot = 0;
  for (const [id, samples] of Object.entries(expected)) {
    const asset = manifest.assets[id];
    const metadataBytes = new Uint8Array(await (await fetch(`${base}/content/${asset.metadata}`)).arrayBuffer());
    const metadata = JSON.parse(new TextDecoder().decode(metadataBytes));
    const binary = new Uint8Array(await (await fetch(`${base}/content/${asset.binary}`)).arrayBuffer());
    const mp = e.alloc(metadataBytes.length), bp = e.alloc(binary.length);
    new Uint8Array(e.memory.buffer, mp, metadataBytes.length).set(metadataBytes);
    new Uint8Array(e.memory.buffer, bp, binary.length).set(binary);
    check(e.scene_load(slot, mp, metadataBytes.length, bp, binary.length));
    e.dealloc(mp, metadataBytes.length); e.dealloc(bp, binary.length);
    let clip = -1, frame = 0;
    for (const sample of samples) {
      if (sample.clip !== clip) { clip = sample.clip; frame = 0; check(e.scene_play(slot, clip)); }
      while (frame < sample.frame) { check(e.scene_advance(slot)); frame++; }
      const bytes = new Uint8Array(e.memory.buffer, e.scene_palette_ptr(slot), metadata.joints * 24 * 4);
      assert.equal(createHash("sha256").update(bytes).digest("hex"), sample.sha256, `${id}, clip ${clip}, frame ${frame}`);
    }
    slot++;
  }
  assert.notEqual(e.scene_advance(99), 0);
  assert.notEqual(e.scene_play(0, 99999), 0);
});
