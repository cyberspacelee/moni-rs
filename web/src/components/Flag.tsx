import { EarthIcon } from "lucide-react";
import { cn } from "@/lib/utils";

// SVG flags instead of emoji: Windows ships no flag glyphs and would show bare letters.
const FLAGS = import.meta.glob<string>("/node_modules/flag-icons/flags/4x3/*.svg", {
  query: "?url",
  import: "default",
  eager: true,
});
const flagUrl = (code: string) => FLAGS[`/node_modules/flag-icons/flags/4x3/${code.toLowerCase()}.svg`];

let names: Intl.DisplayNames | null = null;
export function countryName(code: string): string {
  try {
    names ??= new Intl.DisplayNames(["zh-CN"], { type: "region" });
    return names.of(code) ?? code;
  } catch {
    return code;
  }
}

export function Flag({ code, className }: { code: string | null | undefined; className?: string }) {
  const url = code ? flagUrl(code) : undefined;
  const label = code ? countryName(code) : "未知地区";
  // An empty grey box reads as a broken image; a neutral globe says "region unknown".
  if (!url)
    return (
      <span role="img" aria-label={label} title={label} className={cn("inline-grid h-[0.95em] w-[1.27em] shrink-0 place-items-center text-muted-foreground", className)}>
        <EarthIcon aria-hidden className="size-[1em]" />
      </span>
    );
  return (
    <span
      role="img"
      aria-label={label}
      title={label}
      className={cn(
        "inline-block h-[0.95em] w-[1.27em] shrink-0 overflow-hidden rounded-[3px] bg-muted bg-cover bg-center align-[-0.1em] shadow-[inset_0_0_0_1px_rgb(0_0_0/0.12)]",
        className,
      )}
      style={{ backgroundImage: `url("${url}")` }}
    />
  );
}
