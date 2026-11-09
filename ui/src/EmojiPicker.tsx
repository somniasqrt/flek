import { createMemo, createSignal, For, Show, onMount } from "solid-js";
import Emoji, { customEmojiNames } from "./EmojiView";
import { standardEmojiNames } from "./emoji";

export const quickEmoji = ["+1", "heart", "joy", "tada", "eyes", "pray"];

export default function EmojiPicker(props: {
  onPick: (name: string) => void;
  selected?: string[];
  disabled?: boolean;
}) {
  let search!: HTMLInputElement;
  onMount(() => search.focus());
  const [query, setQuery] = createSignal("");
  const names = createMemo(() => {
    const q = query().trim().replace(/^:|:$/g, "").toLowerCase().replace(/ /g, "_");
    const all = [...new Set([...quickEmoji, ...customEmojiNames(), ...standardEmojiNames])];
    return all.filter((name) => !q || name.includes(q)).slice(0, 160);
  });
  return (
    <div class="emoji-picker">
      <input
        ref={search}
        type="search"
        placeholder="Find an emoji…"
        aria-label="Find an emoji"
        value={query()}
        onInput={(e) => setQuery(e.currentTarget.value)}
      />
      <div class="emoji-grid" aria-label="Emoji">
        <For each={names()}>
          {(name) => (
            <button
              type="button"
              title={`:${name}:`}
              aria-label={`:${name}:`}
              aria-pressed={props.selected?.includes(name) ?? false}
              disabled={props.disabled}
              onClick={() => props.onPick(name)}
            >
              <Emoji name={name} />
            </button>
          )}
        </For>
      </div>
      <Show when={!names().length}>
        <p class="muted">No emoji found</p>
      </Show>
    </div>
  );
}
