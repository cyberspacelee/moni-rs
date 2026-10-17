import { useSyncExternalStore } from "react";
import type { ActiveAlert, DashboardEvent, ServerStatus } from "../api/generated";
import { api } from "./api";

export type ConnState = "connecting" | "open" | "closed";

export type LiveState = {
  conn: ConnState;
  /** True once the first snapshot has arrived. */
  ready: boolean;
  servers: ReadonlyMap<string, ServerStatus>;
  alerts: readonly ActiveAlert[];
};

let state: LiveState = { conn: "closed", ready: false, servers: new Map(), alerts: [] };
const listeners = new Set<() => void>();

function set(next: Partial<LiveState>) {
  state = { ...state, ...next };
  listeners.forEach((l) => l());
}

const alertKey = (a: ActiveAlert) => `${a.rule_id}\u0000${a.server_id}`;

function countFiring(servers: Map<string, ServerStatus>, alerts: readonly ActiveAlert[]) {
  for (const [id, s] of servers) {
    const n = alerts.filter((a) => a.server_id === id).length;
    if (s.alerts_firing !== n) servers.set(id, { ...s, alerts_firing: n });
  }
}

function apply(ev: DashboardEvent) {
  switch (ev.type) {
    case "snapshot": {
      set({ ready: true, servers: new Map(ev.servers.map((s) => [s.id, s])), alerts: ev.alerts });
      return;
    }
    case "server_upsert": {
      const servers = new Map(state.servers);
      servers.set(ev.server.id, ev.server);
      set({ servers });
      return;
    }
    case "server_removed": {
      const servers = new Map(state.servers);
      servers.delete(ev.id);
      set({ servers, alerts: state.alerts.filter((a) => a.server_id !== ev.id) });
      return;
    }
    case "online":
    case "offline":
    case "metrics": {
      const prev = state.servers.get(ev.id);
      if (!prev) return;
      const servers = new Map(state.servers);
      if (ev.type === "online") {
        servers.set(ev.id, { ...prev, online: true, agent_version: ev.agent_version, host: ev.host ?? prev.host });
      } else if (ev.type === "offline") {
        servers.set(ev.id, { ...prev, online: false, metrics: null, last_seen_ms: ev.last_seen_ms });
      } else {
        servers.set(ev.id, { ...prev, online: true, metrics: ev.frame, last_seen_ms: ev.frame.ts_ms });
      }
      set({ servers });
      return;
    }
    case "traffic": {
      const prev = state.servers.get(ev.id);
      if (!prev) return;
      const servers = new Map(state.servers);
      servers.set(ev.id, { ...prev, traffic_month: ev.traffic });
      set({ servers });
      return;
    }
    case "alert": {
      const key = alertKey(ev.alert);
      const rest = state.alerts.filter((a) => alertKey(a) !== key);
      const alerts = ev.kind === "firing" ? [...rest, ev.alert] : rest;
      const servers = new Map(state.servers);
      countFiring(servers, alerts);
      set({ alerts, servers });
      return;
    }
    default: {
      const unhandled: never = ev;
      console.warn("unknown dashboard event", unhandled);
    }
  }
}

let ws: WebSocket | null = null;
let retryMs = 1000;
let retryTimer: number | undefined;
let wanted = false;

function open() {
  if (!wanted) return;
  const proto = location.protocol === "https:" ? "wss" : "ws";
  const sock = new WebSocket(`${proto}://${location.host}/ws/dashboard`);
  ws = sock;
  let opened = false;
  set({ conn: "connecting" });
  sock.onopen = () => {
    opened = true;
    retryMs = 1000;
    set({ conn: "open" });
  };
  sock.onmessage = (e) => {
    try {
      apply(JSON.parse(e.data as string) as DashboardEvent);
    } catch (err) {
      console.warn("bad dashboard event", err);
    }
  };
  sock.onclose = () => {
    if (ws !== sock) return;
    ws = null;
    set({ conn: "closed" });
    if (!wanted) return;
    // A rejected handshake may mean the session expired; the API client raises the
    // unauthorized event on 401, which sends the app back to the login screen.
    if (!opened) void api.me().catch(() => undefined);
    retryTimer = window.setTimeout(open, retryMs + Math.random() * 500);
    retryMs = Math.min(retryMs * 2, 30_000);
  };
}

export function connectLive() {
  if (wanted) return;
  wanted = true;
  open();
}

export function disconnectLive() {
  wanted = false;
  window.clearTimeout(retryTimer);
  ws?.close();
  ws = null;
  state = { conn: "closed", ready: false, servers: new Map(), alerts: [] };
  listeners.forEach((l) => l());
}

/** Reconnect immediately (e.g. tab became visible again) instead of waiting out the backoff. */
export function nudgeLive() {
  if (wanted && !ws) {
    window.clearTimeout(retryTimer);
    retryMs = 1000;
    open();
  }
}

export function useLive(): LiveState {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => state,
  );
}
