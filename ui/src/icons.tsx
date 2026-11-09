// Line icons (20×20, stroked with currentColor) for the chrome.
import type { JSX } from "solid-js";

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
