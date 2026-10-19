/* Brand marks: lucide ships no logos, so these two stay hand-drawn on its 24px / 32px grids. */

/** Lucide dropped brand marks; this is GitHub's own mark, filled, on the same 24px grid. */
export function GitHubMark({ size = 16, className }: { size?: number; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden className={className}>
      <path d="M12 .3a12 12 0 0 0-3.8 23.38c.6.12.83-.26.83-.57L9 21.07c-3.34.72-4.04-1.61-4.04-1.61-.55-1.39-1.34-1.76-1.34-1.76-1.08-.74.09-.73.09-.73 1.2.09 1.84 1.24 1.84 1.24 1.07 1.83 2.81 1.3 3.5 1 .1-.78.42-1.31.76-1.61-2.67-.3-5.47-1.33-5.47-5.93 0-1.31.47-2.38 1.24-3.22-.14-.3-.54-1.52.1-3.18 0 0 1-.32 3.3 1.23a11.5 11.5 0 0 1 6 0c2.28-1.55 3.29-1.23 3.29-1.23.64 1.66.24 2.88.12 3.18a4.65 4.65 0 0 1 1.23 3.22c0 4.61-2.8 5.63-5.48 5.92.42.36.81 1.1.81 2.22l-.01 3.29c0 .32.21.69.82.57A12 12 0 0 0 12 .3" />
    </svg>
  );
}

/** The mark: a lowercase "m" drawn as one smoothed metric line, white on the clay accent. */
export function Logo({ className = "size-7" }: { className?: string }) {
  return (
    <svg viewBox="0 0 32 32" className={className} aria-hidden>
      {/* Corner follows the theme's surface radius, capped at the mark's own 8/32. */}
      <rect width="32" height="32" rx="8" style={{ rx: "min(8px, var(--radius-2xl))" }} fill="var(--brand)" />
      <path
        d="M8 21.5v-6.75C8 12.4 9.5 11 11.25 11S14.5 12.4 14.5 14.75v4.75M14.5 14.75c0-2.35 1.5-3.75 3.25-3.75S21 12.4 21 14.75v3.5c0 2 1 3.25 3 3.25"
        fill="none"
        stroke="var(--on-brand)"
        strokeWidth="2.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
