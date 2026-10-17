import { useSyncExternalStore } from "react";

type Theme = "light" | "dark";
const listeners = new Set<() => void>();

const current = (): Theme => (document.documentElement.classList.contains("dark") ? "dark" : "light");

export function setTheme(t: Theme) {
  document.documentElement.classList.toggle("dark", t === "dark");
  try {
    localStorage.setItem("moni-theme", t);
  } catch {
    // storage unavailable (private mode); theme still applies for this session
  }
  listeners.forEach((l) => l());
}

export const useTheme = () =>
  useSyncExternalStore((cb) => {
    listeners.add(cb);
    return () => listeners.delete(cb);
  }, current);
