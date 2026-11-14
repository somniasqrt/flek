import { createSignal, For, Show } from "solid-js";
import { t, plural, locale, clock24, errorMessage } from "../i18n/index";
import { settings } from "../stores/settings";
import { api, tsDate, type Message } from "../api";
import { AddReaction, ReplyInThread } from "./icons";
import { needsSlack } from "../models/slack-link";
import { openInSlack, storedSlackLink } from "../stores/slack";
import MessageMenu from "./MessageMenu";
import Avatar from "./Avatar";
import Emoji from "./EmojiView";
import Files from "./Files";
import { discard, retry } from "../stores/outbox";
import { people } from "../stores/people";
import { openProfile } from "../stores/profile";
import Rich from "./Rich";
import { plainText } from "../models/mrkdwn";
import { relativeTime } from "../models/time";
import Unfurls from "./Unfurls";

// One message, as both the timeline and the thread panel show it.

/// Messages from one author closer than this are grouped (seconds).
const GROUP_GAP = 5 * 60;

// "Last reply 5 minutes ago" ages while the window stays open.
const [now, setNow] = createSignal(new Date());
setInterval(() => setNow(new Date()), 60_000);

export const timeLabel = (ts: string) =>
  tsDate(ts).toLocaleTimeString(locale(), {
    hour12: !clock24(),
    hour: "2-digit",
    minute: "2-digit",
  });

export const author = (m: Message) =>
  (m.user_id && people()[m.user_id]) || m.author || m.user_id || t("unknown");

export const repliesLabel = (n: number) => plural("replies", n);

/// `:name: count` pairs (core's summary) as chips.
function Reactions(props: {
  m: Message;
  busy: boolean;
  onToggle: (name: string) => void;
  onPicker: (e: MouseEvent) => void;
}) {
  const items = () =>
    [...(props.m.reactions ?? "").matchAll(/:([^:\s]+(?:::skin-tone-\d)?): (\d+)/g)].map((m) => ({
      name: m[1],
      count: m[2],
    }));
  return (
    <div class="reactions">
      <For each={items()}>
        {(r) => (
          <button
            type="button"
            class="reaction"
            classList={{ mine: props.m.own_reactions?.includes(r.name) }}
            aria-pressed={props.m.own_reactions?.includes(r.name) ?? false}
            disabled={props.busy}
            title={t(
              props.m.own_reactions?.includes(r.name)
                ? "Remove :{name}: reaction"
                : "Add :{name}: reaction",
              { name: r.name },
            )}
            onClick={() => props.onToggle(r.name)}
          >
            <Emoji name={r.name} />
            <span>{r.count}</span>
          </button>
        )}
      </For>
      <button
        type="button"
        class="reaction reaction-add"
        title={t("Add reaction")}
        aria-label={t("Add reaction")}
        disabled={props.busy}
        onClick={props.onPicker}
      >
        <AddReaction />
      </button>
    </div>
  );
}

/// Under a thread root: who replied, how many, and when (like Slack).
function ThreadBar(props: { m: Message; onOpen: () => void }) {
  const last = () => props.m.latest_reply;
  return (
    <button class="thread-bar" type="button" onClick={props.onOpen}>
      <Show when={props.m.repliers.length > 0}>
        <span class="thread-faces">
          <For each={props.m.repliers.slice(0, 4)}>
            {(id) => <Avatar id={id} name={people()[id] ?? id} size={20} />}
          </For>
        </span>
      </Show>
      <span class="thread-count">{repliesLabel(props.m.reply_count)}</span>
      <Show when={last()}>
        {(ts) => (
          <span class="thread-last">
            {t("Last reply {time}", { time: relativeTime(tsDate(ts()), now()) })}
          </span>
        )}
      </Show>
      <span class="thread-view">{t("View thread")}</span>
      <span class="thread-chevron" aria-hidden="true">
        ›
      </span>
    </button>
  );
}

