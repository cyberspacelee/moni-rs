/** Every theme the UI offers; the colours live in src/themes/<id>.css. */
export type ThemeDef = { id: string; name: string; tagline: string; darkOnly?: boolean };

/** Also what unknown ids (a theme removed after the admin picked it) fall back to. */
export const DEFAULT_THEME: ThemeDef = { id: "clay", name: "陶土", tagline: "温暖而克制" };

export const THEMES: readonly ThemeDef[] = [
  DEFAULT_THEME,
  { id: "cyberpunk", name: "赛博朋克", tagline: "霓虹与精密", darkOnly: true },
  { id: "material", name: "Material", tagline: "柔和的层次" },
  { id: "cloudflare", name: "Cloudflare", tagline: "工程控制台" },
  { id: "terminal", name: "终端", tagline: "回到命令行", darkOnly: true },
  { id: "nord", name: "Nord", tagline: "北境的冷静" },
  { id: "geist", name: "Geist", tagline: "纯粹的黑与白" },
];

export const findTheme = (id: string | undefined): ThemeDef | undefined => THEMES.find((t) => t.id === id);
