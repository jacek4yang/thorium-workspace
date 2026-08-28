/**
 * The icon set.
 *
 * Inline SVG rather than an icon font or a package: a portable executable
 * should not carry a webfont for eleven glyphs, and inline paths inherit
 * `currentColor` so they follow the theme without extra rules.
 */
export type IconName =
  | "dashboard"
  | "profiles"
  | "accounts"
  | "browser"
  | "vault"
  | "settings"
  | "diagnostics"
  | "lock"
  | "unlock"
  | "plus"
  | "copy"
  | "eye"
  | "eye-off"
  | "trash"
  | "play"
  | "stop"
  | "refresh"
  | "check"
  | "alert"
  | "external"
  | "download"
  | "image"
  | "clipboard"
  | "screen"
  | "key";

const PATHS: Record<IconName, string> = {
  dashboard: "M3 3h7v7H3zM14 3h7v4h-7zM14 10h7v11h-7zM3 13h7v8H3z",
  profiles: "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z",
  accounts:
    "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 20a8 8 0 0 1 16 0",
  browser: "M3 5h18v14H3zM3 9h18M7 7h.01M10 7h.01",
  vault: "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM12 12h4",
  settings:
    "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-1.8-.3 1.6 1.6 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1A1.6 1.6 0 0 0 9 19.4a1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0 .3-1.8 1.6 1.6 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1A1.6 1.6 0 0 0 4.6 9a1.6 1.6 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.8.3H9a1.6 1.6 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1z",
  diagnostics: "M3 12h4l3 8 4-16 3 8h4",
  lock: "M6 11h12v10H6zM9 11V7a3 3 0 0 1 6 0v4",
  unlock: "M6 11h12v10H6zM9 11V7a3 3 0 0 1 5.6-1.5",
  plus: "M12 5v14M5 12h14",
  copy: "M9 9h11v11H9zM5 15H4V4h11v1",
  eye: "M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7-10-7-10-7zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z",
  "eye-off": "M3 3l18 18M10.6 10.6a3 3 0 0 0 4.2 4.2M9.4 5.3A9.7 9.7 0 0 1 12 5c6.4 0 10 7 10 7a17 17 0 0 1-3.2 4M6.2 6.6A17 17 0 0 0 2 12s3.6 7 10 7c1.3 0 2.5-.3 3.5-.7",
  trash: "M4 7h16M9 7V5h6v2M6 7l1 13h10l1-13M10 11v6M14 11v6",
  play: "M7 4l12 8-12 8z",
  stop: "M6 6h12v12H6z",
  refresh: "M20 11a8 8 0 1 0-1.4 5M20 6v5h-5",
  check: "M4 12.5l5 5L20 6.5",
  alert: "M12 4l9 16H3zM12 10v4M12 17h.01",
  external: "M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5",
  download: "M12 3v12M7 11l5 5 5-5M4 20h16",
  image: "M3 5h18v14H3zM7 11a1.6 1.6 0 1 0 0-3.2A1.6 1.6 0 0 0 7 11zM21 16l-5-5-9 8",
  clipboard: "M9 4h6v3H9zM7 5H5v16h14V5h-2M9 12h6M9 16h4",
  screen: "M3 5h18v11H3zM9 20h6M12 16v4",
  key: "M15 3a6 6 0 1 1-5.2 9L4 18v3h3l1-1h2v-2h2l1.6-1.6A6 6 0 0 1 15 3zM17 7h.01",
};

interface IconProps {
  name: IconName;
  className?: string;
  size?: number;
}

/** Renders one icon. Decorative by default; give it a `title` to announce it. */
export function Icon({ name, className, size = 16 }: IconProps) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}

/** The application mark, used in the sidebar and on the onboarding screen. */
export function BrandMark({ className, size = 28 }: { className?: string; size?: number }) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 48 48"
      aria-hidden="true"
      focusable="false"
    >
      <rect width="48" height="48" rx="11" fill="#121821" />
      <g fill="none" strokeWidth="3.4">
        <ellipse cx="24" cy="24" rx="15" ry="6.6" stroke="#4ed19a" transform="rotate(30 24 24)" />
        <ellipse cx="24" cy="24" rx="15" ry="6.6" stroke="#3898ff" transform="rotate(-30 24 24)" />
        <ellipse cx="24" cy="24" rx="15" ry="6.6" stroke="#4ed19a" transform="rotate(90 24 24)" />
      </g>
      <circle cx="24" cy="24" r="4.1" fill="#f0f6fc" />
    </svg>
  );
}
