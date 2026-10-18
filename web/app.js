// Browser shell for the Rust port. All game logic runs in melee_web.wasm;
// this file only reads byte ranges of the chosen disc and draws results.

const $ = (id) => document.getElementById(id);

const HIGHLIGHTS = [
  ["walk_max_vel", "Walk speed"],
  ["dash_initial_velocity", "Initial dash"],
  ["dash_max_velocity", "Run speed"],
  ["ground_friction", "Traction"],
  ["jump_startup_time", "Jumpsquat (frames)"],
  ["jump_v_initial_velocity", "Jump velocity"],
  ["hop_v_initial_velocity", "Short hop velocity"],
  ["max_jumps", "Jumps"],
  ["gravity", "Gravity"],
  ["terminal_velocity", "Fall speed"],
  ["fast_fall_velocity", "Fast-fall speed"],
  ["air_drift_max", "Air speed"],
  ["weight", "Weight"],
];
const PIN_COLORS = ["#4fb3ff", "#59d499", "#ffc94f", "#c78bff", "#ff8f4f"];

let wasm;
const decoder = new TextDecoder();

function output() {
  const { memory, output_ptr, output_len } = wasm;
  return decoder.decode(new Uint8Array(memory.buffer, output_ptr(), output_len()));
}

// Call an export; status 0 means the output buffer holds JSON.
function call(name, ...args) {
  const status = wasm[name](...args);
  const text = output();
  if (status !== 0) throw new Error(text);
  return JSON.parse(text);
}

// Copy bytes into wasm memory for the duration of one call.
function callWithBytes(name, bytes, ...args) {
  const ptr = wasm.alloc(bytes.length);
  new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
  try {
    return call(name, ptr, bytes.length, ...args);
  } finally {
    wasm.dealloc(ptr, bytes.length);
  }
}

async function readRange(file, offset, size) {
  return new Uint8Array(await file.slice(offset, offset + size).arrayBuffer());
}

const state = { file: null, files: new Map(), fighters: [], attrs: null, pins: [] };

async function loadDisc(file) {
  $("disc-error").hidden = true;
  $("fighter").hidden = true;
  $("lab").hidden = true;
  state.file = file;
  const header = callWithBytes("disc_header", await readRange(file, 0, 0x440), file.size);
  const fst = await readRange(file, header.fst_offset, header.fst_size);
  const files = callWithBytes("disc_files", fst, file.size);
  state.files = new Map(files.map(([path, offset, size]) => [path, { offset, size }]));
  const is20xx = state.files.has("AI_Engine.bin");
  const supported = header.game_id === "GALE01" && header.revision === 2;
  const info = $("disc-info");
  info.replaceChildren(
    card("Game ID", header.game_id),
    card("Revision", `1.0${header.revision}`),
    card("Title", header.title),
    card("Files", files.length.toLocaleString()),
    card("Build", is20xx ? "20XX Hack Pack (AI_Engine.bin present)" : "Vanilla"),
    card("Supported", supported ? "Yes" : "No: NTSC 1.02 (GALE01, revision 2) only", supported ? "ok" : "error"),
  );
  info.hidden = false;
  if (!supported) return;

  callWithBytes("load_common", await readFile("PlCo.dat"));
  const select = $("fighter-select");
  select.replaceChildren(
    ...state.fighters.map(([name], index) => new Option(name, index)),
  );
  select.value = "1"; // Fox
  $("fighter").hidden = false;
  $("lab").hidden = false;
  await loadFighter(Number(select.value));
}

async function readFile(path) {
  const entry = state.files.get(path);
  if (!entry) throw new Error(`${path} is not on this disc`);
  return readRange(state.file, entry.offset, entry.size);
}

function card(label, value, cls) {
  const div = document.createElement("div");
  const b = document.createElement("b");
  b.textContent = label;
  const span = document.createElement("span");
  span.textContent = value;
  if (cls) span.className = cls;
  div.append(b, span);
  return div;
}

function format(value) {
  if (Array.isArray(value)) return value.map(format).join(", ");
  if (value && typeof value === "object") return JSON.stringify(value);
  return String(value);
}

async function loadFighter(index) {
  const [, file] = state.fighters[index];
  state.attrs = callWithBytes("load_fighter", await readFile(file), index);
  $("highlights").replaceChildren(
    ...HIGHLIGHTS.map(([key, label]) => card(label, format(state.attrs[key]))),
  );
  const rows = Object.entries(state.attrs).map(([key, value]) => {
    const tr = document.createElement("tr");
    const k = document.createElement("td");
    k.textContent = key;
    const v = document.createElement("td");
    v.textContent = format(value);
    tr.append(k, v);
    return tr;
  });
  $("attrs").tBodies[0].replaceChildren(...rows);
  simulate();
}

