import { createMemo, createSignal, For, Show, onMount } from "solid-js";
import { t } from "./i18n";
import Emoji, { customEmojiNames } from "./EmojiView";
import { emoji } from "./emoji";
import EmojiCategoryIcon from "./EmojiCategoryIcon";
import {
  categories,
  emojiCatalog,
  frequentDefaults,
  readFrequency,
  readSkinTone,
  rememberEmoji,
  tonedName,
  workEmoji,
} from "./emoji-picker-data";

export default function EmojiPicker(props: {
  onPick: (name: string) => void;
  selected?: string[];
  disabled?: boolean;
}) {
  let search!: HTMLInputElement, scroller!: HTMLDivElement;
  const sections = new Map<string, HTMLElement>();
  const [query, setQuery] = createSignal("");
  const [active, setActive] = createSignal("search");
  const [tone, setTone] = createSignal(readSkinTone());
  const [toneOpen, setToneOpen] = createSignal(false);
  const [hover, setHover] = createSignal("");
  const frequency = readFrequency();
  const frequent = () =>
    [
      ...new Set(
        [
          ...Object.keys(frequency).sort((a, b) => frequency[b] - frequency[a]),
          ...frequentDefaults,
        ].map((name) => tonedName(name, tone())),
      ),
    ]
      .filter((name) => !!emoji(name) || customEmojiNames().includes(name))
      .slice(0, 36);
  const q = () => query().trim().replace(/^:|:$/g, "").toLowerCase().replace(/_/g, " ");
  const results = createMemo(() => [
    ...emojiCatalog
      .filter(
        (e) =>
          !q() ||
          `${e.label} ${e.terms}`.replace(/_/g, " ").toLowerCase().includes(q()) ||
          emoji(e.name) === query().trim(),
      )
      .map((e) => e.name),
    ...customEmojiNames().filter(
      (name) => !q() || name.replace(/_/g, " ").toLowerCase().includes(q()),
    ),
  ]);
  const choose = (name: string) => {
    const selected = tonedName(name, tone());
    rememberEmoji(selected);
    props.onPick(selected);
  };
  const jump = (id: string) => {
    setQuery("");
    setActive(id);
    setToneOpen(false);
    if (id === "search") {
      scroller.scrollTop = 0;
      search.focus();
      return;
    }
    requestAnimationFrame(() =>
      scroller.scrollTo({ top: sections.get(id)?.offsetTop ?? 0, behavior: "smooth" }),
    );
  };
  const buttons = (names: string[]) => (
    <div class="emoji-grid">
      <For each={names}>
        {(name) => {
          const full = () => tonedName(name, tone());
          return (
            <button
              type="button"
              title={`:${full()}:`}
              aria-label={`:${full()}:`}
              aria-pressed={props.selected?.includes(full()) ?? false}
              disabled={props.disabled}
              onMouseEnter={() => setHover(full())}
              onFocus={() => setHover(full())}
              onClick={() => choose(name)}
            >
              <Emoji name={full()} />
            </button>
          );
        }}
      </For>
    </div>
  );
  const section = (id: string, label: string, names: string[]) => (
    <Show when={names.length}>
      <section class="emoji-category-section" ref={(el) => sections.set(id, el)} aria-label={label}>
        <h3>{label}</h3>
        {buttons(names)}
      </section>
    </Show>
  );
  onMount(() => search.focus());
  return (
    <div class="emoji-picker" aria-label={t("Emoji picker")}>
      <div class="emoji-categories" role="tablist" aria-label={t("Emoji categories")}>
        <For
          each={[
            { id: "search", label: t("Search all emoji") },
            ...categories.map((c) => ({ ...c, label: t(c.label) })),
          ]}
        >
          {(category) => (
            <button
              type="button"
              role="tab"
              aria-label={category.label}
              title={category.label}
              aria-selected={active() === category.id}
              onClick={() => jump(category.id)}
            >
              <EmojiCategoryIcon name={category.id} />
            </button>
          )}
        </For>
      </div>
      <div class="emoji-search-wrap">
        <input
          ref={search}
          type="search"
          placeholder={t("Search all emoji")}
          aria-label={t("Find an emoji")}
          value={query()}
          onKeyDown={(e) => {
            if (e.key !== "ArrowDown" && e.key !== "Enter") return;
            e.preventDefault();
            e.stopPropagation();
            const first = scroller.querySelector<HTMLButtonElement>(
              ".emoji-grid button:not(:disabled)",
            );
            if (e.key === "Enter") first?.click();
            else first?.focus();
          }}
          onInput={(e) => {
            setQuery(e.currentTarget.value);
            setActive("search");
            scroller.scrollTop = 0;
          }}
        />
      </div>
      <div
        class="emoji-scroll"
        ref={scroller}
        aria-label={t("Emoji")}
        onKeyDown={(e) => {
          if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(e.key)) return;
          const current = (e.target as HTMLElement).closest("button");
          if (!current) return;
          const all = [...scroller.querySelectorAll<HTMLButtonElement>("button")];
          const i = all.indexOf(current as HTMLButtonElement);
          const cols = 9;
          const delta =
            e.key === "ArrowLeft"
              ? -1
              : e.key === "ArrowRight"
                ? 1
                : e.key === "ArrowUp"
                  ? -cols
                  : cols;
          e.preventDefault();
          e.stopPropagation();
          all[Math.max(0, Math.min(all.length - 1, i + delta))]?.focus();
        }}
      >
        <Show
          when={q()}
          fallback={
            <>
              {section("frequent", t("Frequently used"), frequent())}
              {section(
                "work",
                t("Getting work done"),
                workEmoji.filter((name) => !!emoji(name)),
              )}
              <For each={categories}>
                {(category) =>
                  section(
                    category.id,
                    t(category.label),
                    category.id === "custom"
                      ? customEmojiNames()
                      : emojiCatalog.filter((e) => e.group === category.id).map((e) => e.name),
                  )
                }
              </For>
              <Show when={!customEmojiNames().length}>
                <section class="emoji-category-section" ref={(el) => sections.set("custom", el)}>
                  <h3>{t("Workspace emoji")}</h3>
                  <p class="muted">{t("Workspace emoji appear here after syncing.")}</p>
                </section>
              </Show>
            </>
          }
        >
          {section("results", t("Search results"), results())}
          <Show when={!results().length}>
            <p class="emoji-no-results muted">{t("No emoji found")}</p>
          </Show>
        </Show>
      </div>
      <footer class="emoji-picker-footer">
        <span class="emoji-hover-label" title={hover()}>
          <Show when={hover()} fallback="Choose an emoji">
            :{hover()}:{" "}
          </Show>
        </span>
        <div class="emoji-tone-control">
          <button
            type="button"
            class="emoji-tone-toggle"
            aria-label={t("Skin tone")}
            aria-expanded={toneOpen()}
            onClick={() => setToneOpen(!toneOpen())}
          >
            <Emoji name={tone() ? `raised_hand::skin-tone-${tone()}` : "raised_hand"} />
            <span>{t("Skin tone")}</span>
          </button>
          <Show when={toneOpen()}>
            <div class="emoji-tone-options" role="group" aria-label={t("Choose skin tone")}>
              <For each={[0, 2, 3, 4, 5, 6]}>
                {(value) => (
                  <button
                    type="button"
                    aria-label={value ? t("Skin tone {n}", { n: value }) : t("Default skin tone")}
                    aria-pressed={tone() === value}
                    onClick={() => {
                      setTone(value);
                      setToneOpen(false);
                      try {
                        localStorage.setItem("flek.emoji.skinTone", String(value));
                      } catch {
                        /* optional */
                      }
                    }}
                  >
                    <Emoji name={value ? `raised_hand::skin-tone-${value}` : "raised_hand"} />
                  </button>
                )}
              </For>
            </div>
          </Show>
        </div>
      </footer>
    </div>
  );
}
