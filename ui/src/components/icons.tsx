import type { JSX } from "solid-js";

// Line icons (20×20, stroked with currentColor) for the chrome.

const Icon = (props: { size?: number; children: JSX.Element }) => (
  <svg
    class="icon"
    viewBox="0 0 20 20"
    width={props.size ?? 16}
    height={props.size ?? 16}
    aria-hidden="true"
  >
    {props.children}
  </svg>
);

export const Hash = (p: { size?: number }) => (
  <Icon size={p.size}>
    <path d="M8 3L6.5 17M13.5 3L12 17M3.5 7.5h14M2.5 12.5h14" />
  </Icon>
);

export const Lock = (p: { size?: number }) => (
  <Icon size={p.size}>
    <rect x="4.5" y="9" width="11" height="8" rx="1.5" />
    <path d="M7 9V6.5a3 3 0 0 1 6 0V9" />
  </Icon>
);

export const People = (p: { size?: number }) => (
  <Icon size={p.size}>
    <circle cx="7.5" cy="7" r="2.75" />
    <path d="M2.5 16c.5-2.75 2.5-4.25 5-4.25s4.5 1.5 5 4.25" />
    <path d="M12.5 4.5a2.6 2.6 0 0 1 0 5M14.5 11.9c1.6.5 2.7 1.8 3 4.1" />
  </Icon>
);

export const Home = (p: { size?: number }) => (
  <Icon size={p.size}>
    <path d="M3.5 9L10 3.5 16.5 9v7a1 1 0 0 1-1 1h-3.5v-5h-4v5H4.5a1 1 0 0 1-1-1z" />
  </Icon>
);

export const Caret = (p: { size?: number }) => (
  <Icon size={p.size}>
    <path d="M7.5 5.5L12 10l-4.5 4.5" />
  </Icon>
);

export const Search = (p: { size?: number }) => (
  <Icon size={p.size}>
    <circle cx="8.5" cy="8.5" r="5" />
    <path d="M12.3 12.3L17 17" />
  </Icon>
);

const starPath = "m10 2 2.4 4.9 5.4.8-3.9 3.8.9 5.4-4.8-2.5-4.8 2.5.9-5.4-3.9-3.8 5.4-.8Z";
export const Star = (p: { size?: number }) => (
  <Icon size={p.size}><path d={starPath} /></Icon>
);
export const StarFilled = (p: { size?: number }) => (
  <Icon size={p.size}><path d={starPath} fill="currentColor" /></Icon>
);
export const Bell = (p: { size?: number; muted?: boolean }) => (
  <Icon size={p.size}>
    <path d={p.muted
      ? "M7 3.9A4.5 4.5 0 0 1 14.5 7v3M5.5 7v3.5L3 14h10M8 17h4M2.5 2.5l15 15"
      : "M5.5 7a4.5 4.5 0 0 1 9 0v3.5L17 14H3l2.5-3.5ZM8 17h4"} />
  </Icon>
);
export const Kebab = (p: { size?: number }) => (
  <Icon size={p.size}>
    <circle cx="10" cy="4" r="1.4" fill="currentColor" stroke="none" />
    <circle cx="10" cy="10" r="1.4" fill="currentColor" stroke="none" />
    <circle cx="10" cy="16" r="1.4" fill="currentColor" stroke="none" />
  </Icon>
);

// One continuous face-and-plus silhouette, rather than a text emoji and a plus.
export const AddReaction = (p: { size?: number }) => (
  <Icon size={p.size ?? 20}>
    <path d="M10 3.5a7 7 0 1 0 6.5 6.5M14.5 2v5M12 4.5h5" />
    <circle cx="7" cy="8.5" r=".6" fill="currentColor" stroke="none" />
    <circle cx="11.5" cy="8.5" r=".6" fill="currentColor" stroke="none" />
    <path d="M6.5 11.5a3.2 3.2 0 0 0 5.5 0" />
  </Icon>
);

