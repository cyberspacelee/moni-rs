import { useId } from "react";

/** Monotone cubic path (Fritsch–Carlson): smooth like the charts, never overshoots below 0. */
function monotonePath(pts: [number, number][]): string {
  const n = pts.length;
  if (n === 0) return "";
  const first = pts[0]!;
  if (n === 1) return `M${first[0]},${first[1]}`;
  const dx: number[] = [];
  const slope: number[] = [];
  for (let i = 0; i < n - 1; i++) {
    const a = pts[i]!;
    const b = pts[i + 1]!;
    dx.push(b[0] - a[0]);
    slope.push(dx[i]! === 0 ? 0 : (b[1] - a[1]) / dx[i]!);
  }
  const t: number[] = [slope[0]!];
  for (let i = 1; i < n - 1; i++) {
    const s0 = slope[i - 1]!;
    const s1 = slope[i]!;
    t.push(s0 * s1 <= 0 ? 0 : (3 * (dx[i - 1]! + dx[i]!)) / ((2 * dx[i]! + dx[i - 1]!) / s0 + (dx[i]! + 2 * dx[i - 1]!) / s1));
  }
  t.push(slope[n - 2]!);
  let d = `M${first[0]},${first[1]}`;
  for (let i = 0; i < n - 1; i++) {
    const a = pts[i]!;
    const b = pts[i + 1]!;
    const h = dx[i]! / 3;
    d += `C${a[0] + h},${a[1] + h * t[i]!} ${b[0] - h},${b[1] - h * t[i + 1]!} ${b[0]},${b[1]}`;
  }
  return d;
}

/** Live trend of a 0–100 value: a stroke over a fading wash, sized by its container. */
export function Sparkline({ values, color, max = 100 }: { values: readonly number[]; color: string; max?: number }) {
  const id = `spark${useId().replace(/[^\w-]/g, "")}`;
  const W = 200;
  const H = 40;
  const slots = Math.max(values.length, 30);
  // Right-align: the newest sample sits at the right edge while the buffer fills up.
  const offset = slots - values.length;
  const pts = values.map((v, i): [number, number] => [
    ((offset + i) / (slots - 1)) * W,
    H - 2 - (Math.min(max, Math.max(0, v)) / max) * (H - 4),
  ]);
  const line = monotonePath(pts);
  const firstX = pts[0]?.[0] ?? W;
  return (
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className="h-full w-full overflow-visible" aria-hidden>
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" style={{ stopColor: color, stopOpacity: 0.28 }} />
          <stop offset="1" style={{ stopColor: color, stopOpacity: 0 }} />
        </linearGradient>
      </defs>
      {/* The baseline keeps the slot legible while the live buffer is still filling. */}
      <line x1="0" y1={H - 0.5} x2={W} y2={H - 0.5} stroke="var(--line)" strokeDasharray="2 3" vectorEffect="non-scaling-stroke" />
      {pts.length > 1 && (
        <>
          <path d={`${line}L${W},${H}L${firstX},${H}Z`} fill={`url(#${id})`} stroke="none" />
          <path d={line} fill="none" style={{ stroke: color, transition: "stroke 300ms" }} strokeWidth="1.75" vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
        </>
      )}
    </svg>
  );
}
