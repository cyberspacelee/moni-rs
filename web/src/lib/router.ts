import { useSyncExternalStore } from "react";

// Hash routing keeps static asset serving trivial (no SPA fallback needed).
export type Route =
  | { page: "overview" }
  | { page: "login" }
  | { page: "server"; id: string }
  | { page: "settings"; tab: SettingsTab };

export type SettingsTab = "servers" | "alerts" | "events" | "notify" | "appearance";
const TABS: SettingsTab[] = ["servers", "alerts", "events", "notify", "appearance"];

function decode(part: string): string | null {
  try {
    return decodeURIComponent(part);
  } catch {
    return null; // malformed percent-encoding
  }
}

function parse(hash: string): Route {
  const parts = hash.replace(/^#\/?/, "").split("/").filter(Boolean).map(decode);
  if (parts.some((p) => p == null)) return { page: "overview" };
  if (parts[0] === "login") return { page: "login" };
  if (parts[0] === "server" && parts[1]) return { page: "server", id: parts[1] };
  if (parts[0] === "settings") {
    const tab = TABS.find((t) => t === parts[1]) ?? "servers";
    return { page: "settings", tab };
  }
  return { page: "overview" };
}

export function href(route: Route): string {
  switch (route.page) {
    case "overview":
      return "#/";
    case "login":
      return "#/login";
    case "server":
      return `#/server/${encodeURIComponent(route.id)}`;
    case "settings":
      return `#/settings/${route.tab}`;
  }
}

const subscribe = (cb: () => void) => {
  window.addEventListener("hashchange", cb);
  return () => window.removeEventListener("hashchange", cb);
};

// useSyncExternalStore needs a stable snapshot: only re-parse when the hash actually changes.
let cachedHash: string | null = null;
let cachedRoute: Route = { page: "overview" };
function snapshot(): Route {
  if (location.hash !== cachedHash) {
    cachedHash = location.hash;
    cachedRoute = parse(cachedHash);
  }
  return cachedRoute;
}

export const useRoute = () => useSyncExternalStore(subscribe, snapshot);
export const navigate = (route: Route) => {
  location.hash = href(route);
};
