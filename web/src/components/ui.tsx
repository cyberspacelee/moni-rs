import {
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import { IconCheck, IconCopy } from "./icons";

const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(" ");
export { cx };

type Variant = "primary" | "secondary" | "ghost" | "danger";
const VARIANTS: Record<Variant, string> = {
  primary:
    "bg-brand text-on-brand shadow-[0_1px_2px_rgb(0_0_0/0.1)] hover:bg-brand-strong",
  secondary: "border border-line bg-surface text-ink shadow-[0_1px_2px_rgb(0_0_0/0.04)] hover:border-line-strong",
  ghost: "text-ink-2 hover:bg-surface-2 hover:text-ink",
  danger: "text-crit hover:bg-crit-soft",
};

export function Button({
  variant = "secondary",
  size = "md",
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md" | "icon" }) {
  return (
    <button
      {...props}
      className={cx(
        "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-lg font-medium leading-none whitespace-nowrap transition-[background-color,border-color,color] duration-150 select-none disabled:pointer-events-none disabled:opacity-45",
        size === "sm" ? "h-8 px-2.5 text-xs" : size === "icon" ? "size-8" : "h-9 px-3.5 text-sm",
        VARIANTS[variant],
        className,
      )}
    />
  );
}

const control =
  "w-full rounded-lg border border-line bg-surface text-sm text-ink shadow-xs transition-colors placeholder:text-ink-3 hover:border-line-strong focus:border-brand focus:outline-none focus:ring-3 focus:ring-brand/15";

export function Input({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} className={cx(control, "h-10 px-3 sm:h-9", className)} />;
}

export function Select({ className, ...props }: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return <select {...props} className={cx(control, "h-10 px-2.5 sm:h-9", className)} />;
}

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="block space-y-1.5">
      <span className="text-xs font-medium text-ink-2">{label}</span>
      {children}
      {hint && <span className="block text-xs text-ink-3">{hint}</span>}
    </label>
  );
}

export function Card({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <div className={cx("card rounded-2xl border border-line bg-surface shadow-card", className)}>
      {children}
    </div>
  );
}

/** Online/offline indicator: colour plus text, never colour alone; offline is a hollow ring. */
export function StatusDot({ online, label = true }: { online: boolean; label?: boolean }) {
  return (
    <span className={cx("inline-flex h-5 shrink-0 items-center gap-1.5 text-xs leading-none font-medium", online ? "text-good" : "text-ink-3")}>
      <span aria-hidden className={cx("size-2 rounded-full", online ? "bg-good" : "border-[1.5px] border-ink-3")} />
      {label && (online ? "在线" : "离线")}
    </span>
  );
}

export function Badge({
  tone = "neutral",
  children,
  className,
}: {
  tone?: "neutral" | "critical" | "good" | "brand";
  children: ReactNode;
  className?: string;
}) {
  const tones = {
    neutral: "bg-surface-2 text-ink-2",
    critical: "bg-crit-soft text-crit",
    good: "bg-good-soft text-good",
    brand: "bg-brand-soft text-brand-ink",
  };
  return (
    <span
      className={cx(
        "inline-flex h-5 items-center gap-1 rounded-pill px-2 text-[11px] leading-none font-medium whitespace-nowrap",
        tones[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}

/** Fill colour for a utilisation percentage: the accent normally, status colours near the limit. */
export const levelColor = (v: number | null) =>
  v == null ? "transparent" : v >= 90 ? "var(--status-critical)" : v >= 80 ? "var(--status-warning)" : "var(--series-1)";

/**
 * Ratio against a limit. The fill switches to a status colour above the warning / critical
 * thresholds; the numeric value is always printed beside it.
 */
export function Meter({ label, value, detail }: { label: string; value: number | null; detail?: string }) {
  const v = value == null ? 0 : Math.min(100, Math.max(0, value));
  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between gap-2 text-xs leading-4">
        <span className="shrink-0 text-ink-3">{label}</span>
        <span className="tabular truncate">
          {detail && <span className="mr-2 text-ink-3">{detail}</span>}
          <span className="inline-block min-w-[3.25em] text-right font-medium text-ink">{value == null ? "–" : `${value.toFixed(1)}%`}</span>
        </span>
      </div>
      <div
        className="meter-track h-1.5 overflow-hidden rounded-full bg-surface-2"
        role="meter"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={value ?? undefined}
      >
        <div
          className="h-full rounded-full transition-[width] duration-500 ease-out"
          style={{ width: `${v}%`, background: levelColor(value) }}
        />
      </div>
    </div>
  );
}

export function Spinner({ className }: { className?: string }) {
  return (
    <span
      aria-label="加载中"
      className={cx("inline-block size-4 animate-spin rounded-full border-2 border-line-strong border-t-brand", className)}
    />
  );
}

export function PageSpinner() {
  return (
    <div className="grid place-items-center py-28">
      <Spinner className="size-5" />
    </div>
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
      // Bottom sheet on phones, centred dialog from sm up.
      className="mx-auto mt-auto mb-0 max-h-[90dvh] w-full max-w-none rounded-t-2xl border border-line bg-surface p-0 text-ink shadow-2xl backdrop:bg-black/35 sm:m-auto sm:w-[min(34rem,calc(100vw-2rem))] sm:rounded-2xl"
    >
      {open && (
        <div className="space-y-5 p-5 pb-[max(1.25rem,env(safe-area-inset-bottom))] sm:p-6">
          <div aria-hidden className="mx-auto -mt-2 h-1 w-10 rounded-full bg-line-strong sm:hidden" />
          <h2 id={titleId} className="text-base font-semibold">
            {title}
          </h2>
          {children}
        </div>
      )}
    </dialog>
  );
}

export function EmptyState({ title, icon, children }: { title: string; icon?: ReactNode; children?: ReactNode }) {
  return (
    <div className="rounded-2xl border border-dashed border-line-strong px-6 py-14 text-center">
      {icon && <div className="mx-auto mb-3 grid size-10 place-items-center rounded-xl bg-surface-2 text-ink-3">{icon}</div>}
      <p className="text-sm font-medium">{title}</p>
      {children && <div className="mt-1.5 text-sm text-ink-3">{children}</div>}
    </div>
  );
}

export function ErrorText({ children }: { children: ReactNode }) {
  return children ? <p className="text-sm text-crit">{children}</p> : null;
}

export function CopyButton({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  return (
    <Button
      size="sm"
      variant="ghost"
      onClick={() => {
        void navigator.clipboard.writeText(text).then(() => {
          setDone(true);
          setTimeout(() => setDone(false), 1500);
        });
      }}
    >
      {done ? <IconCheck size={14} /> : <IconCopy size={14} />}
      {done ? "已复制" : "复制"}
    </Button>
  );
}

/** Segmented control: tabs that read as one pill. Scrolls horizontally on narrow screens. */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: readonly { id: T; label: ReactNode }[];
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <div role="tablist" aria-label={label} className="no-scrollbar inline-flex max-w-full overflow-x-auto rounded-lg bg-surface-2 p-0.5">
      {options.map((o) => (
        <button
          key={o.id}
          role="tab"
          aria-selected={value === o.id}
          onClick={() => onChange(o.id)}
          className={cx(
            "tabular inline-flex h-7 shrink-0 items-center gap-1.5 rounded-md px-3 text-xs leading-none font-medium transition-colors",
            value === o.id ? "bg-surface text-ink shadow-[0_1px_3px_rgb(0_0_0/0.1)]" : "text-ink-3 hover:text-ink",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
