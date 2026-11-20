import { createEffect, createMemo, createSignal, For, on, onCleanup, onMount, Show } from "solid-js";
import { t, errorMessage, locale, clock24, plural } from "../i18n/index";
import { api, tsDate, type Conversation, type Hit } from "../api";
import MaterialResults from "./MaterialResults";
import Avatar from "../components/Avatar";
import T from "../i18n/T";
import { Hash, Lock, People, Search } from "../components/icons";
import { emoji } from "../models/emoji";
import { people } from "../stores/people";
import { keyCombo, modKeyLabel } from "../platform";
import { pieces, readable } from "../models/snippet";
import { applyFilter, detectFilter, filterSuggestions, type FilterSuggestion } from "../models/search-filter";
import type { ComposerPerson } from "../models/composer-autocomplete";

// Search over the local archive, like Slack's: a field in the middle of the
// top bar (⌘G), a dropdown with filter hints and the first few matches while
// typing, and Enter for the full results in the main pane.

/// Where a result lives: the message, or its thread root for a reply that is
/// only in the thread (the timeline shows roots).
export type Jump = { conversation_id: string; ts: string; thread_ts: string | null };
export type SearchRequest = { prefill: string };

/// The `in:` filter for a conversation: channels by name, DMs by id
/// (their labels can contain spaces).
export function scopeFilter(c: Pick<Conversation, "id" | "kind" | "label">): string {
  return c.kind === "im" || c.kind === "mpim" || /\s/.test(c.label) ? `in:${c.id} ` : `in:#${c.label} `;
}

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
  materials: boolean;
  onModeChange: (materials: boolean) => void;
  conversations: Conversation[];
  /// A fresh object per keypress, so the same request focuses again.
  request: SearchRequest | null;
  onSubmit: (query: string) => void;
  onOpen: (hit: Hit) => void;
}) {
  const [query, setQuery] = createSignal("");
  const [open, setOpen] = createSignal(false);
  const [quick, setQuick] = createSignal<Hit[]>([]);
  // `in:` / `from:` completion: the caret position decides which filter is
  // being typed; Escape hides the list until the next edit.
  const [caret, setCaret] = createSignal(0);
  const [persons, setPersons] = createSignal<ComposerPerson[]>([]);
  const [active, setActive] = createSignal(0);
  const [dismissed, setDismissed] = createSignal(false);
  const token = createMemo(() => (props.materials || dismissed() ? null : detectFilter(query(), caret())));
  const suggestions = createMemo<FilterSuggestion[]>(() => {
    const tk = token();
    return tk ? filterSuggestions(tk, props.conversations, persons()) : [];
  });
  createEffect(on(suggestions, () => setActive(0)));
  let input!: HTMLInputElement;
  let wrap!: HTMLDivElement;

  // ⌘G focuses the field from anywhere; ⌘F starts with the open conversation
  // as an `in:` filter, like Slack (the workspace turns both into requests).
  createEffect(
    on(
      () => props.request,
      (request) => {
        if (!request) return;
        if (request.prefill) setQuery(request.prefill);
        input.focus();
        if (request.prefill) input.setSelectionRange(request.prefill.length, request.prefill.length);
        else input.select();
      },
      { defer: true },
    ),
  );
  onMount(() => {
    const onDown = (e: MouseEvent) => {
      if (!wrap.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    onCleanup(() => window.removeEventListener("mousedown", onDown));
  });

  // First matches while typing; a half-typed filter (`before:2024-`) is an
  // error from core and simply shows none.
  let timer: number | undefined;
  let asked = 0;
  createEffect(
    on([query, () => props.materials], ([q, materials]) => {
      window.clearTimeout(timer);
      if (!q.trim() || materials) {
        ++asked;
        return setQuick([]);
      }
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
    if (!q && !props.materials) return;
    props.onSubmit(q);
    close();
  };
  const edit = (text: string, at: number) => {
    setQuery(text);
    setDismissed(false);
    input.value = text;
    input.focus();
    input.setSelectionRange(at, at);
    setCaret(at);
  };
  const insert = (text: string) => {
    const q = query();
    const next = q && !q.endsWith(" ") ? `${q} ${text}` : q + text;
    edit(next, next.length);
  };
  const accept = (s: FilterSuggestion) => {
    const tk = token();
    if (!tk) return;
    const next = applyFilter(query(), tk, s.insert);
    edit(next.text, next.caret);
  };
  // Refreshed on every focus: the directory may have changed since.
  const loadPersons = () => {
    api.composerPeople("").then(setPersons, () => {});
  };
  const syncCaret = () => setCaret(input.selectionStart ?? query().length);
  const onKey = (e: KeyboardEvent) => {
    const list = suggestions();
    if (list.length && !e.isComposing) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setActive((i) => (i + (e.key === "ArrowDown" ? 1 : list.length - 1)) % list.length);
        return;
      }
      if ((e.key === "Enter" || e.key === "Tab") && !e.shiftKey) {
        e.preventDefault();
        accept(list[active()] ?? list[0]);
        return;
      }
      if (e.key === "Escape") {
        setDismissed(true);
        return;
      }
    }
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
          onInput={(e) => {
            setQuery(e.currentTarget.value);
            setDismissed(false);
            syncCaret();
          }}
          onFocus={() => {
            setOpen(true);
            loadPersons();
          }}
          onKeyDown={onKey}
          onKeyUp={(e) => {
            if (e.key !== "ArrowDown" && e.key !== "ArrowUp") syncCaret();
          }}
          onClick={syncCaret}
        />
        <Show when={!open()}>
          <kbd class="search-kbd">{keyCombo([modKeyLabel(), "G"])}</kbd>
        </Show>
      </label>
      <Show when={open()}>
        <div class="search-panel">
          <SearchMode materials={props.materials} onChange={props.onModeChange} />
          <Show when={props.materials && !query().trim()}>
            <button class="search-row submit" onClick={submit}>
              <Search size={15} />
              <span>{t("Browse links and files")}</span>
            </button>
          </Show>
          <Show when={suggestions().length}>
            <div class="search-suggestions" role="listbox" aria-label={t("Narrow down")}>
              <For each={suggestions()}>
                {(s, i) => (
                  <button
                    class="search-row suggestion"
                    classList={{ active: i() === active() }}
                    role="option"
                    aria-selected={i() === active()}
                    onMouseEnter={() => setActive(i())}
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => accept(s)}
                  >
                    <Show
                      when={s.kind === "person"}
                      fallback={s.kind === "dm" ? <People size={15} /> : s.private ? <Lock size={15} /> : <Hash size={15} />}
                    >
                      <Avatar id={s.id} name={s.label} size={20} />
                    </Show>
                    <span class="grow">{s.label}</span>
                    <Show when={s.detail && s.detail !== s.label}>
                      <span class="muted small">{s.detail}</span>
                    </Show>
                  </button>
                )}
              </For>
            </div>
          </Show>
          <Show when={query().trim() && !suggestions().length}>
            <button class="search-row submit" onClick={submit}>
              <Search size={15} />
              <span class="grow">{query().trim()}</span>
              <span class="muted small">Enter</span>
            </button>
            <For each={props.materials ? [] : quick()}>
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
  revision: number;
  materials: boolean;
  onModeChange: (materials: boolean) => void;
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
    on([() => props.query, recent, () => props.materials], async ([q, r, materials]) => {
      const n = ++asked;
      setHits(null);
      setError(null);
      if (materials) return;
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
          <span class="search-title">{props.query || (props.materials ? t("All materials") : t("Messages"))}</span>
        </h2>
        <Show when={!props.materials && hits() && !error()}>
          <span class="muted small">{count()}</span>
        </Show>
        <div class="grow" />
        <SearchMode materials={props.materials} onChange={props.onModeChange} />
        <Show when={!props.materials}>
          <div class="sort" role="group" aria-label={t("Sort")}>
            <button classList={{ on: !recent() }} onClick={() => setRecent(false)}>
              {t("Most relevant")}
            </button>
            <button classList={{ on: recent() }} onClick={() => setRecent(true)}>
              {t("Newest")}
            </button>
          </div>
        </Show>
        <button class="close" title={t("Close search (Esc)")} onClick={props.onClose}>
          ×
        </button>
      </header>
      <Show when={props.materials} fallback={
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
      }>
        <MaterialResults query={props.query} revision={props.revision} onOpen={props.onOpen} />
      </Show>
    </div>
  );
}

function SearchMode(props: { materials: boolean; onChange: (materials: boolean) => void }) {
  return (
    <div class="sort search-mode" role="group" aria-label={t("Search mode")}>
      <button
        classList={{ on: !props.materials }}
        aria-pressed={!props.materials}
        onClick={() => props.onChange(false)}
      >
        {t("Messages")}
      </button>
      <button
        classList={{ on: props.materials }}
        aria-pressed={props.materials}
        onClick={() => props.onChange(true)}
      >
        {t("Links and files")}
      </button>
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
