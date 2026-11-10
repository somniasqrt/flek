import {
  batch,
  createEffect,
  createMemo,
  createSignal,
  on,
  onCleanup,
  onMount,
  Show,
  Switch,
  Match,
} from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { t, errorMessage } from "./i18n";
import { pick } from "./platform";
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
} from "./api";
import { setMe } from "./outbox";
import { loadPeople } from "./people";
import { directoryChanged, refreshPresence, watchPresence, tickPresence } from "./presence-store";
import { closeProfile, profileUser } from "./profile";
import ConversationMenu, {
  LeaveConfirmation,
  type ConversationMenuTarget,
} from "./ConversationMenu";
import ChannelPanel from "./ChannelPanel";
import ChannelBrowser from "./ChannelBrowser";
import CreateChannel from "./CreateChannel";
import ChannelsMenu from "./ChannelsMenu";
import type { ChannelAction } from "./api";
import { hasUnread } from "./conversation-model";
import { openProfile } from "./profile";
import ProfilePanel from "./ProfilePanel";
import Rail from "./Rail";
import ActivityView from "./ActivityView";
import DirectMessages from "./DirectMessages";
import FileBrowser, { FileNav } from "./FileBrowser";
import NewMessage from "./NewMessage";
import { readRail, type RailView } from "./rail-model";
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
        <Rail
          team={props.status.team}
          icon={icon()}
          userId={props.status.user_id}
          user={props.status.user}
          view={view()}
          dms={
            dms().filter((dm) =>
              hasUnread({
                ...dm,
                notification: conversations.find((c) => c.id === dm.id)?.notification,
              }),
            ).length
          }
          activity={activityBadge()}
          onView={chooseView}
          onCompose={() => setCompose(true)}
        />
        <div
          class="frame"
          classList={{
            "rail-view-frame": view() !== "home",
            "detail-overlay": sizes().overlay,
          }}
          ref={frame}
          style={{
            "--sidebar-width": `${sizes().sidebar}px`,
            "--detail-width": `${sizes().detail}px`,
          }}
        >
          <div class="sidebar-pane" id="conversation-sidebar">
            <Switch>
              <Match when={view() === "dms"}>
                <DirectMessages
                  rows={dms()}
                  mutedIds={conversations
                    .filter((c) => c.notification === "muted")
                    .map((c) => c.id)}
                  me={props.status.user_id}
                  selected={selected()}
                  onContext={setMenu}
                  onSelect={select}
                  onCompose={() => setCompose(true)}
                />
              </Match>
              <Match when={view() === "activity"}>
                <ActivityView
                  revision={revision()}
                  conversations={conversations}
                  me={props.status.user_id}
                  onOpen={openJump}
                />
              </Match>
              <Match when={view() === "files"}>
                <FileNav category={fileCategory()} onCategory={setFileCategory} />
              </Match>
              <Match when={view() === "home"}>
                <Sidebar
                  team={props.status.team}
                  conversations={conversations}
                  selected={selected()}
                  onContext={setMenu}
                  onChannelsMenu={setChannelsMenu}
                  onSelect={select}
                />
              </Match>
            </Switch>
            <ResizeHandle
              side="left"
              label={t("Resize sidebar")}
              controls="conversation-sidebar"
              value={sizes().sidebar}
              min={MIN_SIDEBAR}
              max={sizes().sidebarMax}
              initial={DEFAULT_SIDEBAR * frameWidth()}
              onResize={(px) => setSidebarWidth(px / frameWidth())}
              onCommit={() => savePanelSize(SIDEBAR_KEY, sidebarWidth())}
            />
          </div>
          <section class="main">
            <Show when={readError()}>
              <p class="error view-empty" role="alert">
                {errorMessage(readError())}
              </p>
            </Show>
            <Show when={!browsing()} fallback={
              <ChannelBrowser
                onOpen={openChannel}
                onJoin={joinChannel}
                onCreate={() => setCreatingChannel(true)}
                onClose={() => setBrowsing(false)}
              />
            }>
            <Show
              when={view() === "files" && !searching()}
              fallback={
                <Show
                  when={searching()}
                  fallback={
                    <Show
                      when={current()}
                      fallback={
                        <div class="empty">
                          <Show when={loaded()} fallback={<div class="spinner" />}>
                            <h2>{t("Syncing messages")}</h2>
                            <p class="muted">
                              {pick({
                                mac: t(
                                  "Channels appear here as soon as they are fetched. Flek keeps syncing in the menu bar even when this window is closed.",
                                ),
                                windows: t(
                                  "Channels appear here as soon as they are fetched. Flek keeps syncing in the system tray even when this window is closed.",
                                ),
                                linux: t(
                                  "Channels appear here as soon as they are fetched. Flek keeps syncing in the system tray even when this window is closed.",
                                ),
                              })}
                            </p>
                          </Show>
                        </div>
                      }
                    >
                      {(c) => (
                        <Timeline
                          conversation={c()}
                          onJoin={joinChannel}
                          me={props.status.user_id}
                          members={dms().find((dm) => dm.id === c().id)?.members ?? []}
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
              }
            >
              <FileBrowser
                category={fileCategory()}
                revision={revision()}
                conversations={conversations}
                onOpen={(target) => {
                  chooseView("home");
                  openJump(target);
                }}
              />
            </Show>
            </Show>
          </section>
          <Show when={view() !== "files" && detailOpen()}>
            <div class="detail-pane" id="conversation-detail">
              <ResizeHandle
                side="right"
                label={t("Resize details panel")}
                controls="conversation-detail"
                value={sizes().detail}
                min={MIN_DETAIL}
                max={sizes().detailMax}
                initial={DEFAULT_DETAIL * frameWidth()}
                onResize={(px) => setDetailWidth(px / frameWidth())}
                onCommit={() => savePanelSize(DETAIL_KEY, detailWidth())}
              />
              <Show when={channelDetails()}>
                {(id) => <ChannelPanel id={id()} onClose={() => setChannelDetails(null)} />}
              </Show>
              <Show when={current() && thread()}>
                {(ts) => (
                  <Thread
                    conversation={current()!}
                    ts={ts()}
                    jump={jump()}
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
      <Show when={channelsMenu()}>
        {(position) => (
          <ChannelsMenu
            x={position().x}
            y={position().y}
            onClose={() => setChannelsMenu(null)}
            onBrowse={browseChannels}
            onCreate={() => setCreatingChannel(true)}
          />
        )}
      </Show>
      <Show when={creatingChannel()}>
        <CreateChannel
          onClose={() => setCreatingChannel(false)}
          onCreated={actionCompleted}
        />
      </Show>
      <Show when={compose()}>
        <NewMessage
          conversations={conversations}
          onSelect={composeIn}
          onClose={() => setCompose(false)}
        />
      </Show>
      <Show when={menu() && menuConversation()}>
        {(conversation) => (
          <ConversationMenu
            conversation={conversation()}
            x={menu()!.x}
            y={menu()!.y}
            onClose={() => setMenu(null)}
            onDetails={() => details(conversation())}
            onError={setActionError}
            onLeave={setLeaving}
          />
        )}
      </Show>
      <Show when={leaving()}>
        {(conversation) => (
          <LeaveConfirmation conversation={conversation()} onClose={() => setLeaving(null)} />
        )}
      </Show>
      <Show when={channelNotice()}>
        <div class="conversation-toast" role="status">
          {channelNotice() === "purpose_failed"
            ? t("Channel created, but its description could not be saved.")
            : t("Channel joined. Recent messages will sync when the connection recovers.")}
          <button aria-label={t("Dismiss error")} onClick={() => setChannelNotice(null)}>×</button>
        </div>
      </Show>
      <Show when={actionError()}>
        <div class="conversation-toast" role="alert">
          {errorMessage(actionError())}
          <button aria-label={t("Dismiss error")} onClick={() => setActionError("")}>
            ×
          </button>
        </div>
      </Show>
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
