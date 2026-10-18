import { useSyncExternalStore } from "react";
import type { Appearance, ThemeMode } from "../api/generated";
import { DEFAULT_THEME, type ThemeDef, findTheme } from "./themes";

/** The visitor's own choice in this browser; an absent field follows the site default. */
export type ThemePref = { theme?: string; mode?: ThemeMode };

export type ThemeState = {
  site: Appearance;
  pref: ThemePref;
  theme: ThemeDef;
  mode: ThemeMode;
  dark: boolean;
};

// moni-theme: the visitor's pref. moni-site: last known site default. moni-applied: what
// index.html applies before first paint, so a reload never flashes the wrong theme.
const PREF_KEY = "moni-theme";
const SITE_KEY = "moni-site";
const APPLIED_KEY = "moni-applied";

const DEFAULT_SITE: Appearance = { theme: "clay", mode: "system", locked: false };
const MODES: readonly ThemeMode[] = ["light", "dark", "system"];
const systemDark = matchMedia("(prefers-color-scheme: dark)");

function read<T>(key: string): T | undefined {
  try {
    const raw = localStorage.getItem(key);
    return raw == null ? undefined : (JSON.parse(raw) as T);
  } catch {
    return undefined;
  }
}

function write(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // storage unavailable (private mode); the choice still applies for this session
  }
}

function readPref(): ThemePref {
  // Before themes this key held a bare "light" / "dark" string, which is not valid JSON.
  try {
    const legacy = localStorage.getItem(PREF_KEY);
    if (legacy === "light" || legacy === "dark") return { mode: legacy };
  } catch {
    return {};
  }
  const p = read<ThemePref>(PREF_KEY);
  return { theme: typeof p?.theme === "string" ? p.theme : undefined, mode: MODES.find((m) => m === p?.mode) };
}

let site: Appearance = { ...DEFAULT_SITE, ...read<Appearance>(SITE_KEY) };
let pref = readPref();
let state = resolve();
const listeners = new Set<() => void>();

function resolve(): ThemeState {
  const theme = (!site.locked && findTheme(pref.theme)) || findTheme(site.theme) || DEFAULT_THEME;
  const mode = pref.mode ?? site.mode;
  const dark = !!theme.darkOnly || mode === "dark" || (mode === "system" && systemDark.matches);
  return { site, pref, theme, mode, dark };
}

/** The current logo as an SVG data URI, so the tab icon follows the theme too. */
function favicon(brand: string, onBrand: string, rx: string): string {
  const svg = `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'><rect width='32' height='32' rx='${rx}' fill='${brand}'/><path d='M8 21.5v-6.75C8 12.4 9.5 11 11.25 11S14.5 12.4 14.5 14.75v4.75M14.5 14.75c0-2.35 1.5-3.75 3.25-3.75S21 12.4 21 14.75v3.5c0 2 1 3.25 3 3.25' fill='none' stroke='${onBrand}' stroke-width='2.4' stroke-linecap='round' stroke-linejoin='round'/></svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

function apply() {
  state = resolve();
  const root = document.documentElement;
  root.dataset.theme = state.theme.id;
  root.classList.toggle("dark", state.dark);
  write(APPLIED_KEY, { theme: state.theme.id, mode: state.theme.darkOnly ? "dark" : state.mode });

  const css = getComputedStyle(root);
  const token = (name: string) => css.getPropertyValue(name).trim();
  const icon = document.querySelector<HTMLLinkElement>('link[rel="icon"]');
  // Same corner rule as <Logo>: the theme's surface radius, capped at the mark's own 8/32.
  const radius = token("--radius-2xl");
  const rx = radius.endsWith("px") ? Math.min(8, parseFloat(radius) || 0) : 8;
  if (icon) icon.href = favicon(token("--brand"), token("--on-brand"), String(rx));
  document.querySelectorAll<HTMLMetaElement>('meta[name="theme-color"]').forEach((m) => (m.content = token("--bg")));
  listeners.forEach((l) => l());
}

systemDark.addEventListener("change", () => state.mode === "system" && apply());

/** Site default from the server (public info, or the admin just saved it). */
export function setSiteAppearance(next: Appearance) {
  site = next;
  write(SITE_KEY, next);
  apply();
}

export function setThemePref(next: ThemePref) {
  pref = next;
  write(PREF_KEY, next);
  apply();
}

export const useTheme = () =>
  useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => state,
  );

apply();
