import { useRef } from "react";
import { cx } from "./ui";

const TICKS = Array.from({ length: 12 }, (_, i) => i * 30);

/**
 * A station slave clock for one server. While frames arrive, the hands show each frame's time
 * and the second hand jumps to it; when the agent goes silent the clock stops where the last
 * heartbeat left it, and the second hand parks at 12 like a slave clock waiting for its next
 * minute impulse.
 */
export function StationClock({
  ms,
  running,
  size = 32,
  className,
  label,
}: {
  /** Time to show: the latest frame while running, the last heartbeat once stopped. */
  ms: number | null;
  running: boolean;
  size?: number;
  className?: string;
  label: string;
}) {
  const d = ms == null ? null : new Date(ms);
  const sec = d ? d.getSeconds() + d.getMilliseconds() / 1000 : 0;
  const min = d ? d.getMinutes() + sec / 60 : 0;
  const hour = d ? (d.getHours() % 12) + min / 60 : 0;

  // Accumulate the angle so 55s → 0s steps forward instead of spinning back through the face.
  const secAngle = useRef<number | null>(null);
  const target = running ? sec * 6 : 0;
  const prev = secAngle.current;
  secAngle.current = prev == null ? target : prev + ((((target - prev) % 360) + 360) % 360);

  return (
    <svg
      viewBox="0 0 40 40"
      width={size}
      height={size}
      role="img"
      aria-label={label}
      className={cx("shrink-0", className)}
    >
      <title>{label}</title>
      <circle cx="20" cy="20" r="19" fill="var(--clock-face)" stroke="#111" strokeWidth="1.5" />
      {TICKS.map((a) => (
        <rect key={a} x="19.25" y="3" width="1.5" height={a % 90 === 0 ? 5 : 3.5} fill="#111" transform={`rotate(${a} 20 20)`} />
      ))}
      {d ? (
        <>
          <rect x="18.9" y="9.5" width="2.2" height="12.5" rx="1.1" fill="#111" transform={`rotate(${hour * 30} 20 20)`} />
          <rect x="19.1" y="5.5" width="1.8" height="16.5" rx="0.9" fill="#111" transform={`rotate(${min * 6} 20 20)`} />
        </>
      ) : (
        <circle cx="20" cy="20" r="2" fill="var(--ink-3)" />
      )}
      {d && (
        <g className="clock-hand" style={{ transform: `rotate(${secAngle.current}deg)` }}>
          <rect x="19.5" y="9" width="1" height="15" fill={running ? "var(--signal)" : "#8b8a84"} />
          <circle cx="20" cy="9" r="2.6" fill={running ? "var(--signal)" : "#8b8a84"} />
        </g>
      )}
      <circle cx="20" cy="20" r="1.1" fill={d ? "#111" : "var(--ink-3)"} />
    </svg>
  );
}
