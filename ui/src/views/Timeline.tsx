import { dayLabel, JoinChannelBar } from "./timeline-parts";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  on,
  onCleanup,
  Show,
  untrack,
} from "solid-js";
import { VirtualLayout } from "../models/virtualizer";
import MeasuredRow from "../components/MeasuredRow";
import ConversationMenu, { menuTarget, type ConversationMenuTarget } from "../components/ConversationMenu";
import JumpToDate from "../components/JumpToDate";
import { settings } from "../stores/settings";
import { createStore, reconcile, unwrap } from "solid-js/store";
import { t, errorMessage, plural, locale } from "../i18n/index";
import { api, isReplyOnly, tsDate, type Change, type Conversation, type Message, type ChannelHeader } from "../api";
import { conversationStarLabel, firstUnread } from "../models/conversation-model";
import Avatar from "../components/Avatar";
import Composer from "../components/Composer";
import DmIntro from "../components/DmIntro";
import CustomStatus from "../components/CustomStatus";
import { Hash, Lock, Star, StarFilled, Bell, Search, Kebab } from "../components/icons";
import MessageRow from "../components/MessageRow";
import { pendingFor } from "../stores/outbox";
import { presenceOf, statuses } from "../stores/presence-store";
import { people } from "../stores/people";
import type { Jump } from "./Search";

const PAGE = 60;
/// Load older messages when the top is this close (px).
const LOAD_MARGIN = 400;
/// Bounded initial unread context.
const REVEAL_PAGE = 500;

