import type { ReactNode } from "react";
import { useLive } from "../lib/live";
import { href, type Route } from "../lib/router";
import { setTheme, useTheme } from "../lib/theme";
import { Button, cx } from "./ui";

function Logo() {
  return (
    <svg viewBox="0 0 32 32" className="size-6" aria-hidden>
      <rect width="32" height="32" rx="7" className="fill-zinc-900 dark:fill-zinc-100" />
      <path
        d="M6 19h5l3-8 4 12 3-7h5"
        fill="none"
        stroke="#34d399"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function Layout({
  route,
  onLogout,
  version,
  children,
}: {
  route: Route;
  onLogout: () => void;
  version?: string;
  children: ReactNode;
}) {
  const { conn, alerts } = useLive();
  const theme = useTheme();
  const nav = [
    { label: "总览", to: href({ page: "overview" }), active: route.page !== "settings" },
    { label: "设置", to: href({ page: "settings", tab: "servers" }), active: route.page === "settings" },
  ];
  return (
    <div className="mx-auto flex min-h-dvh max-w-7xl flex-col px-4 sm:px-6">
      <header className="flex h-14 items-center gap-4 border-b border-zinc-200 dark:border-zinc-800">
        <a href="#/" className="flex items-center gap-2 font-semibold">
          <Logo />
          <span>moni-rs</span>
        </a>
        <nav className="flex gap-1">
          {nav.map((n) => (
            <a
              key={n.label}
              href={n.to}
              className={cx(
                "rounded-md px-2.5 py-1.5 text-sm",
                n.active ? "bg-zinc-100 font-medium dark:bg-zinc-800" : "text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100",
              )}
            >
              {n.label}
            </a>
          ))}
        </nav>
        <div className="ml-auto flex items-center gap-2">
          {alerts.length > 0 && (
            <a
              href={href({ page: "settings", tab: "events" })}
              className="rounded-md bg-red-50 px-2 py-1 text-xs font-medium text-red-700 dark:bg-red-950/50 dark:text-red-300"
            >
              ⚠ {alerts.length} 个告警
            </a>
          )}
          <span
            title={conn === "open" ? "实时连接正常" : "实时连接断开，正在重连"}
            className="hidden items-center gap-1.5 text-xs text-zinc-500 sm:inline-flex"
          >
            <span
              aria-hidden
              className={cx(
                "size-1.5 rounded-full",
                conn === "open" ? "bg-[var(--status-good)]" : "animate-pulse bg-[var(--status-warning)]",
              )}
            />
            {conn === "open" ? "实时" : "重连中"}
          </span>
          <Button
            size="sm"
            variant="ghost"
            aria-label="切换主题"
            onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
          >
            {theme === "dark" ? "☀" : "☾"}
          </Button>
          <Button size="sm" variant="ghost" onClick={onLogout}>
            退出
          </Button>
        </div>
      </header>
      <main className="flex-1 py-6">{children}</main>
      <footer className="border-t border-zinc-200 py-4 text-xs text-zinc-500 dark:border-zinc-800">
        moni-rs {version && `v${version}`} · 运行在 Cloudflare Workers
      </footer>
    </div>
  );
}
