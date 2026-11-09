import { createEffect, createSignal, on, onCleanup, onMount, Show } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import {
  api,
  onChange,
  type Change,
  type ConnectionState,
  type Conversation,
  type Progress,
  type Status,
} from "./api";
import { loadPeople } from "./people";
import Rail from "./Rail";
import Viewer, { isViewing } from "./Viewer";
import Sidebar from "./Sidebar";
import SyncBar from "./SyncBar";
import Thread from "./Thread";
import Timeline from "./Timeline";

type Ready = Extract<Status, { phase: "ready" }>;

const LAST_KEY = "flek.lastConversation";

export default function Workspace(props: { status: Ready }) {
  // A store reconciled by id: a refresh only touches rows that changed, so
  // the sidebar does not re-render (and flicker) on every sync step.
  const [conversations, setConversations] = createStore<Conversation[]>([]);
  const [loaded, setLoaded] = createSignal(false);
  const [selected, setSelected] = createSignal<string | null>(readLast());
  const [progress, setProgress] = createSignal<Progress | null>(null);
  const [connection, setConnection] = createSignal<ConnectionState | null>(null);
  // Changes for the open conversation, handed to the timeline.
  const [change, setChange] = createSignal<Change | null>(null, { equals: false });

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
        case "directory":
          refreshSoon();
          loadPeople();
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

  const current = () => conversations.find((c) => c.id === selected()) ?? null;

  // Root ts of the thread open in the side panel; closed when switching
  // conversations, or with Esc (unless the viewer is on top).
  const [thread, setThread] = createSignal<string | null>(null);
  createEffect(on(selected, () => setThread(null), { defer: true }));
  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && thread() && !isViewing()) setThread(null);
    };
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });

  return (
    <div class="shell">
      {/* The window has no title bar: this strip drags it, under the traffic lights. */}
      <header class="topbar" data-tauri-drag-region>
        <SyncBar progress={progress()} connection={connection()} />
      </header>
      <div class="shell-body">
        <Rail team={props.status.team} userId={props.status.user_id} user={props.status.user} />
        <div class="frame">
          <Sidebar
            team={props.status.team}
            conversations={conversations}
            selected={selected()}
            onSelect={select}
          />
          <section class="main">
            <Show
              when={current()}
              fallback={
                <div class="empty">
                  <Show when={loaded()} fallback={<div class="spinner" />}>
                    <h2>Syncing messages</h2>
                    <p class="muted">
                      Channels appear here as soon as they are fetched. Flek keeps syncing in the
                      menu bar even when this window is closed.
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
                />
              )}
            </Show>
          </section>
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
