import { createSignal, onCleanup, onMount, Show } from "solid-js";
import {
  api,
  onChange,
  type Change,
  type ConnectionState,
  type Conversation,
  type Progress,
  type Status,
} from "./api";
import Sidebar from "./Sidebar";
import SyncBar from "./SyncBar";
import Timeline from "./Timeline";

type Ready = Extract<Status, { phase: "ready" }>;

const LAST_KEY = "flek.lastConversation";

export default function Workspace(props: { status: Ready }) {
  const [conversations, setConversations] = createSignal<Conversation[]>([]);
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
    setConversations(list);
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
    const unlisten = await onChange((c) => {
      switch (c.type) {
        case "progress":
          setProgress(c.data);
          if (c.data.stage === "idle" || c.data.stage === "conversations") refreshSoon();
          break;
        case "connection":
          setConnection(c.data);
          break;
        case "directory":
        case "read_marker":
          refreshSoon();
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
      unlisten();
      window.clearTimeout(refreshTimer);
    });
    setProgress(await api.progress());
    await loadConversations();
  });

  const current = () => conversations().find((c) => c.id === selected()) ?? null;

  return (
    <div class="workspace">
      <Sidebar
        team={props.status.team}
        user={props.status.user}
        conversations={conversations()}
        selected={selected()}
        onSelect={select}
      />
      <section class="main">
        <SyncBar progress={progress()} connection={connection()} />
        <Show
          when={current()}
          fallback={
            <div class="empty">
              <Show when={loaded()} fallback={<div class="spinner" />}>
                <h2>Syncing messages</h2>
                <p class="muted">
                  Channels appear here as soon as they are fetched. Flek keeps syncing in the menu
                  bar even when this window is closed.
                </p>
              </Show>
            </div>
          }
        >
          {(c) => <Timeline conversation={c()} change={change()} />}
        </Show>
      </section>
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
