import { DurableObject } from "cloudflare:workers";
import { MAX_PLAYERS, ROOM_ID, validTrial, cleanTrial } from "../web/protocol.js";

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/api/health") {
      return Response.json({ service: "20xx-lobbies", protocol: 2, capacity: MAX_PLAYERS, gameplay: false });
    }
    if (!url.pathname.startsWith("/api/")) return env.ASSETS.fetch(request);
    const match = /^\/api\/rooms\/([^/]+)$/.exec(url.pathname);
    if (!match || !ROOM_ID.test(match[1])) return new Response("Unknown room", { status: 404 });
    if (request.method !== "GET") return new Response("GET required", { status: 405 });
    if (request.headers.get("Origin") !== url.origin) return new Response("Origin rejected", { status: 403 });
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
      return new Response("WebSocket required", { status: 426 });
    }
    return env.ROOMS.getByName(match[1]).fetch(request);
  },
};

// One invitation link = one four-seat room. A link grants entry; no directory.
// Attachments keep readiness and rate limits across hibernation. No disc data,
// chat, or arbitrary peer payloads are accepted or stored.
export class Room extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair("ping", "pong"));
  }

  sockets() {
    return this.ctx.getWebSockets().filter(ws => ws.readyState === WebSocket.OPEN);
  }

  fetch() {
    const sockets = this.sockets();
    if (sockets.length >= MAX_PLAYERS) return new Response("Room full", { status: 409 });
    const taken = new Set(sockets.map(ws => ws.deserializeAttachment().seat));
    const seat = Array.from({ length: MAX_PLAYERS }, (_, i) => i + 1).find(n => !taken.has(n));
    const [client, server] = Object.values(new WebSocketPair());
    this.ctx.acceptWebSocket(server);
    server.serializeAttachment({ seat, trial: null, window: Date.now(), count: 0 });
    server.send(JSON.stringify({ type: "welcome", seat, capacity: MAX_PLAYERS }));
    this.broadcastState();
    return new Response(null, { status: 101, webSocket: client });
  }

  broadcast(message) {
    const text = JSON.stringify(message);
    for (const ws of this.sockets()) ws.send(text);
  }

  broadcastState() {
    this.broadcast({ type: "room", players: this.sockets().map(ws => {
      const { seat, trial } = ws.deserializeAttachment();
      return { seat, ready: trial !== null };
    }).sort((a, b) => a.seat - b.seat) });
  }

  webSocketMessage(ws, raw) {
    if (typeof raw !== "string" || raw.length > 1024) {
      ws.close(1009, "Message too large or not text");
      return;
    }
    const state = ws.deserializeAttachment();
    const now = Date.now();
    if (now - state.window >= 1000) { state.window = now; state.count = 0; }
    state.count += 1;
    ws.serializeAttachment(state);
    if (state.count > 10) { ws.close(1008, "Too many messages"); return; }
    let message;
    try { message = JSON.parse(raw); } catch { ws.close(1008, "Invalid JSON"); return; }
    if (message?.type === "ready" && validTrial(message.trial)) {
      state.trial = cleanTrial(message.trial);
    } else if (message?.type === "unready") {
      state.trial = null;
    } else {
      ws.close(1008, "Unsupported message");
      return;
    }
    ws.serializeAttachment(state);
    const sockets = this.sockets();
    const players = sockets.map(socket => socket.deserializeAttachment()).sort((a, b) => a.seat - b.seat);
    if (players.length >= 2 && players.every(player => player.trial)) {
      this.broadcast({ type: "trial", id: crypto.randomUUID(), trials: players.map(player => ({ ...player.trial, seat: player.seat })) });
      for (const socket of sockets) {
        const attachment = socket.deserializeAttachment();
        attachment.trial = null;
        socket.serializeAttachment(attachment);
      }
    }
    this.broadcastState();
  }

  webSocketClose() {
    // A partner leaving cancels readiness; a replacement cannot start an old test.
    for (const ws of this.sockets()) {
      const state = ws.deserializeAttachment();
      state.trial = null;
      ws.serializeAttachment(state);
    }
    this.broadcastState();
  }

  webSocketError(ws) {
    ws.close(1011, "Connection error");
  }
}
