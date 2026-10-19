import { MAX_PLAYERS, PROFILE, ROOM_ID, parseTrials, trialLink, validTrial } from "./protocol.js";

const $ = id => document.getElementById(id);
let wasm, socket, seat, ready = false, heartbeat, lastPong = 0;
let displayedTrials = [], roomPlayers = [];
const decoder = new TextDecoder();
const colors = ["#ff5968", "#64b9ff", "#ffcf66", "#70dda4"];
function fail(error) {
  $("error").textContent = error.message ?? String(error);
  $("error").hidden = false;
}
function call(name, ...args) {
  const status = wasm[name](...args);
  const text = decoder.decode(new Uint8Array(wasm.memory.buffer, wasm.output_ptr(), wasm.output_len()));
  if (status !== 0) throw new Error(text);
  return JSON.parse(text);
}
function settings() {
  const trial = { profile: PROFILE, stick: Number($("stick").value), flick: Number($("flick").value), frames: Number($("frames").value) };
  if (!$("flick").value || !$("frames").value || !validTrial(trial)) {
    throw new Error("Use 10–600 whole frames and a fast-fall frame from −1 to 599.");
  }
  return trial;
}
function render(trials) {
  const runs = trials.map(t => call("simulate_fall", t.stick, t.flick, t.frames));
  displayedTrials = trials;
  $("error").hidden = true;
  $("stick-out").textContent = Number($("stick").value).toFixed(2);
  $("readout").textContent = runs.map((r, i) => {
    const n = r.x.length - 1;
    return `${runs.length > 1 ? `Player ${trials[i].seat ?? i + 1}` : "Preview"}: ${n + 1} frames · x ${r.x[n].toFixed(4)} · y ${r.y[n].toFixed(4)} · vx ${r.vx[n].toFixed(4)} · vy ${r.vy[n].toFixed(4)}`;
  }).join("\n");
  $("copy-trial").disabled = false;
  // A stale exported link should never silently refer to a different plot.
  $("experiment").hidden = true;
  const canvas = $("plot"), ctx = canvas.getContext("2d"), pad = 40;
  const { width, height } = canvas;
  let minX = 0, maxX = 1, minY = -1, maxY = 0;
  runs.forEach(r => {
    minX = Math.min(minX, ...r.x); maxX = Math.max(maxX, ...r.x);
    minY = Math.min(minY, ...r.y); maxY = Math.max(maxY, ...r.y);
  });
  const scale = Math.min((width - 2 * pad) / (maxX - minX), (height - 2 * pad) / (maxY - minY));
  const x = v => pad + (v - minX) * scale, y = v => pad + (maxY - v) * scale;
  ctx.clearRect(0, 0, width, height);
  ctx.strokeStyle = "#2b3040"; ctx.lineWidth = 1;
  ctx.beginPath(); ctx.moveTo(pad, y(0)); ctx.lineTo(width - pad, y(0));
  ctx.moveTo(x(0), pad); ctx.lineTo(x(0), height - pad); ctx.stroke();
  ctx.font = "14px system-ui";
  runs.forEach((r, i) => {
    ctx.strokeStyle = ctx.fillStyle = colors[i]; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.moveTo(x(0), y(0));
    r.x.forEach((v, frame) => ctx.lineTo(x(v), y(r.y[frame]))); ctx.stroke();
    r.x.forEach((v, frame) => {
      ctx.beginPath(); ctx.arc(x(v), y(r.y[frame]), r.fast[frame] ? 2.5 : 1.4, 0, Math.PI * 2); ctx.fill();
    });
    ctx.fillText(`${runs.length > 1 ? `Player ${trials[i].seat ?? i + 1}` : "Preview"} · stick ${trials[i].stick} · fast fall ${trials[i].flick}`, width - 320, 70 + i * 24);
  });
}
async function copyLink(text, field) {
  $(field).value = text;
  $(field).hidden = false;
  try {
    await navigator.clipboard.writeText(text);
    $("feedback").textContent = "Link copied. Send it to a friend.";
  } catch {
    $(field).focus(); $(field).select();
    $("feedback").textContent = "Select and copy the link above.";
  }
}
function roomId() { return new URL(location.href).searchParams.get("room"); }
function inviteURL(id) {
  const url = new URL(location.pathname, location.origin);
  url.searchParams.set("room", id);
  return url.href;
}
function updateReady(players = roomPlayers) {
  roomPlayers = players;
  ready = players.find(player => player.seat === seat)?.ready ?? false;
  $("ready").disabled = !wasm || socket?.readyState !== WebSocket.OPEN || !seat;
  $("ready").textContent = ready ? "Cancel ready" : "Ready for physics test";
  $("seats").replaceChildren(...Array.from({ length: MAX_PLAYERS }, (_, i) => i + 1).map(number => {
    const player = players.find(p => p.seat === number);
    const card = document.createElement("div"), title = document.createElement("b"), status = document.createElement("span");
    title.textContent = `Player ${number}${number === seat ? " · you" : ""}`;
    status.textContent = !player ? "Waiting for a friend" : player.ready ? "Ready" : "Connected";
    card.append(title, status); return card;
  }));
}
function disconnect() {
  clearInterval(heartbeat);
  const old = socket; socket = null; seat = null; ready = false;
  old?.close(1000, "Left lobby");
  updateReady([]);
}
function connect(id) {
  if (!ROOM_ID.test(id)) { fail(new Error("This invitation link is invalid.")); return; }
  disconnect();
  $("error").hidden = true;
  $("join").hidden = true; $("create").hidden = true;
  $("leave").hidden = false; $("copy-room").hidden = false;
  $("invite").value = inviteURL(id); $("invite").hidden = false;
  $("room-status").textContent = "Connecting to the lobby…";
  const url = new URL(`/api/rooms/${id}`, location.origin);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  const current = socket = new WebSocket(url);
  current.addEventListener("open", () => {
    if (socket !== current) return;
    lastPong = Date.now();
    heartbeat = setInterval(() => {
      if (Date.now() - lastPong > 65000) { current.close(4000, "Connection timed out"); return; }
      if (current.readyState === WebSocket.OPEN) current.send("ping");
    }, 20000);
  });
  current.addEventListener("message", event => {
    if (socket !== current) return;
    if (event.data === "pong") { lastPong = Date.now(); return; }
    try {
      const message = JSON.parse(event.data);
      if (message.type === "welcome") {
        seat = message.seat;
        $("room-status").textContent = `Connected as player ${seat}. Share the invitation to fill the other seats.`;
      } else if (message.type === "room") {
        updateReady(message.players);
        if (message.players.length >= 2) $("room-status").textContent = `${message.players.length} of ${MAX_PLAYERS} players connected. All connected players must ready up to compare inputs.`;
        else $("room-status").textContent = "Waiting for a friend. Share the invitation link.";
      } else if (message.type === "trial") {
        if (!wasm) throw new Error("The engine is still loading; ready up again once it is available.");
        render(message.trials);
        $("feedback").textContent = "Shared experiment complete. All connected browsers received the same inputs. Copy the experiment link to replay it.";
      }
    } catch (error) { fail(error); }
  });
  current.addEventListener("close", () => {
    if (socket !== current) return;
    disconnect();
    $("room-status").textContent = "Disconnected. The lobby may be full, or the connection was lost. Join again to retry.";
    $("join").hidden = false;
  });
  current.addEventListener("error", () => {
    if (socket === current) $("room-status").textContent = "Could not connect. Check your network or try again shortly.";
  });
}
$("create").addEventListener("click", () => {
  const id = crypto.randomUUID().replaceAll("-", "");
  history.replaceState(null, "", inviteURL(id)); connect(id);
});
$("join").addEventListener("click", () => connect(roomId()));
$("leave").addEventListener("click", () => {
  disconnect(); history.replaceState(null, "", location.pathname);
  for (const id of ["leave", "join", "invite", "copy-room"]) $(id).hidden = true;
  $("create").hidden = false; $("room-status").textContent = "You left the lobby.";
});
$("copy-room").addEventListener("click", () => copyLink(inviteURL(roomId()), "invite"));
$("copy-trial").addEventListener("click", () => copyLink(trialLink(displayedTrials, location.href), "experiment"));
$("ready").addEventListener("click", () => {
  try {
    if (socket?.readyState !== WebSocket.OPEN) throw new Error("Join a lobby first.");
    socket.send(JSON.stringify(ready ? { type: "unready" } : { type: "ready", trial: settings() }));
    $("ready").disabled = true;
  } catch (error) { fail(error); }
});
for (const id of ["stick", "flick", "frames"]) $(id).addEventListener("input", () => {
  if (ready && socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type: "unready" }));
  if (wasm) { try { render([settings()]); } catch (error) { fail(error); } }
});
window.addEventListener("pagehide", disconnect);
window.addEventListener("pageshow", event => {
  if (event.persisted && roomId()) { $("join").hidden = false; $("room-status").textContent = "Join again to reconnect."; }
});
async function main() {
  const id = roomId();
  if (id) {
    $("join").hidden = false;
    $("room-status").textContent = "You have an invitation. Join the lobby when you’re ready.";
    if (!ROOM_ID.test(id)) { $("join").hidden = true; fail(new Error("This invitation link is invalid. You can create a new lobby.")); }
  }
  const { instance } = await WebAssembly.instantiateStreaming(fetch("melee_web.wasm"), {});
  wasm = instance.exports; call("load_fixture");
  let trials, linkError;
  try { trials = parseTrials(location.hash); } catch (error) { linkError = error; }
  render(trials ?? [settings()]);
  if (linkError) fail(linkError);
  else if (id && !ROOM_ID.test(id)) fail(new Error("This invitation link is invalid. You can create a new lobby."));
  if (trials) {
    $("stick").value = trials[0].stick; $("flick").value = trials[0].flick; $("frames").value = trials[0].frames;
    $("stick-out").textContent = trials[0].stick.toFixed(2);
    $("feedback").textContent = "Loaded the shared experiment.";
  }
  updateReady();
}
main().catch(error => { $("readout").textContent = "The engine could not load. Reload to retry."; fail(error); });
