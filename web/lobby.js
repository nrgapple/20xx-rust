import { MAX_PLAYERS, ROOM_ID } from "./protocol.js";
const $ = id => document.getElementById(id);
let socket, seat, heartbeat, lastPong = 0;
const menu = $("menu");
function showMenu() { if (!menu.open) menu.showModal(); }
function fail(message) { $("error").textContent = message; $("error").hidden = false; showMenu(); }
function roomId() { return new URL(location.href).searchParams.get("room"); }
function inviteURL(id) {
  const url = new URL("/", location.origin); url.searchParams.set("room", id); return url.href;
}
function renderSeats(players = []) {
  $("seats").replaceChildren(...Array.from({ length: MAX_PLAYERS }, (_, i) => {
    const n = i + 1, connected = players.some(p => p.seat === n), el = document.createElement("span");
    el.className = `seat${connected ? " connected" : ""}${n === seat ? " self" : ""}`;
    el.textContent = `P${n}`;
    el.style.setProperty("--seat-color", ["#ff5968", "#64b9ff", "#ffcf66", "#70dda4"][i]);
    el.setAttribute("aria-label", `Player ${n}: ${connected ? n === seat ? "you" : "connected" : "empty"}`);
    return el;
  }));
}
function disconnect() {
  clearInterval(heartbeat);
  const old = socket; socket = null; seat = null; old?.close(1000, "Left lobby"); renderSeats();
}
function connect(id) {
  if (!ROOM_ID.test(id)) { fail("Invalid invitation."); return; }
  disconnect(); $("error").hidden = true;
  $("join").hidden = true; $("create").hidden = true;
  $("leave").hidden = false; $("copy-room").hidden = false;
  $("room-status").textContent = "Connecting…";
  const url = new URL(`/api/rooms/${id}`, location.origin); url.protocol = location.protocol === "https:" ? "wss:" : "ws:";
  const current = socket = new WebSocket(url);
  current.addEventListener("open", () => {
    if (socket !== current) return;
    lastPong = Date.now(); heartbeat = setInterval(() => {
      if (Date.now() - lastPong > 65000) { current.close(4000, "Connection timed out"); return; }
      if (current.readyState === WebSocket.OPEN) current.send("ping");
    }, 20000);
  });
  current.addEventListener("message", event => {
    if (socket !== current) return;
    if (event.data === "pong") { lastPong = Date.now(); return; }
    try {
      const message = JSON.parse(event.data);
      if (message.type === "welcome") seat = message.seat;
      if (message.type === "room") { renderSeats(message.players); $("room-status").textContent = `${message.players.length} / ${MAX_PLAYERS} connected`; }
    } catch { fail("Invalid lobby response. Rejoin to retry."); }
  });
  current.addEventListener("close", () => {
    if (socket !== current) return;
    disconnect(); $("room-status").textContent = "Disconnected — rejoin to retry"; $("join").hidden = false;
  });
  current.addEventListener("error", () => { if (socket === current) $("room-status").textContent = "Lobby unavailable or full"; });
}
$("menu-toggle").addEventListener("click", showMenu);
$("menu-close").addEventListener("click", () => menu.close());
menu.addEventListener("click", e => { if (e.target === menu) { const r = menu.getBoundingClientRect(); if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) menu.close(); } });
window.addEventListener("keydown", e => { if (e.key === "Escape" && !menu.open) { e.preventDefault(); showMenu(); } });
$("create").addEventListener("click", () => { const id = crypto.randomUUID().replaceAll("-", ""); history.replaceState(null, "", inviteURL(id)); connect(id); });
$("join").addEventListener("click", () => connect(roomId()));
$("leave").addEventListener("click", () => {
  disconnect(); history.replaceState(null, "", "/");
  for (const id of ["leave", "join", "invite", "copy-room", "feedback", "error"]) $(id).hidden = true;
  $("create").hidden = false; $("room-status").textContent = "Offline";
});
$("copy-room").addEventListener("click", async () => {
  const link = inviteURL(roomId()); $("invite").value = link;
  try { await navigator.clipboard.writeText(link); $("feedback").textContent = "Invite copied"; }
  catch { $("invite").hidden = false; $("invite").focus(); $("invite").select(); $("feedback").textContent = "Copy this invitation"; }
  $("feedback").hidden = false;
});
$("fullscreen").hidden = !document.fullscreenEnabled;
$("fullscreen").addEventListener("click", async () => {
  try { if (document.fullscreenElement) await document.exitFullscreen(); else await document.documentElement.requestFullscreen(); menu.close(); }
  catch { fail("Fullscreen is unavailable in this browser."); }
});
document.addEventListener("fullscreenchange", () => { $("fullscreen").textContent = document.fullscreenElement ? "Exit fullscreen" : "Fullscreen"; });
window.addEventListener("pagehide", disconnect);
window.addEventListener("pageshow", e => { if (e.persisted && roomId()) connect(roomId()); });
renderSeats();
if (roomId()) { showMenu(); connect(roomId()); }
