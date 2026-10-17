import type { ReactNode } from "react";
import { useLive } from "../lib/live";
import { href, type Route } from "../lib/router";
import { setTheme, useTheme } from "../lib/theme";
import { IconAlert, IconGitHub, IconLogIn, IconLogOut, IconMoon, IconOverview, IconSliders, IconSun, Logo } from "./icons";
import { Button, cx } from "./ui";

function ConnIndicator() {
  const { conn } = useLive();
  const open = conn === "open";
  return (
    <span
      title={open ? "实时连接正常" : "实时连接断开，正在重连"}
      className="inline-flex h-8 items-center gap-2 rounded-md px-2 text-xs leading-none font-medium text-shell-ink-2"
    >
      <span aria-hidden className={cx("size-1.5 rounded-full", open ? "bg-good" : "animate-pulse bg-warn")} />
      {open ? "实时" : "重连中"}
    </span>
  );
}

const REPO = "https://github.com/cyberspacelee/moni-rs";
const AUTHOR = { name: "cyberspacelee", url: "https://github.com/cyberspacelee" };
const SINCE = 2026;

function Footer({ version, tabBar }: { version?: string; tabBar: boolean }) {
  const year = new Date().getFullYear();
  const link = "inline-flex h-8 items-center gap-1.5 rounded-md px-2 leading-none text-ink-2 transition-colors hover:bg-surface-2 hover:text-ink";
  return (
    // The phone tab bar is fixed; pad the footer so the last line clears it.
    <footer className={cx("border-t border-line", tabBar && "pb-[calc(3.5rem+env(safe-area-inset-bottom))] sm:pb-0")}>
      <div className="mx-auto flex w-full max-w-7xl flex-col gap-3 px-4 py-5 text-xs text-ink-3 sm:flex-row sm:items-center sm:gap-4 sm:px-6">
        <div className="flex items-center gap-2">
          <Logo className="size-5" />
          <span className="font-semibold leading-none text-ink">moni</span>
          {version && <span className="tabular rounded bg-surface-2 px-1.5 py-0.5 leading-none text-ink-2">v{version}</span>}
          <span className="leading-none">开源 VPS 监控 · 运行在 Cloudflare Workers</span>
        </div>
        <nav aria-label="项目链接" className="-mx-2 flex flex-wrap items-center gap-x-1 sm:mx-0 sm:ml-auto">
          <a className={link} href={REPO} target="_blank" rel="noreferrer">
            <IconGitHub size={14} />
            GitHub
          </a>
          <a className={link} href={`${REPO}/blob/main/LICENSE`} target="_blank" rel="noreferrer">
            MIT 许可
          </a>
          <span className="tabular inline-flex h-8 items-center px-2 leading-none">
            ©&nbsp;{year > SINCE ? `${SINCE}–${year}` : SINCE}&nbsp;
            <a className="font-medium text-ink-2 underline-offset-2 hover:text-ink hover:underline" href={AUTHOR.url} target="_blank" rel="noreferrer">
              {AUTHOR.name}
            </a>
          </span>
        </nav>
      </div>
    </footer>
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
      <header className="sticky top-0 z-30 border-b border-shell-line bg-shell pt-[env(safe-area-inset-top)] text-shell-ink">
        <div className="mx-auto flex h-14 max-w-7xl items-center gap-2 px-4 sm:gap-4 sm:px-6">
          <a href="#/" className="flex h-8 items-center gap-2 font-semibold tracking-tight text-shell-ink">
            <Logo className="size-6" />
            <span className="text-[15px] leading-none">moni</span>
          </a>
          {nav.length > 1 && (
            <nav className="ml-2 hidden gap-0.5 sm:flex">
              {nav.map((n) => (
                <a
                  key={n.label}
                  href={n.to}
                  aria-current={n.active ? "page" : undefined}
                  className={cx(
                    "inline-flex h-8 items-center gap-1.5 rounded-md px-2.5 text-sm leading-none transition-colors",
                    n.active ? "bg-white/12 font-medium text-shell-ink" : "text-shell-ink-2 hover:bg-white/8 hover:text-shell-ink",
                  )}
                >
                  {n.label}
                </a>
              ))}
            </nav>
          )}
          <div className="ml-auto flex items-center gap-1">
            {admin && alerts.length > 0 && (
              <a
                href={href({ page: "settings", tab: "events" })}
                className="tabular inline-flex h-8 items-center gap-1.5 rounded-md bg-crit px-2.5 text-xs leading-none font-medium text-bg"
              >
                <IconAlert size={14} />
                {alerts.length}
                <span className="hidden sm:inline"> 个告警</span>
              </a>
            )}
            <ConnIndicator />
            <Button
              size="icon"
              variant="shell"
              aria-label="切换主题"
              title="切换主题"
              onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
            >
              {theme === "dark" ? <IconSun /> : <IconMoon />}
            </Button>
            {admin ? (
              <Button size="icon" variant="shell" aria-label="退出" title="退出" onClick={onLogout}>
                <IconLogOut />
              </Button>
            ) : (
              <a
                href={href({ page: "login" })}
                className="ml-1 inline-flex h-8 items-center gap-1.5 rounded-md bg-shell-ink px-3 text-sm leading-none font-medium text-shell hover:bg-white"
              >
                <IconLogIn size={16} />
                登录
              </a>
            )}
          </div>
        </div>
      </header>
      <main className="mx-auto w-full max-w-7xl flex-1 px-4 py-5 sm:px-6 sm:py-8">{children}</main>
      <Footer version={version} tabBar={nav.length > 1} />
      {nav.length > 1 && (
        // Thumb-reachable tab bar on phones; the header nav takes over from sm up.
        <nav className="fixed inset-x-0 bottom-0 z-30 border-t border-line bg-bg pb-[env(safe-area-inset-bottom)] sm:hidden">
          <div className="mx-auto grid h-14 max-w-md grid-cols-2">
            {nav.map((n) => (
              <a
                key={n.label}
                href={n.to}
                aria-current={n.active ? "page" : undefined}
                className={cx(
                  "flex flex-col items-center justify-center gap-1 text-[11px] leading-none font-medium",
                  n.active ? "text-ink" : "text-ink-3",
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
