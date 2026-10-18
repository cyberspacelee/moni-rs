import type { ThemeMode } from "../api/generated";
import { setThemePref, useTheme } from "../lib/theme";
import { DEFAULT_THEME, THEMES, findTheme } from "../lib/themes";
import { IconCheck } from "./icons";
import { Modal, Segmented, cx } from "./ui";

export const MODE_OPTIONS: readonly { id: ThemeMode; label: string }[] = [
  { id: "light", label: "明" },
  { id: "dark", label: "暗" },
  { id: "system", label: "跟随系统" },
];

/** A theme's own palette, rendered by scoping its tokens to this element. */
function Swatch({ id, dark }: { id: string; dark: boolean }) {
  return (
    <span
      aria-hidden
      data-theme={id}
      className={cx("grid h-7 w-12 shrink-0 grid-cols-4 overflow-hidden rounded-md border border-line", dark && "dark")}
    >
      <span className="bg-bg" />
      <span className="bg-surface-2" />
      <span className="bg-brand" />
      <span className="bg-[var(--series-2)]" />
    </span>
  );
}

/** Radio list of every theme; shared by the visitor picker and the admin default. */
export function ThemeList({
  value,
  onChange,
  dark,
  disabled,
  name,
}: {
  value: string | undefined;
  onChange: (id: string) => void;
  dark: boolean;
  disabled?: boolean;
  name: string;
}) {
  return (
    <div role="radiogroup" className="grid gap-1 sm:grid-cols-2">
      {THEMES.map((t) => {
        const checked = value === t.id;
        return (
          <label
            key={t.id}
            className={cx(
              "flex cursor-pointer items-center gap-3 rounded-lg px-2.5 py-2 transition-colors has-focus-visible:outline-2 has-focus-visible:outline-brand",
              checked ? "bg-brand-soft" : "hover:bg-surface-2",
              disabled && "pointer-events-none opacity-45",
            )}
          >
            <input type="radio" name={name} className="sr-only" checked={checked} disabled={disabled} onChange={() => onChange(t.id)} />
            <Swatch id={t.id} dark={!!t.darkOnly || dark} />
            <span className="min-w-0 flex-1">
              <span className={cx("block truncate text-sm leading-5", checked ? "font-medium text-brand-ink" : "text-ink")}>{t.name}</span>
              <span className="block truncate text-xs leading-4 text-ink-3">
                {t.tagline}
                {t.darkOnly && " · 仅暗色"}
              </span>
            </span>
            {checked && <IconCheck size={16} className="shrink-0 text-brand-ink" />}
          </label>
        );
      })}
    </div>
  );
}

/** Visitor's own appearance for this browser, layered over the site default. */
export function ThemePicker({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { site, pref, theme, mode, dark } = useTheme();
  const siteName = findTheme(site.theme)?.name ?? DEFAULT_THEME.name;
  const following = site.locked || !pref.theme;
  return (
    <Modal open={open} onClose={onClose} title="外观">
      <div className="space-y-4">
        {site.locked ? (
          <p className="rounded-lg bg-surface-2 px-3 py-2.5 text-sm text-ink-2">管理员已锁定站点主题（{siteName}），你仍可切换明暗。</p>
        ) : (
          <>
            <button
              type="button"
              onClick={() => setThemePref({ ...pref, theme: undefined })}
              className={cx(
                "flex w-full items-center justify-between rounded-lg px-3 py-2.5 text-left text-sm transition-colors",
                following ? "bg-brand-soft font-medium text-brand-ink" : "bg-surface-2 text-ink-2 hover:text-ink",
              )}
            >
              跟随站点默认（{siteName}）
              {following && <IconCheck size={16} />}
            </button>
            <ThemeList name="visitor-theme" value={following ? undefined : theme.id} dark={dark} onChange={(id) => setThemePref({ ...pref, theme: id })} />
          </>
        )}
        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-line pt-4">
          {theme.darkOnly ? (
            <span className="text-sm text-ink-3">{theme.name}仅有暗色</span>
          ) : (
            <Segmented label="明暗模式" value={mode} options={MODE_OPTIONS} onChange={(m) => setThemePref({ ...pref, mode: m })} />
          )}
          <span className="text-xs text-ink-3">只影响当前浏览器</span>
        </div>
      </div>
    </Modal>
  );
}