function currentRun() {
  const stick = Number($("stick").value);
  const flick = Number($("flick").value);
  const frames = Number($("frames").value);
  const run = call("simulate_fall", stick, Number.isFinite(flick) ? flick : -1, frames);
  const name = state.fighters[Number($("fighter-select").value)][0];
  run.label = `${name}, stick ${stick.toFixed(2)}${flick >= 0 ? `, flick @${flick}` : ""}`;
  return run;
}

function simulate() {
  $("stick-out").textContent = Number($("stick").value).toFixed(2);
  const run = currentRun();
  draw([...state.pins, { ...run, color: "#ff4f5e" }]);
  const last = run.x.length - 1;
  const ffStart = run.fast.indexOf(1);
  $("readout").textContent =
    `after ${run.x.length} frames: x ${run.x[last].toFixed(4)}, y ${run.y[last].toFixed(4)}, ` +
    `vx ${run.vx[last].toFixed(4)}, vy ${run.vy[last].toFixed(4)}` +
    (ffStart >= 0 ? `; fast fall from frame ${ffStart}` : "");
  state.current = run;
}

function draw(runs) {
  const canvas = $("plot");
  const ctx = canvas.getContext("2d");
  const { width, height } = canvas;
  const pad = 36;
  ctx.clearRect(0, 0, width, height);
  let [minX, maxX, minY, maxY] = [0, 1, -1, 0];
  for (const run of runs) {
    for (const x of run.x) { minX = Math.min(minX, x); maxX = Math.max(maxX, x); }
    for (const y of run.y) { minY = Math.min(minY, y); maxY = Math.max(maxY, y); }
  }
  const scale = Math.min((width - 2 * pad) / (maxX - minX || 1), (height - 2 * pad) / (maxY - minY || 1));
  const px = (x) => pad + (x - minX) * scale;
  const py = (y) => pad + (maxY - y) * scale;

  ctx.strokeStyle = "#262c3a";
  ctx.fillStyle = "#98a0b3";
  ctx.font = "12px ui-monospace, Menlo, monospace";
  ctx.lineWidth = 1;
  const step = 10 ** Math.floor(Math.log10(Math.max(maxX - minX, maxY - minY) / 4));
  for (let x = Math.ceil(minX / step) * step; x <= maxX; x += step) {
    ctx.beginPath(); ctx.moveTo(px(x), pad / 2); ctx.lineTo(px(x), height - pad / 2); ctx.stroke();
    ctx.fillText(x.toFixed(0), px(x) + 3, height - 6);
  }
  for (let y = Math.ceil(minY / step) * step; y <= maxY; y += step) {
    ctx.beginPath(); ctx.moveTo(pad / 2, py(y)); ctx.lineTo(width - pad / 2, py(y)); ctx.stroke();
    ctx.fillText(y.toFixed(0), 4, py(y) - 3);
  }

  runs.forEach((run, index) => {
    ctx.strokeStyle = ctx.fillStyle = run.color;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(px(0), py(0));
    run.x.forEach((x, i) => ctx.lineTo(px(x), py(run.y[i])));
    ctx.stroke();
    run.x.forEach((x, i) => {
      ctx.beginPath();
      ctx.arc(px(x), py(run.y[i]), run.fast[i] ? 2.5 : 1.5, 0, Math.PI * 2);
      ctx.fill();
    });
    ctx.fillText(run.label, width - pad - ctx.measureText(run.label).width, pad + 16 * index);
  });
}

async function main() {
  const { instance } = await WebAssembly.instantiateStreaming(fetch("melee_web.wasm"), {});
  wasm = instance.exports;
  state.fighters = call("fighters");

  $("iso").addEventListener("change", async (event) => {
    const [file] = event.target.files;
    if (!file) return;
    try {
      await loadDisc(file);
    } catch (error) {
      $("disc-error").textContent = String(error.message ?? error);
      $("disc-error").hidden = false;
    }
  });
  $("fighter-select").addEventListener("change", (e) => loadFighter(Number(e.target.value)));
  for (const id of ["stick", "flick", "frames"]) $(id).addEventListener("input", simulate);
  $("pin").addEventListener("click", () => {
    if (!state.current) return;
    state.pins.push({ ...state.current, color: PIN_COLORS[state.pins.length % PIN_COLORS.length] });
    simulate();
  });
  $("clear").addEventListener("click", () => { state.pins = []; simulate(); });
}

main().catch((error) => {
  $("disc-error").textContent = `Failed to start: ${error.message ?? error}`;
  $("disc-error").hidden = false;
});
