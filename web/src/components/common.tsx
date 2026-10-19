import { Meter } from "@base-ui/react/meter";
import { cn } from "@/lib/utils";
import { CheckIcon, CopyIcon } from "lucide-react";
import { type ReactElement, type ReactNode, useState, useSyncExternalStore } from "react";
import { Button } from "@/components/ui/button";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Drawer, DrawerContent, DrawerDescription, DrawerHeader, DrawerTitle } from "@/components/ui/drawer";
import { Spinner } from "@/components/ui/spinner";
import { toast } from "@/components/ui/toast";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { errorText } from "@/lib/api";

/** Online/offline indicator: colour plus text, never colour alone; offline is a hollow ring. */
export function StatusDot({ online, label = true }: { online: boolean; label?: boolean }) {
  return (
    <span className={cn("inline-flex h-5 shrink-0 items-center gap-1.5 text-xs leading-none font-medium", online ? "text-good" : "text-muted-foreground")}>
      <span aria-hidden className={cn("size-2 rounded-full", online ? "bg-good" : "border-[1.5px] border-muted-foreground")} />
      {label && (online ? "在线" : "离线")}
    </span>
  );
}

/** Fill colour for a utilisation percentage: the accent normally, status colours near the limit. */
export const levelColor = (v: number | null) =>
  v == null ? "transparent" : v >= 90 ? "var(--status-critical)" : v >= 80 ? "var(--status-warning)" : "var(--series-1)";

/** The bare bar of a usage meter; `.meter-track` lets a theme restyle every one of them. */
export function MeterBar({ value, label, className }: { value: number | null; label: string; className?: string }) {
  const v = value == null ? 0 : Math.min(100, Math.max(0, value));
  return (
    <Meter.Root value={v} aria-label={label}>
      <Meter.Track className={cn("meter-track h-1.5 overflow-hidden rounded-full bg-muted", className)}>
        <Meter.Indicator className="h-full rounded-full transition-[width] duration-500 ease-out" style={{ background: levelColor(value) }} />
      </Meter.Track>
    </Meter.Root>
  );
}

/**
 * Ratio against a limit. The fill switches to a status colour above the warning / critical
 * thresholds; the numeric value is always printed beside it.
 */
export function UsageMeter({ label, value, detail }: { label: string; value: number | null; detail?: string }) {
  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between gap-2 text-xs leading-4">
        <span className="shrink-0 text-muted-foreground">{label}</span>
        <span className="tabular truncate">
          {detail && <span className="mr-2 text-muted-foreground">{detail}</span>}
          <span className="inline-block min-w-[3.25em] text-right font-medium">{value == null ? "–" : `${value.toFixed(1)}%`}</span>
        </span>
      </div>
      <MeterBar label={label} value={value} />
    </div>
  );
}

export function EmptyState({ title, icon, children }: { title: string; icon?: ReactNode; children?: ReactNode }) {
  return (
    <Empty className="border border-line-strong py-14">
      <EmptyHeader>
        {icon && <EmptyMedia variant="icon">{icon}</EmptyMedia>}
        <EmptyTitle>{title}</EmptyTitle>
        {children && <EmptyDescription>{children}</EmptyDescription>}
      </EmptyHeader>
    </Empty>
  );
}

export function PageSpinner() {
  return (
    <div className="grid place-items-center py-28">
      <Spinner className="size-5 text-muted-foreground" />
    </div>
  );
}

export function ErrorText({ children }: { children: ReactNode }) {
  return children ? <p className="text-sm text-destructive">{children}</p> : null;
}

/** Reports a failed action that has no form to show the error next to. */
export function toastError(err: unknown) {
  toast.add({ title: "操作失败", description: errorText(err), type: "error" });
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
      {done ? <CheckIcon data-icon="inline-start" /> : <CopyIcon data-icon="inline-start" />}
      {done ? "已复制" : "复制"}
    </Button>
  );
}

/** Single-choice pill group; the animated highlight slides between options. */
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
    <ToggleGroup
      aria-label={label}
      value={[value]}
      // A pill group always has one choice; ignore the "deselect" a second click would send.
      onValueChange={(v) => v[0] && onChange(v[0] as T)}
      spacing={0}
      className="no-scrollbar max-w-full overflow-x-auto rounded-lg bg-muted p-0.5"
    >
      {options.map((o) => (
        <ToggleGroupItem
          key={o.id}
          value={o.id}
          size="sm"
          className="tabular h-7 rounded-md! px-3 text-xs text-muted-foreground hover:bg-transparent data-pressed:bg-card data-pressed:text-foreground data-pressed:shadow-xs"
        >
          {o.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}

const DESKTOP = "(min-width: 640px)";
const subscribeDesktop = (cb: () => void) => {
  const m = matchMedia(DESKTOP);
  m.addEventListener("change", cb);
  return () => m.removeEventListener("change", cb);
};

export function useDesktop() {
  return useSyncExternalStore(subscribeDesktop, () => matchMedia(DESKTOP).matches);
}

/** A centred dialog from sm up, a swipe-to-dismiss bottom sheet on phones. */
export function ResponsiveDialog({
  open,
  onOpenChange,
  title,
  description,
  className,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  const desktop = useDesktop();
  if (desktop)
    return (
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className={className}>
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            {description && <DialogDescription>{description}</DialogDescription>}
          </DialogHeader>
          {children}
        </DialogContent>
      </Dialog>
    );
  return (
    <Drawer open={open} onOpenChange={onOpenChange} showSwipeHandle>
      <DrawerContent className="rounded-t-2xl!">
        <DrawerHeader className="text-left">
          <DrawerTitle>{title}</DrawerTitle>
          {description && <DrawerDescription>{description}</DrawerDescription>}
        </DrawerHeader>
        <div className={cn("overflow-y-auto p-4 pb-[max(1rem,env(safe-area-inset-bottom))]", className)}>{children}</div>
      </DrawerContent>
    </Drawer>
  );
}

/** Asks before an irreversible action; `trigger` is the button that opens it. */
export function ConfirmAction({
  trigger,
  title,
  description,
  action,
  destructive = true,
  onConfirm,
}: {
  trigger: ReactElement;
  title: string;
  description: ReactNode;
  action: string;
  destructive?: boolean;
  onConfirm: () => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    try {
      await onConfirm();
      setOpen(false);
    } catch (err) {
      toastError(err);
    } finally {
      setBusy(false);
    }
  };
  return (
    <AlertDialog open={open} onOpenChange={setOpen}>
      <AlertDialogTrigger render={trigger} />
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>取消</AlertDialogCancel>
          <AlertDialogAction variant={destructive ? "destructive" : "default"} disabled={busy} onClick={() => void run()}>
            {busy && <Spinner data-icon="inline-start" />}
            {action}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
