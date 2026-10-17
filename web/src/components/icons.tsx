import type { SVGProps } from "react";

/*
 * One stroke icon family (24px grid, 1.75 stroke, round joins) so every glyph in the UI
 * shares weight and terminals; nothing is borrowed from emoji or text symbols.
 */
type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function icon(paths: React.ReactNode) {
  return function Icon({ size = 16, ...props }: IconProps) {
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth={1.75}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden
        {...props}
      >
        {paths}
      </svg>
    );
  };
}

export const IconOverview = icon(
  <>
    <rect x="3.5" y="3.5" width="7" height="7" rx="2" />
    <rect x="13.5" y="3.5" width="7" height="7" rx="2" />
    <rect x="3.5" y="13.5" width="7" height="7" rx="2" />
    <rect x="13.5" y="13.5" width="7" height="7" rx="2" />
  </>,
);
export const IconSliders = icon(
  <>
    <path d="M4 7h9M17 7h3M4 17h3M11 17h9" />
    <circle cx="15" cy="7" r="2" />
    <circle cx="9" cy="17" r="2" />
  </>,
);
export const IconSun = icon(
  <>
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2.5v2M12 19.5v2M4.6 4.6l1.4 1.4M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4 6 18M18 6l1.4-1.4" />
  </>,
);
export const IconMoon = icon(<path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5Z" />);
export const IconLogIn = icon(
  <>
    <path d="M14 4h4a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-4" />
    <path d="M10 16l4-4-4-4M14 12H4" />
  </>,
);
export const IconLogOut = icon(
  <>
    <path d="M10 4H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h4" />
    <path d="M16 16l4-4-4-4M20 12H10" />
  </>,
);
export const IconAlert = icon(
  <>
    <path d="M10.3 4.2 2.9 17a2 2 0 0 0 1.7 3h14.8a2 2 0 0 0 1.7-3L13.7 4.2a2 2 0 0 0-3.4 0Z" />
    <path d="M12 9.5v4M12 17h.01" />
  </>,
);
export const IconCheck = icon(<path d="m5 12.5 4.5 4.5L19 7.5" />);
export const IconDown = icon(<path d="M12 5v14M6 13l6 6 6-6" />);
export const IconUp = icon(<path d="M12 19V5M6 11l6-6 6 6" />);
export const IconBack = icon(<path d="M15 6l-6 6 6 6" />);
export const IconChevron = icon(<path d="m9 6 6 6-6 6" />);
export const IconSearch = icon(
  <>
    <circle cx="11" cy="11" r="6.5" />
    <path d="m20 20-4.2-4.2" />
  </>,
);
export const IconPlus = icon(<path d="M12 5v14M5 12h14" />);
export const IconCopy = icon(
  <>
    <rect x="8.5" y="8.5" width="11.5" height="11.5" rx="2.5" />
    <path d="M15.5 8.5V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7.5a2 2 0 0 0 2 2h2.5" />
  </>,
);
export const IconServer = icon(
  <>
    <rect x="3.5" y="4" width="17" height="7" rx="2" />
    <rect x="3.5" y="13" width="17" height="7" rx="2" />
    <path d="M7.5 7.5h.01M7.5 16.5h.01" />
  </>,
);
export const IconBell = icon(
  <>
    <path d="M6 16V11a6 6 0 1 1 12 0v5l1.5 2h-15Z" />
    <path d="M10 21h4" />
  </>,
);
export const IconHistory = icon(
  <>
    <path d="M3.5 12a8.5 8.5 0 1 0 2.5-6" />
    <path d="M3.5 4v4h4M12 8v4l3 2" />
  </>,
);
export const IconSend = icon(<path d="M20.5 3.5 10 14M20.5 3.5 14 20.5l-4-6.5-6.5-4Z" />);
export const IconClock = icon(
  <>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 7.5V12l3 2" />
  </>,
);
export const IconCpu = icon(
  <>
    <rect x="6" y="6" width="12" height="12" rx="2.5" />
    <path d="M9.5 2.5v3M14.5 2.5v3M9.5 18.5v3M14.5 18.5v3M2.5 9.5h3M2.5 14.5h3M18.5 9.5h3M18.5 14.5h3" />
  </>,
);
export const IconLock = icon(
  <>
    <rect x="4.5" y="10.5" width="15" height="10" rx="2.5" />
    <path d="M8 10.5V8a4 4 0 0 1 8 0v2.5" />
  </>,
);
export const IconGlobe = icon(
  <>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M3.5 12h17M12 3.5c2.4 2.6 3.5 5.4 3.5 8.5s-1.1 5.9-3.5 8.5c-2.4-2.6-3.5-5.4-3.5-8.5S9.6 6.1 12 3.5Z" />
  </>,
);
export const IconPencil = icon(<path d="M15.5 4.5l4 4L8.5 19.5H4.5v-4Z M13 7l4 4" />);
export const IconKey = icon(
  <>
    <circle cx="8" cy="15" r="4" />
    <path d="m11 12 8.5-8.5M16 7l2.5 2.5M14 9l2 2" />
  </>,
);
export const IconTrash = icon(
  <>
    <path d="M4.5 7h15M10 11v6M14 11v6" />
    <path d="M6.5 7l1 12a2 2 0 0 0 2 1.8h5a2 2 0 0 0 2-1.8l1-12M9 7V4.5h6V7" />
  </>,
);
export const IconPower = icon(
  <>
    <path d="M12 3.5V12" />
    <path d="M6.6 7a7.5 7.5 0 1 0 10.8 0" />
  </>,
);

/**
 * The mark: a single continuous curve that reads as a lowercase "m" and as a smoothed
 * metric line, the same curve language the charts use.
 */
export function Logo({ className = "size-7" }: { className?: string }) {
  return (
    <svg viewBox="0 0 32 32" className={className} aria-hidden>
      <defs>
        <linearGradient id="moni-mark" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#7a6af0" />
          <stop offset="1" stopColor="#4a3aa7" />
        </linearGradient>
      </defs>
      <rect width="32" height="32" rx="9" fill="url(#moni-mark)" />
      <path
        d="M7 22V14.5C7 11.5 8.8 10 10.75 10S14.5 11.5 14.5 14.5V19M14.5 14.5c0-3 1.8-4.5 3.75-4.5S22 11.5 22 14.5V18.5c0 2 1 3.5 3 3.5"
        fill="none"
        stroke="#fff"
        strokeWidth="2.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
