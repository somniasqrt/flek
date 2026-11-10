import {
  batch,
  createEffect,
  createMemo,
  createSignal,
  on,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import {
  api,
  onChange,
  type Change,
  type ConnectionState,
  type Conversation,
  type Hit,
  type Progress,
  type Status,
} from "./api";
import { setMe } from "./outbox";
import { loadPeople } from "./people";
import { directoryChanged, refreshPresence, watchPresence, tickPresence } from "./presence-store";
import { closeProfile, profileUser } from "./profile";
import ProfilePanel from "./ProfilePanel";
import Rail from "./Rail";
import { jumpOf, SearchBar, SearchResults, type Jump } from "./Search";
import Viewer, { isViewing } from "./Viewer";
import Sidebar from "./Sidebar";
import SyncBar from "./SyncBar";
import Thread from "./Thread";
import Timeline from "./Timeline";
import ResizeHandle from "./ResizeHandle";
import {
  DEFAULT_SIDEBAR,
  DEFAULT_DETAIL,
  MIN_SIDEBAR,
  MIN_DETAIL,
  panelSizes,
  readPanelSize,
  savePanelSize,
} from "./panel-sizes";

type Ready = Extract<Status, { phase: "ready" }>;

const LAST_KEY = "flek.lastConversation";

export default function Workspace(props: { status: Ready }) {
  // A store reconciled by id: a refresh only touches rows that changed, so
  // the sidebar does not re-render (and flicker) on every sync step.
  setMe(props.status.user_id);
  const [conversations, setConversations] = createStore<Conversation[]>([]);
  const [loaded, setLoaded] = createSignal(false);
  const [selected, setSelected] = createSignal<string | null>(readLast());
  const [progress, setProgress] = createSignal<Progress | null>(null);
  const [connection, setConnection] = createSignal<ConnectionState | null>(null);
  // Changes for the open conversation, handed to the timeline.
  const [change, setChange] = createSignal<Change | null>(null, { equals: false });
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
    }, 500);
  };

  const loadConversations = async () => {
    const list = await api.conversations();
    setConversations(reconcile(list, { key: "id" }));
    setLoaded(true);
    if (!selected() || !list.some((c) => c.id === selected())) {
      const first = list.find((c) => c.kind === "public_channel" && !c.is_archived) ?? list[0];
      if (first) select(first.id);
    }
  };

  const select = (id: string) => {
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
  const openHit = (h: Hit) => {
    batch(() => {
      select(h.conversation_id);
      setJump(jumpOf(h));
    });
    // A reply: also open its thread (after the switch, which closes threads).
    if (h.thread_ts && h.thread_ts !== h.ts) setThread(h.thread_ts);
  };

  const current = () => conversations.find((c) => c.id === selected()) ?? null;

  // Root ts of the thread open in the side panel; closed when switching
  // conversations, or with Esc (unless the viewer is on top).
  const [thread, setThread] = createSignal<string | null>(null);
  createEffect(on(selected, () => setThread(null), { defer: true }));
  // The thread and profile panels share the right pane: opening one closes
  // the other.
  createEffect(on(thread, (t) => t && closeProfile(), { defer: true }));
  createEffect(on(profileUser, (u) => u && setThread(null), { defer: true }));
  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || isViewing()) return;
      if (profileUser()) closeProfile();
      else if (thread()) setThread(null);
    };
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });

  const SIDEBAR_KEY = "flek.sidebarWidth";
  const DETAIL_KEY = "flek.detailWidth";
  const [sidebarWidth, setSidebarWidth] = createSignal(readPanelSize(SIDEBAR_KEY, DEFAULT_SIDEBAR));
  const [detailWidth, setDetailWidth] = createSignal(readPanelSize(DETAIL_KEY, DEFAULT_DETAIL));
  const [frameWidth, setFrameWidth] = createSignal(window.innerWidth - 78);
  const detailOpen = () => !!(profileUser() || (current() && thread()));
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

  return (
    <div class="shell">
      {/* The window has no title bar: this strip drags it, under the traffic lights. */}
      <header class="topbar" data-tauri-drag-region>
        <div class="topbar-side" data-tauri-drag-region />
        <SearchBar
          team={props.status.team}
          conversations={conversations}
          onSubmit={setSearching}
          onOpen={openHit}
        />
        <div class="topbar-side end" data-tauri-drag-region>
          <SyncBar progress={progress()} connection={connection()} />
        </div>
      </header>
      <div class="shell-body">
        <Rail team={props.status.team} userId={props.status.user_id} user={props.status.user} />
        <div
          class="frame"
          ref={frame}
          style={{
            "--sidebar-width": `${sizes().sidebar}px`,
            "--detail-width": `${sizes().detail}px`,
          }}
        >
          <div class="sidebar-pane" id="conversation-sidebar">
            <Sidebar
              team={props.status.team}
              conversations={conversations}
              selected={selected()}
              onSelect={select}
            />
            <ResizeHandle
              side="left"
              label="Resize sidebar"
              controls="conversation-sidebar"
              value={sizes().sidebar}
              min={MIN_SIDEBAR}
              max={sizes().sidebarMax}
              initial={DEFAULT_SIDEBAR}
              onResize={setSidebarWidth}
              onCommit={() => savePanelSize(SIDEBAR_KEY, sidebarWidth())}
            />
          </div>
          <section class="main">
            <Show
              when={searching()}
              fallback={
                <Show
                  when={current()}
                  fallback={
                    <div class="empty">
                      <Show when={loaded()} fallback={<div class="spinner" />}>
                        <h2>Syncing messages</h2>
                        <p class="muted">
                          Channels appear here as soon as they are fetched. Flek keeps syncing in
                          the menu bar even when this window is closed.
                        </p>
                      </Show>
                    </div>
                  }
                >
                  {(c) => (
                    <Timeline
                      conversation={c()}
                      change={change()}
                      thread={thread()}
                      onThread={setThread}
                      jump={jump()}
                    />
                  )}
                </Show>
              }
            >
              {(q) => (
                <SearchResults
                  query={q()}
                  conversations={conversations}
                  onOpen={openHit}
                  onClose={() => setSearching(null)}
                />
              )}
            </Show>
          </section>
          <Show when={detailOpen()}>
            <div class="detail-pane" id="conversation-detail">
              <ResizeHandle
                side="right"
                label="Resize details panel"
                controls="conversation-detail"
                value={sizes().detail}
                min={MIN_DETAIL}
                max={sizes().detailMax}
                initial={DEFAULT_DETAIL}
                onResize={setDetailWidth}
                onCommit={() => savePanelSize(DETAIL_KEY, detailWidth())}
              />
              <Show when={current() && thread()}>
                {(ts) => (
                  <Thread
                    conversation={current()!}
                    ts={ts()}
                    change={change()}
                    onClose={() => setThread(null)}
                  />
                )}
              </Show>
              <Show when={profileUser()}>
                {(id) => <ProfilePanel userId={id()} onClose={closeProfile} onOpen={select} />}
              </Show>
            </div>
          </Show>
        </div>
      </div>
      <Viewer />
    </div>
  );
}

function readLast(): string | null {
  try {
    return localStorage.getItem(LAST_KEY);
  } catch {
    return null;
  }
}
