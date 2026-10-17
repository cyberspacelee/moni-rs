import { useEffect, useRef } from "react";
import uPlot from "uplot";
import { useTheme } from "../lib/theme";

export type ChartSeries = {
  label: string;
  /** CSS custom property holding the colour, e.g. "--series-1". */
  color: "--series-1" | "--series-2";
  values: (number | null)[];
};

type Props = {
  title: string;
  /** Unix seconds, ascending. */
  xs: number[];
  series: ChartSeries[];
  format: (v: number) => string;
  /** Fixed y max (e.g. 100 for percentages). */
  yMax?: number;
  height?: number;
};

const pad = (n: number) => String(n).padStart(2, "0");

/** Axis tick label: clock time for short spans, month-day for multi-day spans. */
function timeLabel(sec: number, spanSec: number): string {
  const d = new Date(sec * 1000);
  const hm = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  const md = `${d.getMonth() + 1}/${d.getDate()}`;
  if (spanSec <= 2 * 86400) return d.getHours() === 0 && d.getMinutes() === 0 ? md : hm;
  return md;
}

const cssVar = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/** Time-series panel: one y-axis, ≤2 series, crosshair with live legend values. */
export function Chart({ title, xs, series, format, yMax, height = 180 }: Props) {
  const box = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  const theme = useTheme();

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const axis = cssVar("--chart-axis");
    const grid = cssVar("--chart-grid");
    const opts: uPlot.Options = {
      width: el.clientWidth || 300,
      height,
      padding: [8, 8, 0, 0],
      cursor: { points: { size: 8 }, drag: { x: false, y: false } },
      legend: { live: true, show: true },
      scales: { x: { time: true }, y: { range: (_u, min, max) => [Math.min(0, min), yMax ?? (max > 0 ? max * 1.1 : 1)] } },
      axes: [
        {
          stroke: axis,
          grid: { stroke: grid, width: 1 },
          ticks: { show: false },
          font: "11px system-ui",
          values: (u, splits) => {
            const span = (u.scales.x?.max ?? 0) - (u.scales.x?.min ?? 0);
            return splits.map((v) => timeLabel(v, span));
          },
        },
        {
          stroke: axis,
          grid: { stroke: grid, width: 1 },
          ticks: { show: false },
          font: "11px system-ui",
          size: 64,
          values: (_u, splits) => splits.map((v) => format(v)),
        },
      ],
      series: [
        { label: "时间", value: (_u, v) => (v == null ? "–" : new Date(v * 1000).toLocaleString("zh-CN", { hour12: false })) },
        ...series.map((s) => ({
          label: s.label,
          stroke: cssVar(s.color),
          width: 2,
          points: { show: false },
          spanGaps: false,
          value: (_u: uPlot, v: number | null) => (v == null ? "–" : format(v)),
        })),
      ],
    };
    const u = new uPlot(opts, [xs, ...series.map((s) => s.values)] as uPlot.AlignedData, el);
    plot.current = u;
    const ro = new ResizeObserver(() => u.setSize({ width: el.clientWidth, height }));
    ro.observe(el);
    return () => {
      ro.disconnect();
      u.destroy();
      plot.current = null;
    };
    // Rebuild when the shape or theme changes; data-only updates go through setData below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [theme, series.length, height, yMax, title]);

  useEffect(() => {
    plot.current?.setData([xs, ...series.map((s) => s.values)] as uPlot.AlignedData);
  }, [xs, series]);

  return (
    <div className="rounded-lg border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-900">
      <h3 className="mb-1 text-sm font-medium">{title}</h3>
      <div ref={box} className="w-full text-xs" />
    </div>
  );
}
