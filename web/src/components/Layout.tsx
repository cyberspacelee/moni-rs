import type { ReactNode } from "react";
import { useLive } from "../lib/live";
import { href, type Route } from "../lib/router";
import { setTheme, useTheme } from "../lib/theme";
import { IconAlert, IconLogIn, IconLogOut, IconMoon, IconOverview, IconSliders, IconSun, Logo } from "./icons";
import { Button, cx } from "./ui";

function ConnIndicator() {
  const { conn } = useLive();
  const open = conn === "open";
  return (
    <span
      title={open ? "实时连接正常" : "实时连接断开，正在重连"}
      className="inline-flex h-7 items-center gap-1.5 rounded-full border border-line bg-surface px-2.5 text-[11px] font-medium text-ink-2"
    >
      <span aria-hidden className="relative inline-flex size-1.5">
        {open && <span className="pulse-ring absolute inset-0 rounded-full bg-good" />}
        <span className={cx("relative size-1.5 rounded-full", open ? "bg-good" : "animate-pulse bg-warn")} />
      </span>
      {open ? "实时" : "重连中"}
    </span>
  );
}

export function Layout({
  route,
  admin,
  onLogout,
  version,
  children,
}: {
  route: Route;
  admin: boolean;
  onLogout: () => void;
  version?: string;
  children: ReactNode;
}) {
  const { alerts } = useLive();
  const theme = useTheme();
  const nav = [
    { label: "总览", icon: <IconOverview />, to: href({ page: "overview" }), active: route.page === "overview" || route.page === "server" },
    ...(admin
      ? [{ label: "管理", icon: <IconSliders />, to: href({ page: "settings", tab: "servers" }), active: route.page === "settings" }]
      : []),
  ];
  return (
    <div className="flex min-h-dvh flex-col">
      <header className="sticky top-0 z-30 border-b border-line/70 bg-bg/75 pt-[env(safe-area-inset-top)] backdrop-blur-xl backdrop-saturate-150">
        <div className="mx-auto flex h-14 max-w-7xl items-center gap-2 px-4 sm:gap-4 sm:px-6">
          <a href="#/" className="flex items-center gap-2.5 font-semibold tracking-tight">
            <Logo />
            <span className="text-[15px]">moni</span>
          </a>
          {nav.length > 1 && (
            <nav className="ml-1 hidden gap-1 sm:flex">
              {nav.map((n) => (
                <a
                  key={n.label}
                  href={n.to}
                  aria-current={n.active ? "page" : undefined}
                  className={cx(
                    "inline-flex h-8 items-center gap-1.5 rounded-lg px-3 text-sm transition-colors",
                    n.active ? "bg-surface font-medium text-ink shadow-xs ring-1 ring-line" : "text-ink-3 hover:text-ink",
                  )}
                >
                  {n.label}
                </a>
              ))}
            </nav>
          )}
          <div className="ml-auto flex items-center gap-1.5 sm:gap-2">
            {admin && alerts.length > 0 && (
              <a
                href={href({ page: "settings", tab: "events" })}
                className="inline-flex h-7 items-center gap-1 rounded-full bg-crit-soft px-2.5 text-[11px] font-medium text-crit"
              >
                <IconAlert size={13} />
                {alerts.length}
                <span className="hidden sm:inline"> 个告警</span>
              </a>
            )}
            <ConnIndicator />
            <Button
              size="icon"
              variant="ghost"
              aria-label="切换主题"
              title="切换主题"
              onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
            >
              {theme === "dark" ? <IconSun /> : <IconMoon />}
            </Button>
            {admin ? (
              <Button size="icon" variant="ghost" aria-label="退出" title="退出" onClick={onLogout}>
                <IconLogOut />
              </Button>
            ) : (
              <a
                href={href({ page: "login" })}
                className="inline-flex h-8 items-center gap-1.5 rounded-lg border border-line bg-surface px-3 text-xs font-medium text-ink-2 shadow-xs hover:text-ink"
              >
                <IconLogIn size={14} />
                登录
              </a>
            )}
          </div>
        </div>
      </header>
      <main className={cx("mx-auto w-full max-w-7xl flex-1 px-4 py-5 sm:px-6 sm:py-8", nav.length > 1 && "pb-24 sm:pb-8")}>
        {children}
      </main>
      <footer className="mx-auto hidden w-full max-w-7xl items-center gap-2 px-6 py-6 text-xs text-ink-3 sm:flex">
        <Logo className="size-4 opacity-70 grayscale" />
        moni-rs {version && `v${version}`} · 运行在 Cloudflare Workers
      </footer>
      {nav.length > 1 && (
        // Thumb-reachable tab bar on phones; the header nav takes over from sm up.
        <nav className="fixed inset-x-0 bottom-0 z-30 border-t border-line/70 bg-bg/85 pb-[env(safe-area-inset-bottom)] backdrop-blur-xl sm:hidden">
          <div className="mx-auto grid h-14 max-w-md grid-cols-2">
            {nav.map((n) => (
              <a
                key={n.label}
                href={n.to}
                aria-current={n.active ? "page" : undefined}
                className={cx(
                  "flex flex-col items-center justify-center gap-0.5 text-[11px] font-medium",
                  n.active ? "text-brand" : "text-ink-3",
                )}
              >
                {n.icon}
                {n.label}
              </a>
            ))}
          </div>
        </nav>
      )}
    </div>
  );
}
