import type { ThemeMode } from "@/api/generated";
import { Field, FieldContent, FieldDescription, FieldLabel, FieldTitle } from "@/components/ui/field";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { setThemePref, useTheme } from "@/lib/theme";
import { DEFAULT_THEME, THEMES, findTheme } from "@/lib/themes";
import { cn } from "@/lib/utils";
import { ResponsiveDialog, Segmented } from "@/components/common";

export const MODE_OPTIONS: readonly { id: ThemeMode; label: string }[] = [
  { id: "light", label: "明" },
  { id: "dark", label: "暗" },
  { id: "system", label: "跟随系统" },
];

/** A theme's own palette, rendered by scoping its tokens to this element. */
function Swatch({ id, dark }: { id: string; dark: boolean }) {
  return (
    <span aria-hidden data-theme={id} className={cn("grid h-7 w-12 shrink-0 grid-cols-4 overflow-hidden rounded-md border", dark && "dark")}>
      <span className="bg-background" />
      <span className="bg-muted" />
      <span className="bg-primary" />
      <span className="bg-chart-2" />
    </span>
  );
}

/** One choice card in a theme radio group. */
function Choice({ value, title, description, swatch }: { value: string; title: string; description?: string; swatch?: React.ReactNode }) {
  return (
    <FieldLabel>
      <Field orientation="horizontal" className="items-center!">
        {swatch}
        <FieldContent className="min-w-0">
          <FieldTitle className="truncate">{title}</FieldTitle>
          {description && <FieldDescription className="truncate text-xs">{description}</FieldDescription>}
        </FieldContent>
        <RadioGroupItem value={value} />
      </Field>
    </FieldLabel>
  );
}

/** Every theme as a choice card; shared by the visitor picker and the admin default. */
export function ThemeList({
  value,
  onChange,
  dark,
  disabled,
  followSite,
}: {
  value: string;
  onChange: (id: string) => void;
  dark: boolean;
  disabled?: boolean;
  /** Label for an extra leading "use the site default" choice, whose value is "". */
  followSite?: string;
}) {
  return (
    <RadioGroup value={value} onValueChange={(v) => onChange(v as string)} disabled={disabled} className="sm:grid-cols-2">
      {followSite && (
        <div className="sm:col-span-2">
          <Choice value="" title={followSite} description="管理员更换默认主题时随之变化" />
        </div>
      )}
      {THEMES.map((t) => (
        <Choice
          key={t.id}
          value={t.id}
          title={t.name}
          description={t.darkOnly ? `${t.tagline} · 仅暗色` : t.tagline}
          swatch={<Swatch id={t.id} dark={!!t.darkOnly || dark} />}
        />
      ))}
    </RadioGroup>
  );
}

/** Visitor's own appearance for this browser, layered over the site default. */
export function ThemePicker({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const { site, pref, theme, mode, dark } = useTheme();
  const siteName = findTheme(site.theme)?.name ?? DEFAULT_THEME.name;
  return (
    <ResponsiveDialog open={open} onOpenChange={onOpenChange} title="外观" description="只影响当前浏览器">
      <div className="space-y-4">
        {site.locked ? (
          <p className="rounded-lg bg-muted px-3 py-2.5 text-sm text-ink-2">管理员已锁定站点主题（{siteName}），你仍可切换明暗。</p>
        ) : (
          <div className="-m-1 max-h-[50dvh] overflow-x-hidden overflow-y-auto p-1 sm:max-h-none sm:overflow-visible">
            <ThemeList
              value={pref.theme ?? ""}
              dark={dark}
              followSite={`跟随站点默认（${siteName}）`}
              onChange={(id) => setThemePref({ ...pref, theme: id || undefined })}
            />
          </div>
        )}
        <div className="flex flex-wrap items-center justify-between gap-3 border-t pt-4">
          <span className="text-sm font-medium">明暗</span>
          {theme.darkOnly ? (
            <span className="text-sm text-muted-foreground">{theme.name}仅有暗色</span>
          ) : (
            <Segmented label="明暗模式" value={mode} options={MODE_OPTIONS} onChange={(m) => setThemePref({ ...pref, mode: m })} />
          )}
        </div>
      </div>
    </ResponsiveDialog>
  );
}