/// One message; `prev` (same day / same thread) decides grouping under one
/// author line. `onThread` shows the reply bar of a thread root.
export default function MessageRow(props: {
  m: Message;
  readOnly?: boolean;
  conversationId: string;
  prev?: Message;
  onThread?: (ts: string) => void;
  active?: boolean;
  /// Briefly highlighted (a search result jumped to).
  flash?: boolean;
}) {
  let rowElement!: HTMLElement;
  const m = props.m;
  const [menu, setMenu] = createSignal<{
    x: number;
    y: number;
    picker: boolean;
  } | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const openMenu = (e: MouseEvent, picker = false) => {
    if (m.pending) return;
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, picker });
  };
  const react = async (name: string) => {
    if (busy() || props.readOnly) return;
    setMenu(null);
    setBusy(true);
    setError("");
    try {
      await api.setReaction(props.conversationId, m.ts, name, !m.own_reactions?.includes(name));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const copy = async () => {
    setMenu(null);
    try {
      await navigator.clipboard.writeText(plainText(m.text, (id) => people()[id]));
    } catch {
      setError(t("Could not copy the message"));
    }
  };
  const copyLink = async () => {
    setMenu(null);
    try {
      const link = await storedSlackLink(props.conversationId, m.ts);
      await navigator.clipboard.writeText(link);
    } catch {
      setError(t("Could not copy the message link"));
    }
  };
  const openSlack = async () => {
    setMenu(null);
    try { await openInSlack(props.conversationId, m.ts); }
    catch (e) { setError(String(e)); }
  };
  const grouped = () =>
    !!props.prev &&
    props.prev.user_id === m.user_id &&
    !m.subtype &&
    parseFloat(m.ts) - parseFloat(props.prev.ts) < GROUP_GAP;
  return (
    <article
      ref={rowElement}
      class="msg"
      classList={{
        grouped: grouped(),
        "menu-open": !!menu(),
        active: props.active,
        flash: props.flash,
        sending: m.pending === "sending",
        failed: m.pending === "failed",
      }}
      data-ts={m.ts}
      onContextMenu={(e) => {
        const selection = window.getSelection();
        if (selection?.toString()) {
          for (let i = 0; i < selection.rangeCount; i++) {
            if (selection.getRangeAt(i).intersectsNode(e.currentTarget)) {
              setMenu(null);
              return; // Let the native menu copy the selected text.
            }
          }
        }
        openMenu(e);
      }}
      onKeyDown={(e) => {
        if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
          e.preventDefault();
          const rect = e.currentTarget.getBoundingClientRect();
          if (!m.pending) setMenu({ x: rect.left + 48, y: rect.top + 24, picker: false });
        }
      }}
    >
      <Show when={!m.pending}>
        <div class="message-actions">
          <button
            type="button"
            title={t("Add reaction")}
            aria-label={t("Add reaction")}
            disabled={busy() || props.readOnly}
            onClick={(e) => openMenu(e, true)}
          >
            <AddReaction />
          </button>
          <Show when={props.onThread}>
            <button
              type="button"
              title={t("Reply in thread")}
              aria-label={t("Reply in thread")}
              onClick={() => props.onThread?.(m.thread_ts ?? m.ts)}
            >
              <ReplyInThread />
            </button>
          </Show>
          <button
            type="button"
            title={t("More actions")}
            aria-label={t("More message actions")}
            onClick={(e) => openMenu(e)}
          >
            ⋯
          </button>
        </div>
      </Show>
      <Show when={menu()}>
        {(pos) => (
          <MessageMenu
            x={pos().x}
            y={pos().y}
            picker={pos().picker}
            readOnly={props.readOnly}
            own={m.own_reactions ?? []}
            onReaction={react}
            onClose={() => setMenu(null)}
            onCopy={copy}
            onCopyLink={copyLink}
            onOpenSlack={openSlack}
            onThread={props.onThread ? () => props.onThread?.(m.thread_ts ?? m.ts) : undefined}
          />
        )}
      </Show>
      <div class="gutter">
        <Show when={!grouped()} fallback={<time class="hover-time">{timeLabel(m.ts)}</time>}>
          <Show when={m.user_id} fallback={<Avatar id={m.user_id} name={author(m)} />}>
            {(id) => (
              <button
                class="avatar-button"
                type="button"
                title={t("View {name}’s profile", { name: author(m) })}
                onClick={() => openProfile(id())}
              >
                <Avatar id={id()} name={author(m)} />
              </button>
            )}
          </Show>
        </Show>
      </div>
      <div class="body">
        <Show when={!grouped()}>
          <div class="meta">
            <Show when={m.user_id} fallback={<span class="author">{author(m)}</span>}>
              {(id) => (
                <button
                  class="author author-button"
                  type="button"
                  onClick={() => openProfile(id())}
                >
                  {author(m)}
                </button>
              )}
            </Show>
            <time class="muted">{timeLabel(m.ts)}</time>
            <Show when={m.hidden_by_limit}>
              <span class="tag" title={t("Slack hides this message on the Free plan")}>
                {t("from archive")}
              </span>
            </Show>
          </div>
        </Show>
        <Show when={m.text.trim() || m.blocks}>
          <Rich text={m.text} blocks={m.blocks} blockImages={m.block_images} />
        </Show>
        <Show when={!m.pending && needsSlack(m)}>
          <button class="open-slack-fallback" onClick={openSlack}>{t("Open in Slack")}</button>
        </Show>
        <Show when={m.edited}>
          <span class="muted edited">{t("(edited)")}</span>
        </Show>
        <Show when={m.files.length > 0}>
          <Files files={m.files} from={{ user_id: m.user_id, name: author(m), ts: m.ts, conversation_id: props.conversationId }} />
        </Show>
        <Show when={settings().link_previews && m.unfurls.length > 0}>
          <Unfurls items={m.unfurls} />
        </Show>
        <Show when={m.reactions && !m.pending}>
          <Reactions m={m} busy={busy() || !!props.readOnly} onToggle={react} onPicker={(e) => openMenu(e, true)} />
        </Show>
        <Show when={error()}>
          <div class="reaction-error" role="alert">
            {errorMessage(error())}{" "}
            <button type="button" aria-label={t("Dismiss error")} onClick={() => setError("")}>
              ×
            </button>
          </div>
        </Show>
        <Show when={m.pending === "sent"}>
          <div class="send-reason">{t("Sent to Slack; local copy will sync later.")}</div>
        </Show>
        <Show when={m.pending === "failed" && m.client_msg_id}>
          {(id) => (
            <div
              class="send-failed"
              role="alert"
              title={m.error ? errorMessage(m.error) : undefined}
            >
              <span>{t("Not sent")}</span>
              <span aria-hidden="true">·</span>
              <button type="button" onClick={() => retry(id())}>
                {t("Retry")}
              </button>
              <span aria-hidden="true">·</span>
              <button type="button" onClick={() => discard(id())}>
                {t("Discard")}
              </button>
              <Show when={m.error}>
                <span class="send-reason">{errorMessage(m.error)}</span>
              </Show>
            </div>
          )}
        </Show>
        <Show when={props.onThread && m.reply_count > 0}>
          <ThreadBar m={m} onOpen={() => props.onThread?.(m.ts)} />
        </Show>
      </div>
    </article>
  );
}
