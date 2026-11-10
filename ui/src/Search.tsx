import { createEffect, createSignal, For, on, onCleanup, onMount, Show } from "solid-js";
import { t, errorMessage, locale, clock24, plural } from "./i18n";
import { api, tsDate, type Conversation, type Hit } from "./api";
import Avatar from "./Avatar";
import T from "./i18n/T";
import { Hash, Lock, People, Search } from "./icons";
import { emoji } from "./emoji";
import { people } from "./people";
import { keyCombo, modKeyLabel } from "./platform";
import { primaryModifier, usesMacShortcuts } from "./shortcuts";
import { pieces, readable } from "./snippet";

// Search over the local archive, like Slack's: a field in the middle of the
// top bar (⌘F), a dropdown with filter hints and the first few matches while
// typing, and Enter for the full results in the main pane.

/// Where a result lives: the message, or its thread root for a reply that is
/// only in the thread (the timeline shows roots).
export type Jump = { conversation_id: string; ts: string; thread_ts: string | null };

const QUICK = 5;
const FULL = 100;

/// Filters core understands (store/search.rs). Clicking one types it.
const FILTERS = () => [
  { insert: "in:#", label: "in:", hint: t("#channel or person") },
  { insert: "from:@", label: "from:", hint: t("@person") },
  { insert: "has:link", label: "has:link", hint: "" },
  { insert: "has:file", label: "has:file", hint: "" },
  { insert: "after:", label: "after:", hint: t("YYYY-MM-DD") },
  { insert: "before:", label: "before:", hint: t("YYYY-MM-DD") },
];

export function SearchBar(props: {
  team: string;
  conversations: Conversation[];
  onSubmit: (query: string) => void;
  onOpen: (hit: Hit) => void;
}) {
  const mac = usesMacShortcuts();
  const [query, setQuery] = createSignal("");
  const [open, setOpen] = createSignal(false);
  const [quick, setQuick] = createSignal<Hit[]>([]);
  let input!: HTMLInputElement;
  let wrap!: HTMLDivElement;

  // ⌘F focuses the field from anywhere (⌘K stays free for a quick switcher).
  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (
        primaryModifier(e, mac) &&
        !e.shiftKey &&
        !e.altKey &&
        !e.isComposing &&
        e.code === "KeyF"
      ) {
        e.preventDefault();
        input.focus();
        input.select();
      }
    };
    const onDown = (e: MouseEvent) => {
      if (!wrap.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onDown);
    onCleanup(() => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onDown);
    });
  });

  // First matches while typing; a half-typed filter (`before:2024-`) is an
  // error from core and simply shows none.
  let timer: number | undefined;
  let asked = 0;
  createEffect(
    on(query, (q) => {
      window.clearTimeout(timer);
      if (!q.trim()) return setQuick([]);
      timer = window.setTimeout(async () => {
        const n = ++asked;
        const hits = await api.search(q, false, QUICK).catch(() => [] as Hit[]);
        if (n === asked) setQuick(hits);
      }, 120);
    }),
  );
  onCleanup(() => window.clearTimeout(timer));

  const close = () => {
    setOpen(false);
    input.blur();
  };
  const submit = () => {
    const q = query().trim();
    if (!q) return;
    props.onSubmit(q);
    close();
  };
  const insert = (text: string) => {
    const q = query();
    setQuery(q && !q.endsWith(" ") ? `${q} ${text}` : q + text);
    input.focus();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Enter") submit();
    if (e.key === "Escape") {
      if (query()) setQuery("");
      else close();
    }
  };

  return (
    <div class="search" classList={{ open: open() }} ref={wrap}>
      <label class="search-field">
        <Search size={15} />
        <input
          ref={input}
          type="text"
          spellcheck={false}
          autocomplete="off"
          placeholder={t("Search {name}", { name: props.team })}
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
          onFocus={() => setOpen(true)}
          onKeyDown={onKey}
        />
        <Show when={!open()}>
          <kbd class="search-kbd">{keyCombo([modKeyLabel(), "F"])}</kbd>
        </Show>
      </label>
      <Show when={open()}>
        <div class="search-panel">
          <Show when={query().trim()}>
            <button class="search-row submit" onClick={submit}>
              <Search size={15} />
              <span class="grow">{query().trim()}</span>
              <span class="muted small">Enter</span>
            </button>
            <For each={quick()}>
              {(h) => (
                <button
                  class="search-row quick"
                  onClick={() => {
                    props.onOpen(h);
                    close();
                  }}
                >
                  <Avatar id={h.user_id} name={h.author ?? "?"} size={20} />
                  <span class="quick-author">{h.author ?? t("unknown")}</span>
                  <Snippet hit={h} />
                  <span class="muted small quick-where">
                    <Where hit={h} conversations={props.conversations} />
                  </span>
                </button>
              )}
            </For>
          </Show>
          <div class="search-filters">
            <span class="muted small">{t("Narrow down")}</span>
            <For each={FILTERS()}>
              {(f) => (
                <button class="filter" title={f.hint} onClick={() => insert(f.insert)}>
                  <b>{f.label}</b>
                  <Show when={f.hint}>
                    <span class="muted"> {f.hint}</span>
                  </Show>
                </button>
              )}
            </For>
          </div>
        </div>
      </Show>
    </div>
  );
}

