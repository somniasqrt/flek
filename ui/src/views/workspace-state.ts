import {
batch,
createEffect,
createMemo,
createSignal,
on,
onCleanup,
onMount
} from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { t } from "../i18n/index";
import {
api,
onChange,
type Change,
type ConnectionState,
type Conversation,
type DirectMessage,
type Hit,
type Progress,
type Status,
} from "../api";
import { setMe } from "../stores/outbox";
import { loadPeople } from "../stores/people";
import { directoryChanged, refreshPresence, watchPresence, tickPresence } from "../stores/presence-store";
import { closeProfile, profileUser } from "../stores/profile";
import {
type ConversationMenuTarget
} from "../components/ConversationMenu";
import type { ChannelAction } from "../api";
import { openProfile } from "../stores/profile";
import { readRail, type RailView } from "../models/rail-model";
import { jumpOf, type Jump } from "./Search";
import { isViewing } from "../components/Viewer";
import {
DEFAULT_SIDEBAR,
DEFAULT_DETAIL, panelSizes,
readPanelSize,
savePanelSize
} from "../stores/panel-sizes";

type Ready = Extract<Status, { phase: "ready" }>;

const LAST_KEY = "flek.lastConversation";

export function useWorkspace(props: { status: Ready }) {
  // A store reconciled by id: a refresh only touches rows that changed, so
  // the sidebar does not re-render (and flicker) on every sync step.
  setMe(props.status.user_id);
  const [conversations, setConversations] = createStore<Conversation[]>([]);
  const viewKey = `flek.rail.${props.status.team_id}.${props.status.user_id}`;
  const [view, setView] = createSignal<RailView>(readRail(viewKey));
  const [icon, setIcon] = createSignal<string | null>(null);
  const [dms, setDms] = createSignal<DirectMessage[]>([]);
  const [activityBadge, setActivityBadge] = createSignal(0);
  const [revision, setRevision] = createSignal(0);
  const [fileCategory, setFileCategory] = createSignal("all");
  const [compose, setCompose] = createSignal(false);
  const [readError, setReadError] = createSignal("");
  const [actionError, setActionError] = createSignal("");
  const [channelNotice, setChannelNotice] = createSignal<"purpose_failed" | "catch_up_failed" | null>(null);
  const [leaving, setLeaving] = createSignal<Conversation | null>(null);
  const [menu, setMenu] = createSignal<ConversationMenuTarget | null>(null);
  const [channelDetails, setChannelDetails] = createSignal<string | null>(null);
  const menuConversation = () => conversations.find((c) => c.id === menu()?.id);
  const details = (c: Conversation) => {
    if (c.kind === "im" || c.kind === "mpim") {
      const user = c.dm_user_id ?? dms().find((dm) => dm.id === c.id)?.members[0];
      if (user) openProfile(user);
      else setActionError(t("No cached profile for this conversation"));
    } else {
      setThread(null);
      closeProfile();
      setChannelDetails(c.id);
    }
  };
  const chooseView = (next: RailView) => {
    setView(next);
    setBrowsing(false);
    setSearching(null);
    if (next === "dms" && !dms().some((dm) => dm.id === selected())) {
      const first = dms()[0];
      if (first) select(first.id);
    }
    try {
      localStorage.setItem(viewKey, next);
    } catch {
      /* Optional preference. */
    }
  };
  const [browsing, setBrowsing] = createSignal(false);
  const [creatingChannel, setCreatingChannel] = createSignal(false);
  const [channelsMenu, setChannelsMenu] = createSignal<{ x: number; y: number } | null>(null);
  const [preview, setPreview] = createSignal<Conversation | null>(null);
  let openingChannel = 0;
  const [loaded, setLoaded] = createSignal(false);
  const [selected, setSelected] = createSignal<string | null>(readLast());
  const [progress, setProgress] = createSignal<Progress | null>(null);
  const [connection, setConnection] = createSignal<ConnectionState | null>(null);
  // Changes for the open conversation, handed to the timeline.
  const [change, setChange] = createSignal<Change | null>(null, {
    equals: false,
  });
  // The query whose results fill the main pane, and the message to reveal
  // after opening a result (a fresh object per click, so it re-triggers).
  const [searching, setSearching] = createSignal<string | null>(null);
  const [jump, setJump] = createSignal<Jump | null>(null);

  // Counts move with every message; re-read the list at most every 500 ms.
  let refreshTimer: number | undefined;
  const refreshSoon = () => {
    if (refreshTimer !== undefined) return;
    refreshTimer = window.setTimeout(async () => {
      refreshTimer = undefined;
      await loadConversations();
      setRevision((n) => n + 1);
    }, 500);
  };

  const loadConversations = async () => {
    let list: Conversation[];
    try {
      const [convs, direct, badge, image] = await Promise.all([
        api.conversations(),
        api.directMessages(),
        api.activity("all", true, 1, 0),
        api.teamIcon(),
      ]);
      list = convs;
      setDms(direct);
      setActivityBadge(badge.unread_count);
      setIcon(image);
      setReadError("");
    } catch (e) {
      setReadError(String(e));
      return;
    }
    setConversations(reconcile(list, { key: "id" }));
    setLoaded(true);
    if (!selected() || (!list.some((c) => c.id === selected()) && preview()?.id !== selected())) {
      const first = list.find((c) => c.kind === "public_channel" && !c.is_archived) ?? list[0];
      if (first) select(first.id);
    }
    if (view() === "dms" && !dms().some((dm) => dm.id === selected()) && dms()[0]) {
      select(dms()[0].id);
    }
  };

  const select = (id: string) => {
    openingChannel++;
    setBrowsing(false);
    if (preview()?.id !== id) setPreview(null);
    setSearching(null);
    setJump(null);
    setSelected(id);
    try {
      localStorage.setItem(LAST_KEY, id);
    } catch {
      // Storage is a convenience only.
    }
  };

  onMount(async () => {
    const unlisten = onChange((c) => {
      switch (c.type) {
        case "progress":
          setProgress(c.data);
          if (c.data.stage === "idle" || c.data.stage === "conversations") refreshSoon();
          break;
        case "connection":
          setConnection(c.data);
          break;
        case "presence":
          refreshPresence().catch(() => {});
          break;
        case "directory":
          refreshSoon();
          loadPeople();
          directoryChanged();
          refreshPresence().catch(() => {});
          break;
        case "read_marker":
          refreshSoon();
          break;
        case "history":
          refreshSoon();
          if (c.data.conversation_id === selected()) setChange(c);
          break;
        case "message":
        case "message_deleted":
        case "reactions":
          refreshSoon();
          if (c.data.conversation_id === selected()) setChange(c);
          break;
        case "typing":
          break;
      }
    });
    onCleanup(() => {
      unlisten.then((f) => f());
      window.clearTimeout(refreshTimer);
    });
    await unlisten;
    setProgress(await api.progress());
    await Promise.all([loadConversations(), loadPeople()]);
  });

  createEffect(() => {
    const ids = conversations
      .filter((c) => c.kind === "im" && !c.is_archived)
      .map((c) => c.dm_user_id)
      .filter((id): id is string => !!id);
    const profile = profileUser();
    if (profile) ids.unshift(profile);
    ids.unshift(props.status.user_id);
    watchPresence([...new Set(ids)].slice(0, 500)).catch(() => {});
  });
  onMount(() => {
    tickPresence();
    const timer = window.setInterval(tickPresence, 15_000);
    onCleanup(() => window.clearInterval(timer));
  });

  // Batched, so the timeline mounts (or switches) already knowing the target.
  const openHit = async (h: Hit) => {
    if (!conversations.some((c) => c.id === h.conversation_id)) {
      await openChannel(h.conversation_id, true);
    }
    if (view() === "files") chooseView("home");
    batch(() => {
      select(h.conversation_id);
      setJump(jumpOf(h));
    });
    // A reply: also open its thread (after the switch, which closes threads).
    if (h.thread_ts && h.thread_ts !== h.ts) setThread(h.thread_ts);
  };

  const openJump = (target: Jump) => {
    batch(() => {
      select(target.conversation_id);
      setJump(target);
    });
    if (target.thread_ts && target.thread_ts !== target.ts) setThread(target.thread_ts);
  };
  const composeIn = (id: string) => {
    chooseView("home");
    select(id);
    requestAnimationFrame(() => {
      document.querySelector<HTMLElement>('.main .composer [contenteditable="true"]')?.focus();
    });
  };

  const openChannel = async (id: string, fresh: boolean) => {
    const generation = ++openingChannel;
    try {
      let row = await api.channelPreview(id);
      if (fresh) {
        try {
          await api.channelPreview(id, true);
          row = await api.channelPreview(id);
        } catch (e) {
          if (generation === openingChannel) setActionError(String(e));
        }
      }
      if (generation !== openingChannel) return;
      chooseView("home");
      setPreview(row);
      select(id);
    } catch (e) {
      if (generation === openingChannel) setActionError(String(e));
      throw e;
    }
  };
  const actionCompleted = async (result: ChannelAction) => {
    await loadConversations();
    await openChannel(result.id, false);
    if (result.warning === "purpose_failed") {
      setChannelNotice("purpose_failed");
    }
    if (result.warning === "catch_up_failed") {
      setChannelNotice("catch_up_failed");
    }
  };
  const joinChannel = async (id: string) => actionCompleted(await api.joinChannel(id));
  const browseChannels = () => {
    chooseView("home");
    setSearching(null);
    setThread(null);
    setChannelDetails(null);
    closeProfile();
    setBrowsing(true);
  };
  const current = () => conversations.find((c) => c.id === selected())
    ?? (preview()?.id === selected() ? preview() : null);

  // Root ts of the thread open in the side panel; closed when switching
  // conversations, or with Esc (unless the viewer is on top).
  const [thread, setThread] = createSignal<string | null>(null);
  createEffect(
    on(
      selected,
      () => {
        setThread(null);
        setChannelDetails(null);
      },
      { defer: true },
    ),
  );
  // The thread and profile panels share the right pane: opening one closes
  // the other.
  createEffect(on(thread, (t) => t && closeProfile(), { defer: true }));
  createEffect(on(profileUser, (u) => u && setThread(null), { defer: true }));
  createEffect(on(thread, (t) => t && setChannelDetails(null), { defer: true }));
  createEffect(on(profileUser, (u) => u && setChannelDetails(null), { defer: true }));
  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || isViewing()) return;
      if (channelDetails()) setChannelDetails(null);
      else if (profileUser()) closeProfile();
      else if (thread()) setThread(null);
    };
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });

  const SIDEBAR_KEY = "flek.sidebarWidth";
  const DETAIL_KEY = "flek.detailWidth";
  // Widths are kept as fractions of the frame; old pixel values are converted once.
  const startWidth = window.innerWidth - 78;
  const initialSize = (key: string, fallback: number) => {
    const value = readPanelSize(key, fallback, startWidth);
    if (value !== fallback) savePanelSize(key, value);
    return value;
  };
  const [sidebarWidth, setSidebarWidth] = createSignal(initialSize(SIDEBAR_KEY, DEFAULT_SIDEBAR));
  const [detailWidth, setDetailWidth] = createSignal(initialSize(DETAIL_KEY, DEFAULT_DETAIL));
  const [frameWidth, setFrameWidth] = createSignal(startWidth);
  const detailOpen = () =>
    !browsing() && view() !== "files" && !!(channelDetails() || profileUser() || (current() && thread()));
  const sizes = createMemo(() =>
    panelSizes(frameWidth(), sidebarWidth(), detailWidth(), detailOpen()),
  );
  let frame!: HTMLDivElement;
  onMount(() => {
    const observer = new ResizeObserver(() => setFrameWidth(frame.clientWidth));
    observer.observe(frame);
    setFrameWidth(frame.clientWidth);
    onCleanup(() => observer.disconnect());
  });

  return {
    conversations, view, icon, dms, activityBadge, revision, fileCategory, setFileCategory,
    compose, setCompose, readError, actionError, setActionError, channelNotice,
    setChannelNotice, leaving, setLeaving, menu, setMenu, channelDetails, setChannelDetails,
    menuConversation, details, chooseView, browsing, setBrowsing, creatingChannel,
    setCreatingChannel, channelsMenu, setChannelsMenu, loaded, selected, progress, connection,
    change, searching, setSearching, jump, select, openHit, openJump, composeIn, openChannel,
    actionCompleted, joinChannel, browseChannels, current, thread, setThread, sidebarWidth,
    setSidebarWidth, detailWidth, setDetailWidth, frameWidth, sizes, detailOpen, SIDEBAR_KEY,
    DETAIL_KEY, setFrame: (el: HTMLDivElement) => { frame = el; },
  };
}

function readLast(): string | null {
  try {
    return localStorage.getItem(LAST_KEY);
  } catch {
    return null;
  }
}
