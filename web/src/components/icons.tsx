import {
  Activity,
  ArrowDown,
  ArrowUp,
  Bell,
  Check,
  ChevronLeft,
  ChevronRight,
  Clock,
  Copy,
  Cpu,
  Earth,
  Gauge,
  HardDrive,
  History,
  KeyRound,
  LayoutGrid,
  Lock,
  LogIn,
  LogOut,
  type LucideIcon,
  type LucideProps,
  MemoryStick,
  Moon,
  Pencil,
  Plus,
  Power,
  Search,
  Send,
  Server,
  SlidersHorizontal,
  Sun,
  TriangleAlert,
  Trash2,
  Unplug,
} from "lucide-react";

/*
 * Lucide gives every glyph the same 24px grid and optical sizing; the wrapper pins one stroke
 * weight in screen pixels, so a 12px glyph and a 20px glyph both match the text stems beside them.
 */
type IconProps = Omit<LucideProps, "ref"> & { size?: number };

function icon(Glyph: LucideIcon) {
  return function Icon({ size = 16, ...props }: IconProps) {
    return <Glyph size={size} strokeWidth={1.5} absoluteStrokeWidth aria-hidden {...props} />;
  };
}

export const IconOverview = icon(LayoutGrid);
export const IconSliders = icon(SlidersHorizontal);
export const IconSun = icon(Sun);
export const IconMoon = icon(Moon);
export const IconLogIn = icon(LogIn);
export const IconLogOut = icon(LogOut);
export const IconAlert = icon(TriangleAlert);
export const IconCheck = icon(Check);
export const IconDown = icon(ArrowDown);
export const IconUp = icon(ArrowUp);
export const IconBack = icon(ChevronLeft);
export const IconChevron = icon(ChevronRight);
export const IconSearch = icon(Search);
export const IconPlus = icon(Plus);
export const IconCopy = icon(Copy);
export const IconServer = icon(Server);
export const IconBell = icon(Bell);
export const IconHistory = icon(History);
export const IconSend = icon(Send);
export const IconClock = icon(Clock);
export const IconCpu = icon(Cpu);
export const IconMemory = icon(MemoryStick);
export const IconDisk = icon(HardDrive);
export const IconActivity = icon(Activity);
export const IconGauge = icon(Gauge);
export const IconLock = icon(Lock);
export const IconGlobe = icon(Earth);
export const IconPencil = icon(Pencil);
export const IconKey = icon(KeyRound);
export const IconTrash = icon(Trash2);
export const IconPower = icon(Power);
export const IconUnplug = icon(Unplug);

/** Lucide dropped brand marks; this is GitHub's own mark, filled, on the same 24px grid. */
export function IconGitHub({ size = 16, className }: { size?: number; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden className={className}>
      <path d="M12 .3a12 12 0 0 0-3.8 23.38c.6.12.83-.26.83-.57L9 21.07c-3.34.72-4.04-1.61-4.04-1.61-.55-1.39-1.34-1.76-1.34-1.76-1.08-.74.09-.73.09-.73 1.2.09 1.84 1.24 1.84 1.24 1.07 1.83 2.81 1.3 3.5 1 .1-.78.42-1.31.76-1.61-2.67-.3-5.47-1.33-5.47-5.93 0-1.31.47-2.38 1.24-3.22-.14-.3-.54-1.52.1-3.18 0 0 1-.32 3.3 1.23a11.5 11.5 0 0 1 6 0c2.28-1.55 3.29-1.23 3.29-1.23.64 1.66.24 2.88.12 3.18a4.65 4.65 0 0 1 1.23 3.22c0 4.61-2.8 5.63-5.48 5.92.42.36.81 1.1.81 2.22l-.01 3.29c0 .32.21.69.82.57A12 12 0 0 0 12 .3" />
    </svg>
  );
}

/**
 * The mark: a station clock at ten past ten, its red second hand the only colour. Fixed inks,
 * so it reads the same on the black shell, on paper and on the dark theme.
 */
export function Logo({ className = "size-7" }: { className?: string }) {
  return (
    <svg viewBox="0 0 32 32" className={className} aria-hidden>
      <circle cx="16" cy="16" r="15" fill="#f3f3f0" stroke="#111" strokeWidth="1.5" />
      <path d="M16 3.2v2.6M28.8 16h-2.6M16 28.8v-2.6M3.2 16h2.6" stroke="#111" strokeWidth="1.6" />
      <path d="M16 16 11.4 13.3M16 16 22.9 9.1" stroke="#111" strokeWidth="2.4" strokeLinecap="round" />
      <path d="M16 20.5V6.8" stroke="#e63312" strokeWidth="1.2" />
      <circle cx="16" cy="7.4" r="2.3" fill="#e63312" />
      <circle cx="16" cy="16" r="1.3" fill="#111" />
    </svg>
  );
}