export const ReplyInThread = (p: { size?: number }) => (
  <Icon size={p.size ?? 20}>
    <path d="M16.5 14.5a7.5 7.5 0 1 0-3 2l4 1-1-3ZM6.5 7h7M6.5 10h5" />
  </Icon>
);
export const MessageLink = (p: { size?: number }) => (
  <Icon size={p.size ?? 20}>
    <path d="m8 12 4-4M7 13l-1 1a3.5 3.5 0 0 1-5-5l4-4a3.5 3.5 0 0 1 5 0M13 7l1-1a3.5 3.5 0 0 1 5 5l-4 4a3.5 3.5 0 0 1-5 0" />
  </Icon>
);
export const CopyMessage = (p: { size?: number }) => (
  <Icon size={p.size ?? 20}>
    <path d="M3 6V3h14v3M10 3v14M7 17h6" />
  </Icon>
);

export const Moon = (p: { size?: number }) => (
  <Icon size={p.size}>
    <path d="M16.5 12.5A7 7 0 0 1 7.5 3.5a7.25 7.25 0 1 0 9 9Z" />
  </Icon>
);

export const Activity = (p: { size?: number }) => (
  <Icon size={p.size}>
    <path d="M5 13V8a5 5 0 0 1 10 0v5l2 2H3l2-2ZM8 17h4" />
  </Icon>
);
export const Document = (p: { size?: number }) => (
  <Icon size={p.size}>
    <path d="M5 2h7l4 4v12H5zM12 2v5h4M8 10h5M8 13h5" />
  </Icon>
);
export const Plus = (p: { size?: number }) => (
  <Icon size={p.size}>
    <path d="M10 3v14M3 10h14" />
  </Icon>
);

export const Gear = (p: { size?: number }) => (
  <Icon size={p.size}>
    <circle cx="10" cy="10" r="2.5" />
    <path d="M10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2M4.7 4.7l1.4 1.4M13.9 13.9l1.4 1.4M4.7 15.3l1.4-1.4M13.9 6.1l1.4-1.4" />
  </Icon>
);
export const Palette = (p: { size?: number }) => (
  <Icon size={p.size}>
    <path d="M10 2.5a7.5 7.5 0 0 0 0 15c1.2 0 1.8-.8 1.8-1.6 0-1-.9-1.4-.9-2.3 0-.8.7-1.4 1.5-1.4h1.6a3.5 3.5 0 0 0 3.5-3.5C17.5 5.6 14.2 2.5 10 2.5Z" />
    <circle cx="6.5" cy="9" r=".7" fill="currentColor" stroke="none" />
    <circle cx="9" cy="6" r=".7" fill="currentColor" stroke="none" />
    <circle cx="13" cy="6.5" r=".7" fill="currentColor" stroke="none" />
  </Icon>
);
export const SidebarPanel = (p: { size?: number }) => (
  <Icon size={p.size}>
    <rect x="2.5" y="3.5" width="15" height="13" rx="2" />
    <path d="M7.5 3.5v13M4.5 7h1M4.5 9.5h1M4.5 12h1" />
  </Icon>
);
export const ImageIcon = (p: { size?: number }) => (
  <Icon size={p.size}>
    <rect x="2.5" y="3.5" width="15" height="13" rx="2" />
    <circle cx="7" cy="8" r="1.4" />
    <path d="M3 15l4.5-4.5 3 3 2-2 4.5 4.5" />
  </Icon>
);
export const Refresh = (p: { size?: number }) => (
  <Icon size={p.size}>
    <path d="M16.5 10a6.5 6.5 0 0 1-11.3 4.4M3.5 10a6.5 6.5 0 0 1 11.3-4.4M15 2.5v3.3h-3.3M5 17.5v-3.3h3.3" />
  </Icon>
);
export const Person = (p: { size?: number }) => (
  <Icon size={p.size}>
    <circle cx="10" cy="7" r="3" />
    <path d="M3.5 17c.6-3.2 3.2-5 6.5-5s5.9 1.8 6.5 5" />
  </Icon>
);
export const Cross = (p: { size?: number }) => (
  <Icon size={p.size}>
    <path d="M5 5l10 10M15 5L5 15" />
  </Icon>
);
export const Minus = (p: { size?: number }) => (
  <Icon size={p.size}>
    <path d="M4 10h12" />
  </Icon>
);
