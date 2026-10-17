import { type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode, useEffect, useId, useRef } from "react";

const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(" ");
export { cx };

type Variant = "primary" | "secondary" | "ghost" | "danger";
const VARIANTS: Record<Variant, string> = {
  primary: "bg-zinc-900 text-white hover:bg-zinc-700 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-white",
  secondary:
    "border border-zinc-200 bg-white hover:bg-zinc-50 dark:border-zinc-800 dark:bg-zinc-900 dark:hover:bg-zinc-800",
  ghost: "hover:bg-zinc-100 dark:hover:bg-zinc-800",
  danger: "bg-red-600 text-white hover:bg-red-500",
};

export function Button({
  variant = "secondary",
  size = "md",
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md" }) {
  return (
    <button
      {...props}
      className={cx(
        "inline-flex items-center justify-center gap-1.5 rounded-md font-medium transition-colors disabled:pointer-events-none disabled:opacity-50",
        size === "sm" ? "h-7 px-2.5 text-xs" : "h-9 px-3.5 text-sm",
        VARIANTS[variant],
        className,
      )}
    />
  );
}

export function Input({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={cx(
        "h-9 w-full rounded-md border border-zinc-200 bg-white px-3 text-sm placeholder:text-zinc-400 dark:border-zinc-800 dark:bg-zinc-900",
        className,
      )}
    />
  );
}

export function Select({ className, ...props }: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className={cx(
        "h-9 w-full rounded-md border border-zinc-200 bg-white px-2.5 text-sm dark:border-zinc-800 dark:bg-zinc-900",
        className,
      )}
    />
  );
}

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="block space-y-1.5">
      <span className="text-xs font-medium text-zinc-600 dark:text-zinc-400">{label}</span>
      {children}
      {hint && <span className="block text-xs text-zinc-500">{hint}</span>}
    </label>
  );
}

export function Card({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <div className={cx("rounded-lg border border-zinc-200 bg-white dark:border-zinc-800 dark:bg-zinc-900", className)}>
      {children}
    </div>
  );
}

/** Online/offline indicator: colour plus text, never colour alone. */
export function StatusDot({ online, label = true }: { online: boolean; label?: boolean }) {
  return (
    <span className="inline-flex items-center gap-1.5 text-xs text-zinc-600 dark:text-zinc-400">
      <span
        aria-hidden
        className={cx("size-2 rounded-full", online ? "bg-[var(--status-good)]" : "bg-zinc-400 dark:bg-zinc-600")}
      />
      {label && (online ? "在线" : "离线")}
    </span>
  );
}

export function Badge({ tone = "neutral", children }: { tone?: "neutral" | "critical" | "good"; children: ReactNode }) {
  const tones = {
    neutral: "bg-zinc-100 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300",
    critical: "bg-red-50 text-red-700 ring-1 ring-red-200 dark:bg-red-950/50 dark:text-red-300 dark:ring-red-900",
    good: "bg-emerald-50 text-emerald-700 dark:bg-emerald-950/50 dark:text-emerald-300",
  };
  return <span className={cx("inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-medium", tones[tone])}>{children}</span>;
}

/**
 * Ratio against a limit. The fill switches to a status colour above the warning / critical
 * thresholds; the numeric value is always printed beside it.
 */
export function Meter({ label, value, detail }: { label: string; value: number | null; detail?: string }) {
  const v = value == null ? 0 : Math.min(100, Math.max(0, value));
  const fill =
    value == null
      ? "transparent"
      : v >= 90
        ? "var(--status-critical)"
        : v >= 80
          ? "var(--status-warning)"
          : "var(--series-1)";
  return (
    <div className="space-y-1">
      <div className="flex items-baseline justify-between gap-2 text-xs">
        <span className="text-zinc-500">{label}</span>
        <span className="tabular truncate text-zinc-700 dark:text-zinc-300">
          {value == null ? "–" : `${value.toFixed(1)}%`}
          {detail && <span className="ml-1 text-zinc-500">{detail}</span>}
        </span>
      </div>
      <div
        className="h-1.5 overflow-hidden rounded-full bg-zinc-100 dark:bg-zinc-800"
        role="meter"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={value ?? undefined}
      >
        <div className="h-full rounded-full transition-[width] duration-500" style={{ width: `${v}%`, background: fill }} />
      </div>
    </div>
  );
}

export function Spinner({ className }: { className?: string }) {
  return (
    <span
      aria-label="加载中"
      className={cx("inline-block size-4 animate-spin rounded-full border-2 border-zinc-300 border-t-zinc-700 dark:border-zinc-700 dark:border-t-zinc-200", className)}
    />
  );
}

export function Modal({ open, onClose, title, children }: { open: boolean; onClose: () => void; title: string; children: ReactNode }) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) d.showModal();
    if (!open && d.open) d.close();
  }, [open]);
  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      onClose={onClose}
      onClick={(e) => e.target === ref.current && onClose()}
      className="m-auto w-[min(32rem,calc(100vw-2rem))] rounded-lg border border-zinc-200 bg-white p-0 text-zinc-900 shadow-xl backdrop:bg-zinc-950/40 dark:border-zinc-800 dark:bg-zinc-900 dark:text-zinc-100"
    >
      {open && (
        <div className="space-y-4 p-5">
          <h2 id={titleId} className="text-base font-semibold">
            {title}
          </h2>
          {children}
        </div>
      )}
    </dialog>
  );
}

export function EmptyState({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="rounded-lg border border-dashed border-zinc-300 px-6 py-12 text-center dark:border-zinc-700">
      <p className="text-sm font-medium">{title}</p>
      {children && <div className="mt-2 text-sm text-zinc-500">{children}</div>}
    </div>
  );
}

export function ErrorText({ children }: { children: ReactNode }) {
  return children ? <p className="text-sm text-red-600 dark:text-red-400">{children}</p> : null;
}

export function CopyButton({ text }: { text: string }) {
  return (
    <Button
      size="sm"
      variant="ghost"
      onClick={(e) => {
        const btn = e.currentTarget;
        void navigator.clipboard.writeText(text).then(() => {
          btn.textContent = "已复制";
          setTimeout(() => (btn.textContent = "复制"), 1500);
        });
      }}
    >
      复制
    </Button>
  );
}
