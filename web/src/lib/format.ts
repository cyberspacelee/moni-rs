const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

export function bytes(n: number | null | undefined, digits = 1): string {
  if (n == null || !Number.isFinite(n)) return "–";
  let i = 0;
  let v = n;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(i === 0 ? 0 : digits)} ${UNITS[i]}`;
}

export const rate = (bps: number | null | undefined) => (bps == null ? "–" : `${bytes(bps)}/s`);

export function pct(used: number, total: number): number {
  return total > 0 ? (used * 100) / total : 0;
}

export const pctText = (v: number | null | undefined, digits = 1) =>
  v == null || !Number.isFinite(v) ? "–" : `${v.toFixed(digits)}%`;

export function duration(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d} 天 ${h} 小时`;
  if (h > 0) return `${h} 小时 ${m} 分`;
  if (m > 0) return `${m} 分钟`;
  return `${s} 秒`;
}

export function ago(ms: number | null | undefined, now = Date.now()): string {
  if (ms == null) return "从未";
  const s = Math.round((now - ms) / 1000);
  if (s < 10) return "刚刚";
  return `${duration(s)}前`;
}

export function dateTime(ms: number): string {
  return new Date(ms).toLocaleString("zh-CN", { hour12: false });
}

/** Clock time as a station board prints it; the date joins only when it is not today. */
export function clockTime(ms: number, now = Date.now()): string {
  const d = new Date(ms);
  const hm = d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false });
  return new Date(now).toDateString() === d.toDateString() ? hm : `${d.getMonth() + 1}/${d.getDate()} ${hm}`;
}