export default function Timeline(props: {
  conversation: Conversation;
  onDetails: () => void;
  onSearch: () => void;
  onLeave: (conversation: Conversation) => void;
  onError: (message: string) => void;
  revision: number;
  onJoin: (id: string) => Promise<void>;
  me: string;
  members: string[];
  change: Change | null;
  /// Root ts of the thread open in the side panel.
  thread: string | null;
  onThread: (ts: string) => void;
  /// A message to scroll to and highlight (a search result).
  jump: Jump | null;
}) {
  const [headerMenu, setHeaderMenu] = createSignal<(ConversationMenuTarget & {
    placement: "header" | "notifications";
  }) | null>(null);
  const [headerMembers, setHeaderMembers] = createSignal<ChannelHeader | null>(null);
  let headerRequest = 0;
  let headerConversation: string | undefined;
  createEffect(() => {
    const id = props.conversation.id;
    props.revision;
    const request = ++headerRequest;
    if (id !== headerConversation) {
      headerConversation = id;
      setHeaderMembers(null);
      setHeaderMenu(null);
    }
    if (!isChannel(props.conversation)) return;
    // This command only reads SQLite; it cannot enqueue sync or fetch membership.
    void api.channelHeader(id).then(
      (members) => { if (request === headerRequest) setHeaderMembers(members); },
      () => { if (request === headerRequest) setHeaderMembers(null); },
    );
  });
  onCleanup(() => { headerRequest++; });
  const starLabel = () => conversationStarLabel(props.conversation);
  const toggleStar = async () => {
    try {
      await api.conversationPreference(props.conversation.id, !props.conversation.starred, null);
    } catch (e) {
      props.onError(String(e));
    }
  };
  const openHeaderMenu = (e: MouseEvent, placement: "header" | "notifications") => {
    setHeaderMenu({ ...menuTarget(e, props.conversation.id, "header"), placement });
  };
  const name = () => props.conversation.dm_user_id === props.me
    ? t("{name} (you)", { name: props.conversation.label })
    : props.conversation.label;

  // Reconciled by ts: updates touch only the rows that changed, so syncing
  // never re-renders (and flickers) the whole timeline.
  const [rows, setRows] = createStore<Message[]>([]);
  const show = (list: Message[]) => setRows(reconcile(list, { key: "ts" }));
  const current = () => unwrap(rows).slice();
  const [loading, setLoading] = createSignal(false);
  const [hasNewer, setHasNewer] = createSignal(false);
  const [datePicker, setDatePicker] = createSignal<string | null>(null);
  const [timelineError, setTimelineError] = createSignal("");
  const [exhausted, setExhausted] = createSignal(false);
  let scroller!: HTMLDivElement;
  let virtualHost!: HTMLDivElement;
  const layout = new VirtualLayout();
  const [layoutVersion, setLayoutVersion] = createSignal(0);
  const [viewport, setViewport] = createSignal({ top: 0, height: 600 });
  let pinned = true;
  let scrollingFrame = 0;
  let restoringFrame = 0;
  let seeking: string | null = null;
  const updateViewport = () => {
    if (!scroller || !virtualHost) return;
    setViewport({ top: scroller.scrollTop - virtualHost.offsetTop, height: scroller.clientHeight });
  };
  const restoreLayout = (anchor: ReturnType<VirtualLayout["anchor"]>, stick: boolean) => {
    cancelAnimationFrame(restoringFrame);
    restoringFrame = requestAnimationFrame(() => {
      if (!alive || !scroller || !virtualHost) return;
      const seekIndex = seeking ? layout.index(seeking) : undefined;
      if (seekIndex !== undefined) {
        scroller.scrollTop =
          virtualHost.offsetTop + layout.offset(seekIndex) - scroller.clientHeight / 2 + 40;
      } else if (stick) scroller.scrollTop = scroller.scrollHeight;
      else {
        const top = layout.restore(anchor);
        if (top !== undefined) scroller.scrollTop = virtualHost.offsetTop + top;
      }
      updateViewport();
      scheduleRead();
    });
  };
  const rowObserver = new ResizeObserver((entries) => {
    const anchor = layout.anchor(viewport().top);
    let changed = false;
    for (const entry of entries) {
      const el = entry.target as HTMLElement;
      changed =
        layout.measure(el.dataset.virtualKey!, el.getBoundingClientRect().height) || changed;
    }
    if (changed) {
      setLayoutVersion((n) => n + 1);
      restoreLayout(anchor, pinned);
    }
  });
  const viewportObserver = new ResizeObserver(() => {
    const anchor = layout.anchor(viewport().top);
    updateViewport();
    restoreLayout(anchor, pinned);
  });
  onCleanup(() => {
    rowObserver.disconnect();
    viewportObserver.disconnect();
    cancelAnimationFrame(scrollingFrame);
    cancelAnimationFrame(restoringFrame);
  });
  // Bumped on every switch so late pages of the previous conversation are dropped.
  let generation = 0;
  let alive = true;
  const [openingRead, setOpeningRead] = createSignal<string | null>(null);
  const unreadTs = createMemo(() => firstUnread(rows, openingRead()));
  const [readError, setReadError] = createSignal("");
  let readTimer: number | undefined;
  let marking = false;
  let acknowledged: string | null = null;
  const focused = () => document.visibilityState === "visible" && document.hasFocus();
  const seenTs = () => {
    if (!focused() || loading() || !scroller) return null;
    const bounds = scroller.getBoundingClientRect();
    let latest: string | null = null;
    const pendingKeys = new Set(untrack(() => pending().map((m) => m.ts)));
    for (const el of scroller.querySelectorAll<HTMLElement>("[data-ts]")) {
      if (
        !el.dataset.ts ||
        layout.index(el.dataset.ts) === undefined ||
        pendingKeys.has(el.dataset.ts)
      )
        continue;
      const r = el.getBoundingClientRect();
      if (r.bottom <= bounds.bottom + 1 && r.bottom > bounds.top && r.top < bounds.bottom) {
        latest = el.dataset.ts ?? latest;
      }
    }
    return latest;
  };
  // The newest message seen but not yet marked: the debounce and the request
  // both lag behind scrolling, so leaving the conversation sends it at once.
  let unmarked: { id: string; ts: string } | null = null;
  const flushRead = () => {
    const seen = unmarked;
    unmarked = null;
    if (seen) void api.markRead(seen.id, seen.ts).catch(() => {});
  };
  const scheduleRead = () => {
    if (!alive) return;
    if (props.conversation.is_member === false) return;
    const ts = seenTs();
    const stored = props.conversation.last_read;
    const last = acknowledged && (!stored || acknowledged > stored) ? acknowledged : stored;
    if (!ts || (last && ts <= last)) return;
    const id = props.conversation.id;
    if (!unmarked || unmarked.id !== id || ts > unmarked.ts) unmarked = { id, ts };
    if (readTimer !== undefined || marking) return;
    const gen = generation;
    readTimer = window.setTimeout(async () => {
      readTimer = undefined;
      if (gen !== generation || id !== props.conversation.id || !focused()) return;
      const latest = seenTs();
      if (!latest) return;
      marking = true;
      try {
        await api.markRead(id, latest);
        if (gen === generation) {
          acknowledged = latest;
          setReadError("");
          if (unmarked && unmarked.id === id && unmarked.ts <= latest) unmarked = null;
        }
      } catch (e) {
        if (gen === generation) setReadError(String(e));
      } finally {
        marking = false;
        // A newer message or a conversation switch may have arrived during IPC.
        if (gen !== generation || !readError()) scheduleRead();
      }
    }, 1000);
  };
  const focusRead = () => scheduleRead();
  window.addEventListener("focus", focusRead);
  document.addEventListener("visibilitychange", focusRead);
  onCleanup(() => {
    alive = false;
    generation++;
    flushRead();
    window.clearTimeout(readTimer);
    window.removeEventListener("focus", focusRead);
    document.removeEventListener("visibilitychange", focusRead);
  });
  createEffect(() => {
    rows.length;
    props.conversation.last_read;
    requestAnimationFrame(scheduleRead);
  });

  const atBottom = () => scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 40;
  const nearBottom = () => scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 150;
  const toBottom = () =>
    requestAnimationFrame(() => {
      if (!alive) return;
      pinned = true;
      seeking = null;
      scroller.scrollTop = scroller.scrollHeight;
      updateViewport();
      scheduleRead();
    });

  const [flash, setFlash] = createSignal<string | null>(null);
  let flashTimer: number | undefined;
  onCleanup(() => window.clearTimeout(flashTimer));

  createEffect(
    on(
      () => [props.conversation.id, props.jump] as const,
      async ([id, jump], prev) => {
        const target = jump && jump.conversation_id === id ? jump : null;
        // Same conversation, new target: no reload.
        if (prev && prev[0] === id) {
          if (target) await reveal(target);
          return;
        }
        const gen = ++generation;
        flushRead();
        window.clearTimeout(readTimer);
        readTimer = undefined;
        acknowledged = null;
        setReadError("");
        setOpeningRead(props.conversation.last_read ?? null);
        pinned = true;
        seeking = null;
        layout.clear();
        window.clearTimeout(flashTimer);
        setFlash(null);
        show([]);
        setExhausted(false);
        setHasNewer(false);
        setTimelineError("");
        setDatePicker(null);
        setLoading(true);
        try {
          let page = await api.messages(id, null, PAGE);
          // Include the opening cursor, even when unread spans more than one page,
          // but keep initial loading bounded in a never-read channel.
          for (
            let pages = 0;
            pages < 2 && openingRead() && page.length && page[0].ts > openingRead()!;
            pages++
          ) {
            const older = await api.messages(id, page[0].ts, REVEAL_PAGE);
            if (gen !== generation) return;
            if (!older.length) break;
            page = [...older, ...page];
          }
          if (gen !== generation) return;
          show(page);
          setExhausted(page.length < PAGE);
          setLoading(false);
          if (target) await reveal(target);
          else
            requestAnimationFrame(() => {
              const ts = unreadTs();
              const index = ts ? layout.index(ts) : undefined;
              if (
                index !== undefined &&
                layout.total - layout.offset(index) > scroller.clientHeight
              ) {
                pinned = false;
                scroller.scrollTop = virtualHost.offsetTop + layout.offset(index);
                updateViewport();
              } else toBottom();
              scheduleRead();
            });
        } catch (e) {
          if (gen === generation) setTimelineError(String(e));
        } finally {
          if (gen === generation) setLoading(false);
        }
      },
    ),
  );

  /// Loads a bounded local window when needed, then centres and briefly
  /// highlights it. A reply that is only in its thread shows as its root.
  const reveal = async (jump: Jump) => {
    const gen = ++generation;
    window.clearTimeout(readTimer);
    readTimer = undefined;
    setLoading(false);
    setTimelineError("");
    const id = props.conversation.id;
    const target =
      jump.thread_ts && jump.thread_ts < jump.ts && !rows.some((m) => m.ts === jump.ts) ? jump.thread_ts : jump.ts;
    if (!rows.some((m) => m.ts === target)) {
      setLoading(true);
      try {
        const around = await api.timelineWindow(id, target, PAGE * 2);
        if (gen !== generation) return;
        if (!around.rows.some((m) => m.ts === target)) {
          setTimelineError(t("This message is no longer stored in Flek."));
          return;
        }
        pinned = false;
        seeking = target;
        show(around.rows);
        setExhausted(!around.has_older);
        setHasNewer(around.has_newer);
      } catch (e) {
        if (gen === generation) setTimelineError(String(e));
        return;
      } finally {
        if (gen === generation) setLoading(false);
      }
    }
    // A stored target may have just arrived in the initial page. Wait for
    // its keyed layout as well as for newly loaded historical windows.
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    if (gen !== generation) return;
    const key = layout.index(jump.ts) !== undefined ? jump.ts : jump.thread_ts;
    if (!key || layout.index(key) === undefined) return toBottom();
    pinned = false;
    seeking = key;
    restoreLayout(layout.anchor(viewport().top), false);
    setFlash(key);
    window.clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => {
      seeking = null;
      setFlash(null);
    }, 2500);
  };

  const loadOlder = async () => {
    if (loading() || exhausted() || rows.length === 0) return;
    const gen = generation;
    setLoading(true);
    try {
      const page = await api.messages(props.conversation.id, rows[0].ts, PAGE);
      if (gen !== generation) return;
      const anchor = layout.anchor(viewport().top);
      show([...page, ...current()]);
      setExhausted(page.length < PAGE);
      setLoading(false);
      restoreLayout(anchor, pinned);
      // The keyed layout effect preserves the visible message across prepends.
    } catch (e) {
      if (gen === generation) setTimelineError(String(e));
    } finally {
      if (gen === generation) setLoading(false);
    }
  };

  const loadNewer = async () => {
    if (loading() || !hasNewer() || !rows.length) return;
    const gen = generation;
    setLoading(true);
    try {
      const page = await api.timelineAfter(props.conversation.id, rows[rows.length - 1].ts, PAGE);
      if (gen !== generation) return;
      show([...current(), ...page]);
      setHasNewer(page.length === PAGE);
    } catch (e) {
      if (gen === generation) setTimelineError(String(e));
    } finally {
      if (gen === generation) setLoading(false);
    }
  };

  createEffect(
    on(
      () => props.change,
      async (c) => {
        if (!c || !("data" in c) || !("conversation_id" in c.data)) return;
        if (c.data.conversation_id !== props.conversation.id) return;
        if (c.type === "history") {
          try {
            await refreshNewest();
          } catch (e) {
            setTimelineError(String(e));
          }
          return;
        }
        if (!("ts" in c.data)) return;
        const { conversation_id, ts } = c.data;
        const gen = generation;
        if (c.type === "message_deleted") {
          show(current().filter((m) => m.ts !== ts));
          return;
        }
        const stick = !hasNewer() && atBottom();
        const m = await api.message(conversation_id, ts);
        if (gen !== generation || conversation_id !== props.conversation.id) return;
        if (!m) {
          show(current().filter((x) => x.ts !== ts));
        } else if (isReplyOnly(m)) {
          // A reply changes its root's reply count.
          const root = await api.message(conversation_id, m.thread_ts!);
          if (root && gen === generation && conversation_id === props.conversation.id) upsert(root);
        } else {
          upsert(m);
          if (stick) toBottom();
          // A broadcast reply also changes its root's reply count.
          if (m.thread_ts && m.thread_ts !== m.ts) {
            const root = await api.message(conversation_id, m.thread_ts);
            if (root && gen === generation && conversation_id === props.conversation.id)
              upsert(root);
          }
        }
      },
    ),
  );

  const onScroll = () => {
    pinned = atBottom();
    if (scrollingFrame) return;
    scrollingFrame = requestAnimationFrame(() => {
      scrollingFrame = 0;
      updateViewport();
      scheduleRead();
      if (scroller.scrollTop < LOAD_MARGIN) void loadOlder();
      else if (scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < LOAD_MARGIN)
        void loadNewer();
    });
  };

  // Pending rows share the measured layout until the stored echo arrives.
  const pending = createMemo(() => pendingFor(props.conversation.id, null, rows));
  // A pending row that grows ("Not sent · Retry") stays in view at the bottom.
  createEffect(on(pending, () => nearBottom() && toBottom(), { defer: true }));
  const items = createMemo(() => [...rows, ...pending()]);
  const dayStarts = createMemo(
    () =>
      new Set(
        items()
          .filter((m, i, list) => i === 0 || dayKey(m.ts) !== dayKey(list[i - 1].ts))
          .map((m) => m.ts),
      ),
  );
  createEffect(() => {
    const list = items();
    const anchor = layout.anchor(untrack(viewport).top);
    layout.reset(list.map((m) => m.ts));
    setLayoutVersion((n) => n + 1);
    restoreLayout(anchor, pinned);
  });
  createEffect(() => {
    if (scroller) {
      viewportObserver.observe(scroller);
      updateViewport();
    }
  });
  const windowRange = createMemo(() => {
    layoutVersion();
    return layout.range(viewport().top, viewport().height);
  });
  const visible = createMemo(() => items().slice(windowRange().start, windowRange().end));
  const stickyDay = createMemo(() => {
    layoutVersion();
    const index = layout.indexAt(Math.max(0, viewport().top));
    const ts = items()[index]?.ts;
    return ts && dayStarts().has(ts) && viewport().top - layout.offset(index) < 48 ? undefined : ts;
  });

  const upsert = (m: Message) => {
    const r = current();
    const i = r.findIndex((x) => x.ts === m.ts);
    if (i >= 0) r[i] = m;
    // Older than everything loaded: it shows up when scrolling back.
    else if (r.length > 0 && m.ts < r[0].ts) return;
    else if (!hasNewer() || !r.length || m.ts <= r[r.length - 1].ts) r.push(m);
    show(r.sort((a, b) => a.ts.localeCompare(b.ts)));
  };

  /// Sync wrote into this conversation: re-read the newest page and keep the
  /// older rows already loaded.
  const refreshNewest = async () => {
    const id = props.conversation.id;
    const gen = generation;
    const stick = atBottom();
    if (hasNewer()) {
      if (!rows.length) return;
      const loaded = await api.timelineRange(id, rows[0].ts, rows[rows.length - 1].ts);
      if (gen === generation) show(loaded);
      return;
    }
    const latest = await api.messages(id, null, PAGE, true);
    if (gen !== generation) return;
    const page =
      rows.length && latest.length
        ? await api.timelineRange(
            id,
            rows[0].ts < latest[0].ts ? rows[0].ts : latest[0].ts,
            latest[latest.length - 1].ts,
          )
        : latest;
    if (gen !== generation) return;
    show(page);
    if (page.length < PAGE) setExhausted(true);
    if (stick) toBottom();
  };

  const sent = async () => {
    if (hasNewer()) {
      const gen = ++generation;
      window.clearTimeout(readTimer);
      readTimer = undefined;
      try {
        const page = await api.messages(props.conversation.id, null, PAGE);
        if (gen !== generation) return;
        pinned = true;
        seeking = null;
        show(page);
        setHasNewer(false);
        setExhausted(page.length < PAGE);
        setLoading(false);
      } catch (e) {
        if (gen === generation) setTimelineError(String(e));
      }
    }
    toBottom();
  };

  const pickDate = (ts?: string) => {
    const d = ts ? tsDate(ts) : new Date();
    setDatePicker(
      `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`,
    );
  };

  return (
    <div class="timeline">
      <header class="conv-header conversation-header">
        <button class="conv-header-icon conv-star" classList={{ starred: !!props.conversation.starred }}
          title={starLabel()} aria-label={starLabel()} aria-pressed={!!props.conversation.starred}
          onClick={toggleStar}>
          <Show when={props.conversation.starred} fallback={<Star size={20} />}>
            <StarFilled size={20} />
          </Show>
        </button>
        <h2>
          <button class="conv-name" onClick={props.onDetails}
            title={props.conversation.topic || (isChannel(props.conversation) ? t("Channel details") : t("View profile"))}
            aria-label={isChannel(props.conversation) ? t("Channel details") : t("View profile")}>
            <Show when={isChannel(props.conversation)} fallback={
              <Avatar id={props.conversation.dm_user_id ?? props.members[0] ?? null}
                name={name()} size={26}
                presence={props.conversation.dm_user_id && settings().presence_dots
                  ? presenceOf(props.conversation.dm_user_id) : undefined} />
            }>
              <Show when={props.conversation.kind === "private_channel"} fallback={<Hash size={20} />}>
                <Lock size={20} />
              </Show>
            </Show>
            <span class="conv-name-label">{name()}</span>
            <Show when={props.conversation.kind === "im" && props.conversation.dm_user_id}>
              {(id) => <CustomStatus status={statuses()[id()]} text />}
            </Show>
          </button>
        </h2>
        <div class="conv-header-actions">
          <Show when={isChannel(props.conversation) && headerMembers()}>
            {(members) => (
              <button class="conv-members" onClick={props.onDetails}
                title={plural("members", members().num_members)}
                aria-label={plural("members", members().num_members)}>
                <span class="conv-member-avatars" aria-hidden="true">
                  <For each={members().member_ids}>
                    {(id) => <Avatar id={id} name={people()[id] ?? id} size={24} />}
                  </For>
                </span>
                <span>{members().num_members.toLocaleString(locale())}</span>
              </button>
            )}
          </Show>
          <button class="conv-header-icon" aria-haspopup="menu"
            aria-expanded={headerMenu()?.placement === "notifications"}
            title={props.conversation.notification === "muted" ? t("Notifications muted in Flek") : t("Notification preferences")}
            aria-label={props.conversation.notification === "muted" ? t("Notifications muted in Flek") : t("Notification preferences")}
            onClick={(e) => openHeaderMenu(e, "notifications")}>
            <Bell size={20} muted={props.conversation.notification === "muted"} />
          </button>
          <button class="conv-header-icon" title={t("Search in this conversation")}
            aria-label={t("Search in this conversation")} onClick={props.onSearch}>
            <Search size={20} />
          </button>
          <button class="conv-header-icon" title={t("More conversation actions")}
            aria-label={t("More conversation actions")} aria-haspopup="menu"
            aria-expanded={headerMenu()?.placement === "header"}
            onClick={(e) => openHeaderMenu(e, "header")}>
            <Kebab size={20} />
          </button>
        </div>
      </header>
      <Show when={headerMenu()} keyed>
        {(target) => <ConversationMenu conversation={props.conversation}
          x={target.x} y={target.y} placement={target.placement}
          onClose={() => setHeaderMenu(null)} onDetails={props.onDetails}
          onJumpDate={() => pickDate()} onLeave={props.onLeave} onError={props.onError} />}
      </Show>
      <Show when={datePicker()}>
        {(date) => (
          <JumpToDate
            conversationId={props.conversation.id}
            initial={date()}
            onClose={() => setDatePicker(null)}
            onJump={(ts) => reveal({ conversation_id: props.conversation.id, ts, thread_ts: null })}
          />
        )}
      </Show>
      <Show when={timelineError()}>
        <p class="error" role="alert">
          {errorMessage(timelineError())}
        </p>
      </Show>
      <Show when={readError()}>
        <p class="error" role="alert">
          {errorMessage(readError())}
        </p>
      </Show>
      <div class="scroll-area">
        <div
          class="scroller"
          ref={scroller}
          onScroll={onScroll}
          onWheel={() => {
            seeking = null;
            cancelAnimationFrame(restoringFrame);
          }}
          onPointerDown={() => {
            seeking = null;
            cancelAnimationFrame(restoringFrame);
          }}
          onKeyDown={(e) => {
            if (["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End"].includes(e.key))
              seeking = null;
          }}
        >
          <Show
            when={
              exhausted() &&
              (props.conversation.kind === "im" || props.conversation.kind === "mpim")
            }
          >
            <DmIntro conversation={props.conversation} me={props.me} members={props.members} />
          </Show>
          <Show when={exhausted() && rows.length > 0 && isChannel(props.conversation)}>
            <div class="start muted">{t("Start of what Flek has for this conversation")}</div>
          </Show>
          <Show when={!loading() && rows.length === 0}>
            <div class="start muted">{t("No messages yet. They appear here as they sync.")}</div>
          </Show>
          <div class="virtual-timeline" ref={virtualHost}>
            <Show when={stickyDay()}>
              {(ts) => (
                <div class="virtual-day-overlay">
                  <div class="day">
                    <button onClick={() => pickDate(ts())}>{dayLabel(ts())}</button>
                  </div>
                </div>
              )}
            </Show>
            <div aria-hidden="true" style={{ height: `${windowRange().before}px` }} />
            <For each={visible()}>
              {(m) => (
                <MeasuredRow id={m.ts} observer={rowObserver}>
                  <Show when={dayStarts().has(m.ts)}>
                    <div class="day-divider">
                      <div class="day">
                        <button onClick={() => pickDate(m.ts)}>{dayLabel(m.ts)}</button>
                      </div>
                    </div>
                  </Show>
                  <Show when={m.ts === unreadTs()}>
                    <div class="unread-line">
                      <span>{t("New")}</span>
                    </div>
                  </Show>
                  <MessageRow
                    readOnly={props.conversation.is_member === false}
                    m={m}
                    conversationId={props.conversation.id}
                    prev={
                      dayStarts().has(m.ts) ? undefined : items()[(layout.index(m.ts) ?? 0) - 1]
                    }
                    onThread={props.onThread}
                    active={props.thread === m.ts}
                    flash={flash() === m.ts}
                  />
                </MeasuredRow>
              )}
            </For>
            <div aria-hidden="true" style={{ height: `${windowRange().after}px` }} />
          </div>
        </div>
      </div>
      <Show
        when={props.conversation.is_member !== false}
        fallback={<JoinChannelBar conversation={props.conversation} onJoin={props.onJoin} />}
      >
        <Composer
          conversationId={props.conversation.id}
          placeholder={t("Message {name}", {
            name: (isChannel(props.conversation) ? "#" : "") + props.conversation.label,
          })}
          onSent={sent}
        />
      </Show>
    </div>
  );
}

const isChannel = (c: Conversation) => c.kind === "public_channel" || c.kind === "private_channel";

const dayKey = (ts: string) => tsDate(ts).toDateString();
