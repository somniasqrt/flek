import { createSignal } from "solid-js";

// An explicit unread action survives scrolling/focus until this conversation is left.
export const [pausedRead, pauseRead] = createSignal<string | null>(null);

export const [unreadBoundary, setUnreadBoundary] = createSignal<{ id: string; ts: string } | null>(null);