/// Full results in the main pane.
export function SearchResults(props: {
  query: string;
  conversations: Conversation[];
  onOpen: (hit: Hit) => void;
  onClose: () => void;
}) {
  const [hits, setHits] = createSignal<Hit[] | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [recent, setRecent] = createSignal(false);
  let asked = 0;

  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !(document.activeElement instanceof HTMLInputElement)) {
        props.onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });

  createEffect(
    on([() => props.query, recent], async ([q, r]) => {
      const n = ++asked;
      setHits(null);
      setError(null);
      try {
        const list = await api.search(q, r, FULL);
        if (n === asked) setHits(list);
      } catch (e) {
        if (n === asked) {
          setHits([]);
          setError(String(e));
        }
      }
    }),
  );

  const count = () => {
    const n = hits()?.length ?? 0;
    return n >= FULL ? t("{n}+ results", { n: FULL }) : plural("results", n);
  };

  return (
    <div class="timeline search-view">
      <header class="conv-header">
        <h2>
          <Search size={18} />
          <span class="search-title">{props.query}</span>
        </h2>
        <Show when={hits() && !error()}>
          <span class="muted small">{count()}</span>
        </Show>
        <div class="grow" />
        <div class="sort" role="group" aria-label={t("Sort")}>
          <button classList={{ on: !recent() }} onClick={() => setRecent(false)}>
            {t("Most relevant")}
          </button>
          <button classList={{ on: recent() }} onClick={() => setRecent(true)}>
            {t("Newest")}
          </button>
        </div>
        <button class="close" title={t("Close search (Esc)")} onClick={props.onClose}>
          ×
        </button>
      </header>
      <div class="scroller results">
        <Show when={hits()} fallback={<div class="spinner" />}>
          {(list) => (
            <Show
              when={list().length > 0}
              fallback={
                <div class="no-results">
                  <Show
                    when={error()}
                    fallback={
                      <>
                        <h3>{t("No results for “{query}”", { query: props.query })}</h3>
                        <p class="muted">
                          <T
                            k="Flek searches the messages it has synced or imported. Try fewer words, a prefix like <code>word*</code>, or other filters."
                            tags={{ code: (text) => <code>{text}</code> }}
                          />
                        </p>
                      </>
                    }
                  >
                    {(e) => (
                      <>
                        <h3>{t("Can't search for that")}</h3>
                        <p class="muted">{errorMessage(e())}</p>
                      </>
                    )}
                  </Show>
                </div>
              }
            >
              <For each={list()}>
                {(h) => (
                  <button class="hit" onClick={() => props.onOpen(h)}>
                    <div class="hit-where muted small">
                      <Where hit={h} conversations={props.conversations} />
                      <span>·</span>
                      <span>{dateLabel(h.ts)}</span>
                      <Show when={h.thread_ts && h.thread_ts !== h.ts}>
                        <span>· {t("In a thread")}</span>
                      </Show>
                    </div>
                    <div class="hit-body">
                      <Avatar id={h.user_id} name={h.author ?? "?"} />
                      <div class="body">
                        <div class="meta">
                          <span class="author">{h.author ?? t("unknown")}</span>
                          <time class="muted">{timeLabel(h.ts)}</time>
                          <Show when={h.hidden_by_limit}>
                            <span
                              class="tag"
                              title={t("Slack hides this message on the Free plan")}
                            >
                              {t("from archive")}
                            </span>
                          </Show>
                        </div>
                        <Snippet hit={h} />
                      </div>
                    </div>
                  </button>
                )}
              </For>
            </Show>
          )}
        </Show>
      </div>
    </div>
  );
}

function Snippet(props: { hit: Hit }) {
  const parts = () => pieces(readable(props.hit.snippet, people(), emoji));
  return (
    <span class="snippet">
      <Show when={parts().length > 0} fallback={<span class="muted">{t("(no text)")}</span>}>
        <For each={parts()}>{(p) => (p.mark ? <mark>{p.text}</mark> : p.text)}</For>
      </Show>
    </span>
  );
}

function Where(props: { hit: Hit; conversations: Conversation[] }) {
  const kind = () => props.conversations.find((c) => c.id === props.hit.conversation_id)?.kind;
  return (
    <span class="where">
      {kind() === "public_channel" ? (
        <Hash size={12} />
      ) : kind() === "private_channel" ? (
        <Lock size={12} />
      ) : kind() === "mpim" ? (
        <People size={12} />
      ) : null}
      {props.hit.conversation}
    </span>
  );
}

export const jumpOf = (h: Hit): Jump => ({
  conversation_id: h.conversation_id,
  ts: h.ts,
  thread_ts: h.thread_ts,
});

function dateLabel(ts: string): string {
  const d = tsDate(ts);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return t("Today");
  if (d.toDateString() === new Date(now.getTime() - 86_400_000).toDateString())
    return t("Yesterday");
  return d.toLocaleDateString(locale(), {
    month: "short",
    day: "numeric",
    year: d.getFullYear() === now.getFullYear() ? undefined : "numeric",
  });
}

const timeLabel = (ts: string) =>
  tsDate(ts).toLocaleTimeString(locale(), {
    hour12: !clock24(),
    hour: "2-digit",
    minute: "2-digit",
  });
